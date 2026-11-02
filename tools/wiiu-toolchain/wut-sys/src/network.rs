/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Raw nsysnet imports. Socket descriptors are separate from C file descriptors:
//! use `socketclose`, and read errors with `socketlasterr` instead of `errno`.

use core::ffi::{c_int, c_void};

pub const AF_INET: c_int = 2;
pub const SOCK_STREAM: c_int = 1;
pub const SOCK_DGRAM: c_int = 2;
pub const IPPROTO_TCP: c_int = 6;
pub const IPPROTO_UDP: c_int = 17;
pub const SOL_SOCKET: c_int = -1;
pub const SO_REUSEADDR: c_int = 0x0004;
pub const SO_BROADCAST: c_int = 0x0020;
pub const SO_KEEPALIVE: c_int = 0x0008;
pub const SO_SNDBUF: c_int = 0x1001;
pub const SO_RCVBUF: c_int = 0x1002;
pub const SO_ERROR: c_int = 0x1007;
pub const SO_NONBLOCK: c_int = 0x1016;
pub const MSG_DONTWAIT: c_int = 0x0020;
pub const SHUT_RD: c_int = 0;
pub const SHUT_WR: c_int = 1;
pub const SHUT_RDWR: c_int = 2;
pub const INADDR_ANY: u32 = 0;
pub const INADDR_LOOPBACK: u32 = 0x7f00_0001;
pub const INADDR_BROADCAST: u32 = u32::MAX;

/// IPv4 socket address. Port and address are in network byte order.
/// Unlike Wii's socket address, the family occupies the first two bytes.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SocketAddress {
    pub family: u16,
    pub port: u16,
    pub address: u32,
    pub padding: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NNResult {
    pub value: i32,
}

unsafe extern "C" {
    pub fn ACInitialize() -> NNResult;
    pub fn ACFinalize();
    pub fn ACConnect() -> NNResult;
    pub fn ACConnectAsync() -> NNResult;
    pub fn ACClose() -> NNResult;
    /// The console's IPv4 address, once it's connected.
    pub fn ACGetAssignedAddress(ip: *mut u32) -> NNResult;
    pub fn socket_lib_init();
    pub fn socket_lib_finish();
    #[link_name = "__rplwrap_socket"]
    pub fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    #[link_name = "__rplwrap_bind"]
    pub fn bind(socket: c_int, address: *const SocketAddress, length: u32) -> c_int;
    #[link_name = "__rplwrap_listen"]
    pub fn listen(socket: c_int, backlog: c_int) -> c_int;
    #[link_name = "__rplwrap_accept"]
    pub fn accept(socket: c_int, address: *mut SocketAddress, length: *mut u32) -> c_int;
    #[link_name = "__rplwrap_connect"]
    pub fn connect(socket: c_int, address: *const SocketAddress, length: u32) -> c_int;
    #[link_name = "__rplwrap_setsockopt"]
    pub fn setsockopt(
        socket: c_int,
        level: c_int,
        option: c_int,
        value: *const c_void,
        length: u32,
    ) -> c_int;
    #[link_name = "__rplwrap_getsockopt"]
    pub fn getsockopt(
        socket: c_int,
        level: c_int,
        option: c_int,
        value: *mut c_void,
        length: *mut u32,
    ) -> c_int;
    #[link_name = "__rplwrap_send"]
    pub fn send(socket: c_int, bytes: *const c_void, length: usize, flags: c_int) -> c_int;
    #[link_name = "__rplwrap_recv"]
    pub fn recv(socket: c_int, bytes: *mut c_void, length: usize, flags: c_int) -> c_int;
    #[link_name = "__rplwrap_sendto"]
    pub fn sendto(
        socket: c_int,
        bytes: *const c_void,
        length: usize,
        flags: c_int,
        address: *const SocketAddress,
        address_length: u32,
    ) -> c_int;
    #[link_name = "__rplwrap_recvfrom"]
    pub fn recvfrom(
        socket: c_int,
        bytes: *mut c_void,
        length: usize,
        flags: c_int,
        address: *mut SocketAddress,
        address_length: *mut u32,
    ) -> c_int;
    #[link_name = "__rplwrap_shutdown"]
    pub fn shutdown(socket: c_int, how: c_int) -> c_int;
    #[link_name = "__rplwrap_socketclose"]
    pub fn socketclose(socket: c_int) -> c_int;
    #[link_name = "__rplwrap_socketlasterr"]
    pub fn socketlasterr() -> c_int;
}
