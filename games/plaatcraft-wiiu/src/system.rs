/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Threads, mutexes and condition variables of Cafe OS, the nsysnet sockets and the time base.

use alloc::boxed::Box;
use core::cell::UnsafeCell;
use core::ffi::{c_char, c_int, c_void};
use core::mem::MaybeUninit;

use plaatcraft_engine::client::{Address, Io, Lock, Signal, Socket, Thread};
use wut_sys::*;

const STACK_SIZE: usize = 256 * 1024;
const ADDRESS_LENGTH: u32 = size_of::<SocketAddress>() as u32;
// The errors of a nonblocking socket that has nothing to do yet
const WOULD_BLOCK: [c_int; 2] = [6, 11];

pub fn seconds() -> f64 {
    unsafe { OSGetTime() as f64 / f64::from(OSTimerClockSpeed()) }
}

/// Waits about a frame.
pub fn sleep_frame() {
    unsafe { OSSleepTicks(i64::from(OSTimerClockSpeed() / 60)) };
}

// Cafe OS links the waiting threads into its mutexes and conditions and changes them, so they
// stay on the heap in a cell
fn zeroed<T>() -> Box<UnsafeCell<T>> {
    Box::new(UnsafeCell::new(unsafe {
        MaybeUninit::<T>::zeroed().assume_init()
    }))
}

pub struct Mutex(Box<UnsafeCell<OSMutex>>);

// The mutex is made for sharing between threads
unsafe impl Send for Mutex {}
unsafe impl Sync for Mutex {}

impl Lock for Mutex {
    fn new() -> Self {
        let mutex = zeroed();
        unsafe { OSInitMutex(mutex.get()) };
        Self(mutex)
    }

    fn lock(&self) {
        unsafe { OSLockMutex(self.0.get()) };
    }

    fn unlock(&self) {
        unsafe { OSUnlockMutex(self.0.get()) };
    }
}

pub struct Condvar(Box<UnsafeCell<OSCondition>>);

unsafe impl Send for Condvar {}
unsafe impl Sync for Condvar {}

impl Signal<Mutex> for Condvar {
    fn new() -> Self {
        let condition = zeroed();
        unsafe { OSInitCond(condition.get()) };
        Self(condition)
    }

    fn wait(&self, lock: &Mutex) {
        unsafe { OSWaitCond(self.0.get(), lock.0.get()) };
    }

    fn signal(&self) {
        unsafe { OSSignalCond(self.0.get()) };
    }
}

#[repr(C, align(16))]
struct Stack([u8; STACK_SIZE]);

/// Starts a thread that runs forever. The main thread runs on the second core, the worker gets
/// the third core and the network the first, so they build chunks and poll sockets in
/// parallel with the game.
pub fn spawn(thread: Thread, entry: Box<dyn FnOnce() + Send>) {
    unsafe extern "C" fn start(_argc: c_int, argv: *mut *const c_char) -> c_int {
        let entry = unsafe { Box::from_raw(argv.cast::<Box<dyn FnOnce() + Send>>()) };
        entry();
        0
    }
    let (affinity, priority) = match thread {
        Thread::Worker => (OS_THREAD_ATTRIB_AFFINITY_CPU2, 20),
        Thread::Network | Thread::Input => (OS_THREAD_ATTRIB_AFFINITY_CPU0, 18),
    };
    let handle = Box::leak(zeroed::<OSThread>()).get();
    let stack = Box::leak(unsafe { Box::<Stack>::new_zeroed().assume_init() });
    let created = unsafe {
        OSCreateThread(
            handle,
            start,
            0,
            Box::into_raw(Box::new(entry)).cast(),
            stack.0.as_mut_ptr().add(STACK_SIZE).cast(),
            STACK_SIZE as u32,
            priority,
            affinity | OS_THREAD_ATTRIB_DETACHED,
        )
    };
    assert!(created != FALSE, "failed to start thread");
    unsafe { OSResumeThread(handle) };
}

/// Connects to the network with the console's settings, returns its address.
pub fn connect() -> Result<[u8; 4], i32> {
    let failed = |result: NNResult| result.value < 0;
    unsafe {
        let result = ACInitialize();
        if failed(result) {
            return Err(result.value);
        }
        let result = ACConnect();
        if failed(result) {
            return Err(result.value);
        }
        socket_lib_init();
        let mut ip = 0;
        let result = ACGetAssignedAddress(&mut ip);
        if failed(result) {
            return Err(result.value);
        }
        Ok(ip.to_be_bytes())
    }
}

fn socket_address(address: Address) -> SocketAddress {
    SocketAddress {
        family: AF_INET as u16,
        port: address.port.to_be(),
        address: u32::from_ne_bytes(address.ip),
        padding: [0; 8],
    }
}

fn io(result: c_int) -> Io {
    if result >= 0 {
        Io::Done(result as usize)
    } else if WOULD_BLOCK.contains(&unsafe { socketlasterr() }) {
        Io::Wait
    } else {
        Io::Failed
    }
}

pub struct WiiUSocket(c_int);

impl WiiUSocket {
    fn new(kind: c_int) -> Option<Self> {
        let fd = unsafe { socket(AF_INET, kind, 0) };
        (fd >= 0).then_some(Self(fd))
    }

    fn enable(&self, option: c_int) {
        let enabled: u32 = 1;
        unsafe {
            setsockopt(
                self.0,
                SOL_SOCKET,
                option,
                (&raw const enabled).cast::<c_void>(),
                4,
            )
        };
    }
}

impl Socket for WiiUSocket {
    fn tcp() -> Option<Self> {
        Self::new(SOCK_STREAM)
    }

    fn udp() -> Option<Self> {
        Self::new(SOCK_DGRAM)
    }

    fn bind(&self, port: u16) -> bool {
        self.enable(SO_REUSEADDR);
        let address = socket_address(Address { ip: [0; 4], port });
        unsafe { bind(self.0, &address, ADDRESS_LENGTH) >= 0 }
    }

    fn listen(&self, backlog: usize) -> bool {
        unsafe { listen(self.0, backlog as c_int) >= 0 }
    }

    fn nonblocking(&self) {
        self.enable(SO_NONBLOCK);
    }

    fn broadcast(&self) {
        self.enable(SO_BROADCAST);
    }

    fn connect(&self, address: Address) -> bool {
        let address = socket_address(address);
        unsafe { wut_sys::connect(self.0, &address, ADDRESS_LENGTH) >= 0 }
    }

    fn accept(&self) -> Option<Self> {
        let mut from = SocketAddress::default();
        let mut length = ADDRESS_LENGTH;
        let fd = unsafe { accept(self.0, &mut from, &mut length) };
        (fd >= 0).then_some(Self(fd))
    }

    fn send(&self, bytes: &[u8]) -> Io {
        io(unsafe { send(self.0, bytes.as_ptr().cast(), bytes.len(), 0) })
    }

    fn receive(&self, bytes: &mut [u8]) -> Io {
        io(unsafe { recv(self.0, bytes.as_mut_ptr().cast(), bytes.len(), 0) })
    }

    fn send_to(&self, bytes: &[u8], address: Address) {
        let address = socket_address(address);
        unsafe {
            sendto(
                self.0,
                bytes.as_ptr().cast(),
                bytes.len(),
                0,
                &address,
                ADDRESS_LENGTH,
            )
        };
    }

    fn receive_from(&self, bytes: &mut [u8]) -> Option<(usize, Address)> {
        let mut from = SocketAddress::default();
        let mut length = ADDRESS_LENGTH;
        let size = unsafe {
            recvfrom(
                self.0,
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                0,
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

impl Drop for WiiUSocket {
    fn drop(&mut self) {
        unsafe { socketclose(self.0) };
    }
}
