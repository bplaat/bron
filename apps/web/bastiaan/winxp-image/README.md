# Windows XP image

Builds the Windows XP disk and desktop snapshot into `web/public/winxp` from a Windows XP Professional SP3 CD.
The build installs Windows, guest drivers and software, applies settings, activates Windows and saves the snapshot.

## Building

Requires macOS, Docker, Node.js, QEMU and mtools (`brew install qemu mtools`). Run from the root of the repository:

```sh
npm install
apps/web/bastiaan/winxp-image/build.sh           # build missing stages in winxp-image/out
apps/web/bastiaan/winxp-image/build.sh software  # reinstall software from the base Windows install
apps/web/bastiaan/winxp-image/build.sh setup     # rerun guest setup from the software image
apps/web/bastiaan/winxp-image/build.sh install   # rebuild from a fresh Windows install
npm run dev --workspace bastiaan
```

The build runs headless. Approximate times: 25 minutes for Windows, 1 minute for software and 15 minutes for guest
setup and the snapshot.

## Required files

These files are ignored by git. Add them before building:

| Path                                                                           | What                                                                                                                                                 |
| ------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `winxp-image/winxp.iso`                                                        | `en_windows_xp_professional_with_service_pack_3_x86_cd_x14-80428.iso`                                                                                |
| `winxp-image/product-key.txt`                                                  | the Windows XP Professional retail product key, one line                                                                                             |
| `winxp-image/music/`                                                           | Music for My Music: `Snail's House/Ordinary Songs 3` and `4`                                                                                         |
| `winxp-image/software/age-of-empires/cd/AOE1/`                                 | Age of Empires CD contents                                                                                                                           |
| `winxp-image/software/age-of-empires/cd/AOE1ROR/`                              | Age of Empires: The Rise of Rome CD contents                                                                                                         |
| `winxp-image/software/age-of-empires-2/cd/AOE2/`                               | Age of Empires II: The Age of Kings CD contents                                                                                                      |
| `winxp-image/software/age-of-empires-2/cd/AOE2CONQ/`                           | Age of Empires II: The Conquerors CD contents                                                                                                        |
| `winxp-image/software/civilization-3/setup_civilization3_complete_2.0.0.7.exe` | Civilization III Complete (GOG)                                                                                                                      |
| `winxp-image/software/7zip/7z2605.exe`                                         | [7-Zip](https://www.7-zip.org/) 26.05, 32-bit                                                                                                        |
| `winxp-image/software/notepad-plus-plus/npp.7.9.2.Installer.exe`               | [Notepad++](https://notepad-plus-plus.org/) 7.9.2, the last version for XP                                                                           |
| `winxp-image/software/mitec/*.zip`                                             | [MiTeC](https://www.mitec.cz/) `EXE.zip` 2.5.5, `HEXEdit.zip` 6.3.0 and `TMX.zip` 3.5.0, see below                                                   |
| `winxp-image/software/internet-explorer-8/IE8-WindowsXP-x86-ENU.exe`           | Internet Explorer 8 for XP, [Internet Archive](https://archive.org/details/IE8-WindowsXP-x86-ENU) (SHA-1 `e489483e5001f95da04e1ebf3c664173baef3e26`) |

Generated: `winxp-image/out/` (install images, screenshots), the built `.exe`/`.sys` files, `software/*/root/` and
`web/public/winxp/`.

## Build stages

1. **Install** (`install.mjs`, `qemu.mjs`, `winnt.sif`): creates an 8 GiB FAT32 disk with a syslinux MBR and installs
   XP unattended in QEMU with 256 MB RAM and no ACPI. Output: `out/install.img`.
2. **Software** (`disk.mjs software`, `software.mjs`): replaces QEMU's APIC HAL with the Standard PC HAL required by
   v86, removes the extra `boot.ini` entry and copies software onto the disk. QEMU runs `guest/software.cmd` to
   execute each `install.cmd`. Output: `out/software.img`.
3. **Setup** (`disk.mjs setup`, `setup.mjs`, `disk.mjs finish`): copies guest integration, MS Gothic and music, then
   boots v86. `v86setup.exe` installs drivers, applies settings, activates Windows and creates the `bplaat` account.
   Activation failure stops the build. The first user logon reapplies settings reset by Windows and fills the Media
   Player library. Setup files and caches are removed and free space is zeroed. Output: `out/disk.img`.
4. **Site** (`make-state.mjs`, `chunks.mjs`, `bootscreen.mjs`): compresses the disk into 1 MiB zstd chunks, saves a
   desktop snapshot at 1024x768 and extracts boot screen assets from `ntoskrnl.exe`. The build ID in `disk.json`
   invalidates browser sessions from older builds.

## Image contents

- Windows XP Professional SP3, activated, with the Luna theme, stretched Bliss wallpaper and computer name `WINXP`.
- Administrator account `bplaat` (Bastiaan), with no password, automatic logon and the bassiebas picture.
- Hidden files, system files, extensions and full paths shown; Details view in all folders.
- Pagefile, hibernation, System Restore, prefetch, boot defrag and system sounds disabled; reduced animations.
- MS Gothic linked to interface fonts for Japanese text.
- NE2000 (Realtek RTL8029) networking with DHCP. The guest agent disables NetBIOS and renews the lease when the
  browser connects to the relay.
- Printing, updates, Security Center, error reporting, AutoPlay, time sync, Task Scheduler and network services
  other than DHCP disabled.
- Notification balloons, the XP tour, Desktop Cleanup Wizard, low disk warnings, AutoRun and first-run prompts disabled.
- Windows Media Player with its menu bar, album art from MP3 covers (`Folder.jpg`), My Music and Windows MIDI files.
- Desktop shortcuts for My Computer, My Documents, Media Player, Bastiaan, Minesweeper, Pinball and the games below.
- Age of Empires with The Rise of Rome and MIDI music; Age of Empires II with The Conquerors, CD music and videos;
  Civilization III Complete.
- Notepad++ without its updater, associated with text, ini, inf and xml files; MiTeC EXE Explorer, Hexadecimal Editor
  and Task Manager DeLuxe in `Program Files\MiTeC`.
- Start menu entries for 7-Zip and Internet Explorer 8, with IE's first-run wizard and default browser check disabled.

## Adding software

Add a folder to `software/`:

- `Makefile` (optional): runs first in the `winxp-tools` Docker image (MinGW, cabextract, innoextract, Python), generated
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

### Old Microsoft game setups (`ebu.py`)

Silent installers run through `install.cmd`. Other installers are extracted on the host into `root/`: 7-Zip with
7z, Civilization III with innoextract and Age of Empires with `ebu.py` and cabextract.

`ebu.py` reads the Age of Empires setup file table as NUL-terminated CD path and destination pairs. It resolves
`%APPPATH\%STRINGn` names from the setup DLL's string table and copies `SYSTEM` files to Windows. Registry values
in `install.reg` come from the setup scripts and a reference install. Full game data, music, videos and campaign
narration are included.

## Guest integration (`guest/`)

- `v86mou.sys`: mouse filter above mouclass for absolute input through the VMware backdoor. Exposes the
  `\\.\V86Tools` channel to the guest agent.
- `v86vid.sys` and `v86vid.inf`: Bochs VBE video miniport using `framebuf.dll`. Supports 8-bit palette, 16-bit and
  32-bit DirectDraw modes, plus a mode matching the browser size. Monochrome pointers up to 32x32 use the browser
  cursor; Windows draws colour pointers and pointers that invert the screen.
- `v86agent.exe`: syncs the display size, host clock and clipboard, saves dropped files and opens HTTP/HTTPS links
  in the browser. Display resizing pauses when a game uses a different colour depth. On request, the agent disables
  NetBIOS and renews the relay's DHCP lease.
- `v86setup.exe`: configures the guest. `activate.vbs` and `activate/confid.exe` (from
  [UMSKT](https://github.com/UMSKT/UMSKT)) activate Windows; `library.vbs` fills the Media Player library.

Host integration: [`web/src/guest-host.js`](../web/src/guest-host.js). Protocol: `guest/v86.h`. Shared machine
configuration: [`web/src/machine.js`](../web/src/machine.js).
