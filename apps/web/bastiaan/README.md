# Bastiaan

My personal website inside a Windows XP desktop running in your browser with [v86](https://github.com/copy/v86)

## Features

- Windows XP runs in [v86](https://github.com/copy/v86) with 256 MB RAM, Sound Blaster 16 audio and an NE2000 network card.
- A saved desktop snapshot loads behind the XP boot screen. The disk streams in compressed 1 MiB chunks.
- The desktop resizes to the browser window and uses the browser cursor.
- Text, HTML and images sync through the clipboard. Cmd acts as Ctrl on macOS.
- Dropped files appear on the XP desktop. Links open in a new browser tab.
- Sessions are saved in IndexedDB. The welcome back notice lets you start fresh. Browser storage cleanup and new
  disk builds clear saved sessions.
- Visitors share a LAN for multiplayer games. The relay assigns 10.86.0.x addresses without internet access.
- Includes [the website as a native app](../../desktop/bastiaan), [Reversi](../../../games/reversi), Age of Empires I
  and II, Civilization III, Minesweeper, Pinball, Windows Media Player with music and album art, and Internet Explorer 8.
- The website is also available as plain HTML without JavaScript.

## Building the image

Run `apps/web/bastiaan/winxp-image/build.sh` to build the Windows XP disk and snapshot from the XP CD.
See [winxp-image/README.md](winxp-image/README.md) for requirements and build options.

## Development

From the root of the repository:

```sh
cargo run --release -p bastiaan-server  # http://localhost:8080, Vite proxies the LAN relay
npm install
npm run dev --workspace bastiaan
```

## Self hosting

Build the XP image into `web/public/winxp` first. The `Dockerfile` builds the site and Rust server, which serves the
site and LAN relay (`/relay`) from a static binary.

```sh
docker build -t bastiaan -f apps/web/bastiaan/Dockerfile .
```

Server environment variables:

| Variable      | Default    | Purpose                                                                            |
| ------------- | ---------- | ---------------------------------------------------------------------------------- |
| `PORT`        | `8080`     | Listen port                                                                        |
| `DIST_PATH`   | `web/dist` | Site directory                                                                     |
| `ORIGIN`      |            | Site origin allowed to use the relay                                               |
| `TRUST_PROXY` | `false`    | Set to `true` behind a proxy to count visitors by the last `X-Forwarded-For` entry |

Set `VITE_RELAY_URL` at build time to use another relay address, or an empty value to disable networking.

The server uses `immutable` caching for Vite's hashed assets and `no-cache` with `Last-Modified` for other files.
Configure TLS, HTTP/2 and compression in the reverse proxy. Exclude `application/octet-stream` from compression:
the disk chunks and snapshot are already zstd compressed. Example Traefik configuration:

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

The relay accepts IPv4 and ARP, restricted to each visitor's first MAC address and leased IP address. Limits:
1 MB/s, 1000 frames/s including up to 100 broadcasts, 4 tabs per address and 253 visitors total. There is no router
or access to the server's network.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
