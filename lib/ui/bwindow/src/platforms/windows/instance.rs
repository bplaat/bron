/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Key ownership and acknowledged launch redirection, following the AppInstance model.
//! Uses system primitives rather than requiring the Windows App SDK runtime.
//! Reference: https://github.com/microsoft/WindowsAppSDK/blob/main/dev/AppLifecycle/AppInstance.cpp

use std::io;
use std::marker::PhantomData;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::{NonNull, null, null_mut};
use std::rc::Rc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::event_loop::PlatformEventLoopProxy;
use super::headers::*;
use crate::AppId;
use crate::mailbox::Message;

const TIMEOUT: Duration = Duration::from_secs(5);
const VERSION: u32 = 2;

fn name(key: &str, suffix: &str) -> Vec<u16> {
    format!("{key}-{suffix}")
        .encode_utf16()
        .chain([0])
        .collect()
}

fn handle(raw: HANDLE) -> io::Result<OwnedHandle> {
    if raw.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    }
}

fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn mutex(key: &str, suffix: &str) -> io::Result<OwnedHandle> {
    handle(unsafe { CreateMutexW(null_mut(), FALSE, name(key, suffix).as_ptr()) })
}

fn event(key: &str, suffix: &str) -> io::Result<OwnedHandle> {
    handle(unsafe { CreateEventW(null(), FALSE, FALSE, name(key, suffix).as_ptr()) })
}

// Mutex ownership belongs to the acquiring thread, including after abandonment.
struct Lock<'a>(&'a OwnedHandle, PhantomData<Rc<()>>);

impl Drop for Lock<'_> {
    fn drop(&mut self) {
        unsafe { ReleaseMutex(self.0.as_raw_handle()) };
    }
}

fn lock(handle: &OwnedHandle, milliseconds: u32) -> io::Result<Option<Lock<'_>>> {
    match unsafe { WaitForSingleObject(handle.as_raw_handle(), milliseconds) } {
        0 | WAIT_ABANDONED => Ok(Some(Lock(handle, PhantomData))),
        WAIT_TIMEOUT => Ok(None),
        _ => Err(io::Error::last_os_error()),
    }
}

fn timed_out() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "the running instance did not acknowledge activation",
    )
}

fn remaining(deadline: Instant) -> io::Result<u32> {
    let duration = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(timed_out)?;
    Ok(duration.as_millis().max(1).min(u128::from(u32::MAX - 1)) as u32)
}

#[repr(C)]
struct Shared {
    version: u32,
    pid: u32,
    request: u64,
    response: u64,
    accepted: u32,
}

struct Mapping {
    address: NonNull<Shared>,
    _handle: OwnedHandle,
}

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe { UnmapViewOfFile(self.address.as_ptr().cast()) };
    }
}

struct Channel {
    mapping: Mapping,
    data: OwnedHandle,
    send: OwnedHandle,
    request: OwnedHandle,
    response: OwnedHandle,
}

impl Channel {
    fn open(key: &str) -> io::Result<Self> {
        let mapping = handle(unsafe {
            CreateFileMappingW(
                -1isize as HANDLE,
                null(),
                PAGE_READWRITE,
                0,
                size_of::<Shared>() as u32,
                name(key, "data").as_ptr(),
            )
        })?;
        let address = NonNull::new(
            unsafe {
                MapViewOfFile(
                    mapping.as_raw_handle(),
                    FILE_MAP_READ | FILE_MAP_WRITE,
                    0,
                    0,
                    size_of::<Shared>(),
                )
            }
            .cast::<Shared>(),
        )
        .ok_or_else(io::Error::last_os_error)?;
        Ok(Self {
            mapping: Mapping {
                address,
                _handle: mapping,
            },
            data: mutex(key, "data-lock")?,
            send: mutex(key, "send-lock")?,
            request: event(key, "request")?,
            response: event(key, "response")?,
        })
    }

    // Every shared-memory access is protected across processes by data-lock.
    fn with_data<T>(
        &mut self,
        deadline: Instant,
        f: impl FnOnce(&mut Shared) -> io::Result<T>,
    ) -> io::Result<T> {
        let _guard = lock(&self.data, remaining(deadline)?)?.ok_or_else(timed_out)?;
        f(unsafe { self.mapping.address.as_mut() })
    }

    fn initialize(&mut self) -> io::Result<()> {
        let request = self.request.as_raw_handle();
        let response = self.response.as_raw_handle();
        self.with_data(Instant::now() + TIMEOUT, |data| {
            check(unsafe { ResetEvent(request) })?;
            check(unsafe { ResetEvent(response) })?;
            data.version = VERSION;
            data.pid = unsafe { GetCurrentProcessId() };
            data.request = 0;
            data.response = 0;
            data.accepted = 0;
            Ok(())
        })
    }

    // Returns false when the owner exited or has not finished registering yet.
    fn redirect(&mut self, deadline: Instant) -> io::Result<bool> {
        // Keep senders serialized until acknowledgment. The receiver uses a separate
        // mutex, so requests remain safe even if a sender times out or dies.
        let send = self.send.try_clone()?;
        let _guard = lock(&send, remaining(deadline)?)?.ok_or_else(timed_out)?;
        let pid = self.with_data(deadline, |data| {
            Ok(if data.version == VERSION { data.pid } else { 0 })
        })?;
        if pid == 0 {
            return Ok(false);
        }
        let process = match handle(unsafe { OpenProcess(SYNCHRONIZE, FALSE, pid) }) {
            Ok(process) => process,
            Err(error) if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER) => {
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
            0 => return Ok(false),
            WAIT_TIMEOUT => {}
            _ => return Err(io::Error::last_os_error()),
        }
        check(unsafe { ResetEvent(self.response.as_raw_handle()) })?;
        let sequence = self.with_data(deadline, |data| {
            if data.pid != pid {
                return Ok(None);
            }
            data.request = data
                .request
                .checked_add(1)
                .ok_or_else(|| io::Error::other("activation sequence exhausted"))?;
            data.accepted = 0;
            Ok(Some(data.request))
        })?;
        let Some(sequence) = sequence else {
            return Ok(false);
        };
        // Like AppInstance, transfer the launcher's permission to activate a window.
        unsafe { AllowSetForegroundWindow(pid) };
        check(unsafe { SetEvent(self.request.as_raw_handle()) })?;
        loop {
            let handles = [self.response.as_raw_handle(), process.as_raw_handle()];
            match unsafe {
                WaitForMultipleObjects(2, handles.as_ptr(), FALSE, remaining(deadline)?)
            } {
                0 => {
                    let result = self.with_data(deadline, |data| {
                        Ok((data.response, data.accepted, data.pid))
                    })?;
                    if result.2 != pid {
                        return Ok(false);
                    }
                    if result.0 == sequence {
                        return if result.1 != 0 {
                            Ok(true)
                        } else {
                            Err(io::Error::other("the running instance is shutting down"))
                        };
                    }
                }
                1 => return Ok(false),
                WAIT_TIMEOUT => return Err(timed_out()),
                _ => return Err(io::Error::last_os_error()),
            }
        }
    }

    fn receive(&mut self, activate: impl FnOnce() -> bool) -> io::Result<()> {
        let sequence = self.with_data(Instant::now() + TIMEOUT, |data| {
            if data.version != VERSION {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid focus request",
                ));
            }
            // A timed-out sender can leave an extra event signaled after a newer
            // request has already been processed. Do not focus the window twice.
            Ok((data.request != 0 && data.request != data.response).then_some(data.request))
        })?;
        let Some(sequence) = sequence else {
            return Ok(());
        };
        let accepted = activate();
        self.with_data(Instant::now() + TIMEOUT, |data| {
            data.response = sequence;
            data.accepted = u32::from(accepted);
            Ok(())
        })?;
        check(unsafe { SetEvent(self.response.as_raw_handle()) })
    }
}

fn key(app_id: &AppId) -> io::Result<String> {
    // Stable hashing keeps arbitrary IDs within the kernel object name limit.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for part in [&app_id.qualifier, &app_id.organization, &app_id.application] {
        for byte in (part.len() as u64)
            .to_le_bytes()
            .into_iter()
            .chain(part.bytes())
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    }
    // Package identity keeps an installed app separate from an unpackaged build.
    let mut length = 0;
    let status = unsafe { GetCurrentPackageFamilyName(&mut length, null_mut()) };
    if status == ERROR_INSUFFICIENT_BUFFER {
        let mut family = vec![0u16; length as usize];
        let status = unsafe { GetCurrentPackageFamilyName(&mut length, family.as_mut_ptr()) };
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status));
        }
        for byte in family.into_iter().flat_map(u16::to_le_bytes) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    } else if status != APPMODEL_ERROR_NO_PACKAGE {
        return Err(io::Error::from_raw_os_error(status));
    }
    Ok(format!("bwindow-instance-v2-{hash:016x}"))
}

pub(super) struct SingleInstance {
    owner: OwnedHandle,
    stop: Arc<OwnedHandle>,
    worker: Option<JoinHandle<()>>,
    _thread: PhantomData<Rc<()>>,
}

impl SingleInstance {
    // None means the running owner acknowledged the redirected launch.
    pub(super) fn register(
        app_id: &AppId,
        proxy: PlatformEventLoopProxy,
    ) -> io::Result<Option<Self>> {
        Self::register_key(&key(app_id)?, proxy)
    }

    fn register_key(key: &str, proxy: PlatformEventLoopProxy) -> io::Result<Option<Self>> {
        let owner = mutex(key, "owner")?;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(guard) = lock(&owner, 0)? {
                let mut channel = Channel::open(key)?;
                channel.initialize()?;
                let stop = Arc::new(handle(unsafe {
                    CreateEventW(null(), TRUE, FALSE, null())
                })?);
                let thread_stop = stop.clone();
                let thread_key = key.to_owned();
                let lifetime_owner = owner.try_clone()?;
                let (ready, started) = std::sync::mpsc::sync_channel(1);
                let worker = thread::Builder::new()
                    .name("bwindow-instance".into())
                    .spawn(move || {
                        let mut channel = match Channel::open(&thread_key) {
                            Ok(channel) => channel,
                            Err(error) => {
                                let _ = ready.send(Err(error));
                                return;
                            }
                        };
                        if ready.send(Ok(())).is_err() {
                            return;
                        }
                        let handles =
                            [thread_stop.as_raw_handle(), channel.request.as_raw_handle()];
                        loop {
                            match unsafe {
                                WaitForMultipleObjects(2, handles.as_ptr(), FALSE, u32::MAX)
                            } {
                                0 => break,
                                1 => {
                                    if let Err(error) =
                                        channel.receive(|| proxy.send(Message::Activate))
                                    {
                                        eprintln!(
                                            "Could not receive application activation: {error}"
                                        );
                                    }
                                }
                                _ => {
                                    eprintln!(
                                        "Application activation listener stopped: {}",
                                        io::Error::last_os_error()
                                    );
                                    break;
                                }
                            }
                        }
                    })?;
                started
                    .recv()
                    .map_err(|_| io::Error::other("instance listener did not start"))??;
                drop(channel);
                // Transfer ownership of the acquired mutex into the lifetime guard.
                std::mem::forget(guard);
                return Ok(Some(Self {
                    owner: lifetime_owner,
                    stop,
                    worker: Some(worker),
                    _thread: PhantomData,
                }));
            }
            let mut channel = Channel::open(key)?;
            if channel.redirect(deadline)? {
                return Ok(None);
            }
            remaining(deadline)?;
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        let _ = unsafe { SetEvent(self.stop.as_raw_handle()) };
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        unsafe { ReleaseMutex(self.owner.as_raw_handle()) };
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};

    use super::super::event_loop::PlatformEventLoop;
    use super::*;
    use crate::{Event, EventLoopBuilder, EventLoopInterface, WindowBuilder};

    const CHILD: &str = "BWINDOW_INSTANCE_TEST";
    const ENTRY: &str = "platforms::windows::instance::tests::instance_child";

    fn directory() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bwindow-instance-{}-{unique}", std::process::id()));
        std::fs::create_dir(&path).expect("test directory");
        path
    }

    fn child(root: &std::path::Path, role: &str) -> Child {
        Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", ENTRY, "--nocapture"])
            .env(CHILD, role)
            .env("BWINDOW_INSTANCE_ROOT", root)
            .current_dir(root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("test child")
    }

    fn success(child: Child) {
        let output = child.wait_with_output().expect("child exit");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn wait_file(path: &std::path::Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn instance_child() {
        let Some(role) = std::env::var_os(CHILD) else {
            return;
        };
        let root = PathBuf::from(std::env::var_os("BWINDOW_INSTANCE_ROOT").expect("root"));
        let event_loop = PlatformEventLoop::new(
            EventLoopBuilder::new()
                .app_id("test", "bwindow", root.to_string_lossy())
                .single_instance(role != "independent"),
        );
        assert!(role != "sender", "a second owner was incorrectly admitted");
        std::fs::write(root.join("ready"), []).expect("ready marker");
        std::fs::write(root.join(format!("ready-{}", std::process::id())), [])
            .expect("process readiness");
        wait_file(&root.join("run"));
        if role == "independent" {
            return;
        }
        let mut window = WindowBuilder::new()
            .title("Instance activation test")
            .build();
        let attachment = window.attach_content().expect("native window");
        let Some(crate::NativeWindowHandle::Win32(hwnd)) = (unsafe { attachment.window_handle() })
        else {
            panic!("expected Win32 window");
        };
        unsafe { ShowWindow(hwnd, 6) };
        assert_ne!(
            unsafe { IsIconic(hwnd) },
            FALSE,
            "test window must start minimized"
        );
        // This marker follows every focus request already acknowledged during startup.
        assert!(event_loop.create_proxy().send(Message::User(Box::new(()))));
        let watchdog = event_loop.create_proxy();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(10));
            let _ = crate::EventLoopProxyInterface::exit(&watchdog);
        });
        let mut checked = false;
        assert_eq!(
            event_loop.run(move |event| match event {
                Event::UserEvent(_) => {
                    assert_eq!(
                        unsafe { IsIconic(hwnd) },
                        FALSE,
                        "launch did not restore the existing window"
                    );
                    checked = true;
                    window.close();
                }
                Event::LoopExiting => assert!(checked, "focus requests were not processed"),
                _ => {}
            }),
            0
        );
    }

    #[test]
    fn concurrent_launches_are_acknowledged_before_the_ui_loop_starts() {
        let root = directory();
        let receiver = child(&root, "receiver");
        wait_file(&root.join("ready"));
        let senders: Vec<_> = (0..4).map(|_| child(&root, "sender")).collect();
        for sender in senders {
            success(sender);
        }
        // All launchers have exited successfully while the owner still has no window.
        std::fs::write(root.join("run"), []).expect("start UI");
        success(receiver);
        std::fs::remove_dir_all(root).expect("remove test data");
    }

    #[test]
    fn a_crashed_owner_can_be_replaced_even_when_its_mutex_still_exists() {
        let root = directory();
        let mut receiver = child(&root, "receiver");
        wait_file(&root.join("ready"));
        let app_id = AppId {
            qualifier: "test".into(),
            organization: "bwindow".into(),
            application: root.to_string_lossy().into_owned(),
        };
        let _existing_mutex =
            mutex(&key(&app_id).expect("key"), "owner").expect("keep mutex alive");
        receiver.kill().expect("terminate test owner");
        let _ = receiver.wait().expect("reap test owner");
        std::fs::remove_file(root.join("ready")).expect("remove old readiness");
        let replacement = child(&root, "receiver");
        wait_file(&root.join("ready"));
        success(child(&root, "sender"));
        std::fs::write(root.join("run"), []).expect("start UI");
        success(replacement);
        std::fs::remove_dir_all(root).expect("remove test data");
    }

    #[test]
    fn a_completed_focus_request_is_not_queued_again() {
        let root = directory();
        let key = root.file_name().expect("name").to_str().expect("ASCII");
        let mut channel = Channel::open(key).expect("channel");
        channel.initialize().expect("initialize");
        channel
            .with_data(Instant::now() + TIMEOUT, |data| {
                data.request = 1;
                Ok(())
            })
            .expect("request");
        channel.receive(|| true).expect("first delivery");
        channel
            .receive(|| panic!("completed focus request queued twice"))
            .expect("duplicate signal");
        channel
            .with_data(Instant::now() + TIMEOUT, |data| {
                data.request = 2;
                Ok(())
            })
            .expect("new request");
        channel.receive(|| true).expect("new delivery");
        channel
            .with_data(Instant::now() + TIMEOUT, |data| {
                assert_eq!(data.response, 2);
                assert_eq!(data.accepted, 1);
                Ok(())
            })
            .expect("new request acknowledged");
        std::fs::remove_dir(root).expect("remove test data");
    }

    #[test]
    fn an_unresponsive_owner_times_out_instead_of_reporting_success() {
        let root = directory();
        let mut channel = Channel::open(root.file_name().expect("name").to_str().expect("ASCII"))
            .expect("channel");
        channel.initialize().expect("initialize");
        let error = channel
            .redirect(Instant::now() + Duration::from_millis(50))
            .expect_err("must time out without listener");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        std::fs::remove_dir(root).expect("remove test data");
    }

    #[test]
    fn disabled_single_instance_mode_allows_independent_processes() {
        let root = directory();
        let first = child(&root, "independent");
        wait_file(&root.join(format!("ready-{}", first.id())));
        let second = child(&root, "independent");
        wait_file(&root.join(format!("ready-{}", second.id())));
        std::fs::write(root.join("run"), []).expect("release both processes");
        success(first);
        success(second);
        std::fs::remove_dir_all(root).expect("remove test data");
    }

    #[test]
    fn a_late_acknowledgment_cannot_complete_a_different_launch() {
        let root = directory();
        let key = root
            .file_name()
            .expect("name")
            .to_str()
            .expect("ASCII")
            .to_owned();
        let mut channel = Channel::open(&key).expect("channel");
        channel.initialize().expect("initialize");
        let error = channel
            .redirect(Instant::now() + Duration::from_millis(20))
            .expect_err("no receiver");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(
            unsafe { WaitForSingleObject(channel.request.as_raw_handle(), 0) },
            0
        );
        let (done, result) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let mut sender = Channel::open(&key).expect("sender");
            done.send(sender.redirect(Instant::now() + TIMEOUT))
                .expect("result");
        });
        assert_eq!(
            unsafe { WaitForSingleObject(channel.request.as_raw_handle(), 1000) },
            0
        );
        channel
            .with_data(Instant::now() + TIMEOUT, |data| {
                assert_eq!(data.request, 2);
                data.response = 1;
                data.accepted = 1;
                Ok(())
            })
            .expect("stale response");
        check(unsafe { SetEvent(channel.response.as_raw_handle()) }).expect("wake sender");
        assert!(
            result.recv_timeout(Duration::from_millis(20)).is_err(),
            "stale response was accepted"
        );
        channel
            .with_data(Instant::now() + TIMEOUT, |data| {
                data.response = 2;
                Ok(())
            })
            .expect("current response");
        check(unsafe { SetEvent(channel.response.as_raw_handle()) }).expect("wake sender");
        assert!(
            result
                .recv_timeout(TIMEOUT)
                .expect("response")
                .expect("redirection")
        );
        worker.join().expect("sender thread");
        std::fs::remove_dir(root).expect("remove test data");
    }
}
