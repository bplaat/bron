/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Nonblocking LAN I/O on its own thread, using the shared desktop wire format.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::platform::{Address, Io, Platform, Socket, Thread};
use super::sync::Mutex;
use crate::protocol::{
    self, Announcement, ClientFrame, DISCOVERY_PORT, Datagram, FRAME_HEADER, GAME_PORT, MAX_FRAME,
    Request, Response, ServerFrame, VERSION, validate_name,
};

// Connections stop reading while this many events wait, so TCP holds the rest back, lost
// frames would leave the game out of sync. Datagrams over the limit are dropped.
const MAX_EVENTS: usize = 256;
// How often a world open to the LAN announces itself.
const ANNOUNCE_INTERVAL: f64 = 1.5;

pub(super) enum Event {
    /// The world is open to the LAN.
    Hosted,
    Joined(u32, String),
    Request(u32, Request),
    Server(Box<ServerFrame>),
    Left(u32),
    Found(Address, Announcement),
    Status(String),
}

enum Command {
    Discover,
    Host(Announcement),
    Connect(Address, String),
    Client(Request),
    Server(u32, Box<ServerFrame>),
    Stop,
}

struct Shared {
    commands: VecDeque<Command>,
    events: VecDeque<Event>,
}

pub(super) struct Network<P: Platform> {
    shared: &'static Mutex<P::Lock, Shared>,
}

impl<P: Platform> Network<P> {
    pub(super) fn new() -> Self {
        let shared: &'static Mutex<P::Lock, Shared> = Box::leak(Box::new(Mutex::new(Shared {
            commands: VecDeque::new(),
            events: VecDeque::new(),
        })));
        P::spawn(Thread::Network, Box::new(move || run::<P>(shared)));
        Self { shared }
    }

    fn send(&self, command: Command) {
        self.shared.lock().commands.push_back(command);
    }

    pub(super) fn discover(&self) {
        self.send(Command::Discover);
    }

    pub(super) fn host(&self, world: &str, host: &str) {
        self.send(Command::Host(Announcement {
            port: GAME_PORT,
            world: world.into(),
            host: host.into(),
            players: 1,
        }));
    }

    pub(super) fn connect(&self, address: Address, name: &str) {
        self.send(Command::Connect(address, name.into()));
    }

    pub(super) fn request(&self, request: Request) {
        self.send(Command::Client(request));
    }

    pub(super) fn respond(&self, id: u32, frame: ServerFrame) {
        self.send(Command::Server(id, Box::new(frame)));
    }

    pub(super) fn broadcast(&self, response: Response) {
        self.respond(0, ServerFrame::Response(response));
    }

    pub(super) fn stop(&self) {
        self.send(Command::Stop);
    }

    pub(super) fn events(&self) -> VecDeque<Event> {
        core::mem::take(&mut self.shared.lock().events)
    }
}

/// Parses an IPv4 address with an optional port, like 192.168.1.5:47820.
pub(super) fn parse_address(text: &str) -> Option<Address> {
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
    Some(Address {
        ip: parts.try_into().ok()?,
        port,
    })
}

struct Connection<S> {
    socket: S,
    input: Vec<u8>,
    output: VecDeque<Vec<u8>>,
    written: usize,
    token: Option<u64>,
    joined: bool,
    alive: bool,
}

impl<S: Socket> Connection<S> {
    fn new(socket: S) -> Self {
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
        self.output.push_back(protocol::frame(bytes));
    }

    // Sends the queued frames and, when it receives, returns the complete received frames.
    fn poll(&mut self, receive: bool) -> Vec<Vec<u8>> {
        // Bound each poll so that a slow peer cannot starve the others.
        for _ in 0..8 {
            let Some(frame) = self.output.front() else {
                break;
            };
            let remaining = &frame[self.written..];
            match self.socket.send(&remaining[..remaining.len().min(8192)]) {
                Io::Wait => break,
                Io::Done(0) | Io::Failed => {
                    self.alive = false;
                    break;
                }
                Io::Done(sent) => {
                    self.written += sent;
                    if self.written == frame.len() {
                        self.output.pop_front();
                        self.written = 0;
                    }
                }
            }
        }
        let mut frames = Vec::new();
        if !receive {
            return frames;
        }
        let mut bytes = [0u8; 8192];
        for _ in 0..8 {
            let read = match self.socket.receive(&mut bytes) {
                Io::Wait => break,
                Io::Done(0) | Io::Failed => {
                    self.alive = false;
                    break;
                }
                Io::Done(read) => read,
            };
            self.input.extend(&bytes[..read]);
            if self.input.len() > MAX_FRAME + FRAME_HEADER {
                self.alive = false;
                break;
            }
            while let Some(header) = self.input.first_chunk::<FRAME_HEADER>() {
                let Ok(size) = protocol::frame_length(*header) else {
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

fn event<L: super::platform::Lock>(shared: &Mutex<L, Shared>, event: Event) {
    shared.lock().events.push_back(event);
}

// An event of a datagram, which is dropped when too many events wait.
fn datagram_event<L: super::platform::Lock>(shared: &Mutex<L, Shared>, event: Event) {
    let mut state = shared.lock();
    if state.events.len() < MAX_EVENTS {
        state.events.push_back(event);
    }
}

fn udp<S: Socket>(port: u16) -> Option<S> {
    let socket = S::udp()?;
    if !socket.bind(port) {
        return None;
    }
    socket.nonblocking();
    Some(socket)
}

// A world open to the LAN: the listening socket and the UDP socket of the guests' inputs.
struct Host<S> {
    listener: S,
    inputs: S,
    announcement: Announcement,
}

fn host<S: Socket>(backlog: usize) -> Option<(S, S)> {
    let listener = S::tcp()?;
    if !listener.bind(GAME_PORT) || !listener.listen(backlog) {
        return None;
    }
    listener.nonblocking();
    // Guests send their inputs to the UDP port of the same number
    Some((listener, udp(GAME_PORT)?))
}

fn run<P: Platform>(shared: &Mutex<P::Lock, Shared>) {
    let mut ready = false;
    let mut discovery: Option<P::Socket> = None;
    let mut hosted: Option<Host<P::Socket>> = None;
    let mut connections: BTreeMap<u32, Connection<P::Socket>> = BTreeMap::new();
    let mut next_id = 1;
    let mut last_announce = 0.0;
    loop {
        let commands = core::mem::take(&mut shared.lock().commands);
        for command in commands {
            if !ready && !matches!(command, Command::Stop) {
                match P::connect_network() {
                    Ok([a, b, c, d]) => {
                        ready = true;
                        event(
                            shared,
                            Event::Status(format!("{} address: {a}.{b}.{c}.{d}", P::NAME)),
                        );
                    }
                    Err(error) => {
                        event(
                            shared,
                            Event::Status(format!(
                                "Could not initialize {} networking ({error})",
                                P::NAME
                            )),
                        );
                        continue;
                    }
                }
            }
            match command {
                Command::Stop => {
                    for connection in connections.values_mut() {
                        connection.poll(false);
                    }
                    hosted = None;
                    connections.clear();
                }
                Command::Discover => {
                    if discovery.is_none() {
                        discovery = udp(DISCOVERY_PORT);
                    }
                }
                Command::Host(announcement) => {
                    if hosted.is_none()
                        && let Some((listener, inputs)) = host(P::MAX_GUESTS)
                    {
                        hosted = Some(Host {
                            listener,
                            inputs,
                            announcement,
                        });
                    }
                    event(
                        shared,
                        if hosted.is_some() {
                            Event::Hosted
                        } else {
                            Event::Status("Could not open the LAN port".into())
                        },
                    );
                }
                Command::Connect(address, name) => {
                    connections.clear();
                    hosted = None;
                    match P::Socket::tcp().filter(|socket| socket.connect(address)) {
                        Some(socket) => {
                            let mut connection = Connection::new(socket);
                            connection.queue(
                                &ClientFrame::Hello {
                                    version: VERSION,
                                    name,
                                }
                                .encode(),
                            );
                            connections.insert(0, connection);
                        }
                        None => event(
                            shared,
                            Event::Status("Could not connect to the server".into()),
                        ),
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
        if let Some(hosted) = &hosted
            && connections.len() < P::MAX_GUESTS
            && let Some(socket) = hosted.listener.accept()
        {
            connections.insert(next_id, Connection::new(socket));
            next_id += 1;
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
                                && validate_name(&name).is_ok() =>
                        {
                            connection.joined = true;
                            event(shared, Event::Joined(id, name));
                        }
                        Ok(ClientFrame::Request(request)) if connection.joined => {
                            event(shared, Event::Request(id, request));
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
        let sockets = [
            (discovery.as_ref(), true),
            (hosted.as_ref().map(|hosted| &hosted.inputs), false),
        ];
        for (socket, is_discovery) in sockets {
            let Some(socket) = socket else {
                continue;
            };
            for _ in 0..16 {
                let Some((size, from)) = socket.receive_from(&mut buffer) else {
                    break;
                };
                match Datagram::decode(&buffer[..size]) {
                    Ok(Datagram::Announce {
                        version,
                        announcement,
                    }) if is_discovery && version == VERSION => {
                        let from = Address {
                            port: announcement.port,
                            ..from
                        };
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
        let now = P::seconds();
        if now - last_announce > ANNOUNCE_INTERVAL
            && let Some(hosted) = &hosted
        {
            let mut announcement = hosted.announcement.clone();
            announcement.players = connections.len() as u8 + 1;
            let bytes = Datagram::Announce {
                version: VERSION,
                announcement,
            }
            .encode();
            hosted.inputs.broadcast();
            let everyone = Address {
                ip: [255; 4],
                port: DISCOVERY_PORT,
            };
            hosted.inputs.send_to(&bytes, everyone);
            last_announce = now;
        }
        // A video frame is frequent enough for the 20 Hz game state.
        P::idle();
    }
}
