/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The LAN wire format. TCP carries length prefixed frames for joining, chunks, blocks,
//! edits, and chat, UDP datagrams carry the frequent inputs, player states, and LAN
//! announcements.
//! The integrated server of single player passes the same messages without encoding them.

use std::io::{self, Read, Write};

use plaatcraft_engine::camera::{Camera, Input};
use plaatcraft_engine::world::{ChunkData, Coord, Edit};

use crate::database::{MAX_BLOB_SIZE, Player};
use crate::inventory::{Hotbar, SLOTS};
use crate::server::{ClientId, Request, Response, Welcome};

/// Changes whenever the messages change, clients and servers must match.
pub(crate) const VERSION: u32 = 3;
/// The TCP and UDP port of an opened world, unassigned by IANA.
pub(crate) const GAME_PORT: u16 = 47820;
/// The UDP port LAN worlds are announced on.
pub(crate) const DISCOVERY_PORT: u16 = 47821;
const MAGIC: [u8; 4] = *b"PLCR";
const MAX_FRAME: usize = 4 * 1024 * 1024;
const MAX_STRING: usize = 256;
/// The most players in a pose datagram, which stays below a typical MTU.
pub(crate) const MAX_PLAYERS: usize = 16;
// System account names can be long, like first.last names.
pub(crate) const MAX_NAME_LENGTH: usize = 32;
// The most edits in one response, the flowing fluid of a game tick.
const MAX_EDITS: usize = 65_536;
/// The longest chat message in characters.
pub(crate) const MAX_CHAT_LENGTH: usize = 100;
// Input datagrams repeat the last inputs, so a lost datagram loses no input.
pub(crate) const REPEATED_INPUTS: usize = 3;

type Result<T> = std::result::Result<T, String>;

/// Where a player is and looks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Pose {
    pub(crate) position: [f64; 3],
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
}

/// The held controls and the view of a player during a physics tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Controls {
    pub(crate) input: Input,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) auto_jump: bool,
}

impl Controls {
    /// Turns the camera to the controls and holds them.
    pub(crate) const fn apply(self, camera: &mut Camera) {
        camera.input = self.input;
        camera.yaw = self.yaw;
        camera.pitch = self.pitch;
        camera.auto_jump = self.auto_jump;
    }
}

/// The physics state of a player, which the server decides.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Body {
    pub(crate) position: [f64; 3],
    pub(crate) vertical_velocity: f64,
    pub(crate) grounded: bool,
    pub(crate) flying: bool,
}

impl Body {
    pub(crate) const fn of(camera: &Camera) -> Self {
        Self {
            position: camera.position,
            vertical_velocity: camera.vertical_velocity,
            grounded: camera.grounded,
            flying: camera.flying,
        }
    }

    pub(crate) const fn apply(self, camera: &mut Camera) {
        camera.position = self.position;
        camera.vertical_velocity = self.vertical_velocity;
        camera.grounded = self.grounded;
        camera.flying = self.flying;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlayerInfo {
    pub(crate) id: ClientId,
    pub(crate) name: String,
    pub(crate) pose: Pose,
}

/// A world opened to the LAN.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Announcement {
    pub(crate) port: u16,
    pub(crate) world: String,
    pub(crate) host: String,
    pub(crate) players: u8,
}

pub(crate) enum ClientFrame {
    Hello { version: u32, name: String },
    Request(Request),
}

#[expect(
    clippy::large_enum_variant,
    reason = "Frames only live while they are encoded or decoded"
)]
pub(crate) enum ServerFrame {
    Accepted {
        welcome: Welcome,
        token: u64,
        udp_port: u16,
    },
    Rejected(String),
    Response(Response),
}

#[derive(Debug, PartialEq)]
pub(crate) enum Datagram {
    /// The last inputs of a guest, the newest last.
    Inputs {
        id: ClientId,
        token: u64,
        inputs: Vec<(u32, Controls)>,
    },
    State {
        sequence: u32,
        body: Body,
        others: Vec<PlayerInfo>,
    },
    Announce {
        version: u32,
        announcement: Announcement,
    },
}

/// Checks a player name and returns it trimmed.
pub(crate) fn validate_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        Err("Enter a player name".into())
    } else if name.chars().count() > MAX_NAME_LENGTH {
        Err(format!("Use at most {MAX_NAME_LENGTH} characters"))
    } else if !name.chars().all(crate::worlds::valid_char) {
        Err("Names can't contain < > : \" / \\ | ? *".into())
    } else {
        Ok(name)
    }
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, value: u8) -> &mut Self {
        self.0.push(value);
        self
    }
    fn u16(&mut self, value: u16) -> &mut Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    fn u32(&mut self, value: u32) -> &mut Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    fn u64(&mut self, value: u64) -> &mut Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    fn i64(&mut self, value: i64) -> &mut Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    fn f32(&mut self, value: f32) -> &mut Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    fn f64(&mut self, value: f64) -> &mut Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.u32(value.len() as u32);
        self.0.extend_from_slice(value);
        self
    }
    fn string(&mut self, value: &str) -> &mut Self {
        let mut end = value.len().min(MAX_STRING);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        self.u16(end as u16);
        self.0.extend_from_slice(&value.as_bytes()[..end]);
        self
    }
    fn coord(&mut self, (x, z): Coord) -> &mut Self {
        self.i64(x).i64(z)
    }
    fn edit(&mut self, edit: Edit) -> &mut Self {
        for value in edit.position {
            self.i64(value);
        }
        self.u8(edit.material)
    }
    fn chunk(&mut self, chunk: Option<&ChunkData>) -> &mut Self {
        match chunk {
            Some(chunk) => self.u8(1).bytes(&deflate::zlib_compress(
                &chunk.encode(),
                deflate::Level::Fast,
            )),
            None => self.u8(0),
        }
    }
    fn pose(&mut self, pose: Pose) -> &mut Self {
        for value in pose.position {
            self.f64(value);
        }
        self.f32(pose.yaw).f32(pose.pitch)
    }
    fn controls(&mut self, controls: Controls) -> &mut Self {
        let input = controls.input;
        for value in input.direction.into_iter().chain(input.stick) {
            self.f32(value);
        }
        let flags = [input.jump, input.sneak, input.fast, controls.auto_jump]
            .into_iter()
            .enumerate()
            .fold(0, |flags, (bit, set)| flags | u8::from(set) << bit);
        self.u8(flags).f32(controls.yaw).f32(controls.pitch)
    }
    fn body(&mut self, body: Body) -> &mut Self {
        for value in body.position {
            self.f64(value);
        }
        self.f64(body.vertical_velocity)
            .u8(u8::from(body.grounded) | u8::from(body.flying) << 1)
    }
    fn state(&mut self, sequence: u32, body: Body, others: &[PlayerInfo]) -> &mut Self {
        self.u32(sequence).body(body);
        write_players(self, others);
        self
    }
}

struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8]> {
        if size > self.data.len() {
            return Err("Message is too short".into());
        }
        let (taken, rest) = self.data.split_at(size);
        self.data = rest;
        Ok(taken)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().expect("Taken size"))
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.array()?))
    }
    fn f32(&mut self) -> Result<f32> {
        let value = f32::from_le_bytes(self.array()?);
        value
            .is_finite()
            .then_some(value)
            .ok_or("Invalid number".into())
    }
    fn f64(&mut self) -> Result<f64> {
        let value = f64::from_le_bytes(self.array()?);
        value
            .is_finite()
            .then_some(value)
            .ok_or("Invalid number".into())
    }
    fn bytes(&mut self) -> Result<&'a [u8]> {
        let size = self.u32()? as usize;
        self.take(size)
    }
    fn string(&mut self) -> Result<String> {
        let size = usize::from(self.u16()?);
        if size > MAX_STRING {
            return Err("Text is too long".into());
        }
        String::from_utf8(self.take(size)?.to_vec()).map_err(|_| "Invalid text".into())
    }
    fn coord(&mut self) -> Result<Coord> {
        Ok((self.i64()?, self.i64()?))
    }
    fn edit(&mut self) -> Result<Edit> {
        Ok(Edit {
            position: [self.i64()?, self.i64()?, self.i64()?],
            material: self.u8()?,
        })
    }
    fn chunk(&mut self) -> Result<Option<ChunkData>> {
        if self.u8()? == 0 {
            return Ok(None);
        }
        let data = deflate::zlib_decompress(self.bytes()?, MAX_BLOB_SIZE)
            .map_err(|error| format!("Invalid compressed chunk: {error}"))?;
        ChunkData::decode(&data).map(Some)
    }
    fn pose(&mut self) -> Result<Pose> {
        Ok(Pose {
            position: [self.f64()?, self.f64()?, self.f64()?],
            yaw: self.f32()?,
            pitch: self.f32()?,
        })
    }
    fn player(&mut self) -> Result<Player> {
        Player::decode(self.take(Player::SIZE)?)
    }
    fn controls(&mut self) -> Result<Controls> {
        let direction = [self.f32()?, self.f32()?];
        let stick = [self.f32()?, self.f32()?];
        if direction
            .into_iter()
            .chain(stick)
            .any(|value| value.abs() > 1.0)
        {
            return Err("Invalid input".into());
        }
        let flags = self.u8()?;
        let bit = |index: u8| flags & 1 << index != 0;
        Ok(Controls {
            input: Input {
                direction,
                stick,
                jump: bit(0),
                sneak: bit(1),
                fast: bit(2),
            },
            auto_jump: bit(3),
            yaw: self.f32()?,
            pitch: self.f32()?.clamp(-1.55, 1.55),
        })
    }
    fn body(&mut self) -> Result<Body> {
        let position = [self.f64()?, self.f64()?, self.f64()?];
        let vertical_velocity = self.f64()?;
        let flags = self.u8()?;
        Ok(Body {
            position,
            vertical_velocity,
            grounded: flags & 1 != 0,
            flying: flags & 2 != 0,
        })
    }
    fn hotbar(&mut self) -> Result<Hotbar> {
        let slots = self.take(SLOTS)?;
        crate::database::decode_hotbar(slots, i64::from(self.u8()?))
    }
    fn chat(&mut self) -> Result<String> {
        let text = self.string()?;
        if text.chars().count() > MAX_CHAT_LENGTH {
            return Err("Chat message is too long".into());
        }
        Ok(text)
    }
    fn finish<T>(self, value: T) -> Result<T> {
        if self.data.is_empty() {
            Ok(value)
        } else {
            Err("Message is too long".into())
        }
    }
}

impl Request {
    fn write(&self, w: &mut Writer) {
        match self {
            Self::Terrain(coord) => w.u8(1).coord(*coord),
            Self::CancelTerrain => w.u8(2),
            Self::Blocks(coord, tag) => w.u8(3).coord(*coord).u64(*tag),
            Self::Edit(edit) => w.u8(4).edit(*edit),
            Self::Hotbar(hotbar) => {
                w.u8(5);
                w.0.extend_from_slice(&hotbar.slots);
                w.u8(hotbar.selected as u8)
            }
            Self::Input { sequence, controls } => w.u8(6).u32(*sequence).controls(*controls),
            Self::Chat(text) => w.u8(7).string(text),
        };
    }

    fn read(tag: u8, r: &mut Reader<'_>) -> Result<Self> {
        Ok(match tag {
            1 => Self::Terrain(r.coord()?),
            2 => Self::CancelTerrain,
            3 => Self::Blocks(r.coord()?, r.u64()?),
            4 => Self::Edit(r.edit()?),
            5 => Self::Hotbar(r.hotbar()?),
            6 => Self::Input {
                sequence: r.u32()?,
                controls: r.controls()?,
            },
            7 => Self::Chat(r.chat()?),
            _ => return Err("Unknown request".into()),
        })
    }
}

impl Response {
    fn write(&self, w: &mut Writer) {
        match self {
            Self::Terrain {
                coord,
                revision,
                chunk,
            } => w.u8(1).coord(*coord).u64(*revision).chunk(chunk.as_ref()),
            Self::Blocks { coord, tag, chunk } => {
                w.u8(2).coord(*coord).u64(*tag).chunk(chunk.as_ref())
            }
            Self::Edited {
                edits,
                revision,
                by,
            } => {
                w.u8(3).u32(*by).u32(edits.len() as u32);
                for edit in edits {
                    w.edit(*edit);
                }
                match revision {
                    Some(revision) => w.u8(1).u64(*revision),
                    None => w.u8(0),
                }
            }
            Self::State {
                sequence,
                body,
                others,
            } => w.u8(4).state(*sequence, *body, others),
            Self::Chat { name, text } => w.u8(5).string(name).string(text),
            Self::Message(text) => w.u8(6).string(text),
            Self::Time { night } => w.u8(7).u8(u8::from(*night)),
            Self::Disconnected(reason) => w.u8(8).string(reason),
        };
    }

    fn read(tag: u8, r: &mut Reader<'_>) -> Result<Self> {
        Ok(match tag {
            1 => Self::Terrain {
                coord: r.coord()?,
                revision: r.u64()?,
                chunk: r.chunk()?,
            },
            2 => Self::Blocks {
                coord: r.coord()?,
                tag: r.u64()?,
                chunk: r.chunk()?,
            },
            3 => Self::Edited {
                by: r.u32()?,
                edits: {
                    let count = r.u32()? as usize;
                    if count > MAX_EDITS {
                        return Err("Too many edits".into());
                    }
                    (0..count).map(|_| r.edit()).collect::<Result<_>>()?
                },
                revision: if r.u8()? == 0 { None } else { Some(r.u64()?) },
            },
            4 => Self::State {
                sequence: r.u32()?,
                body: r.body()?,
                others: read_players(r)?,
            },
            5 => Self::Chat {
                name: r.string()?,
                text: r.chat()?,
            },
            6 => Self::Message(r.string()?),
            7 => Self::Time {
                night: r.u8()? != 0,
            },
            8 => Self::Disconnected(r.string()?),
            _ => return Err("Unknown response".into()),
        })
    }
}

fn write_players(w: &mut Writer, players: &[PlayerInfo]) {
    let players = &players[..players.len().min(MAX_PLAYERS)];
    w.u8(players.len() as u8);
    for player in players {
        w.u32(player.id).string(&player.name).pose(player.pose);
    }
}

fn read_players(r: &mut Reader<'_>) -> Result<Vec<PlayerInfo>> {
    (0..r.u8()?)
        .map(|_| {
            Ok(PlayerInfo {
                id: r.u32()?,
                name: r.string()?,
                pose: r.pose()?,
            })
        })
        .collect()
}

impl ClientFrame {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        match self {
            Self::Hello { version, name } => {
                w.u8(0).u32(*version).string(name);
            }
            Self::Request(request) => request.write(&mut w),
        }
        w.0
    }

    pub(crate) fn decode(data: &[u8]) -> Result<Self> {
        let mut r = Reader { data };
        let frame = match r.u8()? {
            0 => Self::Hello {
                version: r.u32()?,
                name: r.string()?,
            },
            tag => Self::Request(Request::read(tag, &mut r)?),
        };
        r.finish(frame)
    }
}

impl ServerFrame {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        match self {
            Self::Accepted {
                welcome,
                token,
                udp_port,
            } => {
                w.u8(100).u32(welcome.id).u64(welcome.revision);
                w.0.extend_from_slice(&welcome.player.encode());
                w.u64(*token).u16(*udp_port);
            }
            Self::Rejected(reason) => {
                w.u8(101).string(reason);
            }
            Self::Response(response) => response.write(&mut w),
        }
        w.0
    }

    pub(crate) fn decode(data: &[u8]) -> Result<Self> {
        let mut r = Reader { data };
        let frame = match r.u8()? {
            100 => Self::Accepted {
                welcome: Welcome {
                    id: r.u32()?,
                    revision: r.u64()?,
                    player: r.player()?,
                },
                token: r.u64()?,
                udp_port: r.u16()?,
            },
            101 => Self::Rejected(r.string()?),
            tag => Self::Response(Response::read(tag, &mut r)?),
        };
        r.finish(frame)
    }
}

impl Datagram {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.0.extend_from_slice(&MAGIC);
        match self {
            Self::Inputs { id, token, inputs } => {
                let inputs = &inputs[inputs.len().saturating_sub(REPEATED_INPUTS)..];
                w.u8(0).u32(*id).u64(*token).u8(inputs.len() as u8);
                for &(sequence, controls) in inputs {
                    w.u32(sequence).controls(controls);
                }
            }
            Self::State {
                sequence,
                body,
                others,
            } => {
                w.u8(1).state(*sequence, *body, others);
            }
            Self::Announce {
                version,
                announcement,
            } => {
                w.u8(2)
                    .u32(*version)
                    .u16(announcement.port)
                    .string(&announcement.world)
                    .string(&announcement.host)
                    .u8(announcement.players);
            }
        }
        w.0
    }

    pub(crate) fn decode(data: &[u8]) -> Result<Self> {
        let mut r = Reader { data };
        if r.take(MAGIC.len())? != MAGIC {
            return Err("Not a PlaatCraft datagram".into());
        }
        let datagram = match r.u8()? {
            0 => Self::Inputs {
                id: r.u32()?,
                token: r.u64()?,
                inputs: (0..r.u8()?.min(REPEATED_INPUTS as u8))
                    .map(|_| Ok((r.u32()?, r.controls()?)))
                    .collect::<Result<_>>()?,
            },
            1 => Self::State {
                sequence: r.u32()?,
                body: r.body()?,
                others: read_players(&mut r)?,
            },
            2 => Self::Announce {
                version: r.u32()?,
                announcement: Announcement {
                    port: r.u16()?,
                    world: r.string()?,
                    host: r.string()?,
                    players: r.u8()?,
                },
            },
            _ => return Err("Unknown datagram".into()),
        };
        r.finish(datagram)
    }
}

pub(crate) fn write_frame(stream: &mut impl Write, frame: &[u8]) -> io::Result<()> {
    let mut data = Vec::with_capacity(4 + frame.len());
    data.extend_from_slice(&(frame.len() as u32).to_le_bytes());
    data.extend_from_slice(frame);
    stream.write_all(&data)
}

pub(crate) fn read_frame(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut size = [0; 4];
    stream.read_exact(&mut size)?;
    let size = u32::from_le_bytes(size) as usize;
    if size > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Frame is too large",
        ));
    }
    let mut frame = vec![0; size];
    stream.read_exact(&mut frame)?;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use plaatcraft_engine::world::WORLD_SEED;

    use super::*;

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
            Request::Chat("/time night".into()),
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
                edits: vec![
                    Edit {
                        position: [1, 2, 3],
                        material: 0,
                    },
                    Edit {
                        position: [-1, 64, 3],
                        material: 22,
                    },
                ],
                revision: Some(9),
                by: 3,
            },
            Response::Edited {
                edits: vec![Edit {
                    position: [1, 2, 3],
                    material: 5,
                }],
                revision: None,
                by: 0,
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
        let inputs = |count: u32| Datagram::Inputs {
            id: 4,
            token: 5,
            inputs: (1..=count).map(|sequence| (sequence, controls())).collect(),
        };
        // Only the newest inputs are repeated.
        assert_eq!(
            Datagram::decode(&inputs(5).encode()).unwrap(),
            Datagram::Inputs {
                id: 4,
                token: 5,
                inputs: (3..=5).map(|sequence| (sequence, controls())).collect(),
            }
        );
        for datagram in [
            inputs(1),
            Datagram::Announce {
                version: VERSION,
                announcement: Announcement {
                    port: GAME_PORT,
                    world: "Island".into(),
                    host: "Bastiaan".into(),
                    players: 2,
                },
            },
        ] {
            assert_eq!(Datagram::decode(&datagram.encode()).unwrap(), datagram);
        }
        assert!(Datagram::decode(b"HTTP/1.1 200 OK").is_err());
        let mut truncated = state(players).encode();
        truncated.pop();
        assert!(Datagram::decode(&truncated).is_err());
        // Out of range sticks and NaN views are rejected.
        let mut nan = inputs(1).encode();
        let end = nan.len();
        nan[end - 4..].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(Datagram::decode(&nan).is_err());
        let mut far = inputs(1).encode();
        let stick = far.len() - 25;
        far[stick..stick + 4].copy_from_slice(&5.0f32.to_le_bytes());
        assert!(Datagram::decode(&far).is_err());
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
    }
}
