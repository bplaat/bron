/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The host protocol. The host side lives in the web page (`web/src/guest-host.ts`): it extends
//! v86's VMware backdoor (port 0x5658) with a few extra commands. Bulk data moves through a
//! physically contiguous guest buffer that the host reads and writes directly.

pub const VMWARE_PORT: u16 = 0x5658;
pub const VMWARE_MAGIC: u32 = 0x564D5868;

// Stock VMware commands implemented by v86
pub const VMWARE_CMD_GETVERSION: u32 = 10;
/// EAX = unix seconds, EBX = usec, EDX = UTC offset in minutes
pub const VMWARE_CMD_GETTIME: u32 = 23;
pub const VMWARE_CMD_ABSPOINTER_DATA: u32 = 39;
pub const VMWARE_CMD_ABSPOINTER_STATUS: u32 = 40;
pub const VMWARE_CMD_ABSPOINTER_COMMAND: u32 = 41;
pub const VMWARE_ABSPOINTER_ENABLE: u32 = 0x45414552;
pub const VMWARE_ABSPOINTER_ABSOLUTE: u32 = 0x53424152;
pub const VMWARE_BUTTON_LEFT: u32 = 0x20;
pub const VMWARE_BUTTON_RIGHT: u32 = 0x10;
pub const VMWARE_BUTTON_MIDDLE: u32 = 0x08;
pub const VMWARE_RELATIVE_PACKET: u32 = 0x00010000;

// Extra commands implemented by the page. EDX always holds the port number, so arguments travel
// in EBX, ESI and EDI.
/// Returns [`V86_HELLO_REPLY`]
pub const V86_CMD_HELLO: u32 = 0x8600;
/// Returns `(width << 16) | height` wanted by the host, 0 = none
pub const V86_CMD_GETSIZE: u32 = 0x8601;
/// EBX = buffer phys, ESI = capacity; returns type, EBX = length
pub const V86_CMD_RECV: u32 = 0x8602;
/// EBX = buffer phys, ESI = length, EDI = type
pub const V86_CMD_SEND: u32 = 0x8603;
/// EBX = VIDEO_POINTER_ATTRIBUTES (mono) in nonpaged memory; returns 1 if shown
pub const V86_CMD_POINTER_SHAPE: u32 = 0x8604;
/// EBX = `(column << 16) | row` of the top left, or [`V86_POINTER_HIDDEN`]; returns
/// [`V86_POINTER_RESEND`] when the page lost the shape (a reload)
pub const V86_CMD_POINTER_MOVE: u32 = 0x8605;
pub const V86_POINTER_HIDDEN: u32 = 0xFFFFFFFF;
pub const V86_POINTER_RESEND: u32 = 1;
pub const V86_HELLO_REPLY: u32 = 0x86868686;

// Message types
pub const V86_MSG_NONE: u32 = 0;
/// Both directions: u32 format, u32 size, starts a clipboard format
pub const V86_MSG_CLIP_FORMAT: u32 = 1;
/// Host to guest: u32 size, UTF-16LE name
pub const V86_MSG_FILE_BEGIN: u32 = 2;
/// Host to guest: next chunk
pub const V86_MSG_FILE_DATA: u32 = 3;
/// Host to guest: file complete
pub const V86_MSG_FILE_END: u32 = 4;
/// Guest to host: UTF-16LE diagnostic line
pub const V86_MSG_LOG: u32 = 5;
/// Host to guest: power off (used by the build)
pub const V86_MSG_SHUTDOWN: u32 = 6;
/// Both directions: next chunk of the current clipboard format
pub const V86_MSG_CLIP_DATA: u32 = 7;
/// Both directions: clipboard complete, replaces the previous one
pub const V86_MSG_CLIP_END: u32 = 8;
/// Guest to host: UTF-16LE URL to open in a new browser tab
pub const V86_MSG_OPEN_URL: u32 = 9;
/// Host to guest: renew the DHCP lease, the snapshot comes up with a stale address
pub const V86_MSG_NET_RENEW: u32 = 10;

// Clipboard formats
/// UTF-16LE, CRLF line ends
pub const V86_CLIP_TEXT: u32 = 0;
/// UTF-8 fragment
pub const V86_CLIP_HTML: u32 = 1;
/// u32 width, u32 height, top-down BGRX pixels
pub const V86_CLIP_IMAGE: u32 = 2;
pub const V86_CLIP_COUNT: u32 = 3;
pub const V86_CLIP_MAX: u32 = 64 * 1024 * 1024;

pub const V86_BUFFER_SIZE: u32 = 256 * 1024;

// Control device exposed by v86tools.sys for the user mode agent
pub const V86_DEVICE_NAME: &str = r"\Device\V86Tools";
pub const V86_DOS_NAME: &str = r"\DosDevices\V86Tools";
pub const V86_USER_PATH: &str = r"\\.\V86Tools";

/// `CTL_CODE(FILE_DEVICE_UNKNOWN, 0x860 + n, METHOD_BUFFERED, FILE_ANY_ACCESS)`
const fn v86_ioctl(n: u32) -> u32 {
    (0x22 << 16) | ((0x860 + n) << 2)
}
/// Out: u32
pub const IOCTL_V86_GETSIZE: u32 = v86_ioctl(1);
/// Out: u32 type, u32 length, payload
pub const IOCTL_V86_RECV: u32 = v86_ioctl(2);
/// In: u32 type, payload
pub const IOCTL_V86_SEND: u32 = v86_ioctl(3);
/// Out: u32 seconds, u32 usec, i32 UTC offset in minutes
pub const IOCTL_V86_GETTIME: u32 = v86_ioctl(4);
