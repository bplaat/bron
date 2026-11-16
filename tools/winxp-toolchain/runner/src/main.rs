/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Runs a Windows program in Windows XP in QEMU, headless: sends it to the agent in the guest over
//! COM1 and forwards its output and exit code. Every run starts from the same snapshot of the
//! logged in desktop, which the first run makes from the Windows XP disk.
//!
//! Usage: winxp-runner [--shot <file.png> [--wait <seconds>]] [--timeout <seconds>] <program.exe> [arguments]...

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};
use std::{env, fs, thread};

const SNAPSHOT: &str = "ready";
const STARTUP_DIR: &str = "::/Documents and Settings/All Users/Start Menu/Programs/Startup";
/// Without its other entry the boot menu doesn't wait
const BOOT_INI: &str = "[boot loader]\r\ntimeout=0\r\ndefault=multi(0)disk(0)rdisk(0)partition(1)\\WINDOWS\r\n\
    [operating systems]\r\nmulti(0)disk(0)rdisk(0)partition(1)\\WINDOWS=\"Microsoft Windows XP Professional\" \
    /noexecute=optin /fastdetect\r\n";

struct Options {
    shot: Option<PathBuf>,
    wait: Duration,
    timeout: Duration,
    program: PathBuf,
    arguments: Vec<String>,
}

fn parse_options() -> Option<Options> {
    let mut args = env::args().skip(1);
    let (mut shot, mut wait, mut timeout) = (None, 10, 600);
    loop {
        let arg = args.next()?;
        match arg.as_str() {
            "--shot" => shot = Some(PathBuf::from(args.next()?)),
            "--wait" => wait = args.next()?.parse().ok()?,
            "--timeout" => timeout = args.next()?.parse().ok()?,
            _ => {
                return Some(Options {
                    shot,
                    wait: Duration::from_secs(wait),
                    timeout: Duration::from_secs(timeout),
                    program: PathBuf::from(arg),
                    arguments: args.collect(),
                });
            }
        }
    }
}

fn main() -> ExitCode {
    let Some(options) = parse_options() else {
        eprintln!(
            "Usage: winxp-runner [--shot <file.png> [--wait <seconds>]] [--timeout <seconds>] <program.exe> [arguments]..."
        );
        return ExitCode::FAILURE;
    };
    match run(&options) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("winxp-runner: {err}");
            ExitCode::FAILURE
        }
    }
}

fn build_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/winxp-toolchain");
    dir.canonicalize().unwrap_or(dir)
}

/// The Windows XP disk, which isn't in the repository
fn image() -> io::Result<PathBuf> {
    env::var_os("WINXP_IMAGE")
        .filter(|image| !image.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::other("set WINXP_IMAGE to a Windows XP Professional SP3 disk image")
        })
}

fn run(options: &Options) -> io::Result<u8> {
    let dir = build_dir().join("qemu");
    prepare(&dir)?;
    let program = fs::read(&options.program)?;

    // The snapshot's disk changes during a run, so every run gets its own copy
    let disk = dir.join(format!("run-{}.qcow2", std::process::id()));
    clone_file(&dir.join("ready.qcow2"), &disk)?;
    let result = (|| {
        let mut qemu = Qemu::start(&disk, true)?;
        let mut header = format!("RUN {}", program.len());
        for argument in &options.arguments {
            header.push(' ');
            header.push_str(&quote(argument));
        }
        header.push('\n');
        qemu.serial.write_all(header.as_bytes())?;
        qemu.serial.write_all(&program)?;

        let events = forward_output(qemu.serial.try_clone()?);
        // A screenshot is taken after the wait, also when the program still runs
        let deadline = options
            .shot
            .as_ref()
            .map_or(options.timeout, |_| options.wait);
        let code = match events.recv_timeout(deadline) {
            Ok(Some(code)) => Some(code),
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other("the agent stopped"));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => None,
        };
        if let Some(shot) = &options.shot {
            qemu.screenshot(shot)?;
        }
        qemu.quit()?;
        match code {
            Some(code) => Ok(u8::try_from(code).unwrap_or_else(|_| {
                eprintln!("winxp-runner: the program exited with 0x{code:08x}");
                1
            })),
            None if options.shot.is_some() => Ok(0),
            None => Err(io::Error::other(format!(
                "the program didn't exit within {} seconds",
                options.timeout.as_secs()
            ))),
        }
    })();
    _ = fs::remove_file(&disk);
    result
}

/// Quotes an argument for the Windows command line
fn quote(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '"']) {
        return argument.to_owned();
    }
    // Backslashes are only special before a quote, where they are doubled
    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for c in argument.chars() {
        if c == '\\' {
            backslashes += 1;
            continue;
        }
        let escaped = if c == '"' {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        quoted.push_str(&"\\".repeat(escaped));
        quoted.push(c);
        backslashes = 0;
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

/// Forwards the program's output from the agent, sends its exit code or None when the agent stops
fn forward_output(serial: UnixStream) -> mpsc::Receiver<Option<u32>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut serial = BufReader::new(serial);
        let mut line = String::new();
        let code = loop {
            line.clear();
            if serial.read_line(&mut line).unwrap_or(0) == 0 {
                break None;
            }
            let mut parts = line.split_whitespace();
            let (Some(kind), Some(value)) = (parts.next(), parts.next()) else {
                continue;
            };
            let Ok(value) = value.parse::<u32>() else {
                continue;
            };
            if kind == "EXIT" {
                break Some(value);
            }
            let mut data = vec![0; value as usize];
            if serial.read_exact(&mut data).is_err() {
                break None;
            }
            _ = match kind {
                "OUT" => io::stdout().write_all(&data),
                _ => io::stderr().write_all(&data),
            };
        };
        _ = io::stdout().flush();
        _ = sender.send(code);
    });
    receiver
}

/// Makes the snapshot of the logged in desktop with the agent waiting, again when the disk, the
/// agent or the runner, which prepares the disk, changed
fn prepare(dir: &Path) -> io::Result<()> {
    let ready = dir.join("ready.qcow2");
    let agent = build_dir().join("winxp-runner-agent.exe");
    let runner = env::current_exe()?;
    let image = image()?;
    let changed = [image.as_path(), &agent, &runner]
        .into_iter()
        .filter_map(modified)
        .max();
    if modified(&ready) >= changed {
        return Ok(());
    }
    eprintln!("winxp-runner: making the Windows XP snapshot");
    fs::create_dir_all(dir)?;
    let base = dir.join("base.img");
    _ = fs::remove_file(&ready);
    _ = fs::remove_file(&base);
    clone_file(&image, &base)?;

    // The agent starts at logon from the Startup folder
    let drive = format!("{}@@{}", base.display(), partition_offset(&base)?);
    let boot_ini = dir.join("boot.ini");
    fs::write(&boot_ini, BOOT_INI)?;
    mtools("mattrib", &["-i", &drive, "-r", "-s", "-h", "::/boot.ini"])?;
    mtools(
        "mcopy",
        &[
            "-o",
            "-i",
            &drive,
            &boot_ini.to_string_lossy(),
            "::/boot.ini",
        ],
    )?;
    mtools("mattrib", &["-i", &drive, "+r", "+s", "+h", "::/boot.ini"])?;
    let startup = format!("{STARTUP_DIR}/winxp-runner.exe");
    mtools(
        "mcopy",
        &["-o", "-i", &drive, &agent.to_string_lossy(), &startup],
    )?;

    // Setup picks the APIC HAL in QEMU, which times the performance counter with the TSC it
    // calibrates wrong under emulation. The Standard PC HAL of the driver cache uses the PIT.
    let cab = dir.join("sp3.cab");
    let cab_source = "::/WINDOWS/Driver Cache/i386/sp3.cab";
    mtools(
        "mcopy",
        &["-o", "-i", &drive, cab_source, &cab.to_string_lossy()],
    )?;
    let extracted = Command::new("bsdtar")
        .arg("-xf")
        .arg(&cab)
        .arg("-C")
        .arg(dir)
        .arg("hal.dll")
        .status()?;
    if !extracted.success() {
        return Err(io::Error::other(
            "bsdtar failed to extract the Standard PC HAL",
        ));
    }
    let hal = dir.join("hal.dll");
    let hal_path = hal.to_string_lossy();
    mtools(
        "mcopy",
        &["-o", "-i", &drive, &hal_path, "::/WINDOWS/system32/hal.dll"],
    )?;
    fs::remove_file(&cab)?;
    fs::remove_file(&hal)?;

    // The snapshot lives in a disk on top of the prepared one
    let partial = dir.join("ready.qcow2.part");
    let status = Command::new("qemu-img")
        .args(["create", "-q", "-f", "qcow2", "-F", "raw", "-b", "base.img"])
        .arg(&partial)
        .status()?;
    if !status.success() {
        return Err(io::Error::other("qemu-img failed"));
    }
    let mut qemu = Qemu::start(&partial, false)?;
    qemu.serial
        .set_read_timeout(Some(Duration::from_secs(600)))?;
    let mut serial = BufReader::new(qemu.serial.try_clone()?);
    wait_for(&mut serial, "READY")?;

    // With the other HAL Windows XP installs devices at the first logon and asks for a restart,
    // after which COM1 doesn't receive anymore. Give it a minute and answer No instead, so the
    // snapshot is of a settled desktop.
    thread::sleep(Duration::from_secs(60));
    qemu.command("sendkey n")?;
    thread::sleep(Duration::from_secs(5));
    qemu.command(&format!("savevm {SNAPSHOT}"))?;
    qemu.quit()?;
    fs::rename(&partial, &ready)
}

/// Reads the agent's lines until one starts with the text
fn wait_for(serial: &mut impl BufRead, text: &str) -> io::Result<()> {
    let mut line = String::new();
    while !line.starts_with(text) {
        line.clear();
        if serial.read_line(&mut line)? == 0 {
            return Err(io::Error::other("the agent in Windows XP stopped"));
        }
    }
    Ok(())
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

/// Copies a file as a copy on write clone where the file system can
fn clone_file(from: &Path, to: &Path) -> io::Result<()> {
    let mut cp = Command::new("cp");
    if cfg!(target_os = "macos") {
        cp.arg("-c");
    } else {
        cp.args(["--reflink=auto", "--sparse=always"]);
    }
    if !cp.arg(from).arg(to).status()?.success() {
        return Err(io::Error::other(format!(
            "failed to copy {}",
            from.display()
        )));
    }
    Ok(())
}

/// The byte offset of the first partition of a disk image
fn partition_offset(disk: &Path) -> io::Result<u64> {
    let mut mbr = [0; 512];
    fs::File::open(disk)?.read_exact(&mut mbr)?;
    Ok(u64::from(u32::from_le_bytes([mbr[454], mbr[455], mbr[456], mbr[457]])) * 512)
}

fn mtools(command: &str, args: &[&str]) -> io::Result<()> {
    let status = Command::new(command)
        .args(args)
        .env("MTOOLS_SKIP_CHECK", "1")
        .stdout(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!("{command} failed")));
    }
    Ok(())
}

/// A headless QEMU machine like the one Windows XP was installed on: no ACPI and the same
/// memory. Its CPU has SSE2, which the Rust target needs.
struct Qemu {
    process: Child,
    serial: UnixStream,
    monitor: UnixStream,
}

impl Qemu {
    fn start(disk: &Path, restore: bool) -> io::Result<Self> {
        let id = std::process::id();
        // Socket paths have to be short, so they don't go in the build directory
        let serial = env::temp_dir().join(format!("winxp-runner-{id}-serial.sock"));
        let monitor = env::temp_dir().join(format!("winxp-runner-{id}-monitor.sock"));
        let mut qemu = Command::new("qemu-system-i386");
        qemu.args(["-machine", "pc,acpi=off,hpet=off,vmport=off"])
            .args(["-accel", "tcg,thread=single,tb-size=512"])
            .args(["-cpu", "pentium3,+sse2", "-smp", "1", "-m", "256M"])
            .args([
                "-vga",
                "std",
                "-nic",
                "none",
                "-display",
                "none",
                "-rtc",
                "base=localtime",
            ])
            .arg("-drive")
            .arg(format!(
                "file={},format=qcow2,if=ide,index=0,cache=unsafe",
                disk.display()
            ))
            .arg("-serial")
            .arg(format!("unix:{},server=on,wait=off", serial.display()))
            .arg("-monitor")
            .arg(format!("unix:{},server=on,wait=off", monitor.display()))
            .stdin(Stdio::null());
        if restore {
            qemu.args(["-loadvm", SNAPSHOT]);
        }
        let process = qemu.spawn()?;
        let mut qemu = Self {
            process,
            serial: connect(&serial)?,
            monitor: connect(&monitor)?,
        };
        _ = fs::remove_file(&serial);
        _ = fs::remove_file(&monitor);
        qemu.read_prompt()?;
        Ok(qemu)
    }

    fn read_prompt(&mut self) -> io::Result<String> {
        let mut output = Vec::new();
        let mut byte = [0];
        while !output.ends_with(b"(qemu) ") {
            self.monitor.read_exact(&mut byte)?;
            output.push(byte[0]);
        }
        Ok(String::from_utf8_lossy(&output).into_owned())
    }

    fn command(&mut self, command: &str) -> io::Result<String> {
        self.monitor.write_all(format!("{command}\n").as_bytes())?;
        self.read_prompt()
    }

    fn screenshot(&mut self, path: &Path) -> io::Result<()> {
        let path = env::current_dir()?.join(path);
        let reply = self.command(&format!("screendump {} -f png", path.display()))?;
        if !path.exists() {
            // The monitor echoes the command first, its answer follows on the next lines
            let answer = reply
                .lines()
                .skip(1)
                .filter(|line| !line.starts_with("(qemu)"));
            return Err(io::Error::other(format!(
                "QEMU didn't take the screenshot: {}",
                answer.collect::<Vec<_>>().join(" ")
            )));
        }
        Ok(())
    }

    fn quit(mut self) -> io::Result<()> {
        _ = self.monitor.write_all(b"quit\n");
        self.process.wait()?;
        Ok(())
    }
}

impl Drop for Qemu {
    fn drop(&mut self) {
        _ = self.process.kill();
        _ = self.process.wait();
    }
}

/// Connects to a socket QEMU makes when it starts
fn connect(path: &Path) -> io::Result<UnixStream> {
    for _ in 0..100 {
        if let Ok(stream) = UnixStream::connect(path) {
            return Ok(stream);
        }
        thread::sleep(Duration::from_millis(100));
    }
    UnixStream::connect(path)
}
