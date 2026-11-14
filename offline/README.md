# Offline Windows XP build

Builds the Windows XP disk and desktop snapshot the site runs, from the Windows XP Professional SP3 CD, into
`public/winxp`. Everything is automatic: a fresh unattended install, the v86 drivers, all settings, offline
activation, the software and a desktop snapshot that restores instantly.

## Commands

```sh
npm install                  # v86, used to run the guest headless
offline/build.sh             # build what is missing in offline/out
offline/build.sh software    # reinstall the software on the kept base install, e.g. after adding some
offline/build.sh setup       # redo the v86 setup on the kept software install
offline/build.sh install     # start over with a fresh Windows install
npm run dev                  # try the result
```

Needs macOS with Docker, Node.js, QEMU and mtools (`brew install qemu mtools`). Everything runs headless, no
installer is clicked through. The base install takes about 25 minutes, the software about 1, the v86 setup and the
snapshot about 15.

## Files that are not committed

These must exist before building, they are ignored by git:

| Path                                                   | What                                                          |
| ------------------------------------------------------ | ------------------------------------------------------------- |
| `offline/winxp.iso`                                    | `en_windows_xp_professional_with_service_pack_3_x86_cd_x14-80428.iso` |
| `offline/product-key.txt`                              | the Windows XP Professional retail product key, one line      |
| `offline/music/`                                       | music for bplaat's My Music, now `Snail's House/Ordinary Songs 3` and `4` |
| `offline/software/age-of-empires/cd/AOE1/`             | Age of Empires CD contents                                     |
| `offline/software/age-of-empires/cd/AOE1ROR/`          | Age of Empires: The Rise of Rome CD contents                   |
| `offline/software/age-of-empires-2/cd/AOE2/`           | Age of Empires II: The Age of Kings CD contents                |
| `offline/software/age-of-empires-2/cd/AOE2CONQ/`       | Age of Empires II: The Conquerors CD contents                  |
| `offline/software/civilization-3/setup_civilization3_complete_2.0.0.7.exe` | Civilization III Complete (GOG)             |
| `offline/software/7zip/7z2605.exe`                     | [7-Zip](https://www.7-zip.org/) 26.05, 32-bit                  |
| `offline/software/notepad-plus-plus/npp.7.9.2.Installer.exe`   | [Notepad++](https://notepad-plus-plus.org/) 7.9.2, the last version for XP |
| `offline/software/mitec/*.zip`                         | [MiTeC](https://www.mitec.cz/) `EXE.zip` 2.5.5, `HEXEdit.zip` 6.3.0 and `TMX.zip` 3.5.0, see below |

Generated: `offline/out/` (install images, screenshots), the built `.exe`/`.sys` files, `software/*/root/` and
`public/winxp/`.

## Steps

1. **Install** (`install.mjs`, `qemu.mjs`, `winnt.sif`): creates an 8 GiB disk with syslinux MBR code and one FAT32
   partition, then installs XP unattended in QEMU (no ACPI, 256 MB like v86). Output `out/install.img`.
2. **Software** (`disk.mjs software`, `software.mjs`): puts the Standard PC HAL from the CD in place (QEMU has an
   APIC, so setup picks the APIC HAL, which hangs on v86), drops the unknown OS entry from `boot.ini`, copies every
   `software/*/root/` onto C: and the rest of each folder into `C:\WINDOWS\v86\software`, then boots QEMU where
   `guest/software.cmd` runs every `install.cmd`. Output `out/software.img`.
3. **Setup** (`disk.mjs setup`, `setup.mjs`, `disk.mjs finish`): copies the guest integration, MS Gothic and `music/`
   in and boots the disk headless in v86. At the Administrator logon `v86setup.exe` installs the drivers and the
   Sound Blaster 16, applies the settings below, activates Windows (the build fails when that does not work),
   creates the bplaat account and reboots into it. Its first logon applies the user settings once more, as Windows'
   per user setup resets some, and fills the Media Player library. Afterwards the setup files and caches are deleted
   and the free space zeroed. Output `out/disk.img`.
4. **Site** (`make-state.mjs`, `chunks.mjs`, `bootscreen.mjs`): splits the disk into 1 MiB zstd chunks on all cores,
   boots it once more at 1024x768 and saves the desktop snapshot, and extracts the boot screen bitmaps and palette
   from `ntoskrnl.exe`. `disk.json` gets a build id, browsers drop sessions saved for other builds.

## What the image contains

- Windows XP Professional SP3, activated, computer WINXP, Luna, Bliss stretched, My Computer, My Documents and Media
  Player on the desktop
- Account `bplaat` (Bastiaan), administrator without password, auto logon, the bassiebas picture
- Folder Options: hidden and system files shown, extensions shown, full paths, Details view everywhere
- No network, no pagefile, no hibernation, no System Restore, no prefetch or boot defrag, no system sounds, fewer
  animations
- MS Gothic linked into the interface fonts, so Japanese names show
- Disabled services: everything for network, printing, updates, Security Center, error reporting, AutoPlay, time
  sync and the task scheduler
- No nags: Security Center, updates, balloons, the tour, the Desktop Cleanup Wizard, low disk warnings, AutoRun,
  the Media Player first run wizard, the Display Settings offer
- Windows Media Player with the menu bar, the album art in Now Playing (`Folder.jpg` from the MP3 covers), its library
  with My Music and the Windows MIDI files
- Software on the desktop and in the Start menu: Bastiaan, Age of Empires (+ The Rise of Rome) with MIDI music,
  Age of Empires II (+ The Conquerors, license accepted) with the CD music and videos on disk, Civilization III
  Complete, Notepad++ (without its updater, opens text, ini, inf, xml and similar files), MiTeC EXE Explorer,
  Hexadecimal Editor and Task Manager DeLuxe in `Program Files\MiTeC`
- Minesweeper and Pinball on the desktop
- Start menu: 7-Zip

## Adding software

Add a folder to `software/`:

- `Makefile` (optional): runs first in the `winxp-tools` Docker image (MinGW, cabextract, innoextract, Python), a
  `root/` it builds is copied onto C: without replacing existing files
- `install.cmd`: runs once, as Administrator, in the folder copied to `C:\WINDOWS\v86\software\<name>`, for silent
  installers, `regedit /s` imports and shortcuts
- `.diskignore`: files not to copy to the disk, `cd/`, `root/` and dotfiles never are

Then run `offline/build.sh software`. Helpers in the guest:

- `%SystemRoot%\v86\shortcut.vbs <Desktop | Start menu folder> <name> <target> [arguments]`: shortcut for all users
- `call %SystemRoot%\v86\userreg.cmd <file.reg>`: imports per user settings into the Default User profile, keys
  written as `[HKEY_USERS\V86Default\...]`
- `echo LOG <text>> COM1`: prints progress in the build output

### MiTeC versions

Newer MiTeC builds are packed with Themida, which reports a debugger under v86 (EXE Explorer from 3.x, Hexadecimal
Editor from 7.0, Task Manager DeLuxe from 3.6.0, Desktop Explorer was split out of it later). The last unpacked
builds run on XP, mitec.cz only offers the latest, so they come from the Internet Archive:

- [EXE.zip 2.5.5](https://web.archive.org/web/20200224204612id_/http://www.mitec.cz/Downloads/EXE.zip)
- [HEXEdit.zip 6.3.0](https://web.archive.org/web/20170320033643id_/http://www.mitec.cz/Downloads/HEXEdit.zip)
- [TMX.zip 3.5.0](https://web.archive.org/web/20200622081830id_/http://www.mitec.cz/Downloads/TMX.zip)

### Old Microsoft game setups (`ebu.py`)

Silent installers run in `install.cmd`, installers that do not run on XP or need clicking are installed on the host
into `root/`: 7-Zip (extracted with 7z) and Civilization III (innoextract, the GOG setup's own steps are redone).

The Age of Empires setups have no silent mode, so `ebu.py` does what they do without running them: their setup
programs hold the file table as NUL terminated pairs of CD path and destination (`%APPPATH\%STRINGn` names come from
the setup DLL's string table), files from the CD's `SYSTEM` folder go to Windows. The game data in CABs is extracted
with cabextract. For Age of Empires the result is byte for byte what the real setup installs. The registry values
in each `install.reg` come from the setup scripts and a real install. Full installs, so the music, videos and
campaign narration are on disk.

## Guest integration (`guest/`)

- `v86mou.sys`: mouse filter above mouclass that turns the VMware backdoor queue into absolute input, and the
  `\\.\V86Tools` channel for the agent
- `v86vid.sys` + `v86vid.inf`: video miniport for the Bochs VBE adapter (with the stock `framebuf.dll`) with 8 bit
  (palette), 16 and 32 bit modes for DirectDraw games, plus the page size as an extra mode
- `v86agent.exe`: follows the page size (not while a game runs another colour depth) and the host clock, syncs the
  clipboard, saves dropped files and is the http and https handler, so links open in a new browser tab
- `v86setup.exe`: the one shot setup above, `activate.vbs` + `activate/confid.exe` (from
  [UMSKT](https://github.com/UMSKT/UMSKT)) activate Windows, `library.vbs` fills the Media Player library

The host side is `src/guest-host.js` (protocol in `guest/v86.h`), the machine config shared with the site is
`src/machine.js`.
