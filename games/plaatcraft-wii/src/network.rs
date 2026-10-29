/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Nonblocking LAN I/O on a dedicated LWP thread, using the shared desktop wire format.
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use libogc_sys::*;
use plaatcraft_engine::protocol::{
    Announcement, ClientFrame, DISCOVERY_PORT, Datagram, FRAME_HEADER, GAME_PORT, MAX_FRAME,
    Request, Response, ServerFrame, VERSION,
};

use crate::sync::Mutex;

const ADDRESS_LENGTH: u32 = 16;
// The most hosted guests, and the TCP listen backlog
pub const MAX_CONNECTIONS: usize = 3;
// Connections stop reading while this many events wait, so TCP holds the rest back, lost
// frames would leave the game out of sync. Datagrams over the limit are dropped.
const MAX_EVENTS: usize = 256;

pub enum Event {
    /// The world is open to the LAN.
    Hosted,
    Joined(u32, String),
    Request(u32, Request),
    Server(Box<ServerFrame>),
    Left(u32),
    Found(SocketAddress, Announcement),
    Status(String),
}
enum Command {
    Discover,
    Host(Announcement),
    Connect(SocketAddress, String),
    Client(Request),
    Server(u32, Box<ServerFrame>),
    Stop,
}
#[derive(Default)]
struct Shared {
    commands: VecDeque<Command>,
    events: VecDeque<Event>,
}
pub struct Network {
    shared: &'static Mutex<Shared>,
}
impl Network {
    pub fn new() -> Self {
        let shared: &'static Mutex<Shared> = Box::leak(Box::new(Mutex::new(Shared::default())));
        crate::sync::spawn(256 * 1024, 45, move || run(shared));
        Self { shared }
    }
    fn send(&self, command: Command) {
        self.shared.lock().commands.push_back(command);
    }
    pub fn discover(&self) {
        self.send(Command::Discover);
    }
    pub fn host(&self, world: &str, host: &str) {
        self.send(Command::Host(Announcement {
            port: GAME_PORT,
            world: world.into(),
            host: host.into(),
            players: 1,
        }));
    }
    pub fn connect(&self, address: SocketAddress, name: &str) {
        self.send(Command::Connect(address, name.into()));
    }
    pub fn request(&self, request: Request) {
        self.send(Command::Client(request));
    }
    pub fn respond(&self, id: u32, frame: ServerFrame) {
        self.send(Command::Server(id, Box::new(frame)));
    }
    pub fn broadcast(&self, response: Response) {
        self.respond(0, ServerFrame::Response(response));
    }
    pub fn stop(&self) {
        self.send(Command::Stop);
    }
    pub fn events(&self) -> VecDeque<Event> {
        core::mem::take(&mut self.shared.lock().events)
    }
}

pub fn address(ip: [u8; 4], port: u16) -> SocketAddress {
    SocketAddress {
        length: ADDRESS_LENGTH as u8,
        family: AF_INET,
        port: port.to_be(),
        address: u32::from_ne_bytes(ip),
        padding: [0; 8],
    }
}
pub fn parse_address(text: &str) -> Option<SocketAddress> {
    let (host, port) = text
        .trim()
        .split_once(':')
        .map_or((text.trim(), GAME_PORT), |(host, port)| {
            (host, port.parse().unwrap_or(0))
        });
    if port == 0 {
        return None;
    }
    let parts: Vec<_> = host
        .split('.')
        .map(str::parse::<u8>)
        .collect::<Result<_, _>>()
        .ok()?;
    Some(address(parts.try_into().ok()?, port))
}

struct Socket(i32);
impl Socket {
    fn new(kind: u32) -> Option<Self> {
        let fd = unsafe { net_socket(AF_INET.into(), kind, 0) };
        (fd >= 0).then_some(Self(fd))
    }
    fn nonblocking(&self) {
        unsafe {
            net_fcntl(self.0, F_SETFL, IOS_O_NONBLOCK);
        }
    }
    fn enable(&self, option: u32) {
        let enabled = 1u32;
        unsafe {
            net_setsockopt(self.0, SOL_SOCKET, option, (&raw const enabled).cast(), 4);
        }
    }
    fn bind(&self, port: u16) -> bool {
        self.enable(SO_REUSEADDR);
        unsafe { net_bind(self.0, &mut address([0; 4], port), ADDRESS_LENGTH) >= 0 }
    }
    fn datagram(&self, bytes: &[u8], address: &SocketAddress) {
        let mut address = *address;
        unsafe {
            net_sendto(
                self.0,
                bytes.as_ptr().cast(),
                bytes.len() as i32,
                MSG_DONTWAIT,
                &mut address,
                ADDRESS_LENGTH,
            );
        }
    }
}
impl Drop for Socket {
    fn drop(&mut self) {
        unsafe {
            net_close(self.0);
        }
    }
}
struct Connection {
    socket: Socket,
    input: Vec<u8>,
    output: VecDeque<Vec<u8>>,
    written: usize,
    token: Option<u64>,
    joined: bool,
    alive: bool,
}
impl Connection {
    fn new(socket: Socket) -> Self {
        socket.nonblocking();
        Self {
            socket,
            input: Vec::new(),
            output: VecDeque::new(),
            written: 0,
            token: None,
            joined: false,
            alive: true,
        }
    }
    fn queue(&mut self, bytes: &[u8]) {
        if self.output.iter().map(Vec::len).sum::<usize>() + bytes.len() > MAX_FRAME / 2 {
            self.alive = false;
            return;
        }
        self.output
            .push_back(plaatcraft_engine::protocol::frame(bytes));
    }
    // Sends the queued frames and, when it receives, returns the complete received frames.
    fn poll(&mut self, receive: bool) -> Vec<Vec<u8>> {
        // Bound each poll so that a slow peer cannot starve the others.
        for _ in 0..8 {
            let Some(frame) = self.output.front() else {
                break;
            };
            let remaining = &frame[self.written..];
            let sent = unsafe {
                net_send(
                    self.socket.0,
                    remaining.as_ptr().cast(),
                    remaining.len().min(8192) as i32,
                    MSG_DONTWAIT,
                )
            };
            if sent < 0 {
                if sent != -EAGAIN {
                    self.alive = false;
                }
                break;
            }
            if sent == 0 {
                self.alive = false;
                break;
            }
            self.written += sent as usize;
            if self.written == frame.len() {
                self.output.pop_front();
                self.written = 0;
            }
        }
        let mut frames = Vec::new();
        if !receive {
            return frames;
        }
        let mut bytes = [0u8; 8192];
        for _ in 0..8 {
            let read = unsafe {
                net_recv(
                    self.socket.0,
                    bytes.as_mut_ptr().cast(),
                    bytes.len() as i32,
                    MSG_DONTWAIT,
                )
            };
            if read < 0 {
                if read != -EAGAIN {
                    self.alive = false;
                }
                break;
            }
            if read == 0 {
                self.alive = false;
                break;
            }
            self.input.extend(&bytes[..read as usize]);
            if self.input.len() > MAX_FRAME + FRAME_HEADER {
                self.alive = false;
                break;
            }
            while let Some(header) = self.input.first_chunk::<FRAME_HEADER>() {
                let Ok(size) = plaatcraft_engine::protocol::frame_length(*header) else {
                    self.alive = false;
                    break;
                };
                let end = FRAME_HEADER + size;
                if size == 0 || self.input.len() < end {
                    self.alive &= size != 0;
                    break;
                }
                frames.push(self.input[FRAME_HEADER..end].to_vec());
                self.input.drain(..end);
            }
        }
        frames
    }
}
fn event(shared: &Mutex<Shared>, event: Event) {
    shared.lock().events.push_back(event);
}
// An event of a datagram, which is dropped when too many events wait.
fn datagram_event(shared: &Mutex<Shared>, event: Event) {
    let mut state = shared.lock();
    if state.events.len() < MAX_EVENTS {
        state.events.push_back(event);
    }
}
fn udp(port: u16) -> Option<Socket> {
    let socket = Socket::new(SOCK_DGRAM)?;
    if !socket.bind(port) {
        return None;
    }
    socket.nonblocking();
    Some(socket)
}
// Poll the async initializer so a callback cannot wake a thread before it sleeps.
static INITIALIZING: AtomicBool = AtomicBool::new(false);
static INITIALIZED: AtomicI32 = AtomicI32::new(i32::MIN);
extern "C" fn initialized(result: i32, _argument: *mut core::ffi::c_void) -> i32 {
    INITIALIZED.store(result, Ordering::Release);
    0
}
fn initialize() -> Result<u32, i32> {
    if !INITIALIZING.swap(true, Ordering::AcqRel) {
        INITIALIZED.store(i32::MIN, Ordering::Release);
        let result = unsafe { net_init_async(Some(initialized), ptr::null_mut()) };
        if result < 0 {
            INITIALIZED.store(result, Ordering::Release);
        }
    }
    let start = unsafe { gettime() };
    while INITIALIZED.load(Ordering::Acquire) == i32::MIN {
        if unsafe { gettime() }.wrapping_sub(start) > 10 * 60_750_000 {
            return Err(-110);
        }
        unsafe {
            VIDEO_WaitVSync();
        }
    }
    let result = INITIALIZED.load(Ordering::Acquire);
    if result < 0 {
        INITIALIZING.store(false, Ordering::Release);
        return Err(result);
    }
    let mut address = 0;
    let result = unsafe { if_configex(&mut address, ptr::null_mut(), ptr::null_mut(), true, 20) };
    if result < 0 { Err(result) } else { Ok(address) }
}
fn run(shared: &Mutex<Shared>) {
    let mut ready = false;
    let mut discovery = None;
    let mut game_udp = None;
    let mut listener = None;
    let mut announcement: Option<Announcement> = None;
    let mut connections: BTreeMap<u32, Connection> = BTreeMap::new();
    let mut next_id = 1;
    let mut last_announce = 0;
    loop {
        let commands = core::mem::take(&mut shared.lock().commands);
        for command in commands {
            if !ready && !matches!(command, Command::Stop) {
                let ip = match initialize() {
                    Ok(ip) => ip,
                    Err(error) => {
                        event(
                            shared,
                            Event::Status(format!("Could not initialize Wii networking ({error})")),
                        );
                        continue;
                    }
                };
                ready = true;
                let octets = ip.to_ne_bytes();
                event(
                    shared,
                    Event::Status(format!(
                        "Wii address: {}.{}.{}.{}",
                        octets[0], octets[1], octets[2], octets[3]
                    )),
                );
            }
            match command {
                Command::Stop => {
                    for connection in connections.values_mut() {
                        connection.poll(false);
                    }
                    listener = None;
                    game_udp = None;
                    connections.clear();
                    announcement = None;
                }
                Command::Discover => {
                    if discovery.is_none() {
                        discovery = udp(DISCOVERY_PORT);
                    }
                }
                Command::Host(info) => {
                    // Guests send their inputs to the UDP port of the same number
                    let opened = listener.is_some()
                        || Socket::new(SOCK_STREAM)
                            .filter(|socket| {
                                socket.bind(GAME_PORT)
                                    && unsafe { net_listen(socket.0, MAX_CONNECTIONS as u32) } >= 0
                            })
                            .zip(udp(GAME_PORT))
                            .map(|(socket, udp)| {
                                socket.nonblocking();
                                listener = Some(socket);
                                game_udp = Some(udp);
                                announcement = Some(info);
                            })
                            .is_some();
                    event(
                        shared,
                        if opened {
                            Event::Hosted
                        } else {
                            Event::Status("Could not open the LAN port".into())
                        },
                    );
                }
                Command::Connect(mut address, name) => {
                    connections.clear();
                    listener = None;
                    game_udp = None;
                    announcement = None;
                    if let Some(socket) = Socket::new(SOCK_STREAM) {
                        if unsafe { net_connect(socket.0, &mut address, ADDRESS_LENGTH) } >= 0 {
                            let mut connection = Connection::new(socket);
                            connection.queue(
                                &ClientFrame::Hello {
                                    version: VERSION,
                                    name,
                                }
                                .encode(),
                            );
                            connections.insert(0, connection);
                        } else {
                            event(
                                shared,
                                Event::Status("Could not connect to the server".into()),
                            );
                        }
                    }
                }
                Command::Client(request) => {
                    if let Some(connection) = connections.get_mut(&0) {
                        connection.queue(&ClientFrame::Request(request).encode());
                    }
                }
                Command::Server(id, frame) => {
                    if let ServerFrame::Accepted { token, .. } = frame.as_ref()
                        && let Some(connection) = connections.get_mut(&id)
                    {
                        connection.token = Some(*token);
                    }
                    // Broadcasts only go to accepted guests, they first need their welcome
                    let bytes = frame.encode();
                    for (&peer, connection) in &mut connections {
                        if (id == 0 && connection.token.is_some()) || peer == id {
                            connection.queue(&bytes);
                        }
                    }
                }
            }
        }
        if let Some(listener) = &listener
            && connections.len() < MAX_CONNECTIONS
        {
            let mut from = SocketAddress::default();
            let mut len = ADDRESS_LENGTH;
            let fd = unsafe { net_accept(listener.0, &mut from, &mut len) };
            if fd >= 0 {
                connections.insert(next_id, Connection::new(Socket(fd)));
                next_id += 1;
            }
        }
        let receive = shared.lock().events.len() < MAX_EVENTS;
        for (&id, connection) in &mut connections {
            for frame in connection.poll(receive) {
                if id == 0 {
                    match ServerFrame::decode(&frame) {
                        Ok(frame) => event(shared, Event::Server(Box::new(frame))),
                        Err(_) => connection.alive = false,
                    }
                } else {
                    match ClientFrame::decode(&frame) {
                        Ok(ClientFrame::Hello { version, name })
                            if !connection.joined
                                && version == VERSION
                                && plaatcraft_engine::protocol::validate_name(&name).is_ok() =>
                        {
                            connection.joined = true;
                            event(shared, Event::Joined(id, name));
                        }
                        Ok(ClientFrame::Request(request)) if connection.joined => {
                            event(shared, Event::Request(id, request))
                        }
                        _ => connection.alive = false,
                    }
                }
            }
        }
        connections.retain(|&id, connection| {
            if !connection.alive {
                event(shared, Event::Left(id));
            }
            connection.alive
        });
        let mut buffer = [0u8; 2048];
        for (socket, is_discovery) in [(&discovery, true), (&game_udp, false)] {
            let Some(socket) = socket else {
                continue;
            };
            for _ in 0..16 {
                let mut from = SocketAddress::default();
                let mut len = ADDRESS_LENGTH;
                let size = unsafe {
                    net_recvfrom(
                        socket.0,
                        buffer.as_mut_ptr().cast(),
                        buffer.len() as i32,
                        MSG_DONTWAIT,
                        &mut from,
                        &mut len,
                    )
                };
                if size <= 0 {
                    break;
                }
                match Datagram::decode(&buffer[..size as usize]) {
                    Ok(Datagram::Announce {
                        version,
                        announcement,
                    }) if is_discovery && version == VERSION => {
                        from.port = announcement.port.to_be();
                        datagram_event(shared, Event::Found(from, announcement));
                    }
                    Ok(Datagram::Inputs { id, token, inputs })
                        if !is_discovery
                            && connections.get(&id).is_some_and(|c| c.token == Some(token)) =>
                    {
                        for (sequence, controls) in inputs {
                            datagram_event(
                                shared,
                                Event::Request(id, Request::Input { sequence, controls }),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        let now = unsafe { gettime() };
        if now.wrapping_sub(last_announce) > 90_000_000
            && let (Some(info), Some(socket)) = (&announcement, &game_udp)
        {
            let mut info = info.clone();
            info.players = connections.len() as u8 + 1;
            let bytes = Datagram::Announce {
                version: VERSION,
                announcement: info,
            }
            .encode();
            socket.enable(SO_BROADCAST);
            socket.datagram(&bytes, &address([255; 4], DISCOVERY_PORT));
            last_announce = now;
        }
        // A video-frame poll is frequent enough for the 20 Hz game state.
        unsafe {
            VIDEO_WaitVSync();
        }
    }
}
