/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::ffi::{c_int, c_void};

pub const AF_INET: u8 = 2;
pub const SOCK_STREAM: u32 = 1;
pub const SOCK_DGRAM: u32 = 2;
pub const IPPROTO_TCP: u32 = 6;
pub const IPPROTO_UDP: u32 = 17;
pub const INADDR_ANY: u32 = 0;
pub const INADDR_BROADCAST: u32 = u32::MAX;
pub const SOL_SOCKET: u32 = 0xffff;
pub const SO_REUSEADDR: u32 = 0x0004;
pub const SO_BROADCAST: u32 = 0x0020;
pub const SO_KEEPALIVE: u32 = 0x0008;
pub const SO_SNDBUF: u32 = 0x1001;
pub const SO_RCVBUF: u32 = 0x1002;
pub const SO_ERROR: u32 = 0x1007;
pub const F_SETFL: u32 = 4;
pub const IOS_O_NONBLOCK: u32 = 4;
pub const MSG_DONTWAIT: u32 = 0x40;
pub const EAGAIN: i32 = 11;
/// IPv4 address with storage aligned for IOS. Pass 16 as the socket address length;
/// `size_of::<SocketAddress>()` includes padding to the 32-byte cache line.
/// Port and address are in network byte order.
#[repr(C, align(32))]
#[derive(Clone, Copy, Default)]
pub struct SocketAddress {
    pub length: u8,
    pub family: u8,
    pub port: u16,
    pub address: u32,
    pub padding: [u8; 8],
}
unsafe extern "C" {
    pub fn net_init() -> i32;
    pub fn net_get_status() -> i32;
    pub fn net_init_async(
        callback: Option<extern "C" fn(i32, *mut c_void) -> i32>,
        argument: *mut c_void,
    ) -> i32;
    pub fn if_configex(
        address: *mut u32,
        netmask: *mut u32,
        gateway: *mut u32,
        dhcp: bool,
        retries: c_int,
    ) -> i32;
    pub fn net_socket(domain: u32, kind: u32, protocol: u32) -> i32;
    pub fn net_bind(socket: i32, address: *mut SocketAddress, length: u32) -> i32;
    pub fn net_listen(socket: i32, backlog: u32) -> i32;
    pub fn net_accept(socket: i32, address: *mut SocketAddress, length: *mut u32) -> i32;
    pub fn net_connect(socket: i32, address: *mut SocketAddress, length: u32) -> i32;
    pub fn net_fcntl(socket: i32, command: u32, flags: u32) -> i32;
    pub fn net_setsockopt(
        socket: i32,
        level: u32,
        option: u32,
        value: *const c_void,
        length: u32,
    ) -> i32;
    pub fn net_send(socket: i32, bytes: *const c_void, length: i32, flags: u32) -> i32;
    pub fn net_recv(socket: i32, bytes: *mut c_void, length: i32, flags: u32) -> i32;
    pub fn net_sendto(
        socket: i32,
        bytes: *const c_void,
        length: i32,
        flags: u32,
        address: *mut SocketAddress,
        address_length: u32,
    ) -> i32;
    pub fn net_recvfrom(
        socket: i32,
        bytes: *mut c_void,
        length: i32,
        flags: u32,
        address: *mut SocketAddress,
        address_length: *mut u32,
    ) -> i32;
    pub fn net_getsockname(socket: i32, address: *mut SocketAddress, length: *mut u32) -> i32;
    pub fn net_shutdown(socket: i32, how: u32) -> i32;
    pub fn net_close(socket: i32) -> i32;
}
