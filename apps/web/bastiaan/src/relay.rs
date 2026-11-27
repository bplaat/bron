/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! LAN relay: puts the Windows XP of every visitor on one virtual Ethernet segment, without
//! internet access.
//!
//! v86 sends each Ethernet frame as one binary WebSocket message. The relay is a switch that
//! forwards only IPv4 and ARP between visitors, hands out 10.86.0.0/16 addresses with its own DHCP
//! server and drops frames that use another visitor's MAC or IP address. Nothing leaves the LAN:
//! there is no router and no DNS server. Every connection has a thread that hands its frames to the
//! shared switch and one that sends the switch's frames for it.
//!
//! Environment: ORIGIN, the only page origin allowed to connect (any when unset), and
//! TRUST_PROXY=true behind a reverse proxy, the visitor's address is then the last X-Forwarded-For
//! entry.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use std::{env, thread};

use small_http::{Request, Response, Status};
use small_websocket::{Message, WebSocket};

const MAX_FRAME: usize = 1536;
const QUEUE: usize = 256; // frames waiting for a slow visitor, more are dropped
const MAX_PER_ADDRESS: usize = 4; // browser tabs per visitor address
// Bytes, frames and broadcasts per visitor per second
const LIMITS: [f64; 3] = [1024.0 * 1024.0, 1000.0, 100.0];
const PING_INTERVAL: Duration = Duration::from_secs(30);
const IDLE_TIMEOUT: Duration = Duration::from_secs(75);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const LEASE_SECONDS: u32 = 24 * 60 * 60;
const CLOSE_POLICY: u16 = 1008;

const ETH_IPV4: u16 = 0x0800;
const ETH_ARP: u16 = 0x0806;
const SUBNET: [u8; 2] = [10, 86];
const SUBNET_MASK: [u8; 4] = [255, 255, 0, 0];
const RELAY_IP: [u8; 4] = host_ip(1);
const FIRST_HOST: u16 = 2;
const LAST_HOST: u16 = u16::MAX - 1;
const MAX_VISITORS: usize = (LAST_HOST - FIRST_HOST + 1) as usize;
const RELAY_MAC: [u8; 6] = [0x02, 0x86, 0x86, 0x86, 0x86, 0x01]; // locally administered
const DHCP_MAGIC: [u8; 4] = [0x63, 0x82, 0x53, 0x63];
const DHCP_DISCOVER: u8 = 1;
const DHCP_OFFER: u8 = 2;
const DHCP_REQUEST: u8 = 3;
const DHCP_DECLINE: u8 = 4;
const DHCP_ACK: u8 = 5;
const DHCP_NAK: u8 = 6;
const DHCP_RELEASE: u8 = 7;

const fn host_ip(host: u16) -> [u8; 4] {
    let [high, low] = host.to_be_bytes();
    [SUBNET[0], SUBNET[1], high, low]
}

pub(crate) struct Config {
    origin: Option<String>,
    trust_proxy: bool,
}

impl Config {
    pub(crate) fn from_env() -> Self {
        Self {
            origin: env::var("ORIGIN").ok().filter(|origin| !origin.is_empty()),
            trust_proxy: env::var("TRUST_PROXY").is_ok_and(|value| value == "true" || value == "1"),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Relay {
    switch: Arc<Mutex<Switch>>,
    config: Arc<Config>,
}

impl Relay {
    pub(crate) fn new(config: Config) -> Self {
        Self {
            switch: Arc::default(),
            config: Arc::new(config),
        }
    }

    pub(crate) fn handler(&self, request: &Request) -> Response {
        let origin = request.headers.get("Origin");
        if self
            .config
            .origin
            .as_deref()
            .is_some_and(|allowed| origin != Some(allowed))
        {
            return Response::with_status(Status::Forbidden);
        }
        let forwarded = || {
            let header = request.headers.get("X-Forwarded-For")?;
            header.rsplit(',').next()?.trim().parse().ok()
        };
        let address = self
            .config
            .trust_proxy
            .then(forwarded)
            .flatten()
            .unwrap_or_else(|| request.client_addr.ip());
        let relay = self.clone();
        small_websocket::upgrade(request, move |socket| relay.connection(socket, address))
    }

    fn switch(&self) -> MutexGuard<'_, Switch> {
        self.switch.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // Frames from the socket go to the switch, a writer thread sends the switch's frames for this
    // visitor
    fn connection(&self, mut socket: WebSocket, address: IpAddr) {
        let (frames, queue) = mpsc::sync_channel(QUEUE);
        let Some(id) = self.switch().join(address, frames) else {
            socket
                .send(Message::Close(
                    Some(CLOSE_POLICY),
                    Some("The LAN is full".to_string()),
                ))
                .ok();
            return;
        };

        // The page went away without closing when nothing, not even a pong, arrives in time
        socket.set_read_timeout(Some(IDLE_TIMEOUT)).ok();
        socket.set_write_timeout(Some(WRITE_TIMEOUT)).ok();
        let mut writer = socket.clone();
        if thread::Builder::new()
            .spawn(move || {
                loop {
                    let message = match queue.recv_timeout(PING_INTERVAL) {
                        Ok(frame) => Message::Binary(frame),
                        Err(RecvTimeoutError::Timeout) => Message::Ping(Vec::new()),
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    if writer.send(message).is_err() {
                        break;
                    }
                }
            })
            .is_err()
        {
            self.switch().leave(id);
            return;
        }

        loop {
            match socket.recv() {
                Ok(Message::Binary(frame)) if frame.len() <= MAX_FRAME => {
                    self.switch().frame(id, &frame)
                }
                Ok(Message::Ping(payload)) => {
                    if socket.send(Message::Pong(payload)).is_err() {
                        break;
                    }
                }
                Ok(Message::Binary(_) | Message::Close(..)) | Err(_) => break,
                Ok(_) => {}
            }
        }
        // Drops the frame queue, which stops the writer
        self.switch().leave(id);
    }
}

// Token buckets that refill every second
struct Budget {
    tokens: [f64; 3],
    time: Instant,
}

impl Budget {
    fn new() -> Self {
        Self {
            tokens: LIMITS,
            time: Instant::now(),
        }
    }

    fn take(&mut self, bytes: usize, broadcast: bool) -> bool {
        let now = Instant::now();
        let refill = now.duration_since(self.time).as_secs_f64();
        self.time = now;
        let cost = [bytes as f64, 1.0, if broadcast { 1.0 } else { 0.0 }];
        for (tokens, limit) in self.tokens.iter_mut().zip(LIMITS) {
            *tokens = (*tokens + refill * limit).min(limit);
        }
        if self
            .tokens
            .iter()
            .zip(cost)
            .any(|(tokens, cost)| *tokens < cost)
        {
            return false;
        }
        for (tokens, cost) in self.tokens.iter_mut().zip(cost) {
            *tokens -= cost;
        }
        true
    }
}

struct Visitor {
    frames: SyncSender<Vec<u8>>,
    address: IpAddr,
    mac: Option<[u8; 6]>,
    host: Option<u16>,
    budget: Budget,
}

#[derive(Default)]
struct Switch {
    visitors: HashMap<u64, Visitor>,
    by_mac: HashMap<[u8; 6], u64>,
    by_host: HashMap<u16, u64>,
    per_address: HashMap<IpAddr, usize>,
    next_id: u64,
    last_host: u16,
}

impl Switch {
    fn join(&mut self, address: IpAddr, frames: SyncSender<Vec<u8>>) -> Option<u64> {
        if self.visitors.len() >= MAX_VISITORS {
            return None;
        }
        let count = self.per_address.entry(address).or_default();
        if *count >= MAX_PER_ADDRESS {
            return None;
        }
        *count += 1;
        self.next_id += 1;
        let visitor = Visitor {
            frames,
            address,
            mac: None,
            host: None,
            budget: Budget::new(),
        };
        self.visitors.insert(self.next_id, visitor);
        Some(self.next_id)
    }

    fn leave(&mut self, id: u64) {
        self.release(id);
        let Some(visitor) = self.visitors.remove(&id) else {
            return;
        };
        if let Some(mac) = visitor.mac {
            self.by_mac.remove(&mac);
        }
        if let Some(count) = self.per_address.get_mut(&visitor.address) {
            *count -= 1;
            if *count == 0 {
                self.per_address.remove(&visitor.address);
            }
        }
    }

    // A full queue means the visitor cannot keep up, dropping the frame is what a switch does
    fn send(&self, id: u64, frame: Vec<u8>) {
        if let Some(visitor) = self.visitors.get(&id) {
            visitor.frames.try_send(frame).ok();
        }
    }

    fn host(&self, id: u64) -> Option<u16> {
        self.visitors.get(&id)?.host
    }

    // The visitor's own address: kept, or taken when free, as after a relay restart the guest keeps
    // its old lease
    fn claim(&mut self, id: u64, ip: &[u8]) -> bool {
        let &[first, second, high, low] = ip else {
            return false;
        };
        if [first, second] != SUBNET {
            return false;
        }
        self.claim_host(id, u16::from_be_bytes([high, low]))
    }

    fn claim_host(&mut self, id: u64, host: u16) -> bool {
        let Some(visitor) = self.visitors.get_mut(&id) else {
            return false;
        };
        if visitor.host == Some(host) {
            return true;
        }
        if visitor.host.is_some()
            || !(FIRST_HOST..=LAST_HOST).contains(&host)
            || self.by_host.contains_key(&host)
        {
            return false;
        }
        visitor.host = Some(host);
        self.by_host.insert(host, id);
        true
    }

    fn release(&mut self, id: u64) {
        if let Some(host) = self
            .visitors
            .get_mut(&id)
            .and_then(|visitor| visitor.host.take())
        {
            self.by_host.remove(&host);
        }
    }

    fn frame(&mut self, id: u64, frame: &[u8]) {
        if frame.len() < 14 {
            return;
        }
        let dst: [u8; 6] = frame[0..6].try_into().unwrap_or_default();
        let src: [u8; 6] = frame[6..12].try_into().unwrap_or_default();
        let broadcast = dst[0] & 1 == 1;
        let Some(visitor) = self.visitors.get_mut(&id) else {
            return;
        };
        if !visitor.budget.take(frame.len(), broadcast) || src[0] & 1 == 1 || src == RELAY_MAC {
            return;
        }
        // The first frame fixes the visitor's MAC, v86 gives every page its own
        match visitor.mac {
            None if self.by_mac.contains_key(&src) => return,
            None => {
                visitor.mac = Some(src);
                self.by_mac.insert(src, id);
            }
            Some(mac) if mac != src => return,
            Some(_) => {}
        }

        let packet = &frame[14..];
        match u16::from_be_bytes([frame[12], frame[13]]) {
            ETH_ARP => {
                if packet.len() < 28 || packet[8..14] != src {
                    return;
                }
                let sender = &packet[14..18];
                if sender != [0; 4] && !self.claim(id, sender) {
                    return; // 0.0.0.0 probes for a free address
                }
                if packet[6..8] == [0, 1] && packet[24..28] == RELAY_IP {
                    return self.send(id, arp_reply(packet));
                }
            }
            ETH_IPV4 => {
                let header = usize::from(packet.first().copied().unwrap_or_default() & 15) * 4;
                if header < 20 || packet.len() < header + 8 || packet[0] >> 4 != 4 {
                    return;
                }
                if packet[9] == 17 && packet[header + 2..header + 4] == [0, 67] {
                    return self.dhcp(id, src, &packet[header + 8..]);
                }
                if !self.claim(id, &packet[12..16]) || packet[16..20] == RELAY_IP {
                    return;
                }
            }
            _ => return, // IPv4 and ARP only
        }

        if broadcast {
            for (&other, visitor) in &self.visitors {
                if other != id {
                    visitor.frames.try_send(frame.to_vec()).ok();
                }
            }
        } else if let Some(&target) = self.by_mac.get(&dst)
            && target != id
        {
            self.send(target, frame.to_vec());
        }
    }

    fn dhcp(&mut self, id: u64, mac: [u8; 6], bootp: &[u8]) {
        if bootp.len() < 240
            || bootp[0] != 1
            || bootp[236..240] != DHCP_MAGIC
            || bootp[28..34] != mac
        {
            return;
        }
        let (mut kind, mut requested, mut server) = (None, None, None);
        let mut at = 240;
        while at + 1 < bootp.len() && bootp[at] != 255 {
            if bootp[at] == 0 {
                at += 1; // padding
                continue;
            }
            let end = (at + 2 + usize::from(bootp[at + 1])).min(bootp.len());
            let value = &bootp[at + 2..end];
            match bootp[at] {
                53 => kind = value.first().copied(),
                50 => requested = Some(value),
                54 => server = Some(value),
                _ => {}
            }
            at = end;
        }
        let requested = requested.unwrap_or(&bootp[12..16]); // or the address being renewed
        match kind {
            Some(DHCP_DISCOVER) => {
                if self.host(id).is_none()
                    && !self.claim(id, requested)
                    && let Some(host) = ((self.last_host + 1).max(FIRST_HOST)..=LAST_HOST)
                        .chain(FIRST_HOST..=self.last_host)
                        .find(|host| !self.by_host.contains_key(host))
                    && self.claim_host(id, host)
                {
                    self.last_host = host;
                }
                if let Some(host) = self.host(id) {
                    self.send(id, dhcp_reply(mac, bootp, DHCP_OFFER, Some(host)));
                }
            }
            Some(DHCP_REQUEST) => {
                if server.is_some_and(|server| server != RELAY_IP) {
                    return; // took another server's offer
                }
                let reply = if self.claim(id, requested) {
                    dhcp_reply(mac, bootp, DHCP_ACK, self.host(id))
                } else {
                    dhcp_reply(mac, bootp, DHCP_NAK, None)
                };
                self.send(id, reply);
            }
            Some(DHCP_DECLINE | DHCP_RELEASE) => self.release(id),
            _ => {}
        }
    }
}

fn checksum(data: &[u8]) -> u16 {
    let mut sum = data
        .chunks(2)
        .map(|pair| {
            u32::from(u16::from_be_bytes([
                pair[0],
                pair.get(1).copied().unwrap_or_default(),
            ]))
        })
        .sum::<u32>();
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn ethernet(dst: &[u8], kind: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity((14 + payload.len()).max(60));
    frame.extend_from_slice(dst);
    frame.extend_from_slice(&RELAY_MAC);
    frame.extend_from_slice(&kind.to_be_bytes());
    frame.extend_from_slice(payload);
    frame.resize(frame.len().max(60), 0);
    frame
}

// The relay answers ARP for its own address, DHCP renewals are sent to it directly
fn arp_reply(request: &[u8]) -> Vec<u8> {
    let mut reply = [0; 28];
    reply[..6].copy_from_slice(&request[..6]); // hardware and protocol types and sizes
    reply[6..8].copy_from_slice(&[0, 2]);
    reply[8..14].copy_from_slice(&RELAY_MAC);
    reply[14..18].copy_from_slice(&RELAY_IP);
    reply[18..28].copy_from_slice(&request[8..18]); // the requester's MAC and IP
    ethernet(&request[8..14], ETH_ARP, &reply)
}

fn dhcp_reply(mac: [u8; 6], request: &[u8], kind: u8, host: Option<u16>) -> Vec<u8> {
    let mut options = vec![53, 1, kind, 54, 4];
    options.extend_from_slice(&RELAY_IP);
    if kind != DHCP_NAK {
        options.extend_from_slice(&[51, 4]);
        options.extend_from_slice(&LEASE_SECONDS.to_be_bytes());
        options.extend_from_slice(&[1, 4]);
        options.extend_from_slice(&SUBNET_MASK);
    }
    options.push(255);

    let mut bootp = vec![0; (240 + options.len()).max(300)];
    bootp[..3].copy_from_slice(&[2, 1, 6]);
    bootp[4..8].copy_from_slice(&request[4..8]); // transaction id
    bootp[10..12].copy_from_slice(&request[10..12]); // flags
    if let Some(host) = host {
        bootp[16..20].copy_from_slice(&host_ip(host));
    }
    bootp[20..24].copy_from_slice(&RELAY_IP);
    bootp[28..44].copy_from_slice(&request[28..44]); // client MAC
    bootp[236..240].copy_from_slice(&DHCP_MAGIC);
    bootp[240..240 + options.len()].copy_from_slice(&options);

    // To the broadcast address, the client has no address yet; UDP checksum 0 is none
    let length = 28 + bootp.len();
    let mut ip = Vec::with_capacity(length);
    ip.extend_from_slice(&[0x45, 0]);
    ip.extend_from_slice(&(length as u16).to_be_bytes());
    ip.extend_from_slice(&[0, 0, 0, 0, 64, 17, 0, 0]);
    ip.extend_from_slice(&RELAY_IP);
    ip.extend_from_slice(&[255; 4]);
    let header_checksum = checksum(&ip);
    ip[10..12].copy_from_slice(&header_checksum.to_be_bytes());
    ip.extend_from_slice(&67u16.to_be_bytes());
    ip.extend_from_slice(&68u16.to_be_bytes());
    ip.extend_from_slice(&((8 + bootp.len()) as u16).to_be_bytes());
    ip.extend_from_slice(&[0, 0]);
    ip.extend_from_slice(&bootp);
    ethernet(&mac, ETH_IPV4, &ip)
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    #[test]
    fn dhcp_allocates_and_reuses_the_full_subnet() {
        let mut switch = Switch::default();
        let (frames, replies) = mpsc::sync_channel(1);
        let mac = [2, 0, 0, 0, 0, 1];
        let mut discover = [0; 244];
        discover[0] = 1;
        discover[28..34].copy_from_slice(&mac);
        discover[236..240].copy_from_slice(&DHCP_MAGIC);
        discover[240..].copy_from_slice(&[53, 1, DHCP_DISCOVER, 255]);

        let address = |host| IpAddr::V4(Ipv4Addr::from(u32::from(host)));
        let mut first = None;
        for host in FIRST_HOST..=LAST_HOST {
            let id = switch.join(address(host), frames.clone()).unwrap();
            if host == FIRST_HOST {
                first = Some(id);
                for reserved in [[10, 86, 0, 0], RELAY_IP, [10, 86, 255, 255], [10, 87, 0, 2]] {
                    assert!(!switch.claim(id, &reserved));
                }
            }
            switch.dhcp(id, mac, &discover);
            let offer = replies.try_recv().unwrap();
            let bootp = &offer[14 + 20 + 8..];
            assert_eq!(bootp[16..20], host_ip(host));
            assert_eq!(bootp[255..261], [1, 4, 255, 255, 0, 0]);
            if host == 256 {
                assert_eq!(bootp[16..20], [10, 86, 1, 0]);
                assert!(!switch.claim(id, &[10, 86, 0, 2]));
            }
        }
        assert_eq!(switch.visitors.len(), 65_533);
        assert_eq!(switch.join(address(1), frames.clone()), None);

        switch.leave(first.unwrap());
        let id = switch.join(address(1), frames).unwrap();
        switch.dhcp(id, mac, &discover);
        assert_eq!(switch.host(id), Some(FIRST_HOST));
    }
}
