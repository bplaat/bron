/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Threads, mutexes and condition variables of libogc's LWP kernel, its sockets, the time
//! base and the system settings.

use alloc::boxed::Box;
use alloc::string::String;
use core::ffi::{CStr, c_void};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use libogc_sys::*;
use plaatcraft_engine::client::{Address, Io, Lock, Signal, Socket, Thread};

// The Wii time base runs at a quarter of the 243 MHz bus clock
pub const TICKS_PER_SECOND: f64 = 60_750_000.0;
const STACK_SIZE: usize = 256 * 1024;
const ADDRESS_LENGTH: u32 = 16;

pub fn seconds() -> f64 {
    unsafe { gettime() as f64 / TICKS_PER_SECOND }
}

pub struct Mutex(mutex_t);

impl Lock for Mutex {
    fn new() -> Self {
        let mut handle = LWP_MUTEX_NULL;
        unsafe { LWP_MutexInit(&mut handle, false) };
        Self(handle)
    }

    fn lock(&self) {
        unsafe { LWP_MutexLock(self.0) };
    }

    fn unlock(&self) {
        unsafe { LWP_MutexUnlock(self.0) };
    }
}

pub struct Condvar(cond_t);

impl Signal<Mutex> for Condvar {
    fn new() -> Self {
        let mut handle = LWP_COND_NULL;
        unsafe { LWP_CondInit(&mut handle) };
        Self(handle)
    }

    fn wait(&self, lock: &Mutex) {
        unsafe { LWP_CondWait(self.0, lock.0) };
    }

    fn signal(&self) {
        unsafe { LWP_CondSignal(self.0) };
    }
}

/// Starts a thread that runs forever, higher priorities run first. The stack is allocated
/// on the heap, libogc's own thread stacks come from a small workspace.
pub fn spawn(thread: Thread, entry: Box<dyn FnOnce() + Send>) {
    extern "C" fn start(argument: *mut c_void) -> *mut c_void {
        let entry = unsafe { Box::from_raw(argument.cast::<Box<dyn FnOnce() + Send>>()) };
        entry();
        ptr::null_mut()
    }
    // Lower than the main thread, so generation uses the time the main thread waits for vsync
    let priority = match thread {
        Thread::Worker => 40,
        Thread::Network => 45,
    };
    let stack = Box::leak(alloc::vec![0u8; STACK_SIZE].into_boxed_slice());
    let mut handle = LWP_THREAD_NULL;
    let result = unsafe {
        LWP_CreateThread(
            &mut handle,
            start,
            Box::into_raw(Box::new(entry)).cast(),
            stack.as_mut_ptr().cast(),
            STACK_SIZE as u32,
            priority,
        )
    };
    assert!(result >= 0, "failed to start thread");
}

/// The nickname of the Wii from its system settings.
pub fn nickname() -> Option<String> {
    let mut nickname = [0; 11];
    if unsafe { CONF_GetNickName(nickname.as_mut_ptr()) } < 0 {
        return None;
    }
    let nickname = CStr::from_bytes_until_nul(&nickname).ok()?.to_str().ok()?;
    Some(nickname.into())
}

// Poll the async initializer so a callback cannot wake a thread before it sleeps.
static INITIALIZING: AtomicBool = AtomicBool::new(false);
static INITIALIZED: AtomicI32 = AtomicI32::new(i32::MIN);

extern "C" fn initialized(result: i32, _argument: *mut c_void) -> i32 {
    INITIALIZED.store(result, Ordering::Release);
    0
}

/// Starts the network stack, returns the Wii's address.
pub fn connect() -> Result<[u8; 4], i32> {
    if !INITIALIZING.swap(true, Ordering::AcqRel) {
        INITIALIZED.store(i32::MIN, Ordering::Release);
        let result = unsafe { net_init_async(Some(initialized), ptr::null_mut()) };
        if result < 0 {
            INITIALIZED.store(result, Ordering::Release);
        }
    }
    let start = seconds();
    while INITIALIZED.load(Ordering::Acquire) == i32::MIN {
        if seconds() - start > 10.0 {
            return Err(-110);
        }
        unsafe { VIDEO_WaitVSync() };
    }
    let result = INITIALIZED.load(Ordering::Acquire);
    if result < 0 {
        INITIALIZING.store(false, Ordering::Release);
        return Err(result);
    }
    let mut address = 0;
    let result = unsafe { if_configex(&mut address, ptr::null_mut(), ptr::null_mut(), true, 20) };
    if result < 0 {
        Err(result)
    } else {
        Ok(address.to_ne_bytes())
    }
}

fn socket_address(address: Address) -> SocketAddress {
    SocketAddress {
        length: ADDRESS_LENGTH as u8,
        family: AF_INET,
        port: address.port.to_be(),
        address: u32::from_ne_bytes(address.ip),
        padding: [0; 8],
    }
}

fn io(result: i32) -> Io {
    match result {
        _ if result >= 0 => Io::Done(result as usize),
        _ if result == -EAGAIN => Io::Wait,
        _ => Io::Failed,
    }
}

pub struct WiiSocket(i32);

impl WiiSocket {
    fn new(kind: u32) -> Option<Self> {
        let fd = unsafe { net_socket(AF_INET.into(), kind, 0) };
        (fd >= 0).then_some(Self(fd))
    }

    fn enable(&self, option: u32) {
        let enabled = 1u32;
        unsafe { net_setsockopt(self.0, SOL_SOCKET, option, (&raw const enabled).cast(), 4) };
    }
}

impl Socket for WiiSocket {
    fn tcp() -> Option<Self> {
        Self::new(SOCK_STREAM)
    }

    fn udp() -> Option<Self> {
        Self::new(SOCK_DGRAM)
    }

    fn bind(&self, port: u16) -> bool {
        self.enable(SO_REUSEADDR);
        let mut address = socket_address(Address { ip: [0; 4], port });
        unsafe { net_bind(self.0, &mut address, ADDRESS_LENGTH) >= 0 }
    }

    fn listen(&self, backlog: usize) -> bool {
        unsafe { net_listen(self.0, backlog as u32) >= 0 }
    }

    fn nonblocking(&self) {
        unsafe { net_fcntl(self.0, F_SETFL, IOS_O_NONBLOCK) };
    }

    fn broadcast(&self) {
        self.enable(SO_BROADCAST);
    }

    fn connect(&self, address: Address) -> bool {
        let mut address = socket_address(address);
        unsafe { net_connect(self.0, &mut address, ADDRESS_LENGTH) >= 0 }
    }

    fn accept(&self) -> Option<Self> {
        let mut from = SocketAddress::default();
        let mut length = ADDRESS_LENGTH;
        let fd = unsafe { net_accept(self.0, &mut from, &mut length) };
        (fd >= 0).then_some(Self(fd))
    }

    fn send(&self, bytes: &[u8]) -> Io {
        io(unsafe {
            net_send(
                self.0,
                bytes.as_ptr().cast(),
                bytes.len() as i32,
                MSG_DONTWAIT,
            )
        })
    }

    fn receive(&self, bytes: &mut [u8]) -> Io {
        io(unsafe {
            net_recv(
                self.0,
                bytes.as_mut_ptr().cast(),
                bytes.len() as i32,
                MSG_DONTWAIT,
            )
        })
    }

    fn send_to(&self, bytes: &[u8], address: Address) {
        let mut address = socket_address(address);
        unsafe {
            net_sendto(
                self.0,
                bytes.as_ptr().cast(),
                bytes.len() as i32,
                MSG_DONTWAIT,
                &mut address,
                ADDRESS_LENGTH,
            )
        };
    }

    fn receive_from(&self, bytes: &mut [u8]) -> Option<(usize, Address)> {
        let mut from = SocketAddress::default();
        let mut length = ADDRESS_LENGTH;
        let size = unsafe {
            net_recvfrom(
                self.0,
                bytes.as_mut_ptr().cast(),
                bytes.len() as i32,
                MSG_DONTWAIT,
                &mut from,
                &mut length,
            )
        };
        (size > 0).then(|| {
            let address = Address {
                ip: from.address.to_ne_bytes(),
                port: u16::from_be(from.port),
            };
            (size as usize, address)
        })
    }
}

impl Drop for WiiSocket {
    fn drop(&mut self) {
        unsafe { net_close(self.0) };
    }
}
