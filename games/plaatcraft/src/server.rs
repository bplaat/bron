/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The integrated server that owns a world. Clients only talk to it with messages, the
//! server thread handles them in order with the engine's game rules and terrain workers load
//! and generate chunks. The host talks to it over channels, LAN guests over the sockets of
//! lan.rs.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use plaatcraft_engine::protocol::{ClientId, Player, Request, Response, Welcome};
pub(crate) use plaatcraft_engine::server::WORLD;
use plaatcraft_engine::server::{self as rules, Limits, Outgoing};
use plaatcraft_engine::world::{ChunkData, Coord, Edit};

use crate::database::Database;
use crate::lan;

// The physics tick of the players, which the engine's server runs.
const TICK: Duration = Duration::from_millis((rules::TICK * 1000.0) as u64);

/// Who joins a world.
#[derive(Clone, Debug)]
pub(crate) struct Identity {
    pub(crate) name: String,
}

pub(crate) enum Message {
    Join {
        identity: Identity,
        responses: Sender<Response>,
        welcome: SyncSender<Result<Welcome, String>>,
    },
    Request(ClientId, Request),
    Leave(ClientId),
    Stop,
}

struct Job {
    coord: Coord,
    epoch: u64,
    canceled: Arc<AtomicU64>,
    responses: Sender<Response>,
}

// Where the responses of a player go.
struct Client {
    responses: Sender<Response>,
    // Terrain requests from before the last cancel are skipped.
    epoch: Arc<AtomicU64>,
}

// The world database as the storage of the engine's server.
struct Storage<'a>(&'a Database);

impl rules::World for Storage<'_> {
    fn chunk(&mut self, coord: Coord) -> Option<ChunkData> {
        self.0
            .blocks(coord)
            .inspect_err(|error| eprintln!("Could not read blocks: {error}"))
            .ok()
            .flatten()
    }

    fn save_edits(&mut self, edits: &[Edit]) -> Result<u64, String> {
        self.0.edits(edits)
    }

    fn revision(&mut self) -> Result<u64, String> {
        self.0.revision()
    }

    fn player(&mut self) -> Result<Player, String> {
        self.0.player()
    }

    fn guest(&mut self, name: &str) -> Result<Option<Player>, String> {
        self.0.guest(name)
    }

    fn save_player(&mut self, guest: Option<&str>, player: &Player) -> Result<(), String> {
        match guest {
            Some(name) => self.0.save_guest(name, player),
            None => self.0.save_player(player),
        }
    }

    fn save_fluids(&mut self, positions: &[[i64; 3]]) -> Result<(), String> {
        self.0.save_fluid_updates(positions)
    }
}

// The server side of the host, which owns the server thread and the opened LAN.
struct Host {
    messages: Sender<Message>,
    thread: Option<thread::JoinHandle<()>>,
    lan: Option<lan::Lan>,
    world: String,
    identity: Identity,
    players: Arc<AtomicUsize>,
}

enum Link {
    Local(Sender<Message>),
    Remote(lan::Remote),
}

/// The client side of a world, dropping the host's connection stops the server.
pub(crate) struct Connection {
    link: Link,
    responses: Receiver<Response>,
    pub(crate) id: ClientId,
    host: Option<Host>,
    closed: bool,
}

impl Connection {
    /// Starts the server of a world and joins it as its host.
    pub(crate) fn host(
        path: PathBuf,
        world: String,
        identity: Identity,
    ) -> Result<(Self, Welcome), String> {
        let (messages, incoming) = mpsc::channel();
        let (opened, open) = mpsc::sync_channel(1);
        let players = Arc::new(AtomicUsize::new(0));
        let thread = {
            let players = players.clone();
            thread::Builder::new()
                .name("server".to_owned())
                .spawn(move || match Database::open(&path) {
                    Ok(db) => {
                        if opened.send(Ok(())).is_ok() {
                            serve(&db, path, &incoming, &players);
                        }
                    }
                    Err(error) => drop(opened.send(Err(error))),
                })
                .map_err(|error| error.to_string())?
        };
        let mut host = Host {
            messages: messages.clone(),
            thread: Some(thread),
            lan: None,
            world,
            identity: identity.clone(),
            players,
        };
        if let Err(error) = open.recv().map_err(|_| "World server stopped".to_owned())? {
            host.thread.take().map(thread::JoinHandle::join);
            return Err(error);
        }
        let (responses, received) = mpsc::channel();
        let welcome = join(&messages, identity, responses)?;
        let connection = Self {
            link: Link::Local(messages),
            responses: received,
            id: welcome.id,
            host: Some(host),
            closed: false,
        };
        Ok((connection, welcome))
    }

    /// Joins a world opened to the LAN.
    pub(crate) fn join(address: &str, identity: &Identity) -> Result<(Self, Welcome), String> {
        let (remote, responses, welcome) = lan::Remote::connect(address, identity)?;
        let connection = Self {
            link: Link::Remote(remote),
            responses,
            id: welcome.id,
            host: None,
            closed: false,
        };
        Ok((connection, welcome))
    }

    pub(crate) const fn is_host(&self) -> bool {
        self.host.is_some()
    }

    /// Lets other players on the LAN find and join the world.
    pub(crate) fn open_to_lan(&mut self) -> Result<u16, String> {
        let host = self
            .host
            .as_mut()
            .ok_or("Only the host can open the world")?;
        if let Some(lan) = &host.lan {
            return Ok(lan.port);
        }
        let lan = lan::Lan::open(
            host.messages.clone(),
            host.world.clone(),
            host.identity.name.clone(),
            host.players.clone(),
        )
        .map_err(|error| format!("Could not open to LAN: {error}"))?;
        let port = lan.port;
        host.lan = Some(lan);
        Ok(port)
    }

    pub(crate) fn send(&self, request: Request) {
        match &self.link {
            // A stopped server shows up as a disconnect when receiving.
            Link::Local(messages) => drop(messages.send(Message::Request(self.id, request))),
            Link::Remote(remote) => remote.send(request),
        }
    }

    pub(crate) fn receive(&mut self) -> Option<Response> {
        if self.closed {
            return None;
        }
        match self.responses.try_recv() {
            Ok(response) => {
                self.closed = matches!(response, Response::Disconnected(_));
                Some(response)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.closed = true;
                Some(Response::Disconnected("The world stopped".into()))
            }
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        // Close the LAN first, so no guest joins while the world closes.
        self.lan = None;
        _ = self.messages.send(Message::Stop);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            eprintln!("World server failed during shutdown");
        }
    }
}

/// Asks the server thread to let a player join.
pub(crate) fn join(
    messages: &Sender<Message>,
    identity: Identity,
    responses: Sender<Response>,
) -> Result<Welcome, String> {
    let (welcome, joined) = mpsc::sync_channel(1);
    messages
        .send(Message::Join {
            identity,
            responses,
            welcome,
        })
        .map_err(|_| "The world is closing".to_owned())?;
    joined
        .recv()
        .map_err(|_| "The world is closing".to_owned())?
}

fn serve(db: &Database, path: PathBuf, messages: &Receiver<Message>, players: &AtomicUsize) {
    // Leave one core for the client, which renders and runs the physics.
    let available = thread::available_parallelism().map_or(1, usize::from);
    let (jobs, queue) = mpsc::channel::<Job>();
    let queue = Arc::new(Mutex::new(queue));
    let workers: Vec<_> = (0..available.saturating_sub(1).max(1))
        .map(|index| {
            let (path, queue) = (path.clone(), queue.clone());
            thread::Builder::new()
                .name(format!("terrain-{index}"))
                .spawn(move || {
                    crate::stream::lower_thread_priority();
                    let db = Database::open(&path).expect("Open worker database");
                    loop {
                        // Release the queue lock before doing expensive work.
                        let job = { queue.lock().expect("Terrain queue poisoned").recv() };
                        let Ok(job) = job else { break };
                        let (chunk, revision) = if job.epoch == job.canceled.load(Ordering::Relaxed)
                        {
                            let (chunk, revision) =
                                db.terrain(job.coord).expect("Load or generate chunk");
                            (Some(chunk), revision)
                        } else {
                            (None, 0)
                        };
                        // A player that left no longer gets responses.
                        _ = job.responses.send(Response::Terrain {
                            coord: job.coord,
                            revision,
                            chunk,
                        });
                    }
                })
                .expect("Start terrain worker")
        })
        .collect();

    // Fluid that was still flowing when the world closed flows on.
    let fluids = db.fluid_updates().unwrap_or_else(|error| {
        eprintln!("Could not read fluid updates: {error}");
        Vec::new()
    });
    let mut storage = Storage(db);
    let mut server = rules::Server::new(&mut storage, Limits::default(), &fluids);
    let mut clients = BTreeMap::new();
    let mut next_tick = Instant::now() + TICK;
    loop {
        match messages.recv_timeout(next_tick.saturating_duration_since(Instant::now())) {
            Ok(Message::Join {
                identity,
                responses,
                welcome,
            }) => {
                let joined = server.join(&mut storage, None, &identity.name);
                // The others hear about the player before it is connected.
                deliver(db, &clients, &jobs, server.outgoing());
                if let Ok(joined) = &joined {
                    let epoch = Arc::new(AtomicU64::new(0));
                    clients.insert(joined.id, Client { responses, epoch });
                }
                _ = welcome.send(joined);
            }
            Ok(Message::Request(id, request)) => server.request(&mut storage, id, request),
            Ok(Message::Leave(id)) => {
                server.leave(&mut storage, id);
                clients.remove(&id);
            }
            Ok(Message::Stop) => {
                server.stop(&mut storage);
                deliver(db, &clients, &jobs, server.outgoing());
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                next_tick += TICK;
                // A stalled server catches up instead of ticking in a burst.
                if next_tick < Instant::now() {
                    next_tick = Instant::now() + TICK;
                }
                server.tick(&mut storage);
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
        deliver(db, &clients, &jobs, server.outgoing());
        players.store(server.players(), Ordering::Relaxed);
    }

    // Drop the players first, so the guests' connections close, then skip the queued
    // terrain and wait for the workers before closing the world.
    for client in clients.values() {
        client.epoch.fetch_add(1, Ordering::Relaxed);
    }
    drop(clients);
    drop(jobs);
    for worker in workers {
        if worker.join().is_err() {
            eprintln!("Terrain worker failed during shutdown");
        }
    }
}

// Sends the responses of the server to the players and queues their terrain.
fn deliver(
    db: &Database,
    clients: &BTreeMap<ClientId, Client>,
    jobs: &Sender<Job>,
    outgoing: Vec<Outgoing>,
) {
    for outgoing in outgoing {
        match outgoing {
            Outgoing::All(response) => {
                for client in clients.values() {
                    _ = client.responses.send(response.clone());
                }
            }
            Outgoing::To(id, response) => {
                if let Some(client) = clients.get(&id) {
                    _ = client.responses.send(response);
                }
            }
            Outgoing::Terrain(id, coord) => {
                if let Some(client) = clients.get(&id) {
                    let job = Job {
                        coord,
                        epoch: client.epoch.load(Ordering::Relaxed),
                        canceled: client.epoch.clone(),
                        responses: client.responses.clone(),
                    };
                    jobs.send(job).expect("Queue terrain");
                }
            }
            Outgoing::CancelTerrain(id) => {
                if let Some(client) = clients.get(&id) {
                    client.epoch.fetch_add(1, Ordering::Relaxed);
                }
            }
            Outgoing::Blocks(id, coord, tag) => {
                if let Some(client) = clients.get(&id) {
                    _ = client.responses.send(Response::Blocks {
                        coord,
                        tag,
                        chunk: db.blocks(coord).expect("Read nearby blocks"),
                    });
                }
            }
            Outgoing::Log(message) => eprintln!("{message}"),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use plaatcraft_engine::protocol::Controls;
    use plaatcraft_engine::server::HOST;
    use plaatcraft_engine::world;

    use super::*;
    use crate::database::tests::TestWorld;
    use crate::inventory::Hotbar;

    pub(crate) fn identity(name: &str) -> Identity {
        Identity {
            name: name.to_owned(),
        }
    }

    pub(crate) fn host(world: &TestWorld) -> (Connection, Welcome) {
        Connection::host(world.path.clone(), "World".into(), identity("Host")).unwrap()
    }

    // Waits for the first response that matches.
    pub(crate) fn wait<T>(
        server: &mut Connection,
        mut matches: impl FnMut(Response) -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(found) = server.receive().and_then(&mut matches) {
                return found;
            }
            assert!(Instant::now() < deadline, "World server timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn edited(server: &mut Connection) -> (Option<u64>, ClientId) {
        wait(server, |response| match response {
            Response::Edited { revision, by, .. } => Some((revision, by)),
            _ => None,
        })
    }

    #[test]
    fn the_server_saves_edits_and_the_player_in_its_world() {
        let world = TestWorld::new();
        let (mut server, welcome) = host(&world);
        assert_eq!(welcome.id, HOST);
        assert_eq!(welcome.player.position, Player::default().position);
        server.send(Request::Terrain((0, 0)));
        let terrain = wait(&mut server, |response| match response {
            Response::Terrain { chunk, .. } => chunk,
            _ => None,
        });
        assert_eq!(terrain.voxel([0, 120, 0]), 0);

        server.send(Request::Edit(Edit {
            position: [0, 120, 0],
            material: 3,
        }));
        server.send(Request::Edit(Edit {
            position: [0, 0, 0],
            material: 3,
        }));
        server.send(Request::Blocks((0, 0), 7));
        let (revision, by) = edited(&mut server);
        assert_eq!((revision, by), (Some(welcome.revision + 1), HOST));
        assert_eq!(edited(&mut server), (None, HOST));
        let (tag, blocks) = wait(&mut server, |response| match response {
            Response::Blocks { tag, chunk, .. } => Some((tag, chunk)),
            _ => None,
        });
        assert_eq!(tag, 7);
        assert_eq!(blocks.unwrap().voxel([0, 120, 0]), 3);

        let mut hotbar = Hotbar::default();
        hotbar.pick(25);
        server.send(Request::Hotbar(hotbar));
        server.send(Request::Chat("/fly on".into()));
        server.send(Request::Chat("/time".into()));
        assert!(wait(&mut server, |response| match response {
            Response::Time { night } => Some(night),
            _ => None,
        }));
        drop(server);
        let (_server, welcome) = host(&world);
        assert_eq!(welcome.player.hotbar, hotbar);
        assert!(welcome.player.flying && welcome.player.night);
        assert_eq!(welcome.revision, revision.unwrap());
    }

    fn message(server: &mut Connection) -> String {
        wait(server, |response| match response {
            Response::Message(message) => Some(message),
            _ => None,
        })
    }

    #[test]
    fn players_fill_a_selection_and_undo_it() {
        let world = TestWorld::new();
        let (mut server, _) = host(&world);
        server.send(Request::Chat("/fly on".into()));
        assert_eq!(message(&mut server), "Flying on");
        server.send(Request::Terrain((0, 0)));
        wait(&mut server, |response| match response {
            Response::Terrain { chunk, .. } => chunk,
            _ => None,
        });
        // Missing chunks are only remembered until the next tick.
        thread::sleep(TICK * 2);
        for z in 5..=9 {
            server.send(Request::Edit(Edit {
                position: [8, 113, z],
                material: 3,
            }));
        }
        server.send(Request::Chat("/fill".into()));
        assert_eq!(
            message(&mut server),
            "Mark two corners with /pos1 and /pos2 first"
        );
        // Looking almost straight down and then forward down marks two platform blocks.
        for (sequence, (pitch, corner)) in
            [(-1.55, "/pos1"), (-1.0, "/pos2")].into_iter().enumerate()
        {
            server.send(Request::Input {
                sequence: sequence as u32 + 1,
                controls: Controls {
                    yaw: 0.0,
                    pitch,
                    ..Controls::default()
                },
            });
            server.send(Request::Chat(corner.into()));
        }
        assert_eq!(message(&mut server), "Corner 1 is at 8 113 7");
        assert_eq!(message(&mut server), "Corner 2 is at 8 113 6");
        let mut hotbar = Hotbar::default();
        hotbar.pick(62);
        server.send(Request::Hotbar(hotbar));
        let changes = |server: &mut Connection| {
            let edits = wait(server, |response| match response {
                Response::Edited { edits, by, .. } if by == WORLD => Some(edits),
                _ => None,
            });
            assert_eq!(message(server), "Changed 2 blocks");
            edits
        };
        server.send(Request::Chat("/fill".into()));
        let filled = changes(&mut server);
        assert!(filled.iter().all(|edit| edit.material == 62));
        server.send(Request::Chat("/undo".into()));
        let undone = changes(&mut server);
        assert!(undone.iter().all(|edit| edit.material == 3));
        // The platform edits can be undone one by one, then nothing is left.
        for _ in 0..5 {
            server.send(Request::Chat("/undo".into()));
            assert_eq!(message(&mut server), "Changed 1 block");
        }
        server.send(Request::Chat("/undo".into()));
        assert_eq!(message(&mut server), "There is nothing to undo");
    }

    #[test]
    fn players_see_each_other_chat_and_share_edits() {
        let world = TestWorld::new();
        let (mut host_connection, _) = host(&world);
        let Some(Host { messages, .. }) = &host_connection.host else {
            panic!("Not the host");
        };
        let messages = messages.clone();
        let (responses, guest_responses) = mpsc::channel();
        assert!(join(&messages, identity("host"), responses.clone()).is_err());
        let guest = join(&messages, identity("Guest"), responses).unwrap();
        assert_ne!(guest.id, HOST);
        assert_eq!(message(&mut host_connection), "Guest joined the game");
        host_connection.send(Request::Input {
            sequence: 1,
            controls: Controls {
                yaw: 0.5,
                ..Controls::default()
            },
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        let others = loop {
            if let Ok(Response::State { others, .. }) = guest_responses.try_recv()
                && others.first().is_some_and(|other| other.pose.yaw == 0.5)
            {
                break others;
            }
            assert!(Instant::now() < deadline, "No players");
            thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(others[0].name, "Host");

        let send = |request| messages.send(Message::Request(guest.id, request)).unwrap();
        send(Request::Chat("  hello there ".into()));
        let chat = wait(&mut host_connection, |response| match response {
            Response::Chat { name, text } => Some((name, text)),
            _ => None,
        });
        assert_eq!(chat, ("Guest".into(), "hello there".into()));
        host_connection.send(Request::Chat("/dance".into()));
        assert_eq!(
            message(&mut host_connection),
            "Unknown command /dance, try /help"
        );
        send(Request::Edit(Edit {
            position: [1, 100, 1],
            material: 3,
        }));
        assert_eq!(edited(&mut host_connection).1, guest.id);
        send(Request::Chat("/fly on".into()));
        messages.send(Message::Leave(guest.id)).unwrap();
        assert_eq!(message(&mut host_connection), "Guest left the game");
        drop(host_connection);
        let saved = Database::open(&world.path).unwrap().guest("Guest").unwrap();
        assert!(saved.unwrap().flying);
    }

    #[test]
    fn idle_players_fall_and_fall_again_when_the_ground_breaks() {
        let world = TestWorld::new();
        Database::open(&world.path)
            .unwrap()
            .save_chunk((0, 0), &ChunkData::from_heights([60; 18 * 18]).encode())
            .unwrap();
        // The host sends no input, like a paused game, and still falls.
        let (mut server, _) = host(&world);
        let height = |server: &mut Connection, done: &dyn Fn(f64) -> bool| {
            wait(server, |response| match response {
                Response::State { body, .. } if done(body.position[1]) => Some(body),
                _ => None,
            })
        };
        let body = height(&mut server, &|y| (y - 61.7).abs() < 0.001);
        assert!(body.grounded);
        for [x, z] in [[7, 7], [7, 8], [8, 7], [8, 8]] {
            server.send(Request::Edit(Edit {
                position: [x, 59, z],
                material: 0,
            }));
        }
        let body = height(&mut server, &|y| (y - 60.7).abs() < 0.001);
        assert!(body.grounded);
    }

    #[test]
    fn placed_water_flows_and_drains_again() {
        let world = TestWorld::new();
        Database::open(&world.path)
            .unwrap()
            .save_chunk((0, 0), &ChunkData::from_heights([60; 18 * 18]).encode())
            .unwrap();
        let (mut server, _) = host(&world);
        let flowed = |server: &mut Connection| {
            wait(server, |response| match response {
                Response::Edited {
                    edits, by: WORLD, ..
                } => Some(edits),
                _ => None,
            })
        };
        let source = [8, 60, 8];
        server.send(Request::Edit(Edit {
            position: source,
            material: world::WATER,
        }));
        let edits = flowed(&mut server);
        assert_eq!(edits.len(), 4);
        assert!(
            edits
                .iter()
                .all(|edit| edit.material == world::Fluid::Water.flowing(7))
        );
        // Without its source the flow shrinks level by level until it is dry.
        server.send(Request::Edit(Edit {
            position: source,
            material: 0,
        }));
        let next = [9, 60, 8];
        while !flowed(&mut server).contains(&Edit {
            position: next,
            material: 0,
        }) {}
    }

    #[test]
    fn canceled_terrain_requests_come_back_empty() {
        let world = TestWorld::new();
        let (mut server, _) = host(&world);
        for x in 0..64 {
            server.send(Request::Terrain((x, 0)));
        }
        server.send(Request::CancelTerrain);
        let mut empty = 0;
        for _ in 0..64 {
            if wait(&mut server, |response| match response {
                Response::Terrain { chunk, .. } => Some(chunk.is_none()),
                _ => None,
            }) {
                empty += 1;
            }
        }
        assert!(empty > 0);
    }

    #[test]
    fn missing_worlds_are_not_created() {
        let world = TestWorld::new();
        let missing = world.path.with_file_name("Missing.db");
        assert!(Connection::host(missing.clone(), "Missing".into(), identity("Host")).is_err());
        assert!(!missing.exists());
    }
}
