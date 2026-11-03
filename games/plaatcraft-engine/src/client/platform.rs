/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! What a port gives the client: its threads, sockets, GPU, sound output and controllers.

use alloc::boxed::Box;
use alloc::string::String;

use super::input::{Context, Pad};
use super::mesh::Mesh;
use crate::menu;
use crate::server::Limits;
use crate::sound::Effect;

/// A 3x4 model view matrix of rows, like libogc's `Mtx`.
pub type Mtx = [[f32; 4]; 3];
/// A 4x4 projection matrix of rows, like libogc's `Mtx44`.
pub type Mtx44 = [[f32; 4]; 4];

/// The console the client runs on.
pub trait Platform: Sized + 'static {
    /// A mutex of the system's threads.
    type Lock: Lock;
    /// A condition variable that waits with a [`Platform::Lock`].
    type Signal: Signal<Self::Lock>;
    /// A TCP or UDP socket.
    type Socket: Socket;
    /// The GPU that draws the game.
    type Gpu: Gpu;
    /// The output of the sound effects.
    type Audio: Audio;
    /// The controllers the player plays with.
    type Controller: Controller;

    /// The name of the console in messages, like "Wii".
    const NAME: &'static str;
    /// How the menus look on the console.
    const MENU: menu::Platform;
    /// How much fluid flows, consoles have little time per frame.
    const LIMITS: Limits;
    /// The fixed render distance in chunks.
    const RENDER_DISTANCE: i64;
    /// The most guests of a world opened to the LAN.
    const MAX_GUESTS: usize;
    /// The controls of the inventory, shown below its blocks.
    const INVENTORY_HELP: &'static str;

    /// Starts a thread that runs forever.
    fn spawn(thread: Thread, entry: Box<dyn FnOnce() + Send>);
    /// The seconds since some time in the past, which only go forward.
    fn seconds() -> f64;
    /// A random number, for world seeds and connection tokens.
    fn random() -> u64;
    /// Waits about a frame, the network thread polls its sockets in between.
    fn idle();
    /// Connects to the network, returns the console's IPv4 address or an error code.
    fn connect_network() -> Result<[u8; 4], i32>;
    /// Mounts the SD card again, so a replaced card is noticed, returns whether it's there.
    fn mount_storage() -> bool;
    /// The name of the console's user, when it's set.
    fn nickname() -> Option<String>;
    /// Whether the game keeps running, false once the console powers off or quits.
    fn running() -> bool;
}

/// The threads besides the main thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thread {
    /// Loads, generates and meshes chunks and saves the world, it runs while the main
    /// thread waits for the next frame.
    Worker,
    /// Polls the LAN sockets.
    Network,
}

/// A mutex of the system's threads, it doesn't move after [`Lock::new`].
pub trait Lock: Send + Sync {
    /// Creates an unlocked mutex.
    fn new() -> Self;
    /// Waits until the mutex is unlocked and locks it.
    fn lock(&self);
    /// Unlocks the mutex, which the thread locked.
    fn unlock(&self);
}

/// A condition variable of the system's threads.
pub trait Signal<L>: Send + Sync {
    /// Creates a condition variable.
    fn new() -> Self;
    /// Unlocks the locked mutex until the condition is signaled and locks it again.
    fn wait(&self, lock: &L);
    /// Wakes a waiting thread.
    fn signal(&self);
}

/// An IPv4 address and port.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Address {
    /// The address, like `[192, 168, 1, 5]`.
    pub ip: [u8; 4],
    /// The port.
    pub port: u16,
}

/// The result of a nonblocking send or receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Io {
    /// This many bytes went, zero once the connection closed.
    Done(usize),
    /// Nothing went now, try again later.
    Wait,
    /// The connection failed.
    Failed,
}

/// A socket of the console's network stack.
pub trait Socket: Sized + Send {
    /// A TCP socket.
    fn tcp() -> Option<Self>;
    /// A UDP socket.
    fn udp() -> Option<Self>;
    /// Binds the socket to a port on all addresses, other sockets can bind it too.
    fn bind(&self, port: u16) -> bool;
    /// Listens for connections with the backlog.
    fn listen(&self, backlog: usize) -> bool;
    /// Makes sends and receives return [`Io::Wait`] instead of waiting.
    fn nonblocking(&self);
    /// Allows datagrams to the broadcast address.
    fn broadcast(&self);
    /// Connects to an address, waiting until it's connected.
    fn connect(&self, address: Address) -> bool;
    /// A connection waiting to be accepted.
    fn accept(&self) -> Option<Self>;
    /// Sends bytes of a connection.
    fn send(&self, bytes: &[u8]) -> Io;
    /// Receives bytes of a connection.
    fn receive(&self, bytes: &mut [u8]) -> Io;
    /// Sends a datagram.
    fn send_to(&self, bytes: &[u8], address: Address);
    /// Receives a waiting datagram and its sender.
    fn receive_from(&self, bytes: &mut [u8]) -> Option<(usize, Address)>;
}

/// A vertex of the sky, the players, the held block, the particles and the HUD.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    /// The position in model space.
    pub position: [f32; 3],
    /// The gamma encoded RGBA color, the red byte first like a big-endian `u32`.
    pub color: u32,
    /// The texture coordinate.
    pub uv: [f32; 2],
}

/// The textures of the draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Texture {
    /// No texture, only the vertex colors.
    None,
    /// A block tile of [`crate::tiles::TILES`].
    Tile(usize),
    /// A sky layer: the bottom, side, cloudy side, hilly side and top of the skybox, the
    /// clouds, the sun and the moon.
    Sky(usize),
    /// A character part of [`crate::avatars::PARTS`].
    Character(usize),
    /// The font atlas in assets/font.png.
    Font,
    /// The Wii Remote pointer, in assets/cursor.png.
    Cursor,
}

/// How the draws test and write depth, cull and blend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// Whether draws are hidden behind nearer ones.
    pub depth_test: bool,
    /// Whether draws hide farther ones.
    pub depth_write: bool,
    /// Whether the back faces are left out, the front faces turn clockwise.
    pub cull: bool,
    /// Whether draws blend with their alpha.
    pub blend: bool,
    /// Whether texels with less than half alpha are left out.
    pub cutout: bool,
}

/// Distance fog, with a linear fallback for fixed-function GPUs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    /// Where the fog starts.
    pub start: f32,
    /// Where the fog covers everything.
    pub end: f32,
    /// The normal distance-fog range, also used for water opacity while immersed.
    pub distance: [f32; 2],
    /// The gamma encoded color of the fog.
    pub color: [u8; 3],
    /// The near and far planes of the projection, GX finds the distance from the depth.
    pub planes: [f32; 2],
    /// Exponential immersion density, zero for smooth distance fog.
    pub density: f32,
    /// Linear immersion tint, white above water.
    pub tint: [f32; 3],
}

/// The passes of the chunks, each with its own depth, cutout and blend state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pass {
    /// Opaque blocks.
    Solid,
    /// Blocks with holes, like leaves and plants.
    Cutout,
    /// Blocks that blend, like water.
    Translucent,
}

/// The GPU of the console. It draws with the projection and model view matrices like GX and
/// the 2D HUD with an orthographic projection in [`Gpu::size`] pixels.
pub trait Gpu {
    /// The GPU's form of a chunk mesh.
    type Mesh: Send + 'static;

    /// Converts a chunk mesh, on the worker thread.
    fn mesh(mesh: &Mesh) -> Self::Mesh;
    /// Whether a block tile has texels with alpha, so it's drawn in the cutout pass.
    fn has_alpha(tile: usize) -> bool;
    /// Whether a section of a chunk has something to draw in a pass.
    fn draws(mesh: &Self::Mesh, section: usize, pass: Pass) -> bool;

    /// The size of the 2D screen space, square pixels with a height of 480.
    fn size(&self) -> [f32; 2];
    /// Starts a frame cleared to the color, with chunks lit for the daylight.
    fn begin(&mut self, clear: [u8; 3], daylight: f32, time: f32);
    /// Sets the projection matrix.
    fn projection(&mut self, matrix: &Mtx44, orthographic: bool);
    /// Sets the model view matrix.
    fn model_view(&mut self, matrix: &Mtx);
    /// Sets how the next draws test depth, cull and blend.
    fn state(&mut self, state: State);
    /// Sets the fog of the next draws, `None` without fog.
    fn fog(&mut self, fog: Option<Fog>);
    /// Draws quads, four vertices each turning clockwise for their front.
    fn quads(&mut self, texture: Texture, vertices: &[Vertex]);
    /// Draws thick lines, two vertices each.
    fn lines(&mut self, vertices: &[Vertex]);
    /// Draws the quads of a section of a chunk in a pass.
    fn chunk(&mut self, mesh: &Self::Mesh, section: usize, pass: Pass);
    /// Shows the frame and waits for the next one.
    fn present(&mut self);
}

/// The output of the sound effects.
pub trait Audio {
    /// Plays an effect with the volume of the left and right speaker, from 0.0 to 1.0, at
    /// the playback speed.
    fn play(&mut self, effect: Effect, volume: [f32; 2], pitch: f32);
}

/// The controllers the player plays with.
pub trait Controller {
    /// Reads the controllers, mapped for what the player does.
    fn read(&mut self, context: Context) -> Pad;
}
