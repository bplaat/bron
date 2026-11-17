//! LAN relay: puts the Windows XP of every visitor on one virtual Ethernet segment, without internet access.
//!
//! v86 sends each Ethernet frame as one binary WebSocket message. The relay is a switch that forwards only IPv4 and
//! ARP between visitors, hands out 10.86.0.x addresses with its own DHCP server and drops frames that use another
//! visitor's MAC or IP address. Nothing leaves the LAN: there is no router and no DNS server. One task owns the
//! switch, every connection talks to it over channels.
//!
//! Environment: ORIGIN, the only page origin allowed to connect (any when unset), and TRUST_PROXY=true behind a
//! reverse proxy, the visitor's address is then the last X-Forwarded-For entry.

use std::collections::HashMap;
use std::env;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{self, MissedTickBehavior};

const MAX_FRAME: usize = 1536;
const QUEUE: usize = 256; // frames waiting for a slow visitor, more are dropped
const MAX_PER_ADDRESS: usize = 4; // browser tabs per visitor address
const MAX_VISITORS: usize = 253;
const LIMITS: [f64; 3] = [1024.0 * 1024.0, 1000.0, 100.0]; // bytes, frames and broadcasts per visitor per second
const PING_INTERVAL: Duration = Duration::from_secs(30);
const IDLE_TIMEOUT: Duration = Duration::from_secs(75);
const LEASE_SECONDS: u32 = 24 * 60 * 60;

const ETH_IPV4: u16 = 0x0800;
const ETH_ARP: u16 = 0x0806;
const SUBNET: [u8; 3] = [10, 86, 0]; // 10.86.0.0/24: the relay is .1, visitors get .2 to .254
const RELAY_IP: [u8; 4] = [10, 86, 0, 1];
const RELAY_MAC: [u8; 6] = [0x02, 0x86, 0x86, 0x86, 0x86, 0x01]; // locally administered
const DHCP_MAGIC: [u8; 4] = [0x63, 0x82, 0x53, 0x63];
const DHCP_DISCOVER: u8 = 1;
const DHCP_OFFER: u8 = 2;
const DHCP_REQUEST: u8 = 3;
const DHCP_DECLINE: u8 = 4;
const DHCP_ACK: u8 = 5;
const DHCP_NAK: u8 = 6;
const DHCP_RELEASE: u8 = 7;

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

enum Event {
    Join {
        address: IpAddr,
        frames: mpsc::Sender<Bytes>,
        id: oneshot::Sender<Option<u64>>,
    },
    Frame {
        id: u64,
        frame: Bytes,
    },
    Leave {
        id: u64,
    },
}

#[derive(Clone)]
pub(crate) struct Relay {
    events: mpsc::Sender<Event>,
    config: Arc<Config>,
}

impl Relay {
    pub(crate) fn spawn(config: Config) -> Self {
        let (events, receiver) = mpsc::channel(4096);
        tokio::spawn(Switch::default().run(receiver));
        Self {
            events,
            config: Arc::new(config),
        }
    }
}

pub(crate) async fn handler(
    upgrade: WebSocketUpgrade,
    State(relay): State<Relay>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let origin = headers.get(header::ORIGIN).and_then(|origin| origin.to_str().ok());
    if relay
        .config
        .origin
        .as_deref()
        .is_some_and(|allowed| origin != Some(allowed))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let forwarded = || {
        let header = headers.get("x-forwarded-for")?.to_str().ok()?;
        header.rsplit(',').next()?.trim().parse().ok()
    };
    let address = relay.config.trust_proxy.then(forwarded).flatten().unwrap_or(peer.ip());
    upgrade
        .max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| connection(socket, relay.events, address))
}

// Frames from the socket go to the switch, the switch's frames for this visitor go to the socket
async fn connection(mut socket: WebSocket, events: mpsc::Sender<Event>, address: IpAddr) {
    let (frames, mut queue) = mpsc::channel(QUEUE);
    let (reply, id) = oneshot::channel();
    if events
        .send(Event::Join {
            address,
            frames,
            id: reply,
        })
        .await
        .is_err()
    {
        return;
    }
    let Ok(Some(id)) = id.await else {
        let close = CloseFrame {
            code: close_code::POLICY,
            reason: "The LAN is full".into(),
        };
        socket.send(Message::Close(Some(close))).await.ok();
        return;
    };

    let mut ping = time::interval(PING_INTERVAL);
    ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut deadline = Instant::now() + IDLE_TIMEOUT;
    loop {
        tokio::select! {
            message = socket.recv() => match message {
                Some(Ok(Message::Binary(frame))) => {
                    deadline = Instant::now() + IDLE_TIMEOUT;
                    if events.send(Event::Frame { id, frame }).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => deadline = Instant::now() + IDLE_TIMEOUT, // pongs and pings
            },
            frame = queue.recv() => match frame {
                Some(frame) => {
                    if socket.send(Message::Binary(frame)).await.is_err() {
                        break;
                    }
                }
                None => break,
            },
            _ = ping.tick() => {
                if socket.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
            () = time::sleep_until(deadline.into()) => break, // the page went away without closing
        }
    }
    events.send(Event::Leave { id }).await.ok();
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
        if self.tokens.iter().zip(cost).any(|(tokens, cost)| *tokens < cost) {
            return false;
        }
        for (tokens, cost) in self.tokens.iter_mut().zip(cost) {
            *tokens -= cost;
        }
        true
    }
}

struct Visitor {
    frames: mpsc::Sender<Bytes>,
    address: IpAddr,
    mac: Option<[u8; 6]>,
    host: Option<u8>,
    budget: Budget,
}

#[derive(Default)]
struct Switch {
    visitors: HashMap<u64, Visitor>,
    by_mac: HashMap<[u8; 6], u64>,
    by_host: HashMap<u8, u64>,
    per_address: HashMap<IpAddr, usize>,
    next_id: u64,
}

impl Switch {
    async fn run(mut self, mut events: mpsc::Receiver<Event>) {
        while let Some(event) = events.recv().await {
            match event {
                Event::Join { address, frames, id } => {
                    id.send(self.join(address, frames)).ok();
                }
                Event::Frame { id, frame } => self.frame(id, &frame),
                Event::Leave { id } => self.leave(id),
            }
        }
    }

    fn join(&mut self, address: IpAddr, frames: mpsc::Sender<Bytes>) -> Option<u64> {
        let count = self.per_address.entry(address).or_default();
        if *count >= MAX_PER_ADDRESS || self.visitors.len() >= MAX_VISITORS {
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
    fn send(&self, id: u64, frame: Bytes) {
        if let Some(visitor) = self.visitors.get(&id) {
            visitor.frames.try_send(frame).ok();
        }
    }

    fn host(&self, id: u64) -> Option<u8> {
        self.visitors.get(&id)?.host
    }

    // The visitor's own address: kept, or taken when free, as after a relay restart the guest keeps its old lease
    fn claim(&mut self, id: u64, ip: &[u8]) -> bool {
        if ip.len() != 4 || ip[..3] != SUBNET {
            return false;
        }
        let host = ip[3];
        let Some(visitor) = self.visitors.get_mut(&id) else {
            return false;
        };
        if visitor.host == Some(host) {
            return true;
        }
        if visitor.host.is_some() || !(2..=254).contains(&host) || self.by_host.contains_key(&host) {
            return false;
        }
        visitor.host = Some(host);
        self.by_host.insert(host, id);
        true
    }

    fn release(&mut self, id: u64) {
        if let Some(host) = self.visitors.get_mut(&id).and_then(|visitor| visitor.host.take()) {
            self.by_host.remove(&host);
        }
    }

    fn frame(&mut self, id: u64, frame: &Bytes) {
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
                    visitor.frames.try_send(frame.clone()).ok();
                }
            }
        } else if let Some(&target) = self.by_mac.get(&dst)
            && target != id
        {
            self.send(target, frame.clone());
        }
    }

    fn dhcp(&mut self, id: u64, mac: [u8; 6], bootp: &[u8]) {
        if bootp.len() < 240 || bootp[0] != 1 || bootp[236..240] != DHCP_MAGIC || bootp[28..34] != mac {
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
                if self.host(id).is_none() && !self.claim(id, requested) {
                    let free = (2..=254).find(|host| !self.by_host.contains_key(host));
                    if let Some(host) = free {
                        self.claim(id, &[SUBNET[0], SUBNET[1], SUBNET[2], host]);
                    }
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
        .map(|pair| u32::from(u16::from_be_bytes([pair[0], pair.get(1).copied().unwrap_or_default()])))
        .sum::<u32>();
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn ethernet(dst: &[u8], kind: u16, payload: &[u8]) -> Bytes {
    let mut frame = Vec::with_capacity((14 + payload.len()).max(60));
    frame.extend_from_slice(dst);
    frame.extend_from_slice(&RELAY_MAC);
    frame.extend_from_slice(&kind.to_be_bytes());
    frame.extend_from_slice(payload);
    frame.resize(frame.len().max(60), 0);
    Bytes::from(frame)
}

// The relay answers ARP for its own address, DHCP renewals are sent to it directly
fn arp_reply(request: &[u8]) -> Bytes {
    let mut reply = [0; 28];
    reply[..6].copy_from_slice(&request[..6]); // hardware and protocol types and sizes
    reply[6..8].copy_from_slice(&[0, 2]);
    reply[8..14].copy_from_slice(&RELAY_MAC);
    reply[14..18].copy_from_slice(&RELAY_IP);
    reply[18..28].copy_from_slice(&request[8..18]); // the requester's MAC and IP
    ethernet(&request[8..14], ETH_ARP, &reply)
}

fn dhcp_reply(mac: [u8; 6], request: &[u8], kind: u8, host: Option<u8>) -> Bytes {
    let mut options = vec![53, 1, kind, 54, 4];
    options.extend_from_slice(&RELAY_IP);
    if kind != DHCP_NAK {
        options.extend_from_slice(&[51, 4]);
        options.extend_from_slice(&LEASE_SECONDS.to_be_bytes());
        options.extend_from_slice(&[1, 4, 255, 255, 255, 0]);
    }
    options.push(255);

    let mut bootp = vec![0; (240 + options.len()).max(300)];
    bootp[..3].copy_from_slice(&[2, 1, 6]);
    bootp[4..8].copy_from_slice(&request[4..8]); // transaction id
    bootp[10..12].copy_from_slice(&request[10..12]); // flags
    if let Some(host) = host {
        bootp[16..20].copy_from_slice(&[SUBNET[0], SUBNET[1], SUBNET[2], host]);
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
