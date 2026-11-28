/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! v86agent.exe - guest agent for v86, started at logon: follows the page size and host clock,
//! syncs the clipboard (text, HTML and images), saves files dropped on the page to the desktop
//! and renews the LAN address when the page asks. Started as `v86agent.exe /open <url>` it opens
//! the link in a new browser tab instead (the http and https handler).

#![windows_subsystem = "windows"]

use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use std::{env, mem, ptr, thread};

use v86::protocol::*;
use v86::win32::*;

mod clipboard;

const CHUNK_SIZE: usize = 64 * 1024;
const POLL_MS: u32 = 30;
const RESIZE_EVERY: u32 = 8;
const CLOCK_EVERY: u32 = 300;
const WM_CLIPBOARD_CHANGED: u32 = WM_APP + 1;

/// The control device of v86tools.sys
pub(crate) struct Device(File);

impl Device {
    fn open() -> Option<Self> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(V86_USER_PATH)
            .ok()
            .map(Self)
    }

    /// Returns the length of the output
    fn control(&self, code: u32, input: &[u8], output: &mut [u8]) -> Option<usize> {
        let mut returned = 0;
        // SAFETY: The handle is open and the buffers have their given lengths
        let ok = unsafe {
            DeviceIoControl(
                self.0.as_raw_handle(),
                code,
                input.as_ptr().cast(),
                input.len() as u32,
                output.as_mut_ptr().cast(),
                output.len() as u32,
                &mut returned,
                ptr::null_mut(),
            )
        };
        (ok != 0).then_some(returned as usize)
    }

    pub(crate) fn send(&self, kind: u32, data: &[u8]) {
        let mut packet = kind.to_le_bytes().to_vec();
        packet.extend_from_slice(data);
        self.control(IOCTL_V86_SEND, &packet, &mut []);
    }

    fn send_text(&self, kind: u32, text: &str) {
        self.send(kind, &utf16_bytes(&text.encode_utf16().collect::<Vec<_>>()));
    }

    fn words<const N: usize>(&self, code: u32) -> Option<[u32; N]> {
        let mut output = [0_u8; 12];
        let len = self.control(code, &[], &mut output[..N * 4])?;
        (len == N * 4).then(|| {
            std::array::from_fn(|i| u32::from_le_bytes([0, 1, 2, 3].map(|j| output[i * 4 + j])))
        })
    }

    /// The next message, its type and data
    fn recv<'a>(&self, packet: &'a mut [u8]) -> Option<(u32, &'a [u8])> {
        let got = self.control(IOCTL_V86_RECV, &[], packet)?;
        if got < 8 {
            return None;
        }
        let word = |i: usize| u32::from_le_bytes([0, 1, 2, 3].map(|j| packet[i + j]));
        let (kind, len) = (word(0), (word(4) as usize).min(got - 8));
        Some((kind, &packet[8..8 + len]))
    }
}

/// A file dropped on the page, saved to the desktop
#[derive(Default)]
struct DroppedFile {
    file: Option<File>,
    path: PathBuf,
    failed: bool,
}

struct Agent {
    device: Option<Device>,
    tick: u32,
    last_size: u32,
    packet: Vec<u8>,
    clipboard: clipboard::Clipboard,
    dropped: DroppedFile,
}

thread_local! {
    static AGENT: RefCell<Agent> = RefCell::new(Agent {
        device: None,
        tick: 0,
        last_size: 0,
        packet: vec![0; 8 + CHUNK_SIZE],
        clipboard: clipboard::Clipboard::default(),
        dropped: DroppedFile::default(),
    });
    static NEXT_VIEWER: Cell<HWND> = const { Cell::new(ptr::null_mut()) };
}

/// Shows a warning without blocking the agent
fn warn(text: &'static str) {
    thread::spawn(move || {
        let (text, caption) = (wide(text), w!("v86"));
        // SAFETY: The strings are NUL terminated
        unsafe {
            MessageBoxW(
                ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                MB_ICONWARNING | MB_SYSTEMMODAL,
            )
        };
    });
}

/// The display mode that fits the size best, larger and deeper modes first. Mode 0 makes the
/// system read the mode list of the driver again, which offers the host size.
fn find_mode(width: u32, height: u32) -> DEVMODEW {
    let area = |mode: &DEVMODEW| u64::from(mode.dmPelsWidth) * u64::from(mode.dmPelsHeight);
    let (mut mode, mut best) = (DEVMODEW::default(), DEVMODEW::default());
    let mut index = 0;
    // SAFETY: The mode has its size set
    while unsafe { EnumDisplaySettingsW(ptr::null(), index, &mut mode) } != 0 {
        index += 1;
        if mode.dmBitsPerPel >= 16
            && mode.dmPelsWidth <= width
            && mode.dmPelsHeight <= height
            && (area(&mode), mode.dmBitsPerPel) > (area(&best), best.dmBitsPerPel)
        {
            best = mode;
        }
    }
    best
}

impl Agent {
    fn device(&mut self) -> Option<&Device> {
        if self.device.is_none() {
            self.device = Device::open();
        }
        self.device.as_ref()
    }

    /// Display: follows the size the page asks for
    fn follow_size(&mut self) {
        let Some(device) = self.device.as_ref() else {
            return;
        };
        let Some([size]) = device.words::<1>(IOCTL_V86_GETSIZE) else {
            return;
        };
        if size == 0 || size == self.last_size {
            return;
        }
        // Fullscreen games switch to 8 or 16 bit modes, leave those alone until the desktop is back
        // SAFETY: The screen DC is released after
        let bpp = unsafe {
            let screen = GetDC(ptr::null_mut());
            let bpp = GetDeviceCaps(screen, BITSPIXEL);
            ReleaseDC(ptr::null_mut(), screen);
            bpp
        };
        if bpp != 32 {
            return;
        }
        self.last_size = size;
        let (width, height) = (size >> 16, size & 0xFFFF);
        // SAFETY: Plain metrics queries
        let screen = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        if screen == (width as i32, height as i32) {
            return;
        }

        // The refreshed list only shows up on a later enumeration
        let mut best = find_mode(width, height);
        for _ in 0..2 {
            if (best.dmPelsWidth, best.dmPelsHeight) == (width, height) {
                break;
            }
            best = find_mode(width, height);
        }
        if best.dmPelsWidth == 0 {
            return;
        }
        best.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_BITSPERPEL | DM_DISPLAYFREQUENCY;
        // SAFETY: The mode comes from EnumDisplaySettingsW
        let result = unsafe { ChangeDisplaySettingsW(&best, CDS_UPDATEREGISTRY) };
        if result != DISP_CHANGE_SUCCESSFUL {
            device.send_text(
                V86_MSG_LOG,
                &format!("resize to {width}x{height} failed: {result}"),
            );
            return;
        }
        // Brings the desktop icons back into view
        let (progman, view, list) = (w!("Progman"), w!("SHELLDLL_DefView"), w!("SysListView32"));
        // SAFETY: The class names are NUL terminated, a missing window gives null
        unsafe {
            let desktop = FindWindowW(progman.as_ptr(), ptr::null());
            let view = FindWindowExW(desktop, ptr::null_mut(), view.as_ptr(), ptr::null());
            let icons = FindWindowExW(view, ptr::null_mut(), list.as_ptr(), ptr::null());
            if !icons.is_null() {
                SendMessageW(icons, LVM_ARRANGE, LVA_DEFAULT, 0);
            }
        }
    }

    /// Clock: follows the host, a restored snapshot would otherwise show its own time
    fn follow_clock(&self) {
        let Some([seconds, microseconds, offset]) = self
            .device
            .as_ref()
            .and_then(|device| device.words::<3>(IOCTL_V86_GETTIME))
        else {
            return;
        };
        let bias = (offset as i32).wrapping_neg();
        // SAFETY: All zeroes is a valid time zone, the structures are valid for the calls
        unsafe {
            let mut zone: TIME_ZONE_INFORMATION = mem::zeroed();
            if GetTimeZoneInformation(&mut zone) == TIME_ZONE_ID_INVALID
                || zone.Bias != bias
                || zone.DaylightBias != 0
                || zone.StandardBias != 0
            {
                zone = mem::zeroed();
                zone.Bias = bias;
                let name = w!("Host Time");
                zone.StandardName[..name.len()].copy_from_slice(name);
                zone.DaylightName[..name.len()].copy_from_slice(name);
                SetTimeZoneInformation(&zone);
            }

            // Unix seconds to FILETIME, 100 ns ticks since 1601
            let host =
                (u64::from(seconds) + 11_644_473_600) * 10_000_000 + u64::from(microseconds) * 10;
            let mut guest = 0;
            GetSystemTimeAsFileTime(&mut guest);
            if guest.abs_diff(host) < 20_000_000 {
                return;
            }
            let mut time = [0; 8];
            if FileTimeToSystemTime(&host, &mut time) != 0 {
                SetSystemTime(&time);
            }
        }
    }

    /// Dropped files land on the desktop. The name is UTF-16 like FAT32 long names, so any
    /// character survives, only the ones Windows forbids are dropped.
    fn file_begin(&mut self, data: &[u8]) {
        self.dropped = DroppedFile::default();
        let Some(name) = data.get(4..) else {
            return;
        };
        let mut name = name
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&unit| u16::from_le_bytes(unit))
            .filter(|&unit| unit >= 32 && !"\\/:*?\"<>|".encode_utf16().any(|bad| bad == unit))
            .take(259)
            .collect::<Vec<_>>();
        // Windows strips these
        while name
            .last()
            .is_some_and(|&unit| unit == u16::from(b'.') || unit == u16::from(b' '))
        {
            name.pop();
        }
        if name.is_empty() {
            name = "file".encode_utf16().collect();
        }

        let mut desktop = [0_u16; 260];
        // SAFETY: The buffer holds MAX_PATH characters
        if unsafe {
            SHGetSpecialFolderPathW(
                ptr::null_mut(),
                desktop.as_mut_ptr(),
                CSIDL_DESKTOPDIRECTORY,
                1,
            )
        } == 0
        {
            return;
        }
        let desktop = &desktop[..desktop.iter().position(|&unit| unit == 0).unwrap_or(0)];
        let extension = match name.iter().rposition(|&unit| unit == u16::from(b'.')) {
            Some(dot) if dot > 0 && name.len() - dot <= 16 => name.split_off(dot),
            _ => Vec::new(),
        };
        // Keeps the path short enough: shortens the name before the extension, never inside a
        // surrogate pair
        let mut keep = 120 - extension.len();
        if name.len() > keep {
            if (0xD800..=0xDBFF).contains(&name[keep - 1]) {
                keep -= 1;
            }
            name.truncate(keep);
        }

        let path = |suffix: &str| {
            let mut path = desktop.to_vec();
            path.push(u16::from(b'\\'));
            path.extend_from_slice(&name);
            path.extend(suffix.encode_utf16());
            path.extend_from_slice(&extension);
            PathBuf::from(OsString::from_wide(&path))
        };
        self.dropped.path = (1..1000)
            .map(|i| {
                if i == 1 {
                    path("")
                } else {
                    path(&format!(" ({i})"))
                }
            })
            .find(|path| !path.exists())
            .unwrap_or_else(|| path(" (999)"));
        match File::create_new(&self.dropped.path) {
            Ok(file) => self.dropped.file = Some(file),
            Err(_) => self.dropped.failed = true,
        }
    }

    fn file_data(&mut self, data: &[u8]) {
        let dropped = &mut self.dropped;
        if let Some(file) = &mut dropped.file
            && !dropped.failed
            && file.write_all(data).is_err()
        {
            dropped.failed = true;
        }
    }

    fn file_end(&mut self) {
        let dropped = mem::take(&mut self.dropped);
        match dropped.file {
            None if dropped.failed => warn("Could not save the dropped file."),
            None => {}
            Some(file) => {
                drop(file);
                if dropped.failed {
                    _ = fs::remove_file(&dropped.path);
                    warn("Not enough disk space for the dropped file.");
                } else {
                    // The exact UTF-16, which the name keeps
                    let path = wide(&dropped.path);
                    // SAFETY: The path is NUL terminated
                    unsafe {
                        SHChangeNotify(SHCNE_CREATE, SHCNF_PATHW, path.as_ptr().cast(), ptr::null())
                    };
                }
            }
        }
    }

    fn clipboard_changed(&mut self, hwnd: HWND) {
        if self.device().is_some()
            && let Some(device) = &self.device
        {
            self.clipboard.changed(hwnd, device);
        }
    }

    fn poll(&mut self, hwnd: HWND) {
        if self.device().is_none() {
            return;
        }
        if self.tick.is_multiple_of(RESIZE_EVERY) {
            self.follow_size();
        }
        if self.tick.is_multiple_of(CLOCK_EVERY) {
            self.follow_clock();
        }
        self.tick = self.tick.wrapping_add(1);

        let mut packet = mem::take(&mut self.packet);
        for _ in 0..64 {
            let Some((kind, data)) = self
                .device
                .as_ref()
                .and_then(|device| device.recv(&mut packet))
            else {
                break;
            };
            match kind {
                V86_MSG_NONE => break,
                V86_MSG_CLIP_FORMAT => self.clipboard.begin(data),
                V86_MSG_CLIP_DATA => self.clipboard.data(data),
                V86_MSG_CLIP_END => self.clipboard.end(hwnd),
                V86_MSG_FILE_BEGIN => self.file_begin(data),
                V86_MSG_FILE_DATA => self.file_data(data),
                V86_MSG_FILE_END => self.file_end(),
                V86_MSG_SHUTDOWN => {
                    // SAFETY: Plain call, the shutdown privilege is enabled
                    unsafe { ExitWindowsEx(EWX_POWEROFF | EWX_FORCE, 0) };
                }
                V86_MSG_NET_RENEW => {
                    thread::spawn(renew_network);
                }
                _ => {}
            }
        }
        self.packet = packet;
    }
}

/// Every visitor runs the same snapshot named WINXP: NetBIOS off on all adapters so the shared
/// LAN has no duplicate names, then a DHCP lease from the relay. In a thread, the renew waits for
/// the answer of the relay.
fn renew_network() {
    if let Some(interfaces) = Key::LOCAL_MACHINE.open(
        r"SYSTEM\CurrentControlSet\Services\NetBT\Parameters\Interfaces",
        KEY_READ | KEY_SET_VALUE,
    ) {
        for name in interfaces.subkeys() {
            if let Some(interface) = interfaces.open(&name, KEY_SET_VALUE) {
                interface.set_dword("NetbiosOptions", 2);
            }
        }
    }

    let mut size = 0;
    // SAFETY: A null buffer asks for the size
    if unsafe { GetInterfaceInfo(ptr::null_mut(), &mut size) } != ERROR_INSUFFICIENT_BUFFER {
        return;
    }
    let mut info = vec![0_u32; (size as usize).div_ceil(4)];
    // SAFETY: The buffer holds size bytes, the adapters follow the count and fit in it
    unsafe {
        if GetInterfaceInfo(info.as_mut_ptr().cast(), &mut size) != 0 {
            return;
        }
        let entry = size_of::<IP_ADAPTER_INDEX_MAP>();
        let count = (info[0] as usize).min((size as usize).saturating_sub(4) / entry);
        let adapters = info.as_ptr().add(1).cast::<IP_ADAPTER_INDEX_MAP>();
        for i in 0..count {
            IpRenewAddress(adapters.add(i));
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Messages can arrive while the agent is busy, when it waits for another window or sets the
    // clipboard itself, returns whether the agent was free
    let with_agent = |f: &dyn Fn(&mut Agent)| {
        AGENT.with(|agent| {
            agent
                .try_borrow_mut()
                .map(|mut agent| f(&mut agent))
                .is_ok()
        })
    };
    let next = NEXT_VIEWER.get();
    // SAFETY: The messages go to valid windows of the clipboard viewer chain
    unsafe {
        match message {
            WM_TIMER => {
                with_agent(&|agent| agent.poll(hwnd));
            }
            // A clipboard change while the agent is busy is read once it's free
            WM_DRAWCLIPBOARD | WM_CLIPBOARD_CHANGED => {
                if !with_agent(&|agent| agent.clipboard_changed(hwnd)) {
                    PostMessageW(hwnd, WM_CLIPBOARD_CHANGED, 0, 0);
                }
                if message == WM_DRAWCLIPBOARD && !next.is_null() {
                    SendMessageW(next, message, wparam, lparam);
                }
            }
            WM_CHANGECBCHAIN => {
                if wparam as HWND == next {
                    NEXT_VIEWER.set(lparam as HWND);
                } else if !next.is_null() {
                    SendMessageW(next, message, wparam, lparam);
                }
            }
            WM_DESTROY => {
                ChangeClipboardChain(hwnd, next);
                PostQuitMessage(0);
            }
            _ => return DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
    0
}

fn main() {
    let args = env::args_os().collect::<Vec<_>>();
    if let [_, open, url] = args.as_slice()
        && open == "/open"
    {
        if let Some(device) = Device::open() {
            device.send_text(V86_MSG_OPEN_URL, &url.to_string_lossy());
        }
        return;
    }

    let name = w!("v86agent");
    // SAFETY: The strings are NUL terminated, the structures are valid for the calls and the
    // window procedure lives as long as the program
    unsafe {
        // One agent per session, the mutex stays until the process exits
        CreateMutexW(ptr::null_mut(), 1, name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        CoInitialize(ptr::null_mut());
        enable_privilege("SeSystemtimePrivilege");
        enable_privilege("SeShutdownPrivilege");

        let instance = GetModuleHandleW(ptr::null());
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: ptr::null_mut(),
            hCursor: ptr::null_mut(),
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: name.as_ptr(),
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            0,
            name.as_ptr(),
            name.as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null_mut(),
        );
        let html = w!("HTML Format");
        AGENT.with_borrow_mut(|agent| {
            agent.clipboard.html_format = RegisterClipboardFormatW(html.as_ptr())
        });
        NEXT_VIEWER.set(SetClipboardViewer(hwnd));
        SetTimer(hwnd, 1, POLL_MS, ptr::null_mut());

        let mut message = mem::zeroed::<MSG>();
        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
            DispatchMessageW(&message);
        }
    }
}
