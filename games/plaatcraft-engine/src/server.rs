/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The rules of a world, which every platform runs in its integrated server: who plays, their
//! physics, chat and commands, block edits with undo, and flowing fluid. The platforms keep the
//! world in a [`World`], load the terrain, and deliver the [`Outgoing`] messages.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::{format, vec};

use crate::camera::Camera;
use crate::flow;
use crate::hotbar::Hotbar;
use crate::protocol::{
    Body, ClientId, MAX_CHAT_LENGTH, Player, PlayerInfo, Pose, Request, Response, Welcome,
    validate_name,
};
use crate::raycast::raycast;
use crate::world::{self, CHUNK_SIZE, ChunkData, Coord, Edit, HEIGHT};

/// The player that runs the integrated server.
pub const HOST: ClientId = 0;
/// The player that edits flowing fluid and runs the commands' edits: the world itself.
pub const WORLD: ClientId = ClientId::MAX;
/// The seconds of a physics tick of the players, which also tells them where the others are.
pub const TICK: f32 = 0.05;
// Players that sent no input for this long, like a paused game, keep falling on their own.
const IDLE: f32 = 0.15;
// The players are saved every this many ticks.
const SAVE_TICKS: u32 = 100;
// Collisions use the blocks this many chunks around the players.
const NEARBY_RADIUS: i64 = 2;
/// The commands of the chat.
pub const HELP: &str = "Commands: /fly on|off, /time, /pos1, /pos2, /fill, /clear, /undo, /help";
// The most blocks one /fill or /clear changes, and how many changes and blocks /undo
// remembers of a player.
const MAX_FILL: usize = 32_768;
const UNDO_STEPS: usize = 32;
const UNDO_BLOCKS: usize = 65_536;

/// How much fluid flows, consoles flow less.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Fluid flows this many chunks around the players, like Minecraft only ticks loaded
    /// chunks.
    pub fluid_radius: i64,
    /// The most fluid blocks that change in a tick.
    pub fluid_updates: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            fluid_radius: 4,
            fluid_updates: 4096,
        }
    }
}

/// The storage of a world, which the platform provides.
pub trait World {
    /// The blocks of a chunk, `None` when it isn't generated yet.
    fn chunk(&mut self, coord: Coord) -> Option<ChunkData>;
    /// Saves edits and returns the world revision after them.
    fn save_edits(&mut self, edits: &[Edit]) -> Result<u64, String>;
    /// The world revision, which counts the saved edits.
    fn revision(&mut self) -> Result<u64, String>;
    /// The saved host player.
    fn player(&mut self) -> Result<Player, String>;
    /// The saved player of a guest, by name.
    fn guest(&mut self, name: &str) -> Result<Option<Player>, String>;
    /// Saves the host player, or a guest by name.
    fn save_player(&mut self, guest: Option<&str>, player: &Player) -> Result<(), String>;
    /// Saves the blocks whose fluid still flows, so it flows on when the world opens again.
    fn save_fluids(&mut self, positions: &[[i64; 3]]) -> Result<(), String>;
}

/// What the server asks the platform to deliver or do.
#[derive(Clone)]
pub enum Outgoing {
    /// A response for one player.
    To(ClientId, Response),
    /// A response for everyone.
    All(Response),
    /// Load or generate the terrain of a chunk and send it to a player.
    Terrain(ClientId, Coord),
    /// Skip the queued terrain of a player.
    CancelTerrain(ClientId),
    /// Send the current blocks of a chunk to a player, with the tag.
    Blocks(ClientId, Coord, u64),
    /// Something failed that only the logs should know.
    Log(String),
}

struct Client {
    name: String,
    body: Camera,
    // The last input that moved the body, and the seconds since.
    sequence: u32,
    idle: f32,
    hotbar: Hotbar,
    // The corners of the selection that /fill and /clear change, set by /pos1 and /pos2.
    corners: [Option<[i64; 3]>; 2],
    // The edits that undo the player's last changes, the newest last.
    undo: Vec<Vec<Edit>>,
}

// The blocks around the players for collisions, chunks that were not generated yet are
// remembered as missing until the next tick.
#[derive(Default)]
struct Blocks(BTreeMap<Coord, Option<ChunkData>>);

impl Blocks {
    fn block(&mut self, world: &mut impl World, position: [i64; 3]) -> Option<u8> {
        if !(0..HEIGHT as i64).contains(&position[1]) {
            return Some(0);
        }
        let size = CHUNK_SIZE as i64;
        let coord = (position[0].div_euclid(size), position[2].div_euclid(size));
        let chunk = self.0.entry(coord).or_insert_with(|| world.chunk(coord));
        Some(chunk.as_ref()?.voxel([
            position[0].rem_euclid(size) as i32,
            position[1] as i32,
            position[2].rem_euclid(size) as i32,
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

/// The integrated server of a world, its storage comes with each call.
pub struct Server {
    /// Whether it is night.
    pub night: bool,
    limits: Limits,
    clients: BTreeMap<ClientId, Client>,
    blocks: Blocks,
    // The blocks whose fluid flows at a game tick.
    fluids: BTreeMap<[i64; 3], u32>,
    next_id: ClientId,
    ticks: u32,
    outgoing: Vec<Outgoing>,
}

impl Server {
    /// Opens a world, its fluid that was still flowing flows on.
    pub fn new(world: &mut impl World, limits: Limits, fluids: &[[i64; 3]]) -> Self {
        let night = world.player().is_ok_and(|player| player.night);
        Self {
            night,
            limits,
            clients: BTreeMap::new(),
            blocks: Blocks::default(),
            fluids: fluids
                .iter()
                .map(|&position| (position, flow::WATER_DELAY))
                .collect(),
            next_id: HOST,
            ticks: 0,
            outgoing: Vec::new(),
        }
    }

    /// The messages and tasks for the platform since the last call.
    pub fn outgoing(&mut self) -> Vec<Outgoing> {
        core::mem::take(&mut self.outgoing)
    }

    /// How many players play.
    pub fn players(&self) -> usize {
        self.clients.len()
    }

    /// The ids of the playing players.
    pub fn ids(&self) -> impl Iterator<Item = ClientId> + '_ {
        self.clients.keys().copied()
    }

    /// Where the players are, for streaming the world around them.
    pub fn centers(&self) -> Vec<Coord> {
        self.clients
            .values()
            .map(|client| client.body.chunk())
            .collect()
    }

    fn broadcast(&mut self, response: Response) {
        self.outgoing.push(Outgoing::All(response));
    }

    fn send(&mut self, id: ClientId, response: Response) {
        self.outgoing.push(Outgoing::To(id, response));
    }

    /// Lets a player join, the first one is the host, with the id or else the next free one.
    /// Guests that are new to the world start next to the host.
    pub fn join(
        &mut self,
        world: &mut impl World,
        id: Option<ClientId>,
        name: &str,
    ) -> Result<Welcome, String> {
        let name = String::from(validate_name(name)?);
        if self
            .clients
            .values()
            .any(|client| client.name.eq_ignore_ascii_case(&name))
        {
            return Err(format!("{name} is already playing in this world"));
        }
        let id = id.unwrap_or(self.next_id);
        if id == WORLD || self.clients.contains_key(&id) {
            return Err("That player is already playing".into());
        }
        let mut player = if id == HOST {
            world.player()?
        } else {
            world.guest(&name)?.unwrap_or_else(|| Player {
                position: self
                    .clients
                    .get(&HOST)
                    .map_or(Player::default().position, |host| host.body.position),
                ..Player::default()
            })
        };
        player.night = self.night;
        let revision = world.revision()?;
        let mut body = Camera::default();
        player.restore(&mut body);
        // The players that already play hear about the new one
        let joined = Response::Message(format!("{name} joined the game"));
        let others: Vec<_> = self.ids().collect();
        for other in others {
            self.send(other, joined.clone());
        }
        self.clients.insert(
            id,
            Client {
                name,
                body,
                sequence: 0,
                idle: 0.0,
                hotbar: player.hotbar,
                corners: [None; 2],
                undo: Vec::new(),
            },
        );
        self.next_id = self.next_id.max(id + 1);
        Ok(Welcome {
            id,
            player,
            revision,
        })
    }

    /// Saves a player that left and tells the others.
    pub fn leave(&mut self, world: &mut impl World, id: ClientId) {
        if let Some(client) = self.clients.remove(&id) {
            self.save(world, id, &client);
            self.broadcast(Response::Message(format!("{} left the game", client.name)));
        }
    }

    /// Handles a request of a player.
    pub fn request(&mut self, world: &mut impl World, id: ClientId, request: Request) {
        let Some(client) = self.clients.get_mut(&id) else {
            return;
        };
        match request {
            Request::Terrain(coord) => self.outgoing.push(Outgoing::Terrain(id, coord)),
            Request::CancelTerrain => self.outgoing.push(Outgoing::CancelTerrain(id)),
            Request::Blocks(coord, tag) => self.outgoing.push(Outgoing::Blocks(id, coord, tag)),
            Request::Edit(edit) => match self.save_edits(world, id, vec![edit], true) {
                // Everyone applies the edit, the editor already did.
                Ok((revision, _)) => self.broadcast(Response::Edited {
                    edits: vec![edit],
                    revision: Some(revision),
                    by: id,
                }),
                Err(error) => {
                    self.outgoing
                        .push(Outgoing::Log(format!("Could not save block edit: {error}")));
                    self.send(
                        id,
                        Response::Edited {
                            edits: vec![edit],
                            revision: None,
                            by: id,
                        },
                    );
                }
            },
            Request::Hotbar(hotbar) => client.hotbar = hotbar,
            // Repeated and late inputs are skipped, the client predicted them already.
            Request::Input { sequence, controls } => {
                if sequence > client.sequence {
                    controls.apply(&mut client.body);
                    let blocks = &mut self.blocks;
                    client
                        .body
                        .tick(TICK, |position| blocks.block(world, position));
                    client.sequence = sequence;
                    client.idle = 0.0;
                }
            }
            Request::Chat(text) => {
                let name = client.name.clone();
                let text = text.trim();
                if text.starts_with('/') {
                    let answer = self.command(world, id, text);
                    self.send(id, Response::Message(answer));
                } else if !text.is_empty() {
                    let text = text.chars().take(MAX_CHAT_LENGTH).collect();
                    self.broadcast(Response::Chat { name, text });
                }
            }
        }
    }

    // Runs a chat command and returns the answer for the player.
    fn command(&mut self, world: &mut impl World, id: ClientId, text: &str) -> String {
        let words: Vec<_> = text.split_whitespace().collect();
        let Some(client) = self.clients.get_mut(&id) else {
            return String::new();
        };
        match words.as_slice() {
            ["/fly", rest @ ..] => {
                let flying = match rest {
                    [] => !client.body.flying,
                    ["on"] => true,
                    ["off"] => false,
                    _ => return "Usage: /fly on|off".into(),
                };
                client.body.set_flying(flying);
                if flying { "Flying on" } else { "Flying off" }.into()
            }
            ["/time"] => {
                let name = client.name.clone();
                self.night = !self.night;
                let time = if self.night { "night" } else { "day" };
                self.broadcast(Response::Time { night: self.night });
                self.broadcast(Response::Message(format!("{name} set the time to {time}")));
                format!("It is {time} now")
            }
            ["/time", ..] => "Usage: /time".into(),
            [corner @ ("/pos1" | "/pos2")] => {
                let body = client.body;
                let blocks = &mut self.blocks;
                let Some(hit) = raycast(body.position, body.direction(), |position| {
                    blocks.block(world, position)
                }) else {
                    return "Look at a block to mark a corner".into();
                };
                let index = usize::from(*corner == "/pos2");
                if let Some(client) = self.clients.get_mut(&id) {
                    client.corners[index] = Some(hit.position);
                }
                let [x, y, z] = hit.position;
                format!("Corner {} is at {x} {y} {z}", index + 1)
            }
            [command @ ("/fill" | "/clear")] => {
                let [Some(first), Some(second)] = client.corners else {
                    return "Mark two corners with /pos1 and /pos2 first".into();
                };
                let material = if *command == "/fill" {
                    client.hotbar.material()
                } else {
                    0
                };
                let low: [i64; 3] = core::array::from_fn(|axis| first[axis].min(second[axis]));
                let high: [i64; 3] = core::array::from_fn(|axis| first[axis].max(second[axis]));
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
                self.command_edits(world, id, edits, true)
            }
            ["/undo"] => {
                let Some(edits) = client.undo.pop() else {
                    return "There is nothing to undo".into();
                };
                self.command_edits(world, id, edits, false)
            }
            ["/help"] => HELP.into(),
            _ => format!("Unknown command {}, try /help", words[0]),
        }
    }

    // Saves the edits of a player and applies them to the server's blocks, the blocks they
    // replace go on the player's undo stack when `undoable`. Unchanged blocks are skipped.
    fn save_edits(
        &mut self,
        world: &mut impl World,
        id: ClientId,
        edits: Vec<Edit>,
        undoable: bool,
    ) -> Result<(u64, Vec<Edit>), String> {
        if edits.iter().any(|edit| {
            !(3..HEIGHT as i64).contains(&edit.position[1])
                || edit.material != 0 && !world::valid_material(edit.material)
        }) {
            return Err("Invalid block edit".into());
        }
        let blocks = &mut self.blocks;
        let mut undo = Vec::new();
        let edits: Vec<_> = edits
            .into_iter()
            .filter(|edit| {
                let before = blocks.block(world, edit.position);
                if let Some(material) = before.filter(|&material| material != edit.material) {
                    undo.push(Edit {
                        position: edit.position,
                        material,
                    });
                }
                before != Some(edit.material)
            })
            .collect();
        let revision = world.save_edits(&edits)?;
        blocks.apply(&edits);
        for edit in &edits {
            self.schedule_around(world, edit.position);
        }
        if let Some(client) = self.clients.get_mut(&id)
            && undoable
            && !undo.is_empty()
        {
            while client.undo.len() >= UNDO_STEPS
                || (!client.undo.is_empty()
                    && client.undo.iter().map(Vec::len).sum::<usize>() + undo.len() > UNDO_BLOCKS)
            {
                client.undo.remove(0);
            }
            client.undo.push(undo);
        }
        Ok((revision, edits))
    }

    // Saves the edits of a command and shares them with everyone, also the player, who
    // didn't apply them yet. Returns the answer for the player.
    fn command_edits(
        &mut self,
        world: &mut impl World,
        id: ClientId,
        edits: Vec<Edit>,
        undoable: bool,
    ) -> String {
        match self.save_edits(world, id, edits, undoable) {
            Ok((_, edits)) if edits.is_empty() => "No blocks changed".into(),
            Ok((revision, edits)) => {
                let count = edits.len();
                self.broadcast(Response::Edited {
                    edits,
                    revision: Some(revision),
                    by: WORLD,
                });
                format!("Changed {count} block{}", if count == 1 { "" } else { "s" })
            }
            Err(error) => format!("Could not change the blocks: {error}"),
        }
    }

    // Schedules a changed block and its neighbors next to fluid, water flows faster.
    fn schedule_around(&mut self, world: &mut impl World, position: [i64; 3]) {
        const AROUND: [[i64; 3]; 6] = [
            [0, 0, 0],
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ];
        let blocks = &mut self.blocks;
        for offset in AROUND.into_iter().chain([[0, -1, 0]]) {
            let block: [i64; 3] = core::array::from_fn(|axis| position[axis] + offset[axis]);
            if !(3..HEIGHT as i64).contains(&block[1]) {
                continue;
            }
            let fastest = AROUND
                .into_iter()
                .filter_map(|around| {
                    blocks.block(
                        world,
                        core::array::from_fn(|axis| block[axis] + around[axis]),
                    )
                })
                .filter(|&material| world::fluid(material).is_some())
                .map(flow::delay)
                .min();
            if let Some(fastest) = fastest {
                let due = self.fluids.entry(block).or_insert(self.ticks + fastest);
                *due = (*due).min(self.ticks + fastest);
            }
        }
    }

    fn save(&mut self, world: &mut impl World, id: ClientId, client: &Client) {
        let player = Player::new(&client.body, self.night, client.hotbar);
        let guest = (id != HOST).then_some(client.name.as_str());
        if let Err(error) = world.save_player(guest, &player) {
            self.outgoing
                .push(Outgoing::Log(format!("Could not save player: {error}")));
        }
    }

    fn save_fluids(&mut self, world: &mut impl World) {
        let positions: Vec<_> = self.fluids.keys().copied().collect();
        if let Err(error) = world.save_fluids(&positions) {
            self.outgoing.push(Outgoing::Log(format!(
                "Could not save fluid updates: {error}"
            )));
        }
    }

    /// Saves all players and the flowing fluid.
    pub fn save_all(&mut self, world: &mut impl World) {
        let clients = core::mem::take(&mut self.clients);
        for (&id, client) in &clients {
            self.save(world, id, client);
        }
        self.clients = clients;
        self.save_fluids(world);
    }

    /// Moves idle players, flows fluid, sends everyone their state and the others, and saves
    /// now and then. Runs every [`TICK`].
    pub fn tick(&mut self, world: &mut impl World) {
        let blocks = &mut self.blocks;
        for client in self.clients.values_mut() {
            client.idle += TICK;
            if client.idle >= IDLE {
                client.body.input = Default::default();
                client
                    .body
                    .tick(TICK, |position| blocks.block(world, position));
            }
        }
        let centers = self.centers();
        self.flow(world, &centers);
        let states: Vec<_> = self
            .clients
            .iter()
            .map(|(&id, client)| {
                let others = self
                    .clients
                    .iter()
                    .filter(|&(&other, _)| other != id)
                    .map(|(&other, client)| PlayerInfo {
                        id: other,
                        name: client.name.clone(),
                        pose: Pose {
                            position: client.body.position,
                            yaw: client.body.yaw,
                            pitch: client.body.pitch,
                        },
                    })
                    .collect();
                let state = Response::State {
                    sequence: client.sequence,
                    body: Body::of(&client.body),
                    others,
                };
                Outgoing::To(id, state)
            })
            .collect();
        self.outgoing.extend(states);
        // Forget far and missing chunks, missing ones may have been generated since.
        let radius = NEARBY_RADIUS.max(self.limits.fluid_radius);
        self.blocks.0.retain(|coord, chunk| {
            chunk.is_some()
                && centers.iter().any(|center| {
                    (coord.0 - center.0).abs() <= radius && (coord.1 - center.1).abs() <= radius
                })
        });
        self.ticks += 1;
        if self.ticks.is_multiple_of(SAVE_TICKS) {
            self.save_all(world);
        }
    }

    // Flows the fluid that is due near the players, all read before any changes, and tells
    // everyone as one edit of the world.
    fn flow(&mut self, world: &mut impl World, centers: &[Coord]) {
        let radius = self.limits.fluid_radius;
        let due: Vec<_> = self
            .fluids
            .iter()
            .filter(|&(position, &tick)| {
                tick <= self.ticks
                    && centers.iter().any(|center| {
                        (position[0].div_euclid(CHUNK_SIZE as i64) - center.0).abs() <= radius
                            && (position[2].div_euclid(CHUNK_SIZE as i64) - center.1).abs()
                                <= radius
                    })
            })
            .map(|(&position, _)| position)
            .take(self.limits.fluid_updates)
            .collect();
        let blocks = &mut self.blocks;
        let mut edits = Vec::new();
        for position in due {
            self.fluids.remove(&position);
            if let Some(material) = flow::flow(position, |block| blocks.block(world, block)) {
                edits.push(Edit { position, material });
            }
        }
        if edits.is_empty() {
            return;
        }
        match world.save_edits(&edits) {
            Ok(revision) => {
                self.blocks.apply(&edits);
                for edit in &edits {
                    self.schedule_around(world, edit.position);
                }
                self.broadcast(Response::Edited {
                    edits,
                    revision: Some(revision),
                    by: WORLD,
                });
            }
            Err(error) => self.outgoing.push(Outgoing::Log(format!(
                "Could not save flowing fluid: {error}"
            ))),
        }
    }

    /// Saves everything and tells the guests the world closes.
    pub fn stop(&mut self, world: &mut impl World) {
        self.save_all(world);
        let guests: Vec<_> = self.ids().filter(|&id| id != HOST).collect();
        for id in guests {
            self.send(
                id,
                Response::Disconnected("The host closed the world".into()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::WORLD_SEED;

    // A world of generated chunks that keeps its edits and players in memory.
    #[derive(Default)]
    struct TestWorld {
        edits: Vec<Edit>,
        guests: BTreeMap<String, Player>,
        broken: bool,
    }

    impl World for TestWorld {
        fn chunk(&mut self, coord: Coord) -> Option<ChunkData> {
            let mut chunk = ChunkData::generate(WORLD_SEED, coord);
            chunk.apply_edits(coord, &self.edits);
            Some(chunk)
        }
        fn save_edits(&mut self, edits: &[Edit]) -> Result<u64, String> {
            self.edits.extend_from_slice(edits);
            self.revision()
        }
        fn revision(&mut self) -> Result<u64, String> {
            if self.broken {
                return Err("Broken world".into());
            }
            Ok(self.edits.len() as u64)
        }
        fn player(&mut self) -> Result<Player, String> {
            Ok(Player::default())
        }
        fn guest(&mut self, name: &str) -> Result<Option<Player>, String> {
            Ok(self.guests.get(name).copied())
        }
        fn save_player(&mut self, guest: Option<&str>, player: &Player) -> Result<(), String> {
            if let Some(name) = guest {
                self.guests.insert(name.into(), *player);
            }
            Ok(())
        }
        fn save_fluids(&mut self, _: &[[i64; 3]]) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn players_join_once_and_failed_joins_leave_no_player() {
        let mut world = TestWorld::default();
        let mut server = Server::new(&mut world, Limits::default(), &[]);
        assert_eq!(server.join(&mut world, None, "Host").unwrap().id, HOST);
        assert!(server.join(&mut world, None, "host").is_err());
        assert!(server.join(&mut world, Some(WORLD), "World").is_err());
        world.broken = true;
        assert!(server.join(&mut world, None, "Guest").is_err());
        assert_eq!(server.players(), 1);
        world.broken = false;
        let guest = server.join(&mut world, None, "Guest").unwrap();
        assert_eq!(guest.id, HOST + 1);
        server.request(&mut world, guest.id, Request::Chat("/fly on".into()));
        server.leave(&mut world, guest.id);
        assert!(world.guests["Guest"].flying);
        assert_eq!(server.ids().collect::<Vec<_>>(), [HOST]);
    }

    #[test]
    fn edits_are_validated_and_undone() {
        let mut world = TestWorld::default();
        let mut server = Server::new(&mut world, Limits::default(), &[]);
        server.join(&mut world, None, "Host").unwrap();
        for position in [[0, 120, 0], [0, 0, 0]] {
            let edit = Edit {
                position,
                material: 3,
            };
            server.request(&mut world, HOST, Request::Edit(edit));
        }
        let revisions: Vec<_> = server
            .outgoing()
            .into_iter()
            .filter_map(|outgoing| match outgoing {
                Outgoing::All(Response::Edited { revision, .. })
                | Outgoing::To(_, Response::Edited { revision, .. }) => Some(revision),
                _ => None,
            })
            .collect();
        assert_eq!(revisions, [Some(1), None]);
        server.request(&mut world, HOST, Request::Chat("/undo".into()));
        assert_eq!(
            world.edits.last(),
            Some(&Edit {
                position: [0, 120, 0],
                material: 0,
            })
        );
    }
}
