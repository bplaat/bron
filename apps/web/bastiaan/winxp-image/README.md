# Windows XP image

Builds the Windows XP disk and desktop snapshot into `web/public/winxp` from a Windows XP Professional SP3 CD.
The build installs Windows, guest drivers and software, applies settings, activates Windows and saves the snapshot.

## Building

Builds on macOS and Linux with Node.js, QEMU, mtools, NASM, innoextract and the
[Windows XP toolchain](../../../../tools/winxp-toolchain): `brew install qemu mtools nasm innoextract` on macOS,
`apt install build-essential curl qemu-system-x86 mtools nasm libarchive-tools innoextract` on Linux. Run from the root
of the repository:

```sh
npm install
apps/web/bastiaan/winxp-image/build.sh           # build missing stages, then the native apps and the site
apps/web/bastiaan/winxp-image/build.sh software  # reinstall software from the base Windows install
apps/web/bastiaan/winxp-image/build.sh setup     # rerun guest setup from the software image
apps/web/bastiaan/winxp-image/build.sh install   # rebuild from a fresh Windows install
npm run dev --workspace bastiaan
```

The build runs headless. Approximate times: 25 minutes for Windows, 1 minute for software, 15 minutes for guest setup
and about 2 minutes for the native apps and the site, all a changed native app needs.

## Required files

These files are ignored by git. Add them before building:

| Path                                                                           | What                                                                                               |
| ------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------- |
| `winxp-image/winxp.iso`                                                        | `en_windows_xp_professional_with_service_pack_3_x86_cd_x14-80428.iso`                              |
| `winxp-image/music/`                                                           | Music for My Music: `Snail's House/Ordinary Songs 3` and `4`                                       |
| `winxp-image/software/age-of-empires/cd/AOE1/`                                 | Age of Empires CD contents                                                                         |
| `winxp-image/software/age-of-empires/cd/AOE1ROR/`                              | Age of Empires: The Rise of Rome CD contents                                                       |
| `winxp-image/software/age-of-empires-2/cd/AOE2/`                               | Age of Empires II: The Age of Kings CD contents                                                    |
| `winxp-image/software/age-of-empires-2/cd/AOE2CONQ/`                           | Age of Empires II: The Conquerors CD contents                                                      |
| `winxp-image/software/civilization-3/setup_civilization3_complete_2.0.0.7.exe` | Civilization III Complete (GOG)                                                                    |
| `winxp-image/software/7zip/7z2606.exe`                                         | [7-Zip](https://www.7-zip.org/) 26.06, 32-bit                                                      |
| `winxp-image/software/notepad-plus-plus/npp.7.9.2.Installer.exe`               | [Notepad++](https://notepad-plus-plus.org/) 7.9.2, the last version for XP                         |
| `winxp-image/software/mitec/*.zip`                                             | [MiTeC](https://www.mitec.cz/) `EXE.zip` 2.5.5, `HEXEdit.zip` 6.3.0 and `TMX.zip` 3.5.0, see below |

Generated: `winxp-image/out/` (install images, screenshots), the built `.exe`/`.sys` files, `software/*/root/` and
`web/public/winxp/`.

## Build stages

1. **Install** (`install.mjs`, `qemu.mjs`, `winnt.sif`): creates an 8 GiB FAT32 disk with the MBR of `mbr.asm` and
   installs XP unattended in QEMU with 256 MB RAM and no ACPI, using the retail product key in `winnt.sif`. Output:
   `out/install.img`.
2. **Software** (`disk.mjs software`, `software.mjs`): replaces QEMU's APIC HAL with the Standard PC HAL required by
   v86, removes the extra `boot.ini` entry and copies software onto the disk. QEMU runs `guest/software.cmd` to
   execute each `install.cmd`. Output: `out/software.img`.
3. **Setup** (`disk.mjs setup`, `setup.mjs`, `disk.mjs finish`): copies guest integration, MS Gothic and music, then
   boots v86. `v86setup.exe` installs drivers, applies settings, activates Windows and creates the `bplaat` account.
   Activation failure stops the build. The first user logon reapplies settings reset by Windows and fills the Media
   Player library. Setup files and caches are removed and free space is zeroed. Output: `out/disk.img`.
4. **Native apps** (`disk.mjs apps`): copies freshly built Bastiaan, Reversi, RedSquare and 2048 with its
   MiniBlink DLL onto a copy of the disk. The software stage installs their shortcuts and logon entry. Output: `out/site.img`.
5. **Site** (`make-state.mjs`, `chunks.mjs`, `bootscreen.mjs`): compresses the disk into 1 MiB zstd chunks, only the
   chunks that changed since the last build, saves a desktop snapshot at 1024x768 and extracts boot screen assets from
   `ntoskrnl.exe`. The build ID in `disk.json` invalidates browser sessions from older builds.

## Image contents

- Windows XP Professional SP3, activated, with the Luna theme, stretched Bliss wallpaper and computer name `WINXP`.
- Administrator account `bplaat` (Bastiaan), with no password, automatic logon and the site's avatar as picture.
- Hidden files, system files, extensions and full paths shown; Details view in all folders.
- Pagefile, hibernation, System Restore, prefetch, boot defrag and system sounds disabled; reduced animations.
- MS Gothic linked to interface fonts for Japanese text.
- NE2000 (Realtek RTL8029) networking with DHCP. The guest agent disables NetBIOS and renews the lease when the
  browser connects to the relay.
- Printing, updates, Security Center, error reporting, AutoPlay, time sync, Task Scheduler and network services
  other than DHCP disabled.
- Notification balloons, the XP tour, Desktop Cleanup Wizard, low disk warnings, AutoRun and first-run prompts disabled.
- Windows Media Player with its menu bar, album art from MP3 covers (`Folder.jpg`), My Music and Windows MIDI files.
- Desktop shortcuts for My Computer, My Documents, Media Player, Bastiaan, Reversi, RedSquare, 2048, Minesweeper,
  Pinball and the games below. [Bastiaan](../../../desktop/bastiaan), the website as a native app, starts at logon.
- Age of Empires with The Rise of Rome and MIDI music; Age of Empires II with The Conquerors, CD music and videos;
  Civilization III Complete.
- Notepad++ without its updater, associated with text and source files; MiTeC EXE Explorer, Hexadecimal Editor and
  Task Manager DeLuxe in `Program Files\MiTeC`.
- [Win32 development toolchain](winxp-dev-toolchain/README.md) in `Program Files\Toolchain` with Hello World, Analog
  Clock and the Direct3D 8 Triangle, Cube and Blocks sample projects in the user profile.
- Start menu entry for 7-Zip.

## Adding software

Add a folder to `software/`:

- `Makefile` (optional): runs first on the host (bsdtar, innoextract, Node.js), generated
  `root/` contents are copied onto C: without replacing existing files
- `install.cmd`: runs once, as Administrator, in the folder copied to `C:\WINDOWS\v86\software\<name>`, for silent
  installers, `regedit /s` imports and shortcuts
- `.diskignore`: files excluded from the disk; `cd/`, `root/` and dotfiles are always excluded

Then run `winxp-image/build.sh software`. Helpers in the guest:

- `%SystemRoot%\v86\shortcut.vbs <Desktop | Start menu folder> <name> <target> [arguments]`: shortcut for all users
- `call %SystemRoot%\v86\userreg.cmd <file.reg>`: imports per user settings into the Default User profile, keys
  written as `[HKEY_USERS\V86Default\...]`
- `echo LOG <text>> COM1`: prints progress in the build output

### MiTeC versions

Use the archived versions below. Later builds use Themida, which detects a debugger under v86: EXE Explorer 3.x,
Hexadecimal Editor 7.0 and Task Manager DeLuxe 3.6.0 onward. The listed versions run on XP.

- [EXE.zip 2.5.5](https://web.archive.org/web/20200224204612id_/http://www.mitec.cz/Downloads/EXE.zip)
- [HEXEdit.zip 6.3.0](https://web.archive.org/web/20170320033643id_/http://www.mitec.cz/Downloads/HEXEdit.zip)
- [TMX.zip 3.5.0](https://web.archive.org/web/20200622081830id_/http://www.mitec.cz/Downloads/TMX.zip)

### Old Microsoft game setups (`ebu.mjs`)

Silent installers run through `install.cmd`. Other installers are extracted on the host into `root/`: Civilization III
with innoextract and Age of Empires with `ebu.mjs` and bsdtar.

`ebu.mjs` reads the Age of Empires setup file table as NUL-terminated CD path and destination pairs. It resolves
`%APPPATH\%STRINGn` names from the setup DLL's string table and copies `SYSTEM` files to Windows. Registry values
in `install.reg` come from the setup scripts and a reference install. Full game data, music, videos and campaign
narration are included.

## Guest integration (`guest/`)

A Rust crate with a binary per program and driver, built with the Windows XP toolchain.

- `v86mou.sys`: mouse filter above mouclass for absolute and pointer-locked relative input through the VMware
  backdoor, including buttons and scrolling.
- `v86tools.sys`: independent host channel exposing `\\.\V86Tools` to the guest agent for display size, clock
  and message transfers.
- `v86vid.sys` and `v86vid.inf`: Bochs VBE video miniport with 64 MB of VRAM. Supports 8-bit palette, 16-bit and
  32-bit DirectDraw modes, plus a mode matching the browser size. Monochrome pointers up to 32x32 use the browser
  cursor; Windows draws colour pointers and pointers that invert the screen.
- `v86disp.dll`: the display driver of the paravirtualized graphics adapter. The page draws
  GDI blits, patterns, text, lines, stretching, alpha and color keys onto the VRAM, the engine draws what it declines.
  A DirectDraw HAL with VRAM surfaces, blits, flips, vertical blanks and 8-bit palettes.
- `v86gputest.exe`: the conformance test of the display driver, see [Testing](#testing).
- `v86agent.exe`: syncs the display size, host clock and clipboard, saves dropped files and opens HTTP/HTTPS links
  in the browser. Display resizing pauses when a game uses a different colour depth. On request, the agent disables
  NetBIOS and renews the relay's DHCP lease.
- `v86setup.exe`: configures the guest. `activate.vbs` and `confid.exe` activate Windows offline; `library.vbs` fills
  the Media Player library.

Host integration: [`web/src/guest-host.ts`](../web/src/guest-host.ts) and the display driver's 2D engine in
[`web/src/gpu/`](../web/src/gpu). Protocols: `guest/src/protocol.rs` and `guest/src/gpu.rs`. Shared machine configuration:
[`web/src/machine.ts`](../web/src/machine.ts).

## Testing

```sh
npm test --workspace bastiaan                                                  # the 2D engine on the host
cargo test --manifest-path apps/web/bastiaan/winxp-image/guest/Cargo.toml --lib # the command encoding
apps/web/bastiaan/winxp-image/gpu-test.mjs                                     # the display driver in XP
```

`gpu-test.mjs` boots a copy of `out/site.img` headless with `v86gputest.exe` at logon. It draws every GDI case into a
DIB section, which only the engine draws, and onto the screen through the display driver, in 32 and 16 bpp, and
compares them. Then DirectDraw in 8, 16 and 32 bpp: fills, copies, color keys, stretching, mirroring, overlap, vertical
blanks, flips and palettes, with the shown picture checked by the page. It takes a few minutes and fails on any
difference.
