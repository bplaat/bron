/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! v86gputest.exe - the conformance test of the paravirtualized graphics adapter, run by
//! `winxp-image/gpu-test.mjs` at logon.
//!
//! GDI: every case draws into a DIB section, which only the engine draws, and onto the screen,
//! which the display driver hands to the page; the screen is read back and has to match. Then
//! DirectDraw in 8, 16 and 32 bpp: fills, copies, keys, stretching, mirroring, overlap, vertical
//! blanks, flips and palettes, with the shown picture checked by the page. Results go to COM1 as
//! `LOG` lines and end with `GPUTEST DONE <passed> <failed>`.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

mod ddraw;
mod gdi;

use std::fs::File;
use std::io::Write;
use std::ptr;

use v86::win32::*;

pub type HBITMAP = HANDLE;

pub const SRCCOPY: u32 = 0x00CC_0020;
const WS_POPUP: u32 = 0x8000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_EX_TOPMOST: u32 = 0x8;
const WS_EX_TOOLWINDOW: u32 = 0x80;
const PM_REMOVE: u32 = 1;

#[link(name = "user32")]
unsafe extern "system" {
    fn ShowWindow(hwnd: HWND, command: i32) -> BOOL;
    fn UpdateWindow(hwnd: HWND) -> BOOL;
    fn SetCursorPos(x: i32, y: i32) -> BOOL;
    fn PeekMessageW(message: *mut MSG, hwnd: HWND, min: u32, max: u32, remove: u32) -> BOOL;
    fn DestroyWindow(hwnd: HWND) -> BOOL;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn Sleep(milliseconds: u32);
}

/// Results to COM1, where the page reads them
pub struct Report {
    serial: Option<File>,
    pub passed: u32,
    pub failed: u32,
}

impl Report {
    fn new() -> Self {
        Self {
            serial: File::options().write(true).open(r"\\.\COM1").ok(),
            passed: 0,
            failed: 0,
        }
    }

    pub fn line(&mut self, text: &str) {
        if let Some(serial) = &mut self.serial {
            let _ = serial.write_all(format!("{text}\r\n").as_bytes());
            let _ = serial.flush();
        }
    }

    pub fn log(&mut self, text: &str) {
        self.line(&format!("LOG gputest: {text}"));
    }

    pub fn check(&mut self, name: &str, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.passed += 1;
                self.log(&format!("ok {name}"));
            }
            Err(error) => {
                self.failed += 1;
                self.log(&format!("FAIL {name}: {error}"));
            }
        }
    }
}

/// A DIB section of 8, 16 (565) or 32 bpp, top down, selected into a memory DC
pub struct Dib {
    pub dc: HDC,
    pub bitmap: HBITMAP,
    pub bits: *mut u8,
    pub width: i32,
    pub height: i32,
    pub bpp: u32,
    pub pitch: usize,
}

#[repr(C)]
struct BitmapInfo {
    header: BITMAPINFOHEADER,
    colors: [u32; 256],
}

impl Dib {
    /// `colors` is the color table of indexed formats
    pub fn new(width: i32, height: i32, bpp: u16, colors: &[u32]) -> Self {
        let mut info = BitmapInfo {
            header: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: bpp,
                biCompression: if bpp == 16 { BI_BITFIELDS } else { BI_RGB },
                biClrUsed: colors.len() as u32,
                ..BITMAPINFOHEADER::default()
            },
            colors: [0; 256],
        };
        if bpp == 16 {
            info.colors[..3].copy_from_slice(&[0xF800, 0x07E0, 0x001F]);
            info.header.biClrUsed = 0;
        } else {
            info.colors[..colors.len()].copy_from_slice(colors);
        }
        let mut bits = ptr::null_mut();
        // SAFETY: The info holds the header and its color table, the DC is a new memory DC
        unsafe {
            let bitmap = CreateDIBSection(
                ptr::null_mut(),
                &info.header,
                DIB_RGB_COLORS,
                &mut bits,
                ptr::null_mut(),
                0,
            );
            let dc = CreateCompatibleDC(ptr::null_mut());
            SelectObject(dc, bitmap);
            Self {
                dc,
                bitmap,
                bits: bits.cast(),
                width,
                height,
                bpp: u32::from(bpp),
                pitch: (width as usize * usize::from(bpp)).div_ceil(32) * 4,
            }
        }
    }

    pub fn bytes(&self) -> &[u8] {
        // SAFETY: The section holds pitch * height bytes, GdiFlush before reading finishes drawing
        unsafe {
            GdiFlush();
            std::slice::from_raw_parts(self.bits, self.pitch * self.height as usize)
        }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: Like bytes
        unsafe {
            GdiFlush();
            std::slice::from_raw_parts_mut(self.bits, self.pitch * self.height as usize)
        }
    }

    /// The pixel at x, y as stored, 32 bpp without its unused byte
    pub fn pixel(&self, x: i32, y: i32) -> u32 {
        let row = &self.bytes()[y as usize * self.pitch..];
        let x = x as usize;
        match self.bpp {
            8 => u32::from(row[x]),
            16 => u32::from(u16::from_le_bytes([row[x * 2], row[x * 2 + 1]])),
            _ => u32::from_le_bytes([row[x * 4], row[x * 4 + 1], row[x * 4 + 2], 0]),
        }
    }

    /// A pseudo random picture, the same for every seed
    pub fn noise(&mut self, seed: u32) {
        let mut state = seed | 1;
        for byte in self.bytes_mut() {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = state as u8;
        }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: The DC and section belong to this DIB
        unsafe {
            DeleteDC(self.dc);
            DeleteObject(self.bitmap);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: Windows' own handling of the messages
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// Lets the window and the rest of the desktop finish painting
pub fn settle() {
    // SAFETY: All zeroes is a valid MSG
    let mut message: MSG = unsafe { std::mem::zeroed() };
    for _ in 0..20 {
        // SAFETY: The message structure is ours
        while unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            // SAFETY: A message Windows gave this thread
            unsafe { DispatchMessageW(&message) };
        }
        // SAFETY: Plain sleep
        unsafe { Sleep(25) };
    }
}

/// A topmost window in the top left of the screen, the GDI cases draw into its DC
pub fn create_window(width: i32, height: i32) -> HWND {
    let class = w!("v86gputest");
    // SAFETY: The class and window strings outlive the calls
    unsafe {
        let window_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(ptr::null()),
            hIcon: ptr::null_mut(),
            hCursor: ptr::null_mut(),
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&window_class);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP | WS_VISIBLE,
            0,
            0,
            width,
            height,
            ptr::null_mut(),
            ptr::null_mut(),
            window_class.hInstance,
            ptr::null_mut(),
        );
        ShowWindow(hwnd, 5);
        UpdateWindow(hwnd);
        hwnd
    }
}

/// # Safety
///
/// The window belongs to the test.
pub unsafe fn destroy_window(hwnd: HWND) {
    // SAFETY: The caller passes its own window
    unsafe { DestroyWindow(hwnd) };
}

/// The desktop's depth, changed for a run of the GDI cases
pub fn set_depth(bpp: u32) -> bool {
    let mut mode = DEVMODEW::default();
    mode.dmFields = DM_BITSPERPEL | DM_PELSWIDTH | DM_PELSHEIGHT;
    mode.dmBitsPerPel = bpp;
    mode.dmPelsWidth = 1024;
    mode.dmPelsHeight = 768;
    // SAFETY: A complete DEVMODE
    let changed = unsafe { ChangeDisplaySettingsW(&mode, 0) } == 0;
    settle();
    changed
}

pub fn screen_depth() -> u32 {
    // SAFETY: The screen DC is released again
    unsafe {
        let dc = GetDC(ptr::null_mut());
        let bpp = GetDeviceCaps(dc, BITSPIXEL);
        ReleaseDC(ptr::null_mut(), dc);
        bpp as u32
    }
}

fn main() {
    let mut report = Report::new();
    report.log("start");
    // The pointer goes out of the way of the test window
    // SAFETY: Plain call
    unsafe { SetCursorPos(1000, 700) };
    settle();

    let depth = screen_depth();
    report.log(&format!("desktop at {depth} bpp"));
    for bpp in [32, 16] {
        if set_depth(bpp) {
            gdi::run(&mut report, bpp);
        } else {
            report.check(
                &format!("{bpp} bpp desktop"),
                Err("mode change failed".into()),
            );
        }
    }
    set_depth(depth);
    for bpp in [8, 16, 32] {
        ddraw::run(&mut report, bpp);
    }
    set_depth(depth);

    let (passed, failed) = (report.passed, report.failed);
    report.line(&format!("GPUTEST DONE {passed} {failed}"));
}
