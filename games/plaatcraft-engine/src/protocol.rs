/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The LAN wire format. TCP carries length prefixed frames for joining, chunks, blocks,
//! edits, and chat, UDP datagrams carry the frequent inputs, player states, and LAN
//! announcements. The messages are serde types encoded with postcard, every decoded message
//! is validated before it is used.
//! The integrated server of single player passes the same messages without encoding them.

use alloc::format;
use alloc::string::String;
#[cfg(feature = "std")]
use alloc::vec;
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize, Serializer};

use crate::camera::{Camera, Input};
use crate::hotbar::{Hotbar, SLOTS};
use crate::world::{self, CHUNK_SIZE, ChunkData, Coord, Edit};

/// The largest decompressed chunk, in saves and on the network.
pub const MAX_BLOB_SIZE: usize = 450_000;

/// The id of a player connected to a world.
pub type ClientId = u32;

/// Changes whenever the messages change, clients and servers must match.
pub const VERSION: u32 = 1;
/// The TCP and UDP port of an opened world, unassigned by IANA.
pub const GAME_PORT: u16 = 47820;
/// The UDP port LAN worlds are announced on.
pub const DISCOVERY_PORT: u16 = 47821;
const MAGIC: [u8; 4] = *b"PLCR";
/// The largest TCP frame.
pub const MAX_FRAME: usize = 4 * 1024 * 1024;
// The longest text in a message in bytes, longer text is cut off.
const MAX_STRING: usize = 256;
// The farthest block from the center of the world along an axis.
const MAX_POSITION: i64 = 1_000_000_000_000;
/// The most players in a pose datagram, which stays below a typical MTU.
pub const MAX_PLAYERS: usize = 16;
/// The longest player name in characters, system account names can be long, like first.last
/// names.
pub const MAX_NAME_LENGTH: usize = 32;
// The most edits in one response, the flowing fluid of a game tick.
const MAX_EDITS: usize = 65_536;
/// The longest chat message in characters.
pub const MAX_CHAT_LENGTH: usize = 100;
/// Input datagrams repeat the last inputs, so a lost datagram loses no input.
pub const REPEATED_INPUTS: usize = 3;

type Result<T> = core::result::Result<T, String>;

/// What a player asks the server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Request {
    /// Load or generate the lit terrain of a chunk.
    Terrain(Coord),
    /// Skip the terrain requests that are still queued.
    CancelTerrain,
    /// The current blocks of a chunk, the tag comes back with them.
    Blocks(Coord, u64),
    /// Place or break a block.
    Edit(Edit),
    /// The changed hotbar of the player.
    Hotbar(Hotbar),
    /// The controls of the next physics tick of the player.
    Input {
        /// The increasing number of the input.
        sequence: u32,
        /// The held controls and the view.
        controls: Controls,
    },
    /// A chat message or a command like `/time`.
    Chat(#[serde(serialize_with = "bounded")] String),
}

/// What the server tells a player.
#[derive(Clone, Serialize, Deserialize)]
pub enum Response {
    /// The lit terrain of a chunk, a canceled request has no chunk.
    Terrain {
        /// The chunk coordinate.
        coord: Coord,
        /// The world revision the chunk is at.
        revision: u64,
        /// The chunk, if it wasn't canceled.
        #[serde(with = "compressed")]
        chunk: Option<ChunkData>,
    },
    /// The current blocks of a chunk.
    Blocks {
        /// The chunk coordinate.
        coord: Coord,
        /// The tag of the request.
        tag: u64,
        /// The chunk, if it exists.
        #[serde(with = "compressed")]
        chunk: Option<ChunkData>,
    },
    /// Edits of a player or of flowing fluid were saved at a revision, or rejected without
    /// one.
    Edited {
        /// The changed blocks.
        edits: Vec<Edit>,
        /// The world revision of the saved edits.
        revision: Option<u64>,
        /// Who made the edits.
        by: ClientId,
    },
    /// The player's body after the input with the sequence number, and the other players.
    State {
        /// The sequence number of the last applied input.
        sequence: u32,
        /// The player's body.
        body: Body,
        /// The other players, an encoded state holds the first [`MAX_PLAYERS`].
        #[serde(serialize_with = "first_players")]
        others: Vec<PlayerInfo>,
    },
    /// A chat message of a player.
    Chat {
        /// The name of the sender.
        #[serde(serialize_with = "bounded")]
        name: String,
        /// The message.
        #[serde(serialize_with = "bounded")]
        text: String,
    },
    /// A message of the world, like someone joining or the answer to a command.
    Message(#[serde(serialize_with = "bounded")] String),
    /// The time of day changed.
    Time {
        /// Whether it is night.
        night: bool,
    },
    /// The world closed or the connection broke.
    Disconnected(#[serde(serialize_with = "bounded")] String),
}

/// What the client needs to know when it joins.
#[derive(Serialize, Deserialize)]
pub struct Welcome {
    /// The id of the joined player.
    pub id: ClientId,
    /// The saved player.
    pub player: Player,
    /// The current world revision.
    pub revision: u64,
}

/// The saved player of a world.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Player {
    /// The feet position.
    pub position: [f64; 3],
    /// The horizontal view angle.
    pub yaw: f32,
    /// The vertical view angle.
    pub pitch: f32,
    /// Whether the player flies.
    pub flying: bool,
    /// Whether it is night.
    pub night: bool,
    /// The hotbar.
    pub hotbar: Hotbar,
}

impl Player {
    /// The player of a camera.
    pub const fn new(camera: &Camera, night: bool, hotbar: Hotbar) -> Self {
        Self {
            position: camera.position,
            yaw: camera.yaw,
            pitch: camera.pitch,
            flying: camera.flying,
            night,
            hotbar,
        }
    }

    /// Encodes the player for saves.
    pub fn encode(&self) -> Vec<u8> {
        encode(self, Vec::new())
    }

    /// Decodes and validates a saved player.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let player: Self = decode(data)?;
        player.validate()?;
        Ok(player)
    }

    /// Checks that the player is somewhere in the world and holds valid blocks.
    pub fn validate(&self) -> Result<()> {
        let pose = Pose {
            position: self.position,
            yaw: self.yaw,
            pitch: self.pitch,
        };
        pose.validate()?;
        if self.pitch.abs() > 1.55 {
            return Err("Invalid player position".into());
        }
        validate_hotbar(&self.hotbar)
    }

    /// Moves the camera to the player.
    pub const fn restore(&self, camera: &mut Camera) {
        camera.position = self.position;
        camera.yaw = self.yaw;
        camera.pitch = self.pitch;
        camera.set_flying(self.flying);
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new(&Camera::default(), false, Hotbar::default())
    }
}

/// The hotbar of saved slots and a selected slot, when they are valid.
pub fn decode_hotbar(slots: &[u8], selected: i64) -> Result<Hotbar> {
    let hotbar = Hotbar {
        slots: slots.try_into().map_err(|_| "Invalid hotbar size")?,
        selected: usize::try_from(selected).map_err(|_| "Invalid selected hotbar slot")?,
    };
    validate_hotbar(&hotbar)?;
    Ok(hotbar)
}

fn validate_hotbar(hotbar: &Hotbar) -> Result<()> {
    if hotbar.selected >= SLOTS || !hotbar.slots.iter().all(|&m| world::valid_material(m)) {
        return Err("Invalid hotbar".into());
    }
    Ok(())
}

/// Where a player is and looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    /// The feet position.
    pub position: [f64; 3],
    /// The horizontal view angle.
    pub yaw: f32,
    /// The vertical view angle.
    pub pitch: f32,
}

impl Pose {
    // Checks that the pose is somewhere in the world.
    fn validate(&self) -> Result<()> {
        finite(
            self.position
                .into_iter()
                .chain([f64::from(self.yaw), f64::from(self.pitch)]),
        )?;
        if self
            .position
            .iter()
            .any(|value| value.abs() > MAX_POSITION as f64)
        {
            return Err("Invalid position".into());
        }
        Ok(())
    }
}

/// The held controls and the view of a player during a physics tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Controls {
    /// The held movement controls.
    pub input: Input,
    /// The horizontal view angle.
    pub yaw: f32,
    /// The vertical view angle.
    pub pitch: f32,
    /// Whether the player jumps up single blocks by walking into them.
    pub auto_jump: bool,
}

impl Controls {
    /// Turns the camera to the controls and holds them.
    pub const fn apply(self, camera: &mut Camera) {
        camera.input = self.input;
        camera.yaw = self.yaw;
        camera.pitch = self.pitch.clamp(-1.55, 1.55);
        camera.auto_jump = self.auto_jump;
    }

    fn validate(&self) -> Result<()> {
        let input = self.input;
        let axes = input.direction.into_iter().chain(input.stick);
        finite(axes.clone().chain([self.yaw, self.pitch]).map(f64::from))?;
        if axes.into_iter().any(|value| value.abs() > 1.0) {
            return Err("Invalid input".into());
        }
        Ok(())
    }
}

/// The physics state of a player, which the server decides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Body {
    /// The feet position.
    pub position: [f64; 3],
    /// The falling or jumping speed.
    pub vertical_velocity: f64,
    /// Whether the player stands on a block.
    pub grounded: bool,
    /// Whether the player flies.
    pub flying: bool,
}

impl Body {
    /// The body of a camera.
    pub const fn of(camera: &Camera) -> Self {
        Self {
            position: camera.position,
            vertical_velocity: camera.vertical_velocity,
            grounded: camera.grounded,
            flying: camera.flying,
        }
    }

    /// Moves the camera to the body.
    pub const fn apply(self, camera: &mut Camera) {
        camera.position = self.position;
        camera.vertical_velocity = self.vertical_velocity;
        camera.grounded = self.grounded;
        camera.flying = self.flying;
    }
}

/// Another player in the world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerInfo {
    /// The id of the player.
    pub id: ClientId,
    /// The name of the player.
    #[serde(serialize_with = "bounded")]
    pub name: String,
    /// Where the player is and looks.
    pub pose: Pose,
}

/// A world opened to the LAN.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Announcement {
    /// The TCP port of the world.
    pub port: u16,
    /// The name of the world.
    #[serde(serialize_with = "bounded")]
    pub world: String,
    /// The name of the host.
    #[serde(serialize_with = "bounded")]
    pub host: String,
    /// The number of players in the world.
    pub players: u8,
}

/// A TCP frame of a client.
#[derive(Serialize, Deserialize)]
pub enum ClientFrame {
    /// The first frame, which joins the world.
    Hello {
        /// The protocol [`VERSION`] of the client.
        version: u32,
        /// The player name.
        #[serde(serialize_with = "bounded")]
        name: String,
    },
    /// A request after joining.
    Request(Request),
}

/// A TCP frame of the server.
#[expect(
    clippy::large_enum_variant,
    reason = "Frames only live while they are encoded or decoded"
)]
#[derive(Serialize, Deserialize)]
pub enum ServerFrame {
    /// The client joined the world.
    Accepted {
        /// What the client needs to know.
        welcome: Welcome,
        /// The secret that authenticates the input datagrams.
        token: u64,
        /// The UDP port of the server.
        udp_port: u16,
    },
    /// The client can't join, with the reason.
    Rejected(#[serde(serialize_with = "bounded")] String),
    /// A response after joining.
    Response(Response),
}

/// A UDP datagram.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Datagram {
    /// The last inputs of a guest, the newest last.
    Inputs {
        /// The id of the guest.
        id: ClientId,
        /// The token of [`ServerFrame::Accepted`].
        token: u64,
        /// The sequence numbers and controls, an encoded datagram holds the newest
        /// [`REPEATED_INPUTS`].
        #[serde(serialize_with = "newest_inputs")]
        inputs: Vec<(u32, Controls)>,
    },
    /// The state of [`Response::State`].
    State {
        /// The sequence number of the last applied input.
        sequence: u32,
        /// The player's body.
        body: Body,
        /// The other players, an encoded state holds the first [`MAX_PLAYERS`].
        #[serde(serialize_with = "first_players")]
        others: Vec<PlayerInfo>,
    },
    /// A world opened to the LAN.
    Announce {
        /// The protocol [`VERSION`] of the host.
        version: u32,
        /// The world.
        announcement: Announcement,
    },
}

/// Whether a character may be part of a name, which also names files.
pub fn valid_char(ch: char) -> bool {
    !ch.is_control() && !"<>:\"/\\|?*".contains(ch)
}

/// Checks a player name and returns it trimmed.
pub fn validate_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        Err("Enter a player name".into())
    } else if name.chars().count() > MAX_NAME_LENGTH {
        Err(format!("Use at most {MAX_NAME_LENGTH} characters"))
    } else if !name.chars().all(valid_char) {
        Err("Names can't contain < > : \" / \\ | ? *".into())
    } else {
        Ok(name)
    }
}

fn encode<T: Serialize>(value: &T, prefix: Vec<u8>) -> Vec<u8> {
    postcard::to_extend(value, prefix).expect("Messages always encode")
}

// Text is cut off at the most a message holds.
fn bounded<S: Serializer>(value: &str, serializer: S) -> core::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(&value[..value.floor_char_boundary(MAX_STRING)])
}

fn first_players<S: Serializer>(
    others: &[PlayerInfo],
    serializer: S,
) -> core::result::Result<S::Ok, S::Error> {
    others[..others.len().min(MAX_PLAYERS)].serialize(serializer)
}

fn newest_inputs<S: Serializer>(
    inputs: &[(u32, Controls)],
    serializer: S,
) -> core::result::Result<S::Ok, S::Error> {
    inputs[inputs.len().saturating_sub(REPEATED_INPUTS)..].serialize(serializer)
}

// Decodes a whole message, extra bytes are an error.
fn decode<'a, T: Deserialize<'a>>(data: &'a [u8]) -> Result<T> {
    let (value, rest) =
        postcard::take_from_bytes(data).map_err(|error| format!("Invalid message: {error}"))?;
    if rest.is_empty() {
        Ok(value)
    } else {
        Err("Message is too long".into())
    }
}

fn finite(values: impl IntoIterator<Item = f64>) -> Result<()> {
    if values.into_iter().all(f64::is_finite) {
        Ok(())
    } else {
        Err("Invalid number".into())
    }
}

fn text(value: &str) -> Result<()> {
    if value.len() > MAX_STRING {
        return Err("Text is too long".into());
    }
    Ok(())
}

fn chat(value: &str) -> Result<()> {
    if value.chars().count() > MAX_CHAT_LENGTH {
        return Err("Chat message is too long".into());
    }
    text(value)
}

// Checks that a block is somewhere in the world.
fn block(position: [i64; 3]) -> Result<()> {
    if position
        .iter()
        .any(|value| value.unsigned_abs() > MAX_POSITION as u64)
    {
        return Err("Invalid block position".into());
    }
    Ok(())
}

// Checks that a chunk is somewhere in the world.
fn chunk((x, z): Coord) -> Result<()> {
    let limit = (MAX_POSITION / CHUNK_SIZE as i64) as u64;
    if x.unsigned_abs() > limit || z.unsigned_abs() > limit {
        return Err("Invalid chunk".into());
    }
    Ok(())
}

fn validate_state(body: &Body, others: &[PlayerInfo]) -> Result<()> {
    finite(body.position.into_iter().chain([body.vertical_velocity]))?;
    if others.len() > MAX_PLAYERS {
        return Err("Too many players".into());
    }
    for player in others {
        text(&player.name)?;
        player.pose.validate()?;
    }
    Ok(())
}

impl Request {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Terrain(coord) | Self::Blocks(coord, _) => chunk(*coord),
            Self::Edit(edit) => block(edit.position),
            Self::Hotbar(hotbar) => validate_hotbar(hotbar),
            Self::Input { controls, .. } => controls.validate(),
            Self::Chat(text) => chat(text),
            Self::CancelTerrain => Ok(()),
        }
    }
}

impl Response {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Terrain { coord, .. } | Self::Blocks { coord, .. } => chunk(*coord),
            Self::Edited { edits, .. } if edits.len() > MAX_EDITS => Err("Too many edits".into()),
            Self::Edited { edits, .. } => edits.iter().try_for_each(|edit| block(edit.position)),
            Self::State { body, others, .. } => validate_state(body, others),
            Self::Chat {
                name,
                text: message,
            } => text(name).and(chat(message)),
            Self::Message(message) | Self::Disconnected(message) => text(message),
            Self::Time { .. } => Ok(()),
        }
    }
}

impl ClientFrame {
    /// Encodes the frame.
    pub fn encode(&self) -> Vec<u8> {
        encode(self, Vec::new())
    }

    /// Decodes and validates a frame.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let frame: Self = decode(data)?;
        match &frame {
            Self::Hello { name, .. } => text(name)?,
            Self::Request(request) => request.validate()?,
        }
        Ok(frame)
    }
}

impl ServerFrame {
    /// Encodes the frame.
    pub fn encode(&self) -> Vec<u8> {
        encode(self, Vec::new())
    }

    /// Decodes and validates a frame.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let frame: Self = decode(data)?;
        match &frame {
            Self::Accepted { welcome, .. } => welcome.player.validate()?,
            Self::Rejected(reason) => text(reason)?,
            Self::Response(response) => response.validate()?,
        }
        Ok(frame)
    }
}

impl Datagram {
    /// Encodes the datagram.
    pub fn encode(&self) -> Vec<u8> {
        encode(self, MAGIC.to_vec())
    }

    /// Decodes and validates a datagram, other UDP traffic is rejected.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let data = data
            .strip_prefix(&MAGIC)
            .ok_or("Not a PlaatCraft datagram")?;
        let datagram: Self = decode(data)?;
        match &datagram {
            Self::Inputs { inputs, .. } => {
                if inputs.len() > REPEATED_INPUTS {
                    return Err("Too many inputs".into());
                }
                for (_, controls) in inputs {
                    controls.validate()?;
                }
            }
            Self::State { body, others, .. } => validate_state(body, others)?,
            Self::Announce { announcement, .. } => {
                text(&announcement.world)?;
                text(&announcement.host)?;
            }
        }
        Ok(datagram)
    }
}

// Chunks go over the network as their compressed encoding.
mod compressed {
    use alloc::vec::Vec;

    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::MAX_BLOB_SIZE;
    use crate::world::ChunkData;

    pub(super) fn serialize<S: Serializer>(
        chunk: &Option<ChunkData>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        chunk
            .as_ref()
            .map(|chunk| deflate::zlib_compress(&chunk.encode(), deflate::Level::Fast))
            .serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<ChunkData>, D::Error> {
        Option::<Vec<u8>>::deserialize(deserializer)?
            .map(|data| {
                let raw = deflate::zlib_decompress(&data, MAX_BLOB_SIZE)
                    .map_err(|error| D::Error::custom(format_args!("Invalid chunk: {error}")))?;
                ChunkData::decode(&raw).map_err(D::Error::custom)
            })
            .transpose()
    }
}

/// The bytes of the length prefix of a TCP frame.
pub const FRAME_HEADER: usize = 4;

/// An encoded message with its length prefix, a TCP frame.
pub fn frame(message: &[u8]) -> Vec<u8> {
    let mut data = Vec::with_capacity(FRAME_HEADER + message.len());
    data.extend_from_slice(&(message.len() as u32).to_le_bytes());
    data.extend_from_slice(message);
    data
}

/// The message length of a frame's length prefix, larger frames are an error.
pub fn frame_length(header: [u8; FRAME_HEADER]) -> Result<usize> {
    let length = u32::from_le_bytes(header) as usize;
    if length > MAX_FRAME {
        return Err("Frame is too large".into());
    }
    Ok(length)
}

/// Writes a length prefixed frame.
#[cfg(feature = "std")]
pub fn write_frame(stream: &mut impl Write, message: &[u8]) -> io::Result<()> {
    stream.write_all(&frame(message))
}

/// Reads a length prefixed frame.
#[cfg(feature = "std")]
pub fn read_frame(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0; FRAME_HEADER];
    stream.read_exact(&mut header)?;
    let length =
        frame_length(header).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut message = vec![0; length];
    stream.read_exact(&mut message)?;
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::WORLD_SEED;

    fn pose() -> Pose {
        Pose {
            position: [-12.5, 70.25, 3.0],
            yaw: 1.25,
            pitch: -0.5,
        }
    }

    fn controls() -> Controls {
        Controls {
            input: Input {
                direction: [1.0, -1.0],
                stick: [0.25, 0.5],
                jump: true,
                sneak: false,
                fast: true,
            },
            yaw: 2.0,
            pitch: -0.5,
            auto_jump: true,
        }
    }

    fn body() -> Body {
        Body {
            position: [1.5, 70.0, -3.25],
            vertical_velocity: -4.0,
            grounded: false,
            flying: true,
        }
    }

    #[test]
    fn requests_roundtrip_through_frames() {
        let requests = [
            Request::Terrain((-3, 9)),
            Request::CancelTerrain,
            Request::Blocks((1, -1), 42),
            Request::Edit(Edit {
                position: [-5, 64, 300],
                material: 3,
            }),
            Request::Hotbar(Hotbar::default()),
            Request::Input {
                sequence: 7,
                controls: controls(),
            },
            Request::Chat("/time".into()),
        ];
        for request in requests {
            let mut stream = Vec::new();
            write_frame(&mut stream, &ClientFrame::Request(request.clone()).encode()).unwrap();
            let frame = read_frame(&mut stream.as_slice()).unwrap();
            let ClientFrame::Request(decoded) = ClientFrame::decode(&frame).unwrap() else {
                panic!("Not a request");
            };
            assert_eq!(decoded, request);
        }
        let hello = ClientFrame::Hello {
            version: VERSION,
            name: "Bastiaan".into(),
        }
        .encode();
        assert!(matches!(
            ClientFrame::decode(&hello).unwrap(),
            ClientFrame::Hello { version: VERSION, ref name } if name == "Bastiaan"
        ));
        let long = ClientFrame::Request(Request::Chat("x".repeat(MAX_CHAT_LENGTH + 1))).encode();
        assert!(ClientFrame::decode(&long).is_err());
        let hotbar = Hotbar {
            selected: SLOTS,
            ..Hotbar::default()
        };
        assert!(
            ClientFrame::decode(&ClientFrame::Request(Request::Hotbar(hotbar)).encode()).is_err()
        );
        let far = Request::Edit(Edit {
            position: [i64::MIN, 64, 0],
            material: 3,
        });
        assert!(ClientFrame::decode(&ClientFrame::Request(far).encode()).is_err());
        let far = Request::Terrain((MAX_POSITION, 0));
        assert!(ClientFrame::decode(&ClientFrame::Request(far).encode()).is_err());
    }

    #[test]
    fn long_text_is_cut_off_at_a_character() {
        let answer = format!(
            "Unknown command /{}, try /help",
            "\u{e9}".repeat(MAX_CHAT_LENGTH)
        );
        let frame = ServerFrame::Response(Response::Message(answer.clone())).encode();
        let ServerFrame::Response(Response::Message(decoded)) =
            ServerFrame::decode(&frame).unwrap()
        else {
            panic!("Not a message");
        };
        assert!(decoded.len() <= MAX_STRING && answer.starts_with(&decoded));
    }

    #[test]
    fn responses_compress_chunks_and_roundtrip() {
        let chunk = ChunkData::generate(WORLD_SEED, (0, 0));
        let frame = ServerFrame::Response(Response::Terrain {
            coord: (0, 0),
            revision: 7,
            chunk: Some(chunk.clone()),
        })
        .encode();
        assert!(frame.len() < chunk.encode().len() / 2);
        let ServerFrame::Response(Response::Terrain {
            revision: 7,
            chunk: Some(decoded),
            ..
        }) = ServerFrame::decode(&frame).unwrap()
        else {
            panic!("Not terrain");
        };
        assert_eq!(decoded.encode(), chunk.encode());

        let responses = [
            Response::Blocks {
                coord: (2, 3),
                tag: 4,
                chunk: None,
            },
            Response::Edited {
                edits: vec![Edit {
                    position: [-1, 64, 3],
                    material: 22,
                }],
                revision: Some(9),
                by: 3,
            },
            Response::Message("Bob joined the game".into()),
            Response::Chat {
                name: "Bob".into(),
                text: "Hello!".into(),
            },
            Response::Time { night: true },
            Response::State {
                sequence: 3,
                body: body(),
                others: Vec::new(),
            },
            Response::Disconnected("The host closed the world".into()),
        ];
        for response in responses {
            let frame = ServerFrame::Response(response).encode();
            let ServerFrame::Response(decoded) = ServerFrame::decode(&frame).unwrap() else {
                panic!("Not a response");
            };
            assert_eq!(ServerFrame::Response(decoded).encode(), frame);
        }

        let welcome = ServerFrame::Accepted {
            welcome: Welcome {
                id: 3,
                player: Player::default(),
                revision: 11,
            },
            token: 99,
            udp_port: GAME_PORT,
        };
        assert!(matches!(
            ServerFrame::decode(&welcome.encode()).unwrap(),
            ServerFrame::Accepted {
                welcome: Welcome {
                    id: 3,
                    revision: 11,
                    ..
                },
                token: 99,
                udp_port: GAME_PORT
            }
        ));
    }

    #[test]
    fn datagrams_roundtrip_and_reject_garbage() {
        let players = (0..20)
            .map(|id| PlayerInfo {
                id,
                name: format!("{id:0>MAX_NAME_LENGTH$}"),
                pose: pose(),
            })
            .collect::<Vec<_>>();
        let state = |others: Vec<PlayerInfo>| Datagram::State {
            sequence: 9,
            body: body(),
            others,
        };
        let encoded = state(players.clone()).encode();
        assert!(encoded.len() < 1400);
        assert_eq!(
            Datagram::decode(&encoded).unwrap(),
            state(players[..MAX_PLAYERS].to_vec())
        );
        let inputs = |count: u32, controls: Controls| Datagram::Inputs {
            id: 4,
            token: 5,
            inputs: (1..=count).map(|sequence| (sequence, controls)).collect(),
        };
        // Only the newest inputs are repeated.
        assert_eq!(
            Datagram::decode(&inputs(5, controls()).encode()).unwrap(),
            Datagram::Inputs {
                id: 4,
                token: 5,
                inputs: (3..=5).map(|sequence| (sequence, controls())).collect(),
            }
        );
        let announce = Datagram::Announce {
            version: VERSION,
            announcement: Announcement {
                port: GAME_PORT,
                world: "Island".into(),
                host: "Bastiaan".into(),
                players: 2,
            },
        };
        assert_eq!(Datagram::decode(&announce.encode()).unwrap(), announce);
        assert!(Datagram::decode(b"HTTP/1.1 200 OK").is_err());
        let mut truncated = state(players).encode();
        truncated.pop();
        assert!(Datagram::decode(&truncated).is_err());
        let mut long = announce.encode();
        long.push(0);
        assert!(Datagram::decode(&long).is_err());
        // Out of range sticks and NaN views are rejected.
        let nan = Controls {
            pitch: f32::NAN,
            ..controls()
        };
        assert!(Datagram::decode(&inputs(1, nan).encode()).is_err());
        let mut far = controls();
        far.input.stick[0] = 5.0;
        assert!(Datagram::decode(&inputs(1, far).encode()).is_err());
    }

    #[test]
    fn players_roundtrip_and_reject_invalid_saves() {
        let player = Player {
            position: [10.0, 80.0, -30.0],
            flying: true,
            ..Player::default()
        };
        assert_eq!(Player::decode(&player.encode()).unwrap(), player);
        assert!(Player::decode(&player.encode()[1..]).is_err());
        let far = Player {
            position: [1e13, 0.0, 0.0],
            ..player
        };
        assert!(Player::decode(&far.encode()).is_err());
    }

    #[test]
    fn frames_reject_oversized_lengths() {
        let data = u32::MAX.to_le_bytes();
        assert!(read_frame(&mut data.as_slice()).is_err());
    }

    #[test]
    fn player_names_are_short_and_safe() {
        assert_eq!(validate_name(" Steve "), Ok("Steve"));
        assert!(validate_name("").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name(&"x".repeat(MAX_NAME_LENGTH + 1)).is_err());
        assert!(valid_char('é') && !valid_char('/') && !valid_char('\n'));
    }
}
