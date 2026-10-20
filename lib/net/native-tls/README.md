# native-tls

A minimal replacement for the [native-tls](https://crates.io/crates/native-tls) crate.

## Getting Started

A simple example that opens a TLS connection and sends an HTTPS request:

```rs
use std::io::{Read, Write};
use std::net::TcpStream;
use native_tls::TlsConnector;

let stream = TcpStream::connect("example.com:443").unwrap();
let connector = TlsConnector::new().unwrap();
let mut tls = connector.connect("example.com", stream).unwrap();
tls.write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n").unwrap();
let mut buf = Vec::new();
tls.read_to_end(&mut buf).unwrap();
```

## Platforms

Without the `vendored` feature, the platform's native TLS library is used:

| Platform    | Backend                              | TLS 1.2 | TLS 1.3 |
| ----------- | ------------------------------------ | ------- | ------- |
| Windows     | SChannel (OS built-in)               | Yes     | Yes     |
| macOS / iOS | SecureTransport (Security.framework) | Yes     | No¹     |
| Linux/other | OpenSSL 1.0.2+ / 1.1.x / 3.x / 4.x   | Yes     | Yes²    |

With the `vendored` feature, rustls is used on all platforms instead:

| Platform | Backend                          | TLS 1.2 | TLS 1.3 |
| -------- | -------------------------------- | ------- | ------- |
| All      | rustls + Graviola + webpki-roots | Yes     | Yes     |

¹ SecureTransport is deprecated and limited to TLS 1.2. TLS 1.3 needs Network.framework, whose
async-only API does not fit a synchronous `Read`/`Write` interface.

² TLS 1.3 requires OpenSSL 1.1.1+. OpenSSL 1.0.2 supports TLS 1.2 only.

## Features

- `vendored` - Use [rustls](https://crates.io/crates/rustls) with the
  [Graviola](https://crates.io/crates/rustls-graviola) crypto provider and embedded CA roots
  ([webpki-roots](https://crates.io/crates/webpki-roots)) on all platforms instead of the native
  TLS library.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
