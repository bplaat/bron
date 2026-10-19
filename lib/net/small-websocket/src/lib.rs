/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use sha1::Sha1;
use small_http::{Request, Response, Status};

// Maximum allowed WebSocket frame and reassembled message payload in bytes (64 KiB)
const MAX_PAYLOAD: usize = 64 * 1024;
const READ_BUFFER_SIZE: usize = 8 * 1024;

/// WebSocket message
#[derive(Debug, Clone)]
pub enum Message {
    /// Text message
    Text(String),
    /// Binary message
    Binary(Vec<u8>),
    /// Ping message
    Ping(Vec<u8>),
    /// Pong message
    Pong(Vec<u8>),
    /// Close message
    Close(Option<u16>, Option<String>),
}

/// WebSocket connection
#[derive(Clone)]
pub struct WebSocket {
    state: Arc<Mutex<ConnectionState>>,
    role: Role,
}

impl PartialEq for WebSocket {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}
impl Eq for WebSocket {}

#[derive(Clone, Copy)]
enum Role {
    Server,
    #[cfg(any(feature = "client", test))]
    Client,
}

struct ConnectionState {
    stream: TcpStream,
    read_buffer: Vec<u8>,
    fragmented: Option<FragmentedMessage>,
}

struct FragmentedMessage {
    opcode: u8,
    payload: Vec<u8>,
}

struct Frame {
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
}

impl WebSocket {
    #[cfg(test)]
    fn new(stream: TcpStream, role: Role) -> Self {
        Self::new_with_buffer(stream, role, Vec::new())
    }

    /// Sets the timeout used when writing messages to this connection.
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.state
            .lock()
            .map_err(|_| io::Error::other("WebSocket lock is poisoned"))?
            .stream
            .set_write_timeout(timeout)
    }

    fn new_with_buffer(stream: TcpStream, role: Role, read_buffer: Vec<u8>) -> Self {
        Self {
            state: Arc::new(Mutex::new(ConnectionState {
                stream,
                read_buffer,
                fragmented: None,
            })),
            role,
        }
    }

    /// Connect to a WebSocket server
    #[cfg(feature = "client")]
    pub fn connect(url: impl AsRef<str>) -> Result<Self, ConnectError> {
        use std::io::BufReader;

        let parsed_url = url::Url::parse(url.as_ref()).map_err(|_| ConnectError)?;
        if parsed_url.scheme() != "ws" {
            return Err(ConnectError);
        }
        let host = parsed_url.host().ok_or(ConnectError)?;
        let port = parsed_url.port().unwrap_or(80);
        let address = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let mut stream = TcpStream::connect(address).map_err(|_| ConnectError)?;

        let mut random_key = [0u8; 16];
        getrandom::fill(&mut random_key).map_err(|_| ConnectError)?;
        let random_key = BASE64_STANDARD.encode(random_key);
        let mut req = Request::get(url.as_ref())
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", &random_key);
        req.write_to_stream(&mut stream, false);

        let mut reader = BufReader::new(stream);
        let res = Response::read_from_buffered_stream(&mut reader).map_err(|_| ConnectError)?;
        if res.status != Status::SwitchingProtocols
            || !res
                .headers
                .get("Upgrade")
                .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
            || !header_contains_token(res.headers.get("Connection"), "upgrade")
        {
            return Err(ConnectError);
        }
        let websocket_accept = res
            .headers
            .get("Sec-WebSocket-Accept")
            .ok_or(ConnectError)?;
        if *websocket_accept != accept_key(&random_key) {
            return Err(ConnectError);
        }

        let read_buffer = reader.buffer().to_vec();
        Ok(WebSocket::new_with_buffer(
            reader.into_inner(),
            Role::Client,
            read_buffer,
        ))
    }

    /// Get the underlying TCP stream peer address
    pub fn peer_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.state
            .lock()
            .map_err(|_| io::Error::other("WebSocket lock is poisoned"))?
            .stream
            .peer_addr()
    }

    /// Receive WebSocket message
    pub fn recv(&mut self) -> io::Result<Message> {
        self.receive(false)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "blocking WebSocket receive returned no message",
            )
        })
    }

    /// Receive WebSocket message without blocking
    pub fn recv_non_blocking(&mut self) -> io::Result<Option<Message>> {
        self.receive(true)
    }

    fn receive(&self, nonblocking: bool) -> io::Result<Option<Message>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("WebSocket lock is poisoned"))?;
        loop {
            if let Some((frame, consumed)) = parse_frame(&state.read_buffer, self.role)? {
                state.read_buffer.drain(..consumed);
                if let Some(message) = process_frame(&mut state.fragmented, frame)? {
                    return Ok(Some(message));
                }
                continue;
            }

            let read_result = if nonblocking {
                state.stream.set_nonblocking(true)?;
                let result = read_into_buffer(&mut state);
                let restore_result = state.stream.set_nonblocking(false);
                restore_result?;
                result
            } else {
                read_into_buffer(&mut state)
            };

            match read_result {
                Ok(0) if state.read_buffer.is_empty() && state.fragmented.is_none() => {
                    return Ok(Some(Message::Close(
                        None,
                        Some("Connection closed".to_string()),
                    )));
                }
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "connection closed during WebSocket frame",
                    ));
                }
                Ok(_) => {}
                Err(error) if nonblocking && error.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(None);
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Write a WebSocket message
    pub fn send(&mut self, message: Message) -> io::Result<()> {
        let (opcode, payload) = match message {
            Message::Text(text) => (0x1, text.into_bytes()),
            Message::Binary(data) => (0x2, data),
            Message::Ping(data) => (0x9, data),
            Message::Pong(data) => (0xa, data),
            Message::Close(code, reason) => {
                let mut payload = Vec::new();
                if reason.is_some() && code.is_none() {
                    return Err(invalid_input("a close reason requires a close code"));
                }
                if let Some(code) = code {
                    payload.extend_from_slice(&code.to_be_bytes());
                }
                if let Some(reason) = reason {
                    payload.extend_from_slice(reason.as_bytes());
                }
                (0x8, payload)
            }
        };
        if payload.len() > MAX_PAYLOAD {
            return Err(invalid_input("WebSocket message exceeds maximum size"));
        }
        if opcode >= 0x8 && payload.len() > 125 {
            return Err(invalid_input("WebSocket control frame exceeds 125 bytes"));
        }

        let frame = build_frame(opcode, &payload, self.role)?;
        self.state
            .lock()
            .map_err(|_| io::Error::other("WebSocket lock is poisoned"))?
            .stream
            .write_all(&frame)
    }
}

fn read_into_buffer(state: &mut ConnectionState) -> io::Result<usize> {
    let mut buffer = [0; READ_BUFFER_SIZE];
    let read = state.stream.read(&mut buffer)?;
    state.read_buffer.extend_from_slice(&buffer[..read]);
    Ok(read)
}

fn parse_frame(buffer: &[u8], role: Role) -> io::Result<Option<(Frame, usize)>> {
    if buffer.len() < 2 {
        return Ok(None);
    }
    if buffer[0] & 0x70 != 0 {
        return Err(invalid_data("reserved WebSocket bits are set"));
    }

    let fin = buffer[0] & 0x80 != 0;
    let opcode = buffer[0] & 0x0f;
    if !matches!(opcode, 0x0 | 0x1 | 0x2 | 0x8 | 0x9 | 0xa) {
        return Err(invalid_data("unsupported WebSocket opcode"));
    }

    let masked = buffer[1] & 0x80 != 0;
    let expects_mask = matches!(role, Role::Server);
    if masked != expects_mask {
        return Err(invalid_data(if expects_mask {
            "client WebSocket frames must be masked"
        } else {
            "server WebSocket frames must not be masked"
        }));
    }

    let mut offset = 2;
    let length_marker = buffer[1] & 0x7f;
    let payload_len = match length_marker {
        126 => {
            if buffer.len() < offset + 2 {
                return Ok(None);
            }
            let len = u16::from_be_bytes([buffer[offset], buffer[offset + 1]]) as usize;
            offset += 2;
            if len < 126 {
                return Err(invalid_data("non-minimal WebSocket payload length"));
            }
            len
        }
        127 => {
            if buffer.len() < offset + 8 {
                return Ok(None);
            }
            let len = u64::from_be_bytes(
                buffer[offset..offset + 8]
                    .try_into()
                    .map_err(|_| invalid_data("invalid WebSocket payload length"))?,
            );
            offset += 8;
            if len <= u16::MAX as u64 || len > usize::MAX as u64 {
                return Err(invalid_data("invalid WebSocket payload length"));
            }
            len as usize
        }
        len => len as usize,
    };
    if payload_len > MAX_PAYLOAD {
        return Err(invalid_data("WebSocket frame exceeds maximum size"));
    }
    if opcode >= 0x8 && (!fin || payload_len > 125) {
        return Err(invalid_data(
            "invalid fragmented or oversized control frame",
        ));
    }

    let mask = if masked {
        if buffer.len() < offset + 4 {
            return Ok(None);
        }
        let mask: [u8; 4] = buffer[offset..offset + 4]
            .try_into()
            .map_err(|_| invalid_data("invalid WebSocket mask"))?;
        offset += 4;
        Some(mask)
    } else {
        None
    };
    let frame_end = offset
        .checked_add(payload_len)
        .ok_or_else(|| invalid_data("WebSocket frame length overflow"))?;
    if buffer.len() < frame_end {
        return Ok(None);
    }

    let mut payload = buffer[offset..frame_end].to_vec();
    if let Some(mask) = mask {
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % mask.len()];
        }
    }
    Ok(Some((
        Frame {
            fin,
            opcode,
            payload,
        },
        frame_end,
    )))
}

fn process_frame(
    fragmented: &mut Option<FragmentedMessage>,
    frame: Frame,
) -> io::Result<Option<Message>> {
    match frame.opcode {
        0x0 => {
            let message = fragmented
                .as_mut()
                .ok_or_else(|| invalid_data("unexpected WebSocket continuation frame"))?;
            if message.payload.len().saturating_add(frame.payload.len()) > MAX_PAYLOAD {
                return Err(invalid_data(
                    "fragmented WebSocket message exceeds maximum size",
                ));
            }
            message.payload.extend(frame.payload);
            if frame.fin {
                let message = fragmented
                    .take()
                    .ok_or_else(|| invalid_data("missing fragmented WebSocket message"))?;
                message_from_data_frame(message.opcode, message.payload).map(Some)
            } else {
                Ok(None)
            }
        }
        0x1 | 0x2 => {
            if fragmented.is_some() {
                return Err(invalid_data(
                    "new WebSocket data frame during fragmented message",
                ));
            }
            if frame.fin {
                message_from_data_frame(frame.opcode, frame.payload).map(Some)
            } else {
                *fragmented = Some(FragmentedMessage {
                    opcode: frame.opcode,
                    payload: frame.payload,
                });
                Ok(None)
            }
        }
        0x8 => parse_close_message(frame.payload).map(Some),
        0x9 => Ok(Some(Message::Ping(frame.payload))),
        0xa => Ok(Some(Message::Pong(frame.payload))),
        _ => Err(invalid_data("unsupported WebSocket opcode")),
    }
}

fn message_from_data_frame(opcode: u8, payload: Vec<u8>) -> io::Result<Message> {
    match opcode {
        0x1 => String::from_utf8(payload)
            .map(Message::Text)
            .map_err(|_| invalid_data("WebSocket text message is not valid UTF-8")),
        0x2 => Ok(Message::Binary(payload)),
        _ => Err(invalid_data("invalid WebSocket data opcode")),
    }
}

fn parse_close_message(payload: Vec<u8>) -> io::Result<Message> {
    if payload.len() == 1 {
        return Err(invalid_data(
            "WebSocket close payload has an invalid length",
        ));
    }
    let code = (payload.len() >= 2).then(|| u16::from_be_bytes([payload[0], payload[1]]));
    let reason = if payload.len() > 2 {
        Some(
            String::from_utf8(payload[2..].to_vec())
                .map_err(|_| invalid_data("WebSocket close reason is not valid UTF-8"))?,
        )
    } else {
        None
    };
    Ok(Message::Close(code, reason))
}

fn build_frame(opcode: u8, payload: &[u8], role: Role) -> io::Result<Vec<u8>> {
    let masked = !matches!(role, Role::Server);
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x80 | opcode);
    let mask_bit = if masked { 0x80 } else { 0 };
    match payload.len() {
        0..=125 => frame.push(mask_bit | payload.len() as u8),
        126..=65535 => {
            frame.push(mask_bit | 126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        _ => {
            frame.push(mask_bit | 127);
            frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }
    }

    if masked {
        #[cfg(feature = "client")]
        {
            let mut mask = [0; 4];
            getrandom::fill(&mut mask)
                .map_err(|error| io::Error::other(format!("failed to generate mask: {error}")))?;
            frame.extend_from_slice(&mask);
            frame.extend(
                payload
                    .iter()
                    .enumerate()
                    .map(|(index, byte)| byte ^ mask[index % mask.len()]),
            );
        }
        #[cfg(not(feature = "client"))]
        return Err(invalid_input("client support is disabled"));
    } else {
        frame.extend_from_slice(payload);
    }
    Ok(frame)
}

/// Compute the Sec-WebSocket-Accept value for a Sec-WebSocket-Key (RFC 6455 section 4.2.2)
fn accept_key(key: &str) -> String {
    let mut sha1 = Sha1::new();
    sha1.update(key.as_bytes());
    sha1.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    BASE64_STANDARD.encode(sha1.finalize())
}

fn header_contains_token(value: Option<&str>, expected: &str) -> bool {
    value.is_some_and(|value| {
        value
            .split(',')
            .any(|token| token.trim().eq_ignore_ascii_case(expected))
    })
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// ConnectError
#[derive(Debug)]
pub struct ConnectError;

impl Display for ConnectError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Connect error")
    }
}

impl Error for ConnectError {}

/// Upgrade HTTP request to WebSocket connection.
/// Returns a 400 Bad Request response if the request does not conform to RFC 6455.
pub fn upgrade(request: &Request, handler: impl FnOnce(WebSocket) + Send + 'static) -> Response {
    // Validate required WebSocket upgrade headers (RFC 6455 Section 4.2.1)
    let upgrade_ok = request
        .headers
        .get("Upgrade")
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let connection_ok = header_contains_token(request.headers.get("Connection"), "upgrade");
    let version_ok = request
        .headers
        .get("Sec-WebSocket-Version")
        .is_some_and(|v| v == "13");
    let key = request.headers.get("Sec-WebSocket-Key");

    if !upgrade_ok || !connection_ok || !version_ok || key.is_none() {
        return Response::with_status(Status::BadRequest).body("400 Bad Request");
    }

    let mut res = Response::with_status(Status::SwitchingProtocols)
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade");
    res = res.header(
        "Sec-WebSocket-Accept",
        accept_key(key.expect("checked above")),
    );
    res = res.takeover(|reader| {
        let read_buffer = reader.buffer().to_vec();
        handler(WebSocket::new_with_buffer(
            reader.into_inner(),
            Role::Server,
            read_buffer,
        ));
    });
    res
}

// MARK: Tests
#[cfg(test)]
mod test {
    use std::net::{Ipv4Addr, Shutdown, TcpListener};
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[cfg(feature = "client")]
    #[test]
    fn test_websocket_server_client() {
        // Create WebSocket server
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        thread::Builder::new()
            .name("test-websocket-server".to_string())
            .spawn(move || {
                small_http::serve(listener, |req| {
                    upgrade(req, |mut ws| {
                        loop {
                            if let Message::Text(text) =
                                ws.recv().expect("Failed to receive message")
                            {
                                ws.send(Message::Text(text)).unwrap();
                            }
                        }
                    })
                });
            })
            .expect("Failed to spawn test WebSocket server thread");

        // Connect WebSocket client
        let mut ws = WebSocket::connect(format!("ws://{}:{}/", addr.ip(), addr.port())).unwrap();
        ws.send(Message::Text("Hello".to_string())).unwrap();
        if let Message::Text(text) = ws.recv().unwrap() {
            assert_eq!(text, "Hello")
        }
    }

    fn parse_message(buf: &[u8], role: Role) -> Option<Message> {
        let (frame, consumed) = parse_frame(buf, role).ok()??;
        if consumed != buf.len() {
            return None;
        }
        let mut fragmented = None;
        process_frame(&mut fragmented, frame).ok()?
    }

    // Build a minimal unmasked WebSocket frame: FIN + opcode, then length, then payload
    fn make_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
        make_frame_with_fin(opcode, payload, true)
    }

    fn make_frame_with_fin(opcode: u8, payload: &[u8], fin: bool) -> Vec<u8> {
        let mut frame = vec![if fin { 0x80 | opcode } else { opcode }];
        match payload.len() {
            0..=125 => frame.push(payload.len() as u8),
            126..=65535 => {
                frame.push(126);
                frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
            }
            _ => {
                frame.push(127);
                frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
            }
        }
        frame.extend_from_slice(payload);
        frame
    }

    fn connected_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }

    #[test]
    fn configures_write_timeout() {
        let (stream, _peer) = connected_pair();
        let websocket = WebSocket::new(stream, Role::Server);
        let timeout = Duration::from_millis(250);
        websocket
            .set_write_timeout(Some(timeout))
            .expect("Failed to set write timeout");
        assert_eq!(
            websocket
                .state
                .lock()
                .expect("Failed to lock connection")
                .stream
                .write_timeout()
                .expect("Failed to read write timeout"),
            Some(timeout)
        );
    }

    #[cfg(feature = "client")]
    #[test]
    fn test_connect_preserves_frame_read_with_handshake() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        thread::Builder::new()
            .name("test-websocket-handshake-server".to_string())
            .spawn(move || {
            use std::io::{BufRead, BufReader};

            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut key = None;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.strip_prefix("Sec-WebSocket-Key:") {
                    key = Some(value.trim().to_string());
                }
            }

            let accept = accept_key(&key.unwrap());
            let mut response = format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
            )
            .into_bytes();
            response.extend(make_frame(0x1, b"ready"));
            reader.get_mut().write_all(&response).unwrap();
            })
            .expect("Failed to spawn test WebSocket handshake server thread");

        let mut websocket = WebSocket::connect(format!("ws://{address}/")).unwrap();
        assert!(matches!(
            websocket.recv().unwrap(),
            Message::Text(text) if text == "ready"
        ));
    }

    #[test]
    fn test_parse_text_frame() {
        let frame = make_frame(0x1, b"Hello");
        let msg = parse_message(&frame, Role::Client).unwrap();
        assert!(matches!(msg, Message::Text(t) if t == "Hello"));
    }

    #[test]
    fn test_parse_binary_frame() {
        let frame = make_frame(0x2, &[0xDE, 0xAD, 0xBE, 0xEF]);
        let msg = parse_message(&frame, Role::Client).unwrap();
        assert!(matches!(msg, Message::Binary(b) if b == [0xDE, 0xAD, 0xBE, 0xEF]));
    }

    #[test]
    fn test_parse_ping_pong_frames() {
        let ping = make_frame(0x9, b"ping-data");
        assert!(
            matches!(parse_message(&ping, Role::Client).unwrap(), Message::Ping(b) if b == b"ping-data")
        );

        let pong = make_frame(0xA, b"pong-data");
        assert!(
            matches!(parse_message(&pong, Role::Client).unwrap(), Message::Pong(b) if b == b"pong-data")
        );
    }

    #[test]
    fn test_parse_close_frame_with_code_and_reason() {
        // Close frame: 2-byte code (1000 = 0x03E8) + reason
        let mut payload = vec![0x03u8, 0xE8]; // 1000
        payload.extend_from_slice(b"bye");
        let frame = make_frame(0x8, &payload);
        match parse_message(&frame, Role::Client).unwrap() {
            Message::Close(code, reason) => {
                assert_eq!(code, Some(1000));
                assert_eq!(reason.as_deref(), Some("bye"));
            }
            _ => panic!("expected Close"),
        }
    }

    #[test]
    fn test_parse_close_frame_no_payload() {
        let frame = make_frame(0x8, &[]);
        match parse_message(&frame, Role::Client).unwrap() {
            Message::Close(code, reason) => {
                assert_eq!(code, None);
                assert_eq!(reason, None);
            }
            _ => panic!("expected Close"),
        }
    }

    #[test]
    fn test_parse_masked_frame() {
        // Client-masked text frame with key [0x37, 0xFA, 0x21, 0x3D] and payload "Hello"
        let mask = [0x37u8, 0xFA, 0x21, 0x3D];
        let masked: Vec<u8> = b"Hello"
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ mask[i % 4])
            .collect();

        let mut frame = vec![0x81u8, 0x80 | 5u8]; // FIN+text, MASKED+5
        frame.extend_from_slice(&mask);
        frame.extend_from_slice(&masked);

        let msg = parse_message(&frame, Role::Server).unwrap();
        assert!(matches!(msg, Message::Text(t) if t == "Hello"));
    }

    #[test]
    fn test_parse_medium_length_frame() {
        // 200-byte binary payload uses 2-byte extended length (126 marker)
        let payload = vec![0xABu8; 200];
        let frame = make_frame(0x2, &payload);
        assert_eq!(frame[1], 126); // extended length marker
        assert_eq!(u16::from_be_bytes([frame[2], frame[3]]), 200);
        let msg = parse_message(&frame, Role::Client).unwrap();
        assert!(matches!(msg, Message::Binary(b) if b.len() == 200));
    }

    #[test]
    fn test_parse_unknown_opcode_returns_none() {
        let frame = make_frame(0x3, b"data"); // 0x3 is reserved/unknown
        assert!(parse_message(&frame, Role::Client).is_none());
    }

    #[test]
    fn test_parse_truncated_frame_returns_none() {
        // Frame header says 10 bytes but buffer has only 3
        let frame = vec![0x82u8, 10u8, 0x01]; // binary, 10 bytes, only 1 provided
        assert!(parse_message(&frame, Role::Client).is_none());
    }

    #[test]
    fn test_recv_buffers_partial_frame() {
        let (mut sender, receiver) = connected_pair();
        let mut websocket = WebSocket::new(receiver, Role::Client);
        let frame = make_frame(0x1, b"split message");
        let split = 4;
        sender.write_all(&frame[..split]).unwrap();
        thread::Builder::new()
            .name("test-frame-writer".to_string())
            .spawn(move || {
                thread::sleep(Duration::from_millis(10));
                sender.write_all(&frame[split..]).unwrap();
            })
            .expect("Failed to spawn test frame writer thread");

        assert!(matches!(
            websocket.recv().unwrap(),
            Message::Text(text) if text == "split message"
        ));
    }

    #[test]
    fn test_recv_preserves_coalesced_frames() {
        let (mut sender, receiver) = connected_pair();
        let mut websocket = WebSocket::new(receiver, Role::Client);
        let mut frames = make_frame(0x1, b"first");
        frames.extend(make_frame(0x1, b"second"));
        sender.write_all(&frames).unwrap();

        assert!(matches!(
            websocket.recv().unwrap(),
            Message::Text(text) if text == "first"
        ));
        assert!(matches!(
            websocket.recv().unwrap(),
            Message::Text(text) if text == "second"
        ));
    }

    #[test]
    fn test_recv_large_frame_across_multiple_reads() {
        let (mut sender, receiver) = connected_pair();
        let mut websocket = WebSocket::new(receiver, Role::Client);
        let payload = vec![0xab; 32 * 1024];
        sender.write_all(&make_frame(0x2, &payload)).unwrap();

        assert!(matches!(
            websocket.recv().unwrap(),
            Message::Binary(data) if data == payload
        ));
    }

    #[test]
    fn test_recv_reassembles_fragmented_message() {
        let (mut sender, receiver) = connected_pair();
        let mut websocket = WebSocket::new(receiver, Role::Client);
        let mut frames = make_frame_with_fin(0x1, b"frag", false);
        frames.extend(make_frame(0x9, b"ping"));
        frames.extend(make_frame(0x0, b"mented"));
        sender.write_all(&frames).unwrap();

        assert!(matches!(
            websocket.recv().unwrap(),
            Message::Ping(data) if data == b"ping"
        ));
        assert!(matches!(
            websocket.recv().unwrap(),
            Message::Text(text) if text == "fragmented"
        ));
    }

    #[cfg(feature = "client")]
    #[test]
    fn test_client_send_masks_frame() {
        let (sender, mut receiver) = connected_pair();
        let mut websocket = WebSocket::new(sender, Role::Client);
        websocket.send(Message::Text("masked".to_string())).unwrap();

        let mut frame = [0; 12];
        receiver.read_exact(&mut frame).unwrap();
        assert_ne!(frame[1] & 0x80, 0);
        assert!(matches!(
            parse_message(&frame, Role::Server).unwrap(),
            Message::Text(text) if text == "masked"
        ));
    }

    #[test]
    fn test_server_rejects_unmasked_client_frame() {
        let frame = make_frame(0x1, b"unmasked");
        assert!(parse_frame(&frame, Role::Server).is_err());
    }

    #[test]
    fn test_non_blocking_receive_restores_blocking_mode() {
        let (mut sender, receiver) = connected_pair();
        let mut websocket = WebSocket::new(receiver, Role::Client);
        let frame = make_frame(0x1, b"delayed");
        sender.write_all(&frame[..2]).unwrap();
        assert!(websocket.recv_non_blocking().unwrap().is_none());

        thread::Builder::new()
            .name("test-delayed-frame-writer".to_string())
            .spawn(move || {
                thread::sleep(Duration::from_millis(10));
                sender.write_all(&frame[2..]).unwrap();
                sender.shutdown(Shutdown::Write).unwrap();
            })
            .expect("Failed to spawn test delayed frame writer thread");
        assert!(matches!(
            websocket.recv().unwrap(),
            Message::Text(text) if text == "delayed"
        ));
    }

    #[test]
    fn test_upgrade_accept_key() {
        // RFC 6455 Section 1.3 example: known input/output pair
        let req = Request::new()
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==");
        let res = upgrade(&req, |_ws| {});
        assert_eq!(res.status, Status::SwitchingProtocols);
        assert_eq!(
            res.headers.get("Sec-WebSocket-Accept").unwrap(),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn test_upgrade_missing_headers_returns_bad_request() {
        let req = Request::new(); // no WebSocket headers
        let res = upgrade(&req, |_ws| {});
        assert_eq!(res.status, Status::BadRequest);
    }
}
