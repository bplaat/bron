# bplaat/bastiaan

My personal website inside a Windows XP desktop running in your browser with [v86](https://github.com/copy/v86)

## Features

- Boots straight to the desktop from a saved snapshot behind the real Windows XP boot screen, C: streams in
  compressed 1 MB chunks as Windows reads it
- Legacy PC: 256 MB RAM, Standard PC HAL (no ACPI), Sound Blaster 16, no network
- Absolute mouse over the VMware backdoor v86 emulates, the desktop resizes to the browser window
- Clipboard sync in both directions for text, HTML and images (Cmd acts as Ctrl on macOS)
- Drop files on the page to copy them to the XP desktop, links opened in Windows open in a new browser tab
- The session (memory and the disk blocks Windows wrote) is saved in IndexedDB and restored on the next visit, a
  click on the welcome back notice starts fresh. Best effort: the browser may clear it, and a new disk build drops it
  with a notice
- `bastiaan.exe`: this website as a native Win32 GDI program, opened on the desktop
- Age of Empires, Age of Empires II, Civilization III, Minesweeper and Pinball on the desktop, music with album art in
  Windows Media Player

## Building the disk

`offline/build.sh` builds the Windows XP disk and snapshot from the XP CD, see [offline/README.md](offline/README.md).

## Development

```sh
npm run dev
```
