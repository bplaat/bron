# Small-HTTP Rust library

A simple and small HTTP/1.1 server/client library.

## Getting Started

A simple example that opens an HTTP server and serves a response:

```rs
use std::net::{Ipv4Addr, TcpListener};
use small_http::{Request, Response};

fn handler(_req: &Request) -> Response {
    Response::with_body("Hello World!")
}

fn main() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 8080))
        .unwrap_or_else(|_| panic!("Can't bind to port"));
    small_http::serve(listener, handler);
}
```

A simple example of an HTTPS client that fetches a JSON response. This needs the `tls` feature:

```rs
#[derive(serde::Deserialize)]
struct IpInfo {
    hostname: String,
}

fn main() {
    let res = small_http::Request::get("https://ipinfo.io/json")
        .fetch()
        .expect("Can't fetch");
    println!("{}", String::from_utf8_lossy(&res.body));
    let ip_info = res.into_json::<IpInfo>().expect("Can't parse JSON");
    println!("Hostname: {}", ip_info.hostname);
}
```

`Request::fetch` uses connect and read/write timeouts and follows up to 5 redirects. It refuses
HTTPS to HTTP downgrades and drops `Authorization` and `Cookie` headers on cross-origin redirects.

See the [examples](examples/) for many more examples.

The multi-threaded server uses a scalable thread pool by default. Use
`small_http::serve_with_options` and `small_http::ThreadPoolOptions` to configure its minimum and
maximum worker thread counts and worker timeout.

## Important: reduce `url` dependencies

You can greatly reduce the dependencies of the [url](https://crates.io/crates/url) crate, by removing the `idna` support with the following crate update:

```sh
cargo update -p idna_adapter --precise 1.0.0
```

## License

Copyright © 2023-2025 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
