/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The guest side of winxp-runner: runs from the Startup folder of Windows XP and runs the
//! programs the host sends over COM1. The protocol is lines with a binary payload:
//!
//! ```text
//! guest: READY                     waiting for a program
//! host:  RUN <size> <arguments>    then <size> bytes of the program
//! guest: OUT <size> / ERR <size>   then <size> bytes of its stdout or stderr
//! guest: EXIT <code>               the program exited, then READY again
//! ```
//!
//! Built for Windows XP with `tools/winxp-toolchain/bin/cargo`, `bin/run` does that.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    agent::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("winxp-runner-agent runs in Windows XP, winxp-runner puts it there");
    std::process::exit(1);
}

#[cfg(windows)]
mod agent {
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex, PoisonError, mpsc};
    use std::thread;
    use std::time::{Duration, Instant};

    const PROGRAM_DIR: &str = r"C:\runner";
    const PROGRAM_PATH: &str = r"C:\runner\program.exe";

    /// COM1 for writing, the stdout and stderr threads share it
    type Serial = Arc<Mutex<File>>;

    pub(crate) fn main() -> io::Result<()> {
        let mut serial = serial::open()?;
        let writer: Serial = Arc::new(Mutex::new(serial.try_clone()?));
        fs::create_dir_all(PROGRAM_DIR)?;
        loop {
            write(&writer, b"READY\n");
            let line = read_line(&mut serial)?;
            let Some((size, arguments)) = line
                .strip_prefix("RUN ")
                .map(|rest| rest.split_once(' ').unwrap_or((rest, "")))
            else {
                continue;
            };
            let Ok(size) = size.parse::<u64>() else {
                continue;
            };
            let copied = io::copy(
                &mut (&mut serial).take(size),
                &mut File::create(PROGRAM_PATH)?,
            )?;
            if copied != size {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let code = run_program(arguments.trim_start(), &writer);
            write(&writer, format!("EXIT {code}\n").as_bytes());
        }
    }

    /// Reads a line without its newline, a byte at a time: reads block until they're complete
    fn read_line(serial: &mut File) -> io::Result<String> {
        let mut line = Vec::new();
        let mut byte = [0];
        loop {
            serial.read_exact(&mut byte)?;
            if byte[0] == b'\n' {
                return Ok(String::from_utf8_lossy(&line).into_owned());
            }
            line.push(byte[0]);
        }
    }

    /// Writes one frame at a time, the host has gone when COM1 fails
    fn write(serial: &Serial, data: &[u8]) {
        let mut serial = serial.lock().unwrap_or_else(PoisonError::into_inner);
        if serial.write_all(data).is_err() {
            std::process::exit(1);
        }
    }

    fn send_frame(serial: &Serial, name: &str, data: &[u8]) {
        let mut frame = format!("{name} {}\n", data.len()).into_bytes();
        frame.extend_from_slice(data);
        write(serial, &frame);
    }

    fn run_program(arguments: &str, serial: &Serial) -> u32 {
        let child = Command::new(PROGRAM_PATH)
            .raw_arg(arguments)
            .current_dir(PROGRAM_DIR)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(err) => {
                send_frame(
                    serial,
                    "ERR",
                    format!("can't start the program: {err}\n").as_bytes(),
                );
                return 255;
            }
        };

        // Forwards the pipes until the program closes them
        let (done, finished) = mpsc::channel();
        let mut streams: Vec<(Box<dyn Read + Send>, &str)> = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            streams.push((Box::new(stdout), "OUT"));
        }
        if let Some(stderr) = child.stderr.take() {
            streams.push((Box::new(stderr), "ERR"));
        }
        let count = streams.len();
        for (mut pipe, name) in streams {
            let serial = Arc::clone(serial);
            let done = done.clone();
            thread::spawn(move || {
                let mut buffer = [0; 4096];
                while let Ok(read @ 1..) = pipe.read(&mut buffer) {
                    send_frame(&serial, name, &buffer[..read]);
                }
                _ = done.send(());
            });
        }

        let code = child
            .wait()
            .ok()
            .and_then(|status| status.code())
            .map_or(255, |code| code as u32);
        // Child processes can keep the pipes open, give them a moment before moving on
        let deadline = Instant::now() + Duration::from_secs(2);
        for _ in 0..count {
            if finished
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .is_err()
            {
                break;
            }
        }
        code
    }

    #[allow(unsafe_code)]
    mod serial {
        use std::os::windows::io::{AsRawHandle, RawHandle};

        use super::*;

        #[repr(C)]
        struct Dcb {
            length: u32,
            baud_rate: u32,
            flags: u32,
            reserved: u16,
            xon_limit: u16,
            xoff_limit: u16,
            byte_size: u8,
            parity: u8,
            stop_bits: u8,
            xon_char: i8,
            xoff_char: i8,
            error_char: i8,
            eof_char: i8,
            event_char: i8,
            reserved1: u16,
        }

        const BINARY: u32 = 1;
        const OUTX_CTS_FLOW: u32 = 1 << 2;
        const OUTX_DSR_FLOW: u32 = 1 << 3;
        const DTR_CONTROL_MASK: u32 = 3 << 4;
        const DTR_CONTROL_ENABLE: u32 = 1 << 4;
        const DSR_SENSITIVITY: u32 = 1 << 6;
        const OUT_X: u32 = 1 << 8;
        const IN_X: u32 = 1 << 9;
        const RTS_CONTROL_MASK: u32 = 3 << 12;
        const RTS_CONTROL_ENABLE: u32 = 1 << 12;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCommState(file: RawHandle, dcb: *mut Dcb) -> i32;
            fn SetCommState(file: RawHandle, dcb: *const Dcb) -> i32;
            fn SetupComm(file: RawHandle, in_queue: u32, out_queue: u32) -> i32;
            fn SetCommTimeouts(file: RawHandle, timeouts: *const [u32; 5]) -> i32;
        }

        /// Opens COM1 at 115200 baud 8N1 without flow control, reads block until they're complete
        pub(crate) fn open() -> io::Result<File> {
            // Communication devices can't be shared
            let serial = OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(r"\\.\COM1")?;
            let handle = serial.as_raw_handle();
            let mut dcb = Dcb {
                length: size_of::<Dcb>() as u32,
                baud_rate: 115200,
                flags: 0,
                reserved: 0,
                xon_limit: 0,
                xoff_limit: 0,
                byte_size: 8,
                parity: 0,
                stop_bits: 0,
                xon_char: 0,
                xoff_char: 0,
                error_char: 0,
                eof_char: 0,
                event_char: 0,
                reserved1: 0,
            };
            // SAFETY: The handle is open and the DCB is valid for writes
            unsafe { GetCommState(handle, &mut dcb) };
            dcb.baud_rate = 115200;
            dcb.byte_size = 8;
            dcb.parity = 0;
            dcb.stop_bits = 0;
            dcb.flags &= !(OUTX_CTS_FLOW
                | OUTX_DSR_FLOW
                | DSR_SENSITIVITY
                | OUT_X
                | IN_X
                | DTR_CONTROL_MASK
                | RTS_CONTROL_MASK);
            dcb.flags |= BINARY | DTR_CONTROL_ENABLE | RTS_CONTROL_ENABLE;
            // SAFETY: The handle is open, the DCB and the timeouts are valid for reads. The
            // driver drops what doesn't fit in its input buffer, so make it hold a whole
            // program. Zero timeouts block until data comes in, the host decides when the next
            // program runs.
            unsafe {
                SetCommState(handle, &dcb);
                SetupComm(handle, 16 << 20, 64 << 10);
                SetCommTimeouts(handle, &[0; 5]);
            }
            Ok(serial)
        }
    }
}
