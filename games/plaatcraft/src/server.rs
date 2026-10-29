/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The integrated server that owns a world. Clients only talk to it with messages, the
//! server thread handles them in order and terrain workers load and generate chunks. The
//! host talks to it over channels, LAN guests over the sockets of lan.rs.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use plaatcraft_engine::world::{ChunkData, Coord, Edit};

use plaatcraft_engine::camera::Camera;
use plaatcraft_engine::flow;
use plaatcraft_engine::raycast::raycast;
use plaatcraft_engine::world::{self, HEIGHT};

use crate::database::{Database, Player};
use crate::inventory::Hotbar;
use crate::lan;
use crate::protocol::{Body, Controls, MAX_CHAT_LENGTH, PlayerInfo, Pose};

pub(crate) type ClientId = u32;
/// The player that runs the integrated server.
pub(crate) const HOST: ClientId = 0;
/// The physics tick of the players, which also tells them where the others are.
pub(crate) const TICK: Duration = Duration::from_millis(50);
// Players that sent no input for this long, like a paused game, keep falling on their own.
const IDLE: Duration = Duration::from_millis(150);
// The players are saved every this many ticks.
const SAVE_TICKS: u32 = 100;
// Collisions use the blocks this many chunks around the players.
const NEARBY_RADIUS: i64 = 2;
// Fluid flows this many chunks around the players, like Minecraft only ticks loaded
// chunks, and changes at most this many blocks a tick.
const FLUID_RADIUS: i64 = 4;
const MAX_FLUID_UPDATES: usize = 4096;
/// The player that edits flowing fluid: the world itself.
pub(crate) const WORLD: ClientId = ClientId::MAX;
const HELP: &str =
    "Commands: /fly on|off, /time day|night, /pos1, /pos2, /fill, /clear, /undo, /help";
// The most blocks one /fill or /clear changes, and how many changes /undo remembers.
const MAX_FILL: usize = 32_768;
const UNDO_STEPS: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Request {
    /// Load or generate the lit terrain of a chunk.
    Terrain(Coord),
    /// Skip the terrain requests that are still queued.
    CancelTerrain,
    /// The current blocks of a chunk, the tag comes back with them.
    Blocks(Coord, u64),
    Edit(Edit),
    Hotbar(Hotbar),
    /// The controls of the next physics tick of the player.
    Input {
        sequence: u32,
        controls: Controls,
    },
    /// A chat message or a command like `/time night`.
    Chat(String),
}

#[derive(Clone)]
pub(crate) enum Response {
    /// A canceled request has no chunk.
    Terrain {
        coord: Coord,
        revision: u64,
        chunk: Option<ChunkData>,
    },
    Blocks {
        coord: Coord,
        tag: u64,
        chunk: Option<ChunkData>,
    },
    /// Edits of a player or of flowing fluid were saved at a revision, or rejected without
    /// one.
    Edited {
        edits: Vec<Edit>,
        revision: Option<u64>,
        by: ClientId,
    },
    /// The player's body after the input with the sequence number, and the other players.
    State {
        sequence: u32,
        body: Body,
        others: Vec<PlayerInfo>,
    },
    Chat {
        name: String,
        text: String,
    },
    /// A message of the world, like someone joining or the answer to a command.
    Message(String),
    Time {
        night: bool,
    },
    /// The world closed or the connection broke.
    Disconnected(String),
}

/// What the client needs to know when it joins.
pub(crate) struct Welcome {
    pub(crate) id: ClientId,
    pub(crate) player: Player,
    pub(crate) revision: u64,
}

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

struct Client {
    identity: Identity,
    responses: Sender<Response>,
    // Terrain requests from before the last cancel are skipped.
    epoch: Arc<AtomicU64>,
    body: Camera,
    // The last input that moved the body.
    sequence: u32,
    last_input: Instant,
    hotbar: Hotbar,
    // The corners of the selection that /fill and /clear change, set by /pos1 and /pos2.
    corners: [Option<[i64; 3]>; 2],
    // The edits that undo the player's last changes, the newest last.
    undo: Vec<Vec<Edit>>,
}

impl Client {
    const fn player(&self, night: bool) -> Player {
        Player {
            night,
            ..Player::new(&self.body, false, self.hotbar)
        }
    }
}

// The blocks around the players for collisions, chunks that were not generated yet are
// remembered as missing until the next tick.
#[derive(Default)]
struct Blocks(HashMap<Coord, Option<ChunkData>>);

impl Blocks {
    fn block(&mut self, db: &Database, position: [i64; 3]) -> Option<u8> {
        if !(0..HEIGHT as i64).contains(&position[1]) {
            return Some(0);
        }
        let coord = (position[0].div_euclid(16), position[2].div_euclid(16));
        let chunk = self.0.entry(coord).or_insert_with(|| {
            db.blocks(coord)
                .inspect_err(|error| eprintln!("Could not read blocks: {error}"))
                .ok()
                .flatten()
        });
        Some(chunk.as_ref()?.voxel([
            position[0].rem_euclid(16) as i32,
            position[1] as i32,
            position[2].rem_euclid(16) as i32,
        ]))
    }

    fn apply(&mut self, edits: &[Edit]) {
        for (&coord, chunk) in &mut self.0 {
            if let Some(chunk) = chunk {
                chunk.apply_edits(coord, edits);
            }
        }
    }
}

// The blocks whose fluid flows at a game tick.
#[derive(Default)]
struct Fluids(HashMap<[i64; 3], u32>);

impl Fluids {
    fn schedule(&mut self, position: [i64; 3], tick: u32) {
        let due = self.0.entry(position).or_insert(tick);
        *due = (*due).min(tick);
    }

    // Schedules a changed block and its neighbors next to fluid, water flows faster.
    fn schedule_around(
        &mut self,
        position: [i64; 3],
        now: u32,
        blocks: &mut Blocks,
        db: &Database,
    ) {
        const AROUND: [[i64; 3]; 6] = [
            [0, 0, 0],
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ];
        for offset in AROUND.into_iter().chain([[0, -1, 0]]) {
            let block: [i64; 3] = std::array::from_fn(|axis| position[axis] + offset[axis]);
            if !(3..HEIGHT as i64).contains(&block[1]) {
                continue;
            }
            let fluids: Vec<_> = AROUND
                .into_iter()
                .filter_map(|around| {
                    blocks.block(db, std::array::from_fn(|axis| block[axis] + around[axis]))
                })
                .filter(|&material| world::fluid(material).is_some())
                .collect();
            if let Some(fastest) = fluids.iter().map(|&material| flow::delay(material)).min() {
                self.schedule(block, now + fastest);
            }
        }
    }

    // The new blocks of the fluid that is due near the players, all read before any
    // changes.
    fn update(
        &mut self,
        now: u32,
        blocks: &mut Blocks,
        db: &Database,
        centers: &[Coord],
    ) -> Vec<Edit> {
        let due: Vec<_> = self
            .0
            .iter()
            .filter(|&(position, &tick)| {
                tick <= now
                    && centers.iter().any(|center| {
                        (position[0].div_euclid(16) - center.0).abs() <= FLUID_RADIUS
                            && (position[2].div_euclid(16) - center.1).abs() <= FLUID_RADIUS
                    })
            })
            .map(|(&position, _)| position)
            .take(MAX_FLUID_UPDATES)
            .collect();
        let mut edits = Vec::new();
        for position in due {
            self.0.remove(&position);
            if let Some(material) = flow::flow(position, |block| blocks.block(db, block)) {
                edits.push(Edit { position, material });
            }
        }
        edits
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

    let mut server = Server {
        db,
        night: db.player().is_ok_and(|player| player.night),
        clients: BTreeMap::new(),
        blocks: Blocks::default(),
        fluids: Fluids::default(),
        jobs,
        next_id: HOST,
        ticks: 0,
        players,
    };
    // Fluid that was still flowing when the world closed flows on.
    match db.fluid_updates() {
        Ok(positions) => {
            for position in positions {
                server.fluids.schedule(position, flow::WATER_DELAY);
            }
        }
        Err(error) => eprintln!("Could not read fluid updates: {error}"),
    }
    let mut next_tick = Instant::now() + TICK;
    loop {
        match messages.recv_timeout(next_tick.saturating_duration_since(Instant::now())) {
            Ok(Message::Stop) => {
                server.stop();
                break;
            }
            Ok(message) => server.handle(message),
            Err(RecvTimeoutError::Timeout) => {
                next_tick += TICK;
                // A stalled server catches up instead of ticking in a burst.
                if next_tick < Instant::now() {
                    next_tick = Instant::now() + TICK;
                }
                server.tick();
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    // Drop the players first, so the guests' connections close, then skip the queued
    // terrain and wait for the workers before closing the world.
    for client in server.clients.values() {
        client.epoch.fetch_add(1, Ordering::Relaxed);
    }
    let Server { clients, jobs, .. } = server;
    drop(clients);
    drop(jobs);
    for worker in workers {
        if worker.join().is_err() {
            eprintln!("Terrain worker failed during shutdown");
        }
    }
}

struct Server<'a> {
    db: &'a Database,
    night: bool,
    clients: BTreeMap<ClientId, Client>,
    blocks: Blocks,
    fluids: Fluids,
    jobs: Sender<Job>,
    next_id: ClientId,
    ticks: u32,
    players: &'a AtomicUsize,
}

impl Server<'_> {
    fn broadcast(&self, response: &Response) {
        for client in self.clients.values() {
            _ = client.responses.send(response.clone());
        }
    }

    fn handle(&mut self, message: Message) {
        match message {
            Message::Join {
                identity,
                responses,
                welcome,
            } => self.join(identity, responses, welcome),
            Message::Leave(id) => {
                if let Some(client) = self.clients.remove(&id) {
                    self.save(id, &client);
                    self.broadcast(&Response::Message(format!(
                        "{} left the game",
                        client.identity.name
                    )));
                    self.players.store(self.clients.len(), Ordering::Relaxed);
                }
            }
            Message::Request(id, request) => self.request(id, request),
            Message::Stop => {}
        }
    }

    fn join(
        &mut self,
        identity: Identity,
        responses: Sender<Response>,
        welcome: SyncSender<Result<Welcome, String>>,
    ) {
        let joined = admit(self.db, &self.clients, self.next_id, &identity);
        let Ok(mut joined) = joined else {
            _ = welcome.send(joined);
            return;
        };
        joined.player.night = self.night;
        let mut body = Camera::default();
        joined.player.restore(&mut body);
        let client = Client {
            identity,
            responses,
            epoch: Arc::new(AtomicU64::new(0)),
            body,
            sequence: 0,
            last_input: Instant::now(),
            hotbar: joined.player.hotbar,
            corners: [None; 2],
            undo: Vec::new(),
        };
        _ = welcome.send(Ok(joined));
        self.broadcast(&Response::Message(format!(
            "{} joined the game",
            client.identity.name
        )));
        self.clients.insert(self.next_id, client);
        self.next_id += 1;
        self.players.store(self.clients.len(), Ordering::Relaxed);
    }

    fn request(&mut self, id: ClientId, request: Request) {
        let Some(client) = self.clients.get_mut(&id) else {
            return;
        };
        match request {
            Request::Terrain(coord) => {
                let job = Job {
                    coord,
                    epoch: client.epoch.load(Ordering::Relaxed),
                    canceled: client.epoch.clone(),
                    responses: client.responses.clone(),
                };
                self.jobs.send(job).expect("Queue terrain");
            }
            Request::CancelTerrain => {
                client.epoch.fetch_add(1, Ordering::Relaxed);
            }
            Request::Blocks(coord, tag) => {
                _ = client.responses.send(Response::Blocks {
                    coord,
                    tag,
                    chunk: self.db.blocks(coord).expect("Read nearby blocks"),
                });
            }
            Request::Edit(edit) => match self.save_edits(id, vec![edit]) {
                // Everyone applies the edit, the editor already did.
                Ok(revision) => self.broadcast(&Response::Edited {
                    edits: vec![edit],
                    revision: Some(revision),
                    by: id,
                }),
                Err(error) => {
                    eprintln!("Could not save block edit: {error}");
                    if let Some(client) = self.clients.get(&id) {
                        _ = client.responses.send(Response::Edited {
                            edits: vec![edit],
                            revision: None,
                            by: id,
                        });
                    }
                }
            },
            Request::Hotbar(hotbar) => client.hotbar = hotbar,
            // Repeated and late inputs are skipped, the client predicted them already.
            Request::Input { sequence, controls } => {
                if sequence > client.sequence {
                    controls.apply(&mut client.body);
                    let (db, blocks) = (self.db, &mut self.blocks);
                    client
                        .body
                        .tick(TICK.as_secs_f32(), |position| blocks.block(db, position));
                    client.sequence = sequence;
                    client.last_input = Instant::now();
                }
            }
            Request::Chat(text) => {
                let text = text.trim();
                if text.starts_with('/') {
                    let answer = self.command(id, text);
                    if let Some(client) = self.clients.get(&id) {
                        _ = client.responses.send(Response::Message(answer));
                    }
                } else if !text.is_empty() {
                    let text = text.chars().take(MAX_CHAT_LENGTH).collect();
                    let name = client.identity.name.clone();
                    self.broadcast(&Response::Chat { name, text });
                }
            }
        }
    }

    // Runs a chat command and returns the answer for the player.
    fn command(&mut self, id: ClientId, text: &str) -> String {
        let words: Vec<_> = text.split_whitespace().collect();
        match words.as_slice() {
            ["/fly", rest @ ..] => {
                let client = self.clients.get_mut(&id).expect("Commanding client");
                let flying = match rest {
                    [] => !client.body.flying,
                    ["on"] => true,
                    ["off"] => false,
                    _ => return "Usage: /fly on|off".into(),
                };
                client.body.set_flying(flying);
                if flying { "Flying on" } else { "Flying off" }.into()
            }
            ["/time", time @ ("day" | "night")] => {
                self.night = *time == "night";
                self.broadcast(&Response::Time { night: self.night });
                let name = &self.clients[&id].identity.name;
                self.broadcast(&Response::Message(format!("{name} set the time to {time}")));
                format!("It is {time} now")
            }
            ["/time", ..] => "Usage: /time day|night".into(),
            [corner @ ("/pos1" | "/pos2")] => {
                let client = &self.clients[&id];
                let (db, blocks) = (self.db, &mut self.blocks);
                let body = client.body;
                let Some(hit) = raycast(body.position, body.direction(), |position| {
                    blocks.block(db, position)
                }) else {
                    return "Look at a block to mark a corner".into();
                };
                let index = usize::from(*corner == "/pos2");
                let client = self.clients.get_mut(&id).expect("Commanding client");
                client.corners[index] = Some(hit.position);
                let [x, y, z] = hit.position;
                format!("Corner {} is at {x} {y} {z}", index + 1)
            }
            [command @ ("/fill" | "/clear")] => {
                let client = &self.clients[&id];
                let [Some(first), Some(second)] = client.corners else {
                    return "Mark two corners with /pos1 and /pos2 first".into();
                };
                let material = if *command == "/fill" {
                    client.hotbar.material()
                } else {
                    0
                };
                let low: [i64; 3] = std::array::from_fn(|axis| first[axis].min(second[axis]));
                let high: [i64; 3] = std::array::from_fn(|axis| first[axis].max(second[axis]));
                let count: usize = (0..3)
                    .map(|axis| (high[axis] - low[axis] + 1) as usize)
                    .product();
                if count > MAX_FILL {
                    return format!("The selection has {count} blocks, at most {MAX_FILL} fit");
                }
                let mut edits = Vec::with_capacity(count);
                for y in low[1]..=high[1] {
                    for z in low[2]..=high[2] {
                        for x in low[0]..=high[0] {
                            edits.push(Edit {
                                position: [x, y, z],
                                material,
                            });
                        }
                    }
                }
                self.command_edits(id, edits, true)
            }
            ["/undo"] => {
                let client = self.clients.get_mut(&id).expect("Commanding client");
                let Some(edits) = client.undo.pop() else {
                    return "There is nothing to undo".into();
                };
                self.command_edits(id, edits, false)
            }
            ["/help"] => HELP.into(),
            _ => format!("Unknown command {}, try /help", words[0]),
        }
    }

    // Saves the edits of a player and applies them to the server's blocks, the blocks they
    // replace go on the player's undo stack when `undoable`. Unchanged blocks are skipped.
    fn save_edits(&mut self, id: ClientId, edits: Vec<Edit>) -> Result<u64, String> {
        self.save_undoable(id, edits, true)
            .map(|(revision, _)| revision)
    }

    fn save_undoable(
        &mut self,
        id: ClientId,
        edits: Vec<Edit>,
        undoable: bool,
    ) -> Result<(u64, Vec<Edit>), String> {
        let (db, blocks) = (self.db, &mut self.blocks);
        let mut undo = Vec::new();
        let edits: Vec<_> = edits
            .into_iter()
            .filter(|edit| {
                let before = blocks.block(db, edit.position);
                if let Some(material) = before.filter(|&material| material != edit.material) {
                    undo.push(Edit {
                        position: edit.position,
                        material,
                    });
                }
                before != Some(edit.material)
            })
            .collect();
        let revision = db.edits(&edits).map_err(|error| error.to_string())?;
        blocks.apply(&edits);
        for edit in &edits {
            self.fluids
                .schedule_around(edit.position, self.ticks, blocks, db);
        }
        if let Some(client) = self.clients.get_mut(&id)
            && undoable
            && !undo.is_empty()
        {
            if client.undo.len() == UNDO_STEPS {
                client.undo.remove(0);
            }
            client.undo.push(undo);
        }
        Ok((revision, edits))
    }

    // Saves the edits of a command and shares them with everyone, also the player, who
    // didn't apply them yet. Returns the answer for the player.
    fn command_edits(&mut self, id: ClientId, edits: Vec<Edit>, undoable: bool) -> String {
        match self.save_undoable(id, edits, undoable) {
            Ok((_, edits)) if edits.is_empty() => "No blocks changed".into(),
            Ok((revision, edits)) => {
                let count = edits.len();
                self.broadcast(&Response::Edited {
                    edits,
                    revision: Some(revision),
                    by: WORLD,
                });
                format!("Changed {count} block{}", if count == 1 { "" } else { "s" })
            }
            Err(error) => format!("Could not change the blocks: {error}"),
        }
    }

    fn save(&self, id: ClientId, client: &Client) {
        let player = client.player(self.night);
        let saved = if id == HOST {
            self.db.save_player(&player)
        } else {
            self.db.save_guest(&client.identity.name, &player)
        };
        if let Err(error) = saved {
            eprintln!("Could not save player: {error}");
        }
    }

    // Moves idle players, sends everyone their state and the others, and saves now and then.
    fn tick(&mut self) {
        let (db, blocks) = (self.db, &mut self.blocks);
        for client in self.clients.values_mut() {
            if client.last_input.elapsed() >= IDLE {
                client.body.input = Default::default();
                client
                    .body
                    .tick(TICK.as_secs_f32(), |position| blocks.block(db, position));
            }
        }
        let centers: Vec<_> = self.clients.values().map(|c| c.body.chunk()).collect();
        self.flow(&centers);
        for (&id, client) in &self.clients {
            let others = self
                .clients
                .iter()
                .filter(|&(&other, _)| other != id)
                .map(|(&other, client)| PlayerInfo {
                    id: other,
                    name: client.identity.name.clone(),
                    pose: Pose {
                        position: client.body.position,
                        yaw: client.body.yaw,
                        pitch: client.body.pitch,
                    },
                })
                .collect();
            _ = client.responses.send(Response::State {
                sequence: client.sequence,
                body: Body::of(&client.body),
                others,
            });
        }
        // Forget far and missing chunks, missing ones may have been generated since.
        let radius = NEARBY_RADIUS.max(FLUID_RADIUS);
        self.blocks.0.retain(|coord, chunk| {
            chunk.is_some()
                && centers.iter().any(|center| {
                    (coord.0 - center.0).abs() <= radius && (coord.1 - center.1).abs() <= radius
                })
        });
        self.ticks += 1;
        if self.ticks.is_multiple_of(SAVE_TICKS) {
            for (&id, client) in &self.clients {
                self.save(id, client);
            }
            self.save_fluids();
        }
    }

    // Flows the fluid that is due and tells everyone, as one edit of the world.
    fn flow(&mut self, centers: &[Coord]) {
        let (db, blocks, fluids) = (self.db, &mut self.blocks, &mut self.fluids);
        let edits = fluids.update(self.ticks, blocks, db, centers);
        if edits.is_empty() {
            return;
        }
        match db.edits(&edits) {
            Ok(revision) => {
                blocks.apply(&edits);
                for edit in &edits {
                    fluids.schedule_around(edit.position, self.ticks, blocks, db);
                }
                self.broadcast(&Response::Edited {
                    edits,
                    revision: Some(revision),
                    by: WORLD,
                });
            }
            Err(error) => eprintln!("Could not save flowing fluid: {error}"),
        }
    }

    fn save_fluids(&self) {
        let positions: Vec<_> = self.fluids.0.keys().copied().collect();
        if let Err(error) = self.db.save_fluid_updates(&positions) {
            eprintln!("Could not save fluid updates: {error}");
        }
    }

    fn stop(&self) {
        self.save_fluids();
        for (&id, client) in &self.clients {
            self.save(id, client);
            if id != HOST {
                _ = client
                    .responses
                    .send(Response::Disconnected("The host closed the world".into()));
            }
        }
    }
}

// Checks whether a player may join and welcomes them, guests that are new to the world
// start next to the host.
fn admit(
    db: &Database,
    clients: &BTreeMap<ClientId, Client>,
    id: ClientId,
    identity: &Identity,
) -> Result<Welcome, String> {
    let name = crate::protocol::validate_name(&identity.name)?;
    if clients
        .values()
        .any(|client| client.identity.name.eq_ignore_ascii_case(name))
    {
        return Err(format!("{name} is already playing in this world"));
    }
    let player = if id == HOST {
        db.player()?
    } else {
        db.guest(name)?.unwrap_or_else(|| Player {
            position: clients
                .get(&HOST)
                .map_or(Player::default().position, |host| host.body.position),
            ..Player::default()
        })
    };
    Ok(Welcome {
        id,
        player,
        revision: db.revision()?,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::database::tests::TestWorld;

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
        server.send(Request::Chat("/time night".into()));
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
