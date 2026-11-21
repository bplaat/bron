# Windows XP Toolchain

Builds Rust programs with the full standard library for 32-bit Windows XP SP3 and newer, with plain
clang and lld, without a Microsoft SDK. One build runs on every Windows version from XP on. The
target has the `winxp` vendor, distinct from the `win7` vendor of Windows 7 targets. Each target
imports the APIs provided by its minimum Windows version and loads newer APIs at runtime with
fallbacks. The normal Windows targets need Windows 10.
`build.sh` builds it all in the repository root's `target/winxp-toolchain/`, the `bin/cargo`
wrapper runs it for you:

- Copies the std sources of nightly Rust (`nightly-2026-11-14`), which match its
  compiler, to `../../target/winxp-toolchain/src/rust-std/` and applies `patches/rust-std/` as commits. Programs build std from them with
  `-Zbuild-std`
- Clones [mingw-w64](https://github.com/mingw-w64/mingw-w64) v14.0.0 to `../../target/winxp-toolchain/src/` and builds its headers for
  Windows XP (`_WIN32_WINNT=0x501`), its C runtime on XP's own `msvcrt.dll` and the import
  libraries of the system DLLs to `../../target/winxp-toolchain/sysroot/`
- Checks out LLVM's [compiler-rt](https://github.com/llvm/llvm-project/tree/main/compiler-rt/lib/builtins)
  builtins of llvmorg-23.1.3 and builds them to the sysroot

```sh
cd tools/winxp-toolchain
./build.sh                 # fetch, patch and build
./build.sh clean           # remove ../../target/winxp-toolchain/
./build.sh export-patches  # write the commits in ../../target/winxp-toolchain/src/rust-std/ back to patches/rust-std/
./build.sh test            # build test/rust/ and run it in Windows XP in QEMU
```

Everything is compiled for the Pentium 4 (`pentium4`, SSE2), the oldest CPU of the Rust i686
targets. Programs always abort on panics (`panic = "abort"`), so they link no unwinder and have no
unwind tables.

## Requirements

Rustup, LLVM with clang and lld, QEMU, mtools and bsdtar for the runner: on macOS `brew install llvm
lld qemu mtools` (macOS has bsdtar), on Linux the distribution's or apt.llvm.org's LLVM in
`/usr/lib/llvm-<version>` (or on the PATH) and `apt install qemu-system-x86 mtools libarchive-tools`.
`llvm.sh` finds LLVM for the scripts. Running programs needs a raw Windows XP
Professional SP3 disk image, set `WINXP_IMAGE` to its path. Building works without the disk, only
the runner needs it.

## Integration

Run cargo through the `bin/cargo` wrapper, it builds the toolchain, then runs the nightly
cargo with the patched std, the `i686-winxp-windows-gnu.json` target and `bin/run` as the runner:

```sh
tools/winxp-toolchain/bin/cargo build --release -p reversi
tools/winxp-toolchain/bin/cargo run --release -p example-bcanvas-showcase
```

Windows XP has some limits that the toolchain doesn't work around:

- `std::os::windows::process::ProcThreadAttributeList` links functions from Windows Vista, a
  program that uses it doesn't start on Windows XP
- DLLs that are loaded with `LoadLibrary` can't use thread locals, the Windows XP loader only sets
  up the TLS of the DLLs that a program links
- Without `SetThreadStackGuarantee` a stack overflow can end the process without its message

## Files

- `i686-winxp-windows-gnu.json` is the Rust target: the `i686-pc-windows-gnullvm` target linked by
  rust-lld for Windows XP (subsystem 5.01), with the `winxp` vendor. The std patches and bron libraries
  share compatibility paths with `win7`, while `cfg(target_vendor = "winxp")` selects XP-specific code
- `i686-winxp-windows-driver.json` is the Rust target for kernel drivers: `no_std` without MMX,
  SSE or the x87, which the kernel doesn't save for drivers, linked as a `.sys` for the native
  subsystem with `DriverEntry` as its entry point and the image checksum, without which Windows
  doesn't load a driver. Drivers import from `ntoskrnl.exe`, `hal.dll`
  and `videoprt.sys` with raw-dylib, so they don't need the sysroot. The
  [v86 guest integration](../../apps/web/bastiaan/winxp-image/guest/) builds its three drivers with it
- `bin/cargo` is the cargo wrapper, `bin/cargo-driver` the one for kernel drivers, which builds
  only core for the driver target. `bin/cc` and `bin/ar` are the C compiler, which also links, and
  the archiver for cc-rs build scripts and C programs, `bin/windres` is LLVM's resource compiler
  for winresource build scripts
- `bin/run` runs a program in Windows XP with `winxp-runner`, which it builds first with its agent
- `runner/` is `winxp-runner`, which runs a program headless in Windows XP in QEMU, with
  `winxp-runner-agent`, its guest side, which `bin/run` builds for Windows XP. The first run
  copies the Windows XP disk as a copy-on-write clone, puts the agent in its Startup
  folder, boots it until the agent is ready and saves that as a QEMU snapshot. Every run restores
  a copy of the snapshot, sends the program to the agent over COM1, prints its output and exits
  with its exit code. `--shot <file.png>` takes a screenshot after `--wait <seconds>` (default 10),
  for windowed programs, `--timeout <seconds>` (default 600) limits a run:

    ```sh
    bin/run --shot reversi.png --wait 30 ../../target/i686-winxp-windows-gnu/release/reversi.exe
    ```

    The machine is the one Windows XP was installed on: a Pentium III with SSE2 added, no ACPI,
    256 MB. The runner puts the Standard PC HAL of Windows XP's driver cache in place of the APIC
    HAL that setup picks in QEMU, which times the performance counter with a TSC it calibrates
    about 5 times too fast under emulation, so `Instant` was wrong. The Standard PC HAL uses the
    PIT, which QEMU emulates exactly, and boots faster. Windows XP installs devices for it at the
    first logon and asks for a restart, after which COM1 doesn't receive anymore, so the runner
    waits a minute and answers No before it saves the snapshot

- `patches/rust-std/` are the std patches, see below
- `test/rust/` is a Rust program that tests the toolchain, see below

## Test

`./build.sh test` builds `test/rust/` and runs it in Windows XP. It checks threads, parking and sleeps,
the mutex, condition variable, read-write lock, once cells, barrier and channels, thread local
destructors, the clocks, the environment and arguments, files, directories, metadata, permissions,
renames, canonical paths, hard links, file locks, child processes with piped output and the exit code of a panic, TCP
and UDP on the loopback interface, name lookups and random numbers. It prints every failed check
and `DONE <n> failed`.

`cargo xtask check-others` builds the toolchain and Reversi and the bcanvas showcase for Windows
XP, without running them, so it doesn't need a Windows XP disk.

## Patches

The std sources are the nightly's, the patches make its Windows 7 paths work on Windows XP. To
change them, edit and commit in `../../target/winxp-toolchain/src/rust-std/`, run `./build.sh` to try it
and `./build.sh export-patches` to write the commits back. The build stops instead of applying
changed patches over commits or edits that aren't exported yet. Cargo doesn't notice changes to the
std sources, the wrapper passes the patch commit to rustc so std is rebuilt.

- `0001` loads `GetFileInformationByHandleEx` and `SetFileInformationByHandle` at runtime and
  falls back to the NT functions they wrap, `NtQueryInformationFile`, `NtQueryDirectoryFile` and
  `NtSetInformationFile`. It lists directories with `FindExInfoStandard`, `FindExInfoBasic` came
  with Windows 7, and lets `SetThreadStackGuarantee` fail
- `0002` runs the TLS destructors from a TLS callback, Windows XP has no fiber local storage
- `0003` implements `Mutex`, `Condvar` and thread parking without SRW locks, condition variables
  and `WaitOnAddress`. The mutex waits on keyed events. Those can't wait with a timeout on Windows
  XP, which can give a release to a wait that times out at the same time, which then reports the
  timeout. So the thread parker waits on an auto-reset event of its own, and the condition
  variable queues its waiting threads, which park until a notify takes them
- `0004` falls back for `GetFinalPathNameByHandleW` with the NT path of `NtQueryObject`, for
  which std finds the drive letter itself, lets `CreateSymbolicLinkW` fail, Windows XP has no
  symbolic links, compares with `RtlUpcaseUnicodeChar` for `CompareStringOrdinal`, and sleeps
  with `Sleep` without `CreateWaitableTimerExW`
- `0005` names the pipes of child processes, which Windows XP can't create without a name
- `0006` aborts with `TerminateProcess` and the exit code of a fast fail, 0xC0000409, when the
  processor doesn't support the `__fastfail` interrupt, which is before Windows 8. There it's an
  access violation, which starts the error reporting
- `0007` uses the aborting personality stub of the MSVC targets on MinGW with `panic = "abort"`,
  which aborts on a foreign exception like the real one does there, so programs don't need
  libunwind
