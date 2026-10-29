/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Playing over the LAN, like Minecraft's Open to LAN. The host accepts guests over TCP,
//! receives their inputs and sends their states over UDP and announces the world with UDP
//! broadcasts. Guests
//! connect with [`Remote`] and find worlds with [`Discovery`].

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, RandomState};
use std::io;
use std::net::{
    Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::protocol::{
    Announcement, ClientFrame, Controls, DISCOVERY_PORT, Datagram, GAME_PORT, REPEATED_INPUTS,
    ServerFrame, VERSION, read_frame, write_frame,
};
use crate::server::{self, ClientId, Identity, Message, Request, Response, Welcome};

const POLL: Duration = Duration::from_millis(100);
const ANNOUNCE_INTERVAL: Duration = Duration::from_millis(1500);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const JOIN_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_DATAGRAM: usize = 2048;
// Leaving waits this long at most for the last requests, like the save, to go out.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

// The guests' secret tokens, which their pose datagrams must carry, and the address their
// datagrams come from.
type Guests = Arc<Mutex<HashMap<ClientId, (u64, Option<SocketAddr>)>>>;

fn random() -> u64 {
    RandomState::new().hash_one(Instant::now())
}

/// A world opened to the LAN, dropping it stops accepting and announcing.
pub(crate) struct Lan {
    pub(crate) port: u16,
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}

impl Lan {
    pub(crate) fn open(
        messages: Sender<Message>,
        world: String,
        host: String,
        players: Arc<AtomicUsize>,
    ) -> io::Result<Self> {
        // Another world on this computer can have the game port, then any port works.
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, GAME_PORT))
            .or_else(|_| TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)))?;
        let port = listener.local_addr()?.port();
        let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))
            .or_else(|_| UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)))?;
        let udp_port = udp.local_addr()?.port();
        listener.set_nonblocking(true)?;
        udp.set_read_timeout(Some(POLL))?;
        let announcer = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        announcer.set_broadcast(true)?;

        let stop = Arc::new(AtomicBool::new(false));
        let guests = Guests::default();
        let spawn = |name: &str, run: Box<dyn FnOnce() + Send>| {
            thread::Builder::new().name(name.to_owned()).spawn(run)
        };
        let accepting = {
            let (stop, guests, udp) = (stop.clone(), guests.clone(), udp.try_clone()?);
            let messages = messages.clone();
            spawn(
                "lan-accept",
                Box::new(move || {
                    while !stop.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                let (messages, guests) = (messages.clone(), guests.clone());
                                let Ok(udp) = udp.try_clone() else { continue };
                                _ = thread::Builder::new().name("lan-guest".to_owned()).spawn(
                                    move || serve_guest(stream, &messages, &guests, udp, udp_port),
                                );
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                thread::sleep(POLL);
                            }
                            Err(error) => {
                                eprintln!("Could not accept a LAN player: {error}");
                                thread::sleep(POLL);
                            }
                        }
                    }
                }),
            )?
        };
        let inputs = {
            let (stop, guests) = (stop.clone(), guests);
            spawn(
                "lan-inputs",
                Box::new(move || {
                    let mut buffer = [0; MAX_DATAGRAM];
                    while !stop.load(Ordering::Relaxed) {
                        let Ok((size, from)) = udp.recv_from(&mut buffer) else {
                            continue;
                        };
                        let Ok(Datagram::Inputs { id, token, inputs }) =
                            Datagram::decode(&buffer[..size])
                        else {
                            continue;
                        };
                        if let Some((secret, address)) =
                            guests.lock().expect("Guests poisoned").get_mut(&id)
                            && *secret == token
                        {
                            *address = Some(from);
                            // The server skips the repeated inputs it already has.
                            for (sequence, controls) in inputs {
                                let input = Request::Input { sequence, controls };
                                _ = messages.send(Message::Request(id, input));
                            }
                        }
                    }
                }),
            )?
        };
        let announcing = {
            let stop = stop.clone();
            spawn(
                "lan-announce",
                Box::new(move || {
                    let mut next = Instant::now();
                    while !stop.load(Ordering::Relaxed) {
                        if Instant::now() >= next {
                            let datagram = Datagram::Announce {
                                version: VERSION,
                                announcement: Announcement {
                                    port,
                                    world: world.clone(),
                                    host: host.clone(),
                                    players: players.load(Ordering::Relaxed).min(255) as u8,
                                },
                            }
                            .encode();
                            // Broadcasts don't always loop back, so this computer is told too.
                            for address in [Ipv4Addr::BROADCAST, Ipv4Addr::LOCALHOST] {
                                _ = announcer.send_to(&datagram, (address, DISCOVERY_PORT));
                            }
                            next = Instant::now() + ANNOUNCE_INTERVAL;
                        }
                        thread::sleep(POLL);
                    }
                }),
            )?
        };
        Ok(Self {
            port,
            stop,
            threads: vec![accepting, inputs, announcing],
        })
    }
}

impl Drop for Lan {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            if thread.join().is_err() {
                eprintln!("LAN thread failed during shutdown");
            }
        }
    }
}

fn reject(stream: &mut TcpStream, reason: &str) {
    _ = write_frame(stream, &ServerFrame::Rejected(reason.to_owned()).encode());
    _ = stream.shutdown(Shutdown::Both);
}

// Lets a guest join, then forwards its requests to the server on this thread and the
// server's responses to it on a writer thread, the poses over UDP.
fn serve_guest(
    mut stream: TcpStream,
    messages: &Sender<Message>,
    guests: &Guests,
    udp: UdpSocket,
    udp_port: u16,
) {
    if stream.set_nonblocking(false).is_err()
        || stream.set_nodelay(true).is_err()
        || stream.set_read_timeout(Some(JOIN_TIMEOUT)).is_err()
    {
        return;
    }
    let hello = read_frame(&mut stream)
        .ok()
        .and_then(|frame| ClientFrame::decode(&frame).ok());
    let Some(ClientFrame::Hello { version, name }) = hello else {
        return reject(&mut stream, "Not a PlaatCraft player");
    };
    if version != VERSION {
        return reject(
            &mut stream,
            "The world runs a different version of PlaatCraft",
        );
    }
    let (responses, outgoing) = mpsc::channel();
    let welcome = match server::join(messages, Identity { name }, responses) {
        Ok(welcome) => welcome,
        Err(reason) => return reject(&mut stream, &reason),
    };
    let id = welcome.id;
    let token = random();
    guests
        .lock()
        .expect("Guests poisoned")
        .insert(id, (token, None));
    let accepted = ServerFrame::Accepted {
        welcome,
        token,
        udp_port,
    };
    let writer = stream.try_clone();
    if write_frame(&mut stream, &accepted.encode()).is_ok()
        && stream.set_read_timeout(None).is_ok()
        && let Ok(mut writer) = writer
    {
        let guests_writer = guests.clone();
        _ = thread::Builder::new()
            .name("lan-writer".to_owned())
            .spawn(move || {
                // Ends when the server lets go of the guest. States go over UDP once the
                // guest's address is known.
                for response in outgoing {
                    let address = guests_writer
                        .lock()
                        .expect("Guests poisoned")
                        .get(&id)
                        .and_then(|&(_, address)| address);
                    if let (
                        Response::State {
                            sequence,
                            body,
                            others,
                        },
                        Some(address),
                    ) = (&response, address)
                    {
                        let datagram = Datagram::State {
                            sequence: *sequence,
                            body: *body,
                            others: others.clone(),
                        };
                        _ = udp.send_to(&datagram.encode(), address);
                    } else if write_frame(&mut writer, &ServerFrame::Response(response).encode())
                        .is_err()
                    {
                        break;
                    }
                }
                _ = writer.shutdown(Shutdown::Both);
            });
        while let Ok(frame) = read_frame(&mut stream) {
            let Ok(ClientFrame::Request(request)) = ClientFrame::decode(&frame) else {
                break;
            };
            if messages.send(Message::Request(id, request)).is_err() {
                break;
            }
        }
    }
    _ = messages.send(Message::Leave(id));
    guests.lock().expect("Guests poisoned").remove(&id);
    _ = stream.shutdown(Shutdown::Both);
}

// The address of a world, the game port is used when it has none.
fn resolve(address: &str) -> Result<SocketAddr, String> {
    let address = address.trim();
    let mut found = address
        .to_socket_addrs()
        .or_else(|_| (address, GAME_PORT).to_socket_addrs())
        .or_else(|_| {
            // IPv6 addresses without a port have no brackets.
            let bare = address.trim_start_matches('[').trim_end_matches(']');
            (bare, GAME_PORT).to_socket_addrs()
        })
        .map_err(|_| format!("Unknown address {address}"))?;
    found
        .next()
        .ok_or_else(|| format!("Unknown address {address}"))
}

/// A guest's connection to a world on the LAN.
pub(crate) struct Remote {
    requests: Option<Sender<Request>>,
    // The last inputs, which every input datagram repeats.
    inputs: RefCell<VecDeque<(u32, Controls)>>,
    writer: Option<thread::JoinHandle<()>>,
    stream: TcpStream,
    udp: UdpSocket,
    id: ClientId,
    token: u64,
    stop: Arc<AtomicBool>,
}

impl Remote {
    pub(crate) fn connect(
        address: &str,
        identity: &Identity,
    ) -> Result<(Self, Receiver<Response>, Welcome), String> {
        let address = resolve(address)?;
        let failed = |error: io::Error| format!("Could not connect to {address}: {error}");
        let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).map_err(failed)?;
        stream.set_nodelay(true).map_err(failed)?;
        stream
            .set_read_timeout(Some(JOIN_TIMEOUT))
            .map_err(failed)?;
        stream
            .set_write_timeout(Some(WRITE_TIMEOUT))
            .map_err(failed)?;
        let hello = ClientFrame::Hello {
            version: VERSION,
            name: identity.name.clone(),
        };
        write_frame(&mut stream, &hello.encode()).map_err(failed)?;
        let frame = read_frame(&mut stream).map_err(failed)?;
        let (welcome, token, udp_port) = match ServerFrame::decode(&frame)? {
            ServerFrame::Accepted {
                welcome,
                token,
                udp_port,
            } => (welcome, token, udp_port),
            ServerFrame::Rejected(reason) => return Err(reason),
            ServerFrame::Response(_) => return Err("The world did not welcome you".into()),
        };
        stream.set_read_timeout(None).map_err(failed)?;
        let udp = match address {
            SocketAddr::V4(_) => UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)),
            SocketAddr::V6(_) => UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0)),
        }
        .and_then(|udp| {
            udp.connect((address.ip(), udp_port))?;
            udp.set_read_timeout(Some(POLL))?;
            Ok(udp)
        })
        .map_err(failed)?;

        let stop = Arc::new(AtomicBool::new(false));
        let (responses, received) = mpsc::channel();
        let (requests, outgoing) = mpsc::channel::<Request>();
        let spawned = (|| -> io::Result<thread::JoinHandle<()>> {
            let (mut reader, read) = (stream.try_clone()?, responses.clone());
            thread::Builder::new()
                .name("lan-reader".to_owned())
                .spawn(move || {
                    let reason = loop {
                        let frame = match read_frame(&mut reader) {
                            Ok(frame) => frame,
                            Err(_) => break "Lost the connection to the world".to_owned(),
                        };
                        match ServerFrame::decode(&frame) {
                            Ok(ServerFrame::Response(Response::Disconnected(reason))) => {
                                break reason;
                            }
                            Ok(ServerFrame::Response(response)) => {
                                if read.send(response).is_err() {
                                    return;
                                }
                            }
                            _ => break "The world sent an invalid message".to_owned(),
                        }
                    };
                    _ = read.send(Response::Disconnected(reason));
                })?;
            let mut writer = stream.try_clone()?;
            let writing =
                thread::Builder::new()
                    .name("lan-writer".to_owned())
                    .spawn(move || {
                        for request in outgoing {
                            if write_frame(&mut writer, &ClientFrame::Request(request).encode())
                                .is_err()
                            {
                                break;
                            }
                        }
                    })?;
            let (receiver, stop) = (udp.try_clone()?, stop.clone());
            thread::Builder::new()
                .name("lan-states".to_owned())
                .spawn(move || {
                    let mut buffer = [0; MAX_DATAGRAM];
                    while !stop.load(Ordering::Relaxed) {
                        let Ok(size) = receiver.recv(&mut buffer) else {
                            continue;
                        };
                        if let Ok(Datagram::State {
                            sequence,
                            body,
                            others,
                        }) = Datagram::decode(&buffer[..size])
                        {
                            let state = Response::State {
                                sequence,
                                body,
                                others,
                            };
                            if responses.send(state).is_err() {
                                break;
                            }
                        }
                    }
                })?;
            Ok(writing)
        })();
        let writer = spawned.map_err(failed)?;
        let remote = Self {
            requests: Some(requests),
            inputs: RefCell::default(),
            writer: Some(writer),
            stream,
            udp,
            id: welcome.id,
            token,
            stop,
        };
        Ok((remote, received, welcome))
    }

    pub(crate) fn send(&self, request: Request) {
        if let Request::Input { sequence, controls } = request {
            let mut inputs = self.inputs.borrow_mut();
            if inputs.len() == REPEATED_INPUTS {
                inputs.pop_front();
            }
            inputs.push_back((sequence, controls));
            let datagram = Datagram::Inputs {
                id: self.id,
                token: self.token,
                inputs: inputs.iter().copied().collect(),
            };
            _ = self.udp.send(&datagram.encode());
        } else if let Some(requests) = &self.requests {
            // A broken connection shows up as a disconnect when receiving.
            _ = requests.send(request);
        }
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        // Send the last requests, like the save, before closing the connection.
        self.requests = None;
        if let Some(writer) = self.writer.take()
            && writer.join().is_err()
        {
            eprintln!("LAN writer failed during shutdown");
        }
        self.stop.store(true, Ordering::Relaxed);
        _ = self.stream.shutdown(Shutdown::Both);
    }
}

/// A world found on the LAN.
#[derive(Clone, Debug)]
pub(crate) struct Found {
    pub(crate) address: SocketAddr,
    pub(crate) announcement: Announcement,
    /// Whether it runs this version of the game.
    pub(crate) compatible: bool,
    pub(crate) seen: Instant,
}

/// Listens for worlds announced on the LAN.
pub(crate) struct Discovery {
    found: Receiver<Found>,
    stop: Arc<AtomicBool>,
}

impl Discovery {
    pub(crate) fn start() -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT))?;
        socket.set_read_timeout(Some(POLL))?;
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, found) = mpsc::channel();
        let stopped = stop.clone();
        thread::Builder::new()
            .name("lan-discovery".to_owned())
            .spawn(move || {
                let mut buffer = [0; MAX_DATAGRAM];
                while !stopped.load(Ordering::Relaxed) {
                    let Ok((size, from)) = socket.recv_from(&mut buffer) else {
                        continue;
                    };
                    if let Ok(Datagram::Announce {
                        version,
                        announcement,
                    }) = Datagram::decode(&buffer[..size])
                    {
                        let found = Found {
                            address: SocketAddr::new(from.ip(), announcement.port),
                            announcement,
                            compatible: version == VERSION,
                            seen: Instant::now(),
                        };
                        if sender.send(found).is_err() {
                            break;
                        }
                    }
                }
            })?;
        Ok(Self { found, stop })
    }

    /// The announcements since the last call.
    pub(crate) fn receive(&self) -> Vec<Found> {
        self.found.try_iter().collect()
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Adds announcements to the found worlds, forgets the worlds that stopped announcing and
/// returns whether the list changed. A world on this computer is announced twice, once on
/// the loopback address, which is only kept when its LAN address is not.
pub(crate) fn merge(worlds: &mut Vec<Found>, found: Vec<Found>) -> bool {
    let before: Vec<_> = worlds
        .iter()
        .map(|world| (world.address, world.announcement.clone()))
        .collect();
    for found in found {
        let same = |world: &&mut Found| {
            world.announcement.host == found.announcement.host
                && world.announcement.world == found.announcement.world
                && world.address.port() == found.address.port()
        };
        if let Some(world) = worlds.iter_mut().find(same) {
            if world.address.ip().is_loopback() || !found.address.ip().is_loopback() {
                world.address = found.address;
            }
            world.announcement = found.announcement;
            world.compatible = found.compatible;
            world.seen = found.seen;
        } else {
            worlds.push(found);
        }
    }
    worlds.retain(|world| world.seen.elapsed() < ANNOUNCE_INTERVAL * 4);
    worlds
        .iter()
        .map(|world| (world.address, world.announcement.clone()))
        .ne(before)
}

#[cfg(test)]
mod tests {
    use plaatcraft_engine::world::Edit;

    use super::*;
    use crate::database::tests::TestWorld;
    use crate::server::Connection;
    use crate::server::tests::{host, identity, wait};

    #[test]
    fn guests_join_over_tcp_and_see_the_host_over_udp() {
        let world = TestWorld::new();
        let (mut host_connection, _) = host(&world);
        let port = host_connection.open_to_lan().unwrap();
        assert_eq!(host_connection.open_to_lan(), Ok(port));
        let address = format!("127.0.0.1:{port}");
        assert_eq!(
            Connection::join(&address, &identity("Host")).err().unwrap(),
            "Host is already playing in this world"
        );
        let (mut guest, welcome) = Connection::join(&address, &identity("Guest")).unwrap();
        assert!(!guest.is_host());
        assert_eq!(
            wait(&mut host_connection, |response| match response {
                Response::Message(message) => Some(message),
                _ => None,
            }),
            "Guest joined the game"
        );

        // Terrain and edits go over TCP.
        guest.send(Request::Terrain((0, 0)));
        assert!(wait(&mut guest, |response| match response {
            Response::Terrain { chunk, .. } => Some(chunk.is_some()),
            _ => None,
        }));
        let edit = Edit {
            position: [2, 110, 2],
            material: 3,
        };
        guest.send(Request::Edit(edit));
        let (by, revision) = wait(&mut host_connection, |response| match response {
            Response::Edited { by, revision, .. } => Some((by, revision)),
            _ => None,
        });
        assert_eq!((by, revision), (welcome.id, Some(welcome.revision + 1)));

        // Inputs and states go over UDP, the guest's body follows its inputs.
        let controls = |yaw| Controls {
            yaw,
            ..Controls::default()
        };
        host_connection.send(Request::Input {
            sequence: 1,
            controls: controls(1.0),
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut sequence = 0;
        let (acknowledged, others) = loop {
            sequence += 1;
            guest.send(Request::Input {
                sequence,
                controls: controls(2.0),
            });
            if let Some(Response::State {
                sequence, others, ..
            }) = guest.receive()
                && sequence > 0
                && others.first().is_some_and(|other| other.pose.yaw == 1.0)
            {
                break (sequence, others);
            }
            assert!(Instant::now() < deadline, "No states over UDP");
            thread::sleep(Duration::from_millis(5));
        };
        assert!(acknowledged <= sequence);
        assert_eq!(others[0].name, "Host");

        drop(host_connection);
        let reason = wait(&mut guest, |response| match response {
            Response::Disconnected(reason) => Some(reason),
            _ => None,
        });
        assert_eq!(reason, "The host closed the world");
    }

    #[test]
    fn guests_are_saved_in_the_world_when_they_leave() {
        let world = TestWorld::new();
        let (mut host_connection, _) = host(&world);
        let port = host_connection.open_to_lan().unwrap();
        let (guest, _) =
            Connection::join(&format!("127.0.0.1:{port}"), &identity("Guest")).unwrap();
        let mut hotbar = crate::inventory::Hotbar::default();
        hotbar.pick(25);
        guest.send(Request::Hotbar(hotbar));
        guest.send(Request::Chat("/fly on".into()));
        drop(guest);
        wait(&mut host_connection, |response| match response {
            Response::Message(message) if message == "Guest left the game" => Some(()),
            _ => None,
        });
        drop(host_connection);
        let saved = crate::database::Database::open(&world.path)
            .unwrap()
            .guest("Guest")
            .unwrap()
            .unwrap();
        assert!(saved.flying);
        assert_eq!(saved.hotbar, hotbar);
        // Guests continue where they left the world.
        let (mut host_connection, _) = host(&world);
        let port = host_connection.open_to_lan().unwrap();
        let (_guest, welcome) =
            Connection::join(&format!("127.0.0.1:{port}"), &identity("Guest")).unwrap();
        assert_eq!(welcome.player.position, saved.position);
        assert!(welcome.player.flying);
    }

    #[test]
    fn found_worlds_merge_and_prefer_lan_addresses() {
        let found = |address: &str, seen: Instant| Found {
            address: address.parse().unwrap(),
            announcement: Announcement {
                port: GAME_PORT,
                world: "Island".into(),
                host: "Bastiaan".into(),
                players: 1,
            },
            compatible: true,
            seen,
        };
        let mut worlds = Vec::new();
        assert!(merge(
            &mut worlds,
            vec![found("127.0.0.1:47820", Instant::now())]
        ));
        assert!(merge(
            &mut worlds,
            vec![found("192.168.1.5:47820", Instant::now())]
        ));
        assert!(!merge(
            &mut worlds,
            vec![found("127.0.0.1:47820", Instant::now())]
        ));
        assert_eq!(worlds.len(), 1);
        assert_eq!(worlds[0].address, "192.168.1.5:47820".parse().unwrap());
        worlds[0].seen = Instant::now() - ANNOUNCE_INTERVAL * 5;
        assert!(merge(&mut worlds, Vec::new()));
        assert!(worlds.is_empty());
    }

    #[test]
    fn addresses_default_to_the_game_port() {
        assert_eq!(
            resolve("127.0.0.1").unwrap(),
            SocketAddr::from((Ipv4Addr::LOCALHOST, GAME_PORT))
        );
        assert_eq!(resolve(" 127.0.0.1:1234 ").unwrap().port(), 1234);
        assert_eq!(resolve("::1").unwrap().port(), GAME_PORT);
        assert_eq!(resolve("[::1]:99").unwrap().port(), 99);
        assert!(resolve("not a host!").is_err());
    }
}
