# Small-WebSocket Rust library

A simple and small WebSocket library for the [small-http](../small-http) library.

## Getting Started

A simple WebSocket client that sends and receives a text message:

```rs
use small_websocket::{Message, WebSocket};

let mut websocket = WebSocket::connect("ws://127.0.0.1:8080/ws").unwrap();
websocket.send(Message::Text("Hello!".to_string())).unwrap();

if let Message::Text(text) = websocket.recv().unwrap() {
    println!("{text}");
}
```

Use `small_websocket::upgrade` from a small-http request handler to accept server connections. See
the [example](examples/small-websocket-simple.rs) for a complete echo server.

## Features

- WebSocket client and server handshakes
- Text, binary, ping, pong, and close messages
- Blocking and non-blocking receive methods, clones send while another thread receives
- Optional client support, enabled by default

## License

Copyright © 2025 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
