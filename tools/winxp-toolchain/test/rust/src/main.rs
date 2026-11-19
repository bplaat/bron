/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Tests the parts of std the Windows XP toolchain patches or that use newer Windows functions,
//! in Windows XP. Prints every failed check and `DONE <n> failed`, exits with 1 on failures.

use std::cell::Cell;
use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Condvar, LazyLock, Mutex, OnceLock, RwLock, mpsc};
use std::time::{Duration, Instant, SystemTime};
use std::{env, fs, thread};

static FAILED: AtomicUsize = AtomicUsize::new(0);

macro_rules! check {
    ($condition:expr) => {
        if !$condition {
            FAILED.fetch_add(1, Ordering::Relaxed);
            println!("  line {}: {}", line!(), stringify!($condition));
        }
    };
}

fn main() -> ExitCode {
    // The child processes of the process test
    match env::args().nth(1).as_deref() {
        Some("child") => {
            print!("{}", env::args().skip(2).collect::<Vec<_>>().join("|"));
            return ExitCode::from(42);
        }
        Some("panic") => panic!("child panic"),
        _ => {}
    }
    for (name, test) in [
        ("threads", test_threads as fn()),
        ("sync", test_sync),
        ("thread locals", test_thread_locals),
        ("time", test_time),
        ("env", test_env),
        ("fs", test_fs),
        ("process", test_process),
        ("net", test_net),
        ("random", test_random),
    ] {
        let failed = FAILED.load(Ordering::Relaxed);
        test();
        println!(
            "{} {name}",
            if FAILED.load(Ordering::Relaxed) == failed {
                "ok  "
            } else {
                "FAIL"
            }
        );
    }
    let failed = FAILED.load(Ordering::Relaxed);
    println!("DONE {failed} failed");
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn test_threads() {
    let handles = (0..16)
        .map(|i| thread::spawn(move || i * 2))
        .collect::<Vec<_>>();
    check!(handles.into_iter().map(|h| h.join().unwrap()).sum::<i32>() == 240);

    let named = thread::Builder::new()
        .name("worker".into())
        .spawn(|| thread::current().name().map(str::to_owned));
    check!(named.unwrap().join().unwrap().as_deref() == Some("worker"));

    let mut values = [1, 2, 3];
    thread::scope(|scope| {
        for value in &mut values {
            scope.spawn(move || *value *= 10);
        }
    });
    check!(values == [10, 20, 30]);

    // Parking, with and without an unpark
    let start = Instant::now();
    let parked = thread::spawn(|| {
        thread::park();
        Instant::now()
    });
    thread::sleep(Duration::from_millis(100));
    parked.thread().unpark();
    check!(parked.join().unwrap() - start >= Duration::from_millis(90));
    let start = Instant::now();
    thread::park_timeout(Duration::from_millis(100));
    check!(start.elapsed() >= Duration::from_millis(90));

    let start = Instant::now();
    thread::sleep(Duration::from_millis(200));
    check!(start.elapsed() >= Duration::from_millis(190));
    check!(thread::available_parallelism().is_ok());
}

fn test_sync() {
    // Contended mutex
    let counter = Arc::new(Mutex::new(0));
    let handles = (0..8)
        .map(|_| {
            let counter = counter.clone();
            thread::spawn(move || {
                for _ in 0..10000 {
                    *counter.lock().unwrap() += 1;
                }
            })
        })
        .collect::<Vec<_>>();
    handles.into_iter().for_each(|h| h.join().unwrap());
    check!(*counter.lock().unwrap() == 80000);
    let mutex = Mutex::new(());
    let guard = mutex.lock().unwrap();
    check!(mutex.try_lock().is_err());
    drop(guard);
    check!(mutex.try_lock().is_ok());

    // Condition variable with many waiters, notify_one and notify_all
    let pair = Arc::new((Mutex::new(0), Condvar::new()));
    let waiters = (0..4)
        .map(|_| {
            let pair = pair.clone();
            thread::spawn(move || {
                let (lock, condvar) = &*pair;
                let _guard = condvar
                    .wait_while(lock.lock().unwrap(), |value| *value == 0)
                    .unwrap();
            })
        })
        .collect::<Vec<_>>();
    thread::sleep(Duration::from_millis(100));
    *pair.0.lock().unwrap() = 1;
    pair.1.notify_one();
    pair.1.notify_all();
    waiters.into_iter().for_each(|h| h.join().unwrap());
    let (lock, condvar) = &*pair;
    let start = Instant::now();
    let (_guard, result) = condvar
        .wait_timeout(lock.lock().unwrap(), Duration::from_millis(100))
        .unwrap();
    check!(result.timed_out() && start.elapsed() >= Duration::from_millis(90));

    // Ping pong between two threads, which also races notifies against timeouts
    let pair = Arc::new((Mutex::new(0u32), Condvar::new()));
    let other = {
        let pair = pair.clone();
        thread::spawn(move || {
            let (lock, condvar) = &*pair;
            let mut value = lock.lock().unwrap();
            while *value < 1000 {
                if *value % 2 == 1 {
                    *value += 1;
                    condvar.notify_one();
                }
                value = condvar
                    .wait_timeout(value, Duration::from_millis(1))
                    .unwrap()
                    .0;
            }
        })
    };
    {
        let (lock, condvar) = &*pair;
        let mut value = lock.lock().unwrap();
        while *value < 1000 {
            if *value % 2 == 0 {
                *value += 1;
                condvar.notify_one();
            }
            value = condvar
                .wait_timeout(value, Duration::from_millis(1))
                .unwrap()
                .0;
        }
    }
    other.join().unwrap();

    let lock = Arc::new(RwLock::new(0));
    let readers = (0..4)
        .map(|_| {
            let lock = lock.clone();
            thread::spawn(move || (0..1000).map(|_| *lock.read().unwrap()).max())
        })
        .collect::<Vec<_>>();
    for _ in 0..1000 {
        *lock.write().unwrap() += 1;
    }
    readers
        .into_iter()
        .for_each(|h| check!(h.join().unwrap() <= Some(1000)));
    check!(*lock.read().unwrap() == 1000);

    static ONCE: OnceLock<usize> = OnceLock::new();
    static LAZY: LazyLock<usize> = LazyLock::new(|| 7);
    let handles = (0..8)
        .map(|i| thread::spawn(move || *ONCE.get_or_init(|| i) + *LAZY))
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    check!(results.iter().all(|&r| r == results[0]));

    let barrier = Arc::new(Barrier::new(4));
    let handles = (0..4)
        .map(|_| {
            let barrier = barrier.clone();
            thread::spawn(move || barrier.wait().is_leader())
        })
        .collect::<Vec<_>>();
    check!(
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|&leader| leader)
            .count()
            == 1
    );

    let (sender, receiver) = mpsc::channel();
    for i in 0..4 {
        let sender = sender.clone();
        thread::spawn(move || (0..100).for_each(|j| sender.send(i * 100 + j).unwrap()));
    }
    drop(sender);
    check!(receiver.iter().sum::<i32>() == (0..400).sum());
    let (_sender, receiver) = mpsc::channel::<()>();
    check!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
}

fn test_thread_locals() {
    static DROPPED: AtomicUsize = AtomicUsize::new(0);
    struct Local(Cell<usize>);
    impl Drop for Local {
        fn drop(&mut self) {
            DROPPED.fetch_add(self.0.get(), Ordering::Relaxed);
        }
    }
    thread_local! {
        static LOCAL: Local = const { Local(Cell::new(0)) };
    }
    let handles = (1..=4)
        .map(|i| thread::spawn(move || LOCAL.with(|l| l.0.set(i))))
        .collect::<Vec<_>>();
    handles.into_iter().for_each(|h| h.join().unwrap());
    // The destructors run when the threads exit
    check!(DROPPED.load(Ordering::Relaxed) == 10);
    LOCAL.with(|l| l.0.set(5));
    check!(LOCAL.with(|l| l.0.get()) == 5);
}

fn test_time() {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    check!(now.as_secs() > 1_700_000_000);
    // The performance counter behind Instant runs at the speed of the system clock
    let (start, system_start) = (Instant::now(), SystemTime::now());
    thread::sleep(Duration::from_millis(500));
    let (elapsed, system_elapsed) = (start.elapsed(), system_start.elapsed().unwrap());
    check!(elapsed.abs_diff(system_elapsed) < Duration::from_millis(100));
}

fn test_env() {
    check!(env::args().count() == 3 && env::args().nth(2).as_deref() == Some("two words"));
    // SAFETY: the other tests don't run at the same time
    unsafe { env::set_var("WINXP_TEST", "value \u{e9}") };
    check!(env::var("WINXP_TEST").as_deref() == Ok("value \u{e9}"));
    check!(env::vars().any(|(key, _)| key == "WINXP_TEST"));
    check!(env::current_dir().is_ok_and(|dir| dir.is_absolute()));
    check!(env::current_exe().is_ok_and(|exe| exe.exists()));
    check!(env::temp_dir().is_dir());
    check!(env::home_dir().is_some_and(|home| home.is_dir()));
}

fn test_fs() {
    let dir = env::temp_dir().join("winxp-toolchain-test");
    _ = fs::remove_dir_all(&dir);
    check!(fs::create_dir_all(dir.join("a/b")).is_ok());
    let path = dir.join("a/file.txt");
    check!(fs::write(&path, "hello").is_ok());
    check!(fs::read_to_string(&path).as_deref().ok() == Some("hello"));

    let mut file = fs::OpenOptions::new()
        .read(true)
        .append(true)
        .open(&path)
        .unwrap();
    check!(file.write_all(" world".as_bytes()).is_ok());
    check!(file.seek(SeekFrom::Start(6)).is_ok());
    let mut text = String::new();
    check!(file.read_to_string(&mut text).is_ok() && text == "world");
    let metadata = file.metadata().unwrap();
    check!(metadata.len() == 11 && metadata.is_file() && metadata.modified().is_ok());
    drop(file);

    let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    check!(file.set_len(5).is_ok() && file.metadata().unwrap().len() == 5);
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    check!(file.set_modified(time).is_ok());
    check!(fs::metadata(&path).unwrap().modified().ok() == Some(time));
    drop(file);

    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    check!(fs::set_permissions(&path, permissions.clone()).is_ok());
    check!(fs::metadata(&path).unwrap().permissions().readonly());
    permissions.set_readonly(false);
    check!(fs::set_permissions(&path, permissions.clone()).is_ok());
    // Through the handle, with SetFileInformationByHandle
    let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    permissions.set_readonly(true);
    check!(file.set_permissions(permissions.clone()).is_ok());
    check!(file.metadata().unwrap().permissions().readonly());
    permissions.set_readonly(false);
    check!(file.set_permissions(permissions).is_ok());
    check!(
        file.try_clone()
            .is_ok_and(|clone| clone.metadata().is_ok_and(|m| m.len() == 5))
    );
    check!(file.lock().is_ok() && file.unlock().is_ok());
    drop(file);
    check!(fs::hard_link(&path, dir.join("a/hard.txt")).is_ok());
    check!(fs::metadata(dir.join("a/hard.txt")).is_ok_and(|m| m.len() == 5));
    check!(fs::remove_file(dir.join("a/hard.txt")).is_ok());

    check!(fs::copy(&path, dir.join("a/b/copy.txt")).ok() == Some(5));
    check!(fs::rename(dir.join("a/b/copy.txt"), dir.join("a/b/moved.txt")).is_ok());
    check!(fs::rename(&path, dir.join("a/b/moved.txt")).is_ok());
    let mut names = fs::read_dir(dir.join("a"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    check!(names == ["b"]);
    check!(
        fs::canonicalize(dir.join("a/b/../b/moved.txt")).is_ok_and(|p| p.ends_with("moved.txt"))
    );
    check!(fs::symlink_metadata(dir.join("a/b")).is_ok_and(|m| m.is_dir()));
    // Windows XP has no symbolic links, std fails with ERROR_CALL_NOT_IMPLEMENTED there
    let symlink =
        std::os::windows::fs::symlink_file(dir.join("a/b/moved.txt"), dir.join("a/link.txt"));
    check!(symlink.is_err_and(|error| error.raw_os_error() == Some(120)));
    check!(fs::remove_file(dir.join("a/b/moved.txt")).is_ok());
    check!(fs::write(dir.join("a/b/again.txt"), "x").is_ok());
    check!(fs::remove_dir_all(&dir).is_ok() && !dir.exists());
}

fn test_process() {
    let output = Command::new("cmd")
        .args(["/c", "echo hello"])
        .output()
        .unwrap();
    check!(output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "hello");

    let output = Command::new(env::current_exe().unwrap())
        .args(["child", "a b", "c\"d"])
        .env("CHILD", "1")
        .output()
        .unwrap();
    check!(output.status.code() == Some(42));
    check!(output.stdout == b"a b|c\"d");

    // A panic aborts with the exit code of a fast fail, also without one before Windows 8
    let output = Command::new(env::current_exe().unwrap())
        .arg("panic")
        .output()
        .unwrap();
    check!(output.status.code() == Some(0xC0000409_u32 as i32));
    check!(String::from_utf8_lossy(&output.stderr).contains("child panic"));

    let mut child = Command::new("cmd")
        .args(["/c", "ping -n 30 127.0.0.1 > nul"])
        .spawn()
        .unwrap();
    check!(child.try_wait().is_ok_and(|status| status.is_none()));
    check!(child.kill().is_ok() && child.wait().is_ok());
    check!(std::process::id() != 0);
}

fn test_net() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0; 5];
        stream.read_exact(&mut buffer).unwrap();
        stream.write_all(&buffer).unwrap();
    });
    let mut stream = TcpStream::connect(address).unwrap();
    check!(
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .is_ok()
    );
    check!(stream.set_nodelay(true).is_ok());
    check!(stream.write_all(b"hello").is_ok());
    let mut buffer = [0; 5];
    check!(stream.read_exact(&mut buffer).is_ok() && &buffer == b"hello");
    server.join().unwrap();
    check!(TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_err());

    let a = UdpSocket::bind("127.0.0.1:0").unwrap();
    let b = UdpSocket::bind("127.0.0.1:0").unwrap();
    check!(a.send_to(b"ping", b.local_addr().unwrap()).is_ok());
    let mut buffer = [0; 4];
    check!(
        b.recv_from(&mut buffer)
            .is_ok_and(|(n, from)| n == 4 && from == a.local_addr().unwrap())
    );
    check!(
        ("localhost", 80)
            .to_socket_addrs()
            .is_ok_and(|mut addrs| addrs.next().is_some())
    );
}

fn test_random() {
    let (a, b) = (RandomState::new(), RandomState::new());
    check!(a.hash_one(1) != b.hash_one(1));
    let mut map = HashMap::new();
    map.insert("key", 1);
    check!(map.get("key") == Some(&1));
}
