# bplaat/bastiaan

My personal website inside a Windows XP desktop running in your browser with [v86](https://github.com/copy/v86)

## Features

- Boots straight to the desktop from a saved snapshot behind the real Windows XP boot screen, C: streams in
  compressed 1 MB chunks as Windows reads it
- Legacy PC: 256 MB RAM, Standard PC HAL (no ACPI), Sound Blaster 16, NE2000 network card
- Absolute mouse over the VMware backdoor v86 emulates, the desktop resizes to the browser window
- Clipboard sync in both directions for text, HTML and images (Cmd acts as Ctrl on macOS)
- Drop files on the page to copy them to the XP desktop, links opened in Windows open in a new browser tab
- The session (memory and the disk blocks Windows wrote) is saved in IndexedDB and restored on the next visit, a
  click on the welcome back notice starts fresh. Best effort: the browser may clear it, and a new disk build drops it
  with a notice
- Every visitor's Windows XP is on one LAN through the server's relay, for LAN games like Age of Empires: no
  internet access, the relay hands out 10.86.0.x addresses and only forwards between visitors
- `bastiaan.exe`: this website as a native Win32 GDI program, opened on the desktop
- Age of Empires, Age of Empires II, Civilization III, Minesweeper and Pinball on the desktop, music with album art in
  Windows Media Player
- Internet Explorer 8
- Without JavaScript, like for search engines, the page is the website as plain HTML

## Building the disk

`offline/build.sh` builds the Windows XP disk and snapshot from the XP CD, see [offline/README.md](offline/README.md).

## Development

```sh
npm run server  # the Rust server in server/ on http://localhost:8080, Vite proxies its LAN relay
npm run dev
```

## Self hosting

The `Dockerfile` builds the site (`npm run build` into `dist/`, which needs the built disk in `public/winxp`) and the
server in `server/`: one static musl binary on Tokio and axum that serves `dist/` and the LAN relay on `/relay`, in an
image with nothing else.

```sh
docker build -t bastiaan .
```

Environment: `PORT` (8080), `DIST_PATH` (`dist`), `ORIGIN`, the site's origin so other pages cannot use the relay,
and `TRUST_PROXY=true` behind a reverse proxy: the relay then counts visitors by the last `X-Forwarded-For` entry.
Build with `VITE_RELAY_URL` to use another relay address, an empty value builds without network.

The server sends the caching headers (`immutable` for Vite's hashed assets, `no-cache` with `Last-Modified` for the
rest) and leaves TLS, HTTP/2 and compression to the proxy. With Traefik, whose `compress` middleware must skip the disk
chunks and snapshot (`application/octet-stream`, zstd already) and passes the relay's WebSocket on as is:

```yaml
services:
    bastiaan:
        image: bastiaan
        environment:
            ORIGIN: https://bastiaan.example
            TRUST_PROXY: 'true'
        labels:
            traefik.enable: 'true'
            traefik.http.routers.bastiaan.rule: Host(`bastiaan.example`)
            traefik.http.routers.bastiaan.tls.certresolver: letsencrypt
            traefik.http.routers.bastiaan.middlewares: bastiaan-compress
            traefik.http.middlewares.bastiaan-compress.compress.excludedcontenttypes: application/octet-stream
            traefik.http.services.bastiaan.loadbalancer.server.port: 8080
```

What the relay allows, per visitor: one MAC (the first one it uses) and the IPv4 address it leased, IPv4 and ARP
only, 1 MB/s, 1000 frames/s of which 100 broadcasts, 4 tabs per address, 253 visitors in total. There is no router, so
nothing reaches the internet or the server's own network.
