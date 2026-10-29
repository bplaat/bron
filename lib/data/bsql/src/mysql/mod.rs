/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};

#[cfg(feature = "mysql-tls")]
use native_tls::TlsConnector;
#[cfg(feature = "mysql-native-password")]
use sha1::Sha1;
use sha2::Sha256;

pub use crate::mysql::connection::MysqlTransport;
pub(crate) use crate::mysql::connection::{Client, MysqlOptions, OpenedStream, Stream};
pub(crate) use crate::mysql::statement::{Column, Prepared};
use crate::mysql::utils::*;
use crate::{StatementError, Value};

mod connection;
mod statement;
mod utils;

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct Capabilities: u32 {
        const LONG_PASSWORD = 0x0000_0001;
        const LONG_FLAG = 0x0000_0004;
        const CONNECT_WITH_DB = 0x0000_0008;
        const PROTOCOL_41 = 0x0000_0200;
        const SSL = 0x0000_0800;
        const TRANSACTIONS = 0x0000_2000;
        const SECURE_CONNECTION = 0x0000_8000;
        const MULTI_STATEMENTS = 0x0001_0000;
        const MULTI_RESULTS = 0x0002_0000;
        const PS_MULTI_RESULTS = 0x0004_0000;
        const PLUGIN_AUTH = 0x0008_0000;
        const PLUGIN_AUTH_LENENC_CLIENT_DATA = 0x0020_0000;
        const DEPRECATE_EOF = 0x0100_0000;
        // Servers may send flags that this client does not interpret.
        const _ = !0;
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct ServerStatus: u16 {
        const MORE_RESULTS_EXISTS = 0x0008;
        const IN_TRANS = 0x0001;
        // Servers may send flags that this client does not interpret.
        const _ = !0;
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct ColumnFlags: u16 {
        const UNSIGNED = 0x0020;
        // Servers may send flags that this client does not interpret.
        const _ = !0;
    }
}

const BINARY_CHARSET: u16 = 63;
const MAX_PACKET_PAYLOAD: usize = 0x00ff_ffff;

const COM_QUERY: u8 = 0x03;
const COM_STMT_PREPARE: u8 = 0x16;
const COM_STMT_EXECUTE: u8 = 0x17;
const COM_STMT_CLOSE: u8 = 0x19;
const COM_STMT_RESET: u8 = 0x1a;

impl Client {
    pub(crate) fn connect(options: &MysqlOptions) -> Result<Self, String> {
        let (mut stream, tls_host, secure) = open_stream(options)?;
        let mut sequence = 0;
        let handshake_packet =
            read_packet(&mut *stream, &mut sequence).map_err(|error| error.to_string())?;
        let handshake = Handshake::parse(&handshake_packet)?;

        let base_capabilities = Capabilities::LONG_PASSWORD
            | Capabilities::LONG_FLAG
            | Capabilities::PROTOCOL_41
            | Capabilities::TRANSACTIONS
            | Capabilities::SECURE_CONNECTION
            | Capabilities::MULTI_STATEMENTS
            | Capabilities::MULTI_RESULTS
            | Capabilities::PS_MULTI_RESULTS
            | Capabilities::PLUGIN_AUTH
            | Capabilities::PLUGIN_AUTH_LENENC_CLIENT_DATA
            | Capabilities::DEPRECATE_EOF;
        let wants_tls = cfg!(feature = "mysql-tls") && tls_host.is_some() && options.tls;
        if wants_tls && !handshake.capabilities.contains(Capabilities::SSL) {
            return Err("MySQL server does not support TLS".to_string());
        }
        let mut capabilities = base_capabilities;
        if options.database.is_some() {
            capabilities |= Capabilities::CONNECT_WITH_DB;
        }
        if wants_tls {
            capabilities |= Capabilities::SSL;
        }
        capabilities &= handshake.capabilities;

        #[cfg(feature = "mysql-tls")]
        if let Some(host) = tls_host
            && wants_tls
        {
            let request = ssl_request(capabilities);
            write_packet(&mut *stream, &mut sequence, &request)
                .map_err(|error| error.to_string())?;
            let connector = TlsConnector::new().map_err(|error| error.to_string())?;
            let tls = connector
                .connect(&host, stream)
                .map_err(|error| format!("MySQL TLS handshake failed: {error}"))?;
            stream = Box::new(tls);
        }

        let plugin = handshake.auth_plugin.as_str();
        let auth = auth_response(plugin, &options.password, &handshake.scramble)?;
        let response = handshake_response(capabilities, options, plugin, &auth);
        write_packet(&mut *stream, &mut sequence, &response).map_err(|error| error.to_string())?;
        finish_authentication(
            &mut *stream,
            &mut sequence,
            options,
            secure || wants_tls,
            plugin,
        )?;

        Ok(Self {
            stream,
            affected_rows: 0,
            last_insert_id: 0,
            capabilities,
            in_transaction: false,
        })
    }

    pub(crate) fn execute_script(&mut self, sql: &str) -> Result<(), StatementError> {
        let mut payload = Vec::with_capacity(sql.len() + 1);
        payload.push(COM_QUERY);
        payload.extend_from_slice(sql.as_bytes());
        let mut sequence = 0;
        write_packet(&mut *self.stream, &mut sequence, &payload).map_err(statement_io)?;
        loop {
            let status = self.drain_query_response(&mut sequence)?;
            self.in_transaction = status.contains(ServerStatus::IN_TRANS);
            if !status.contains(ServerStatus::MORE_RESULTS_EXISTS) {
                return Ok(());
            }
        }
    }

    fn drain_query_response(&mut self, sequence: &mut u8) -> Result<ServerStatus, StatementError> {
        let packet = read_packet(&mut *self.stream, sequence).map_err(statement_io)?;
        if packet.first() == Some(&0xff) {
            return Err(server_error(&packet));
        }
        if packet.first() == Some(&0xfb) {
            return Err(StatementError::broken_connection(
                "MySQL LOCAL INFILE requests are not supported",
            ));
        }
        if matches!(packet.first(), Some(0x00 | 0xfe)) {
            let ok = parse_ok(&packet)?;
            self.affected_rows = ok.affected_rows;
            self.last_insert_id = ok.last_insert_id;
            return Ok(ok.status);
        }
        let mut reader = Reader::new(&packet);
        let column_count = usize::try_from(reader.lenenc_int()?)
            .map_err(|_| protocol_error("MySQL column count is too large"))?;
        for _ in 0..column_count {
            let packet = read_packet(&mut *self.stream, sequence).map_err(statement_io)?;
            if packet.first() == Some(&0xff) {
                return Err(server_error(&packet));
            }
        }
        if column_count > 0 && !self.deprecates_eof() {
            let packet = read_packet(&mut *self.stream, sequence).map_err(statement_io)?;
            parse_result_terminator(&packet, false)?;
        }
        loop {
            let packet = read_packet(&mut *self.stream, sequence).map_err(statement_io)?;
            if packet.first() == Some(&0xff) {
                return Err(server_error(&packet));
            }
            if is_result_terminator(&packet) {
                let ok = parse_result_terminator(&packet, self.deprecates_eof())?;
                return Ok(ok.status);
            }
        }
    }

    pub(crate) fn prepare(&mut self, query: &str) -> Result<Prepared, StatementError> {
        let (rewritten, parameter_names) = rewrite_named_parameters(query)?;
        let mut payload = Vec::with_capacity(rewritten.len() + 1);
        payload.push(COM_STMT_PREPARE);
        payload.extend_from_slice(rewritten.as_bytes());
        let mut sequence = 0;
        write_packet(&mut *self.stream, &mut sequence, &payload).map_err(statement_io)?;
        let packet = read_packet(&mut *self.stream, &mut sequence).map_err(statement_io)?;
        if packet.first() == Some(&0xff) {
            return Err(server_error(&packet));
        }
        let mut reader = Reader::new(&packet);
        if reader.u8()? != 0 {
            return Err(protocol_error("invalid COM_STMT_PREPARE response"));
        }
        let id = reader.u32()?;
        let column_count = reader.u16()? as usize;
        let parameter_count = reader.u16()? as usize;
        reader.skip(1)?;
        let _warnings = reader.u16()?;

        for _ in 0..parameter_count {
            let packet = read_packet(&mut *self.stream, &mut sequence).map_err(statement_io)?;
            parse_column(&packet)?;
        }
        if parameter_count > 0 && !self.deprecates_eof() {
            let packet = read_packet(&mut *self.stream, &mut sequence).map_err(statement_io)?;
            parse_result_terminator(&packet, false)?;
        }
        let mut columns = Vec::with_capacity(column_count);
        for _ in 0..column_count {
            let packet = read_packet(&mut *self.stream, &mut sequence).map_err(statement_io)?;
            columns.push(parse_column(&packet)?);
        }
        if column_count > 0 && !self.deprecates_eof() {
            let packet = read_packet(&mut *self.stream, &mut sequence).map_err(statement_io)?;
            parse_result_terminator(&packet, false)?;
        }
        if parameter_names.len() != parameter_count {
            return Err(protocol_error("prepared parameter count mismatch"));
        }
        Ok(Prepared {
            id,
            query: query.to_string(),
            parameter_names,
            params: (0..parameter_count).map(|_| None).collect(),
            columns,
            rows: Vec::new(),
            row_index: 0,
            current_row: None,
            executed: false,
        })
    }

    pub(crate) fn execute_prepared(
        &mut self,
        statement: &mut Prepared,
    ) -> Result<(), StatementError> {
        if let Some(index) = statement.params.iter().position(Option::is_none) {
            return Err(StatementError::new(format!(
                "parameter {index} is not bound in statement '{}'",
                statement.query
            )));
        }
        let mut payload = Vec::new();
        payload.push(COM_STMT_EXECUTE);
        payload.extend_from_slice(&statement.id.to_le_bytes());
        payload.push(0);
        payload.extend_from_slice(&1_u32.to_le_bytes());
        if !statement.params.is_empty() {
            let mut null_bitmap = vec![0_u8; statement.params.len().div_ceil(8)];
            for (index, value) in statement.params.iter().enumerate() {
                if matches!(value, Some(Value::Null)) {
                    null_bitmap[index / 8] |= 1 << (index % 8);
                }
            }
            payload.extend_from_slice(&null_bitmap);
            payload.push(1);
            for value in &statement.params {
                payload.push(parameter_type(value.as_ref().expect("checked above")));
                payload.push(0);
            }
            for value in &statement.params {
                encode_parameter(&mut payload, value.as_ref().expect("checked above"))?;
            }
        }

        let mut sequence = 0;
        write_packet(&mut *self.stream, &mut sequence, &payload).map_err(statement_io)?;
        let first = read_packet(&mut *self.stream, &mut sequence).map_err(statement_io)?;
        if first.first() == Some(&0xff) {
            return Err(server_error(&first));
        }
        statement.rows.clear();
        statement.row_index = 0;
        statement.current_row = None;
        if matches!(first.first(), Some(0x00 | 0xfe)) {
            let ok = parse_ok(&first)?;
            self.affected_rows = ok.affected_rows;
            self.last_insert_id = ok.last_insert_id;
            let mut status = ok.status;
            while status.contains(ServerStatus::MORE_RESULTS_EXISTS) {
                status = self.drain_query_response(&mut sequence)?;
            }
            statement.columns.clear();
            statement.executed = true;
            self.in_transaction = status.contains(ServerStatus::IN_TRANS);
            return Ok(());
        }

        let mut reader = Reader::new(&first);
        let column_count = usize::try_from(reader.lenenc_int()?)
            .map_err(|_| protocol_error("MySQL column count is too large"))?;
        let mut columns = Vec::with_capacity(column_count);
        for _ in 0..column_count {
            let packet = read_packet(&mut *self.stream, &mut sequence).map_err(statement_io)?;
            columns.push(parse_column(&packet)?);
        }
        if column_count > 0 && !self.deprecates_eof() {
            let packet = read_packet(&mut *self.stream, &mut sequence).map_err(statement_io)?;
            parse_result_terminator(&packet, false)?;
        }
        let status = loop {
            let packet = read_packet(&mut *self.stream, &mut sequence).map_err(statement_io)?;
            if packet.first() == Some(&0xff) {
                return Err(server_error(&packet));
            }
            if is_result_terminator(&packet) {
                let ok = parse_result_terminator(&packet, self.deprecates_eof())?;
                self.affected_rows = ok.affected_rows;
                self.last_insert_id = ok.last_insert_id;
                break ok.status;
            }
            statement.rows.push(decode_binary_row(&packet, &columns)?);
        };
        let mut status = status;
        while status.contains(ServerStatus::MORE_RESULTS_EXISTS) {
            status = self.drain_query_response(&mut sequence)?;
        }
        self.in_transaction = status.contains(ServerStatus::IN_TRANS);
        statement.columns = columns;
        statement.executed = true;
        Ok(())
    }

    pub(crate) fn reset(&mut self, statement: &mut Prepared) -> Result<(), StatementError> {
        let mut payload = Vec::with_capacity(5);
        payload.push(COM_STMT_RESET);
        payload.extend_from_slice(&statement.id.to_le_bytes());
        let mut sequence = 0;
        write_packet(&mut *self.stream, &mut sequence, &payload).map_err(statement_io)?;
        let packet = read_packet(&mut *self.stream, &mut sequence).map_err(statement_io)?;
        if packet.first() == Some(&0xff) {
            return Err(server_error(&packet));
        }
        parse_ok(&packet)?;
        statement.rows.clear();
        statement.row_index = 0;
        statement.current_row = None;
        statement.executed = false;
        Ok(())
    }

    pub(crate) fn close(&mut self, statement_id: u32) -> Result<(), StatementError> {
        let mut payload = Vec::with_capacity(5);
        payload.push(COM_STMT_CLOSE);
        payload.extend_from_slice(&statement_id.to_le_bytes());
        let mut sequence = 0;
        write_packet(&mut *self.stream, &mut sequence, &payload).map_err(statement_io)
    }

    const fn deprecates_eof(&self) -> bool {
        self.capabilities.contains(Capabilities::DEPRECATE_EOF)
    }
}

fn open_stream(options: &MysqlOptions) -> Result<OpenedStream, String> {
    match &options.transport {
        MysqlTransport::Tcp { host, port, tls: _ } => {
            let address = (host.as_str(), *port)
                .to_socket_addrs()
                .map_err(|error| error.to_string())?
                .next()
                .ok_or_else(|| format!("could not resolve MySQL host '{host}'"))?;
            let stream = TcpStream::connect_timeout(&address, options.timeout)
                .map_err(|error| error.to_string())?;
            stream
                .set_read_timeout(Some(options.timeout))
                .map_err(|error| error.to_string())?;
            stream
                .set_write_timeout(Some(options.timeout))
                .map_err(|error| error.to_string())?;
            Ok((Box::new(stream), Some(host.clone()), false))
        }
        #[cfg(unix)]
        MysqlTransport::Unix { path } => {
            use std::os::unix::net::UnixStream;
            let stream = UnixStream::connect(path).map_err(|error| error.to_string())?;
            stream
                .set_read_timeout(Some(options.timeout))
                .map_err(|error| error.to_string())?;
            stream
                .set_write_timeout(Some(options.timeout))
                .map_err(|error| error.to_string())?;
            Ok((Box::new(stream), None, true))
        }
    }
}

struct Handshake {
    capabilities: Capabilities,
    scramble: Vec<u8>,
    auth_plugin: String,
}

impl Handshake {
    fn parse(packet: &[u8]) -> Result<Self, String> {
        let mut reader = Reader::new(packet);
        if reader.u8().map_err(|error| error.to_string())? != 10 {
            return Err("unsupported MySQL handshake protocol".to_string());
        }
        reader.nul_bytes().map_err(|error| error.to_string())?;
        reader.u32().map_err(|error| error.to_string())?;
        let mut scramble = reader.bytes(8).map_err(|error| error.to_string())?.to_vec();
        reader.skip(1).map_err(|error| error.to_string())?;
        let low = reader.u16().map_err(|error| error.to_string())? as u32;
        if reader.remaining() == 0 {
            return Err("incomplete MySQL protocol 10 handshake".to_string());
        }
        reader.skip(1).map_err(|error| error.to_string())?;
        reader.skip(2).map_err(|error| error.to_string())?;
        let high = reader.u16().map_err(|error| error.to_string())? as u32;
        let capabilities = Capabilities::from_bits_retain(low | high << 16);
        let auth_len = reader.u8().map_err(|error| error.to_string())? as usize;
        reader.skip(10).map_err(|error| error.to_string())?;
        let second_len = auth_len.saturating_sub(8).max(13).min(reader.remaining());
        let second = reader
            .bytes(second_len)
            .map_err(|error| error.to_string())?;
        scramble.extend(second.iter().copied().take_while(|byte| *byte != 0));
        scramble.truncate(20);
        let auth_plugin =
            if capabilities.contains(Capabilities::PLUGIN_AUTH) && reader.remaining() > 0 {
                String::from_utf8_lossy(reader.nul_bytes().map_err(|error| error.to_string())?)
                    .into_owned()
            } else {
                "mysql_native_password".to_string()
            };
        let auth_plugin = if auth_plugin.is_empty() {
            "mysql_native_password".to_string()
        } else {
            auth_plugin
        };
        Ok(Self {
            capabilities,
            scramble,
            auth_plugin,
        })
    }
}

fn finish_authentication(
    stream: &mut dyn Stream,
    sequence: &mut u8,
    options: &MysqlOptions,
    secure: bool,
    initial_plugin: &str,
) -> Result<(), String> {
    let mut current_plugin = initial_plugin.to_string();
    loop {
        let packet = read_packet(stream, sequence).map_err(|error| error.to_string())?;
        match packet.first().copied() {
            Some(0x00) => return Ok(()),
            Some(0xff) => return Err(server_error_text(&packet)),
            Some(0xfe) => {
                let mut reader = Reader::new(&packet[1..]);
                let plugin =
                    String::from_utf8_lossy(reader.nul_bytes().map_err(|error| error.to_string())?)
                        .into_owned();
                let mut scramble = reader.remaining_bytes().to_vec();
                while scramble.last() == Some(&0) {
                    scramble.pop();
                }
                if !plugin.is_empty() {
                    current_plugin = plugin;
                }
                let auth = auth_response(&current_plugin, &options.password, &scramble)?;
                write_packet(stream, sequence, &auth).map_err(|error| error.to_string())?;
            }
            Some(0x01) if packet.get(1) == Some(&0x03) => {
                let packet = read_packet(stream, sequence).map_err(|error| error.to_string())?;
                if packet.first() == Some(&0x00) {
                    return Ok(());
                }
                return Err(server_error_text(&packet));
            }
            Some(0x01) if packet.get(1) == Some(&0x04) => {
                if current_plugin != "caching_sha2_password" {
                    return Err(format!(
                        "unexpected additional authentication data for '{current_plugin}'"
                    ));
                }
                if !secure && !options.password.is_empty() {
                    return Err(
                        "MySQL requested full password authentication over an insecure TCP connection; enable verified TLS or use a Unix socket"
                            .to_string(),
                    );
                }
                let mut password = options.password.as_bytes().to_vec();
                password.push(0);
                write_packet(stream, sequence, &password).map_err(|error| error.to_string())?;
            }
            _ => return Err("unexpected MySQL authentication packet".to_string()),
        }
    }
}

fn ssl_request(capabilities: Capabilities) -> Vec<u8> {
    let mut packet = Vec::with_capacity(32);
    packet.extend_from_slice(&capabilities.bits().to_le_bytes());
    packet.extend_from_slice(&(MAX_PACKET_PAYLOAD as u32).to_le_bytes());
    packet.push(45);
    packet.extend_from_slice(&[0; 23]);
    packet
}

fn handshake_response(
    capabilities: Capabilities,
    options: &MysqlOptions,
    plugin: &str,
    auth: &[u8],
) -> Vec<u8> {
    let mut packet = ssl_request(capabilities);
    packet.extend_from_slice(options.user.as_bytes());
    packet.push(0);
    if capabilities.contains(Capabilities::PLUGIN_AUTH_LENENC_CLIENT_DATA) {
        put_lenenc_int(&mut packet, auth.len() as u64);
        packet.extend_from_slice(auth);
    } else if capabilities.contains(Capabilities::SECURE_CONNECTION) {
        packet.push(auth.len() as u8);
        packet.extend_from_slice(auth);
    } else {
        packet.extend_from_slice(auth);
        packet.push(0);
    }
    if capabilities.contains(Capabilities::CONNECT_WITH_DB) {
        packet.extend_from_slice(options.database.as_deref().unwrap_or_default().as_bytes());
        packet.push(0);
    }
    if capabilities.contains(Capabilities::PLUGIN_AUTH) {
        packet.extend_from_slice(plugin.as_bytes());
        packet.push(0);
    }
    packet
}

fn auth_response(plugin: &str, password: &str, nonce: &[u8]) -> Result<Vec<u8>, String> {
    match plugin {
        "caching_sha2_password" => Ok(caching_sha2_scramble(password, nonce)),
        "mysql_native_password" => mysql_native_auth_response(password, nonce),
        "auth_socket" | "unix_socket" => Ok(Vec::new()),
        _ => Err(format!(
            "unsupported MySQL authentication plugin '{plugin}'"
        )),
    }
}

#[cfg(feature = "mysql-native-password")]
fn mysql_native_auth_response(password: &str, nonce: &[u8]) -> Result<Vec<u8>, String> {
    Ok(mysql_native_scramble(password, nonce))
}

#[cfg(not(feature = "mysql-native-password"))]
fn mysql_native_auth_response(_password: &str, _nonce: &[u8]) -> Result<Vec<u8>, String> {
    Err(
        "MySQL authentication plugin 'mysql_native_password' requires the \
         'mysql-native-password' feature"
            .to_string(),
    )
}

#[cfg(feature = "mysql-native-password")]
fn mysql_native_scramble(password: &str, nonce: &[u8]) -> Vec<u8> {
    if password.is_empty() {
        return Vec::new();
    }
    let stage_one = Sha1::digest(password.as_bytes());
    let stage_two = Sha1::digest(stage_one);
    let mut input = Vec::with_capacity(nonce.len() + stage_two.len());
    input.extend_from_slice(nonce);
    input.extend_from_slice(&stage_two);
    let stage_three = Sha1::digest(input);
    stage_one
        .iter()
        .zip(stage_three)
        .map(|(left, right)| left ^ right)
        .collect()
}

fn caching_sha2_scramble(password: &str, nonce: &[u8]) -> Vec<u8> {
    if password.is_empty() {
        return Vec::new();
    }
    let stage_one = Sha256::digest(password.as_bytes());
    let stage_two = Sha256::digest(stage_one);
    let mut input = Vec::with_capacity(stage_two.len() + nonce.len());
    input.extend_from_slice(&stage_two);
    input.extend_from_slice(nonce);
    let stage_three = Sha256::digest(input);
    stage_one
        .iter()
        .zip(stage_three)
        .map(|(left, right)| left ^ right)
        .collect()
}

fn rewrite_named_parameters(query: &str) -> Result<(String, Vec<Option<String>>), StatementError> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        Single,
        Double,
        Backtick,
        LineComment,
        BlockComment,
    }
    let bytes = query.as_bytes();
    let mut output = Vec::with_capacity(query.len());
    let mut names = Vec::new();
    let mut state = State::Normal;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match state {
            State::Normal => {
                if byte == b'\'' {
                    state = State::Single;
                } else if byte == b'"' {
                    state = State::Double;
                } else if byte == b'`' {
                    state = State::Backtick;
                } else if byte == b'#'
                    || (byte == b'-'
                        && bytes.get(index + 1) == Some(&b'-')
                        && bytes.get(index + 2).is_none_or(u8::is_ascii_whitespace))
                {
                    state = State::LineComment;
                } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    state = State::BlockComment;
                } else if byte == b'?' {
                    names.push(None);
                } else if byte == b':'
                    && bytes
                        .get(index + 1)
                        .is_some_and(|next| next.is_ascii_alphabetic() || *next == b'_')
                {
                    let start = index;
                    index += 1;
                    while bytes
                        .get(index)
                        .is_some_and(|next| next.is_ascii_alphanumeric() || *next == b'_')
                    {
                        index += 1;
                    }
                    names.push(Some(query[start..index].to_string()));
                    output.push(b'?');
                    continue;
                }
            }
            State::Single if byte == b'\'' => {
                if bytes.get(index + 1) == Some(&b'\'') {
                    output.push(b'\'');
                    index += 1;
                } else {
                    state = State::Normal;
                }
            }
            State::Single | State::Double if byte == b'\\' => {
                output.push(b'\\');
                if let Some(next) = bytes.get(index + 1) {
                    output.push(*next);
                    index += 2;
                    continue;
                }
            }
            State::Double if byte == b'"' => {
                if bytes.get(index + 1) == Some(&b'"') {
                    output.push(b'"');
                    index += 1;
                } else {
                    state = State::Normal;
                }
            }
            State::Backtick if byte == b'`' => {
                if bytes.get(index + 1) == Some(&b'`') {
                    output.push(b'`');
                    index += 1;
                } else {
                    state = State::Normal;
                }
            }
            State::LineComment if byte == b'\n' => state = State::Normal,
            State::BlockComment if byte == b'*' && bytes.get(index + 1) == Some(&b'/') => {
                output.push(b'*');
                output.push(b'/');
                index += 2;
                state = State::Normal;
                continue;
            }
            _ => {}
        }
        output.push(byte);
        index += 1;
    }
    if state == State::BlockComment {
        return Err(protocol_error("unterminated SQL block comment"));
    }
    let output = String::from_utf8(output)
        .map_err(|_| protocol_error("rewritten SQL is not valid UTF-8"))?;
    Ok((output, names))
}

const fn parameter_type(value: &Value) -> u8 {
    match value {
        Value::Null => 6,
        Value::Integer(_) => 8,
        Value::Float(_) => 5,
        Value::Text(_) => 253,
        Value::Blob(_) => 252,
    }
}

fn encode_parameter(output: &mut Vec<u8>, value: &Value) -> Result<(), StatementError> {
    match value {
        Value::Null => {}
        Value::Integer(value) => output.extend_from_slice(&value.to_le_bytes()),
        Value::Float(value) => output.extend_from_slice(&value.to_le_bytes()),
        Value::Text(value) => put_lenenc_bytes(output, value.as_bytes()),
        Value::Blob(value) => put_lenenc_bytes(output, value),
    }
    Ok(())
}

fn parse_column(packet: &[u8]) -> Result<Column, StatementError> {
    if packet.first() == Some(&0xff) {
        return Err(server_error(packet));
    }
    let mut reader = Reader::new(packet);
    reader.lenenc_bytes()?;
    reader.lenenc_bytes()?;
    let table = nonempty_lossy(reader.lenenc_bytes()?);
    reader.lenenc_bytes()?;
    let name = String::from_utf8_lossy(reader.lenenc_bytes()?).into_owned();
    let origin_name = nonempty_lossy(reader.lenenc_bytes()?);
    let fixed_len = usize::try_from(reader.lenenc_int()?)
        .map_err(|_| protocol_error("MySQL column definition is too large"))?;
    if fixed_len < 12 {
        return Err(protocol_error("invalid MySQL column definition"));
    }
    let charset = reader.u16()?;
    reader.u32()?;
    let type_code = reader.u8()?;
    let flags = ColumnFlags::from_bits_retain(reader.u16()?);
    reader.skip(fixed_len - 9)?;
    Ok(Column {
        name,
        table,
        origin_name,
        type_code,
        flags,
        charset,
    })
}

fn decode_binary_row(packet: &[u8], columns: &[Column]) -> Result<Vec<Value>, StatementError> {
    let mut reader = Reader::new(packet);
    if reader.u8()? != 0 {
        return Err(protocol_error("invalid binary result row header"));
    }
    let null_bitmap = reader.bytes((columns.len() + 9) / 8)?;
    let mut values = Vec::with_capacity(columns.len());
    for (index, column) in columns.iter().enumerate() {
        if null_bitmap[(index + 2) / 8] & (1 << ((index + 2) % 8)) != 0 {
            values.push(Value::Null);
            continue;
        }
        values.push(decode_binary_value(&mut reader, column)?);
    }
    if reader.remaining() != 0 {
        return Err(protocol_error("binary result row has trailing bytes"));
    }
    Ok(values)
}

fn decode_binary_value(reader: &mut Reader<'_>, column: &Column) -> Result<Value, StatementError> {
    let unsigned = column.flags.contains(ColumnFlags::UNSIGNED);
    match column.type_code {
        1 => integer_value(reader.u8()? as u64, unsigned, 8),
        2 | 13 => integer_value(reader.u16()? as u64, unsigned, 16),
        3 | 9 => integer_value(reader.u32()? as u64, unsigned, 32),
        8 => integer_value(reader.u64()?, unsigned, 64),
        4 => Ok(Value::Float(reader.f32()? as f64)),
        5 => Ok(Value::Float(reader.f64()?)),
        0 | 246 => Ok(Value::Text(
            String::from_utf8(reader.lenenc_bytes()?.to_vec())
                .map_err(|_| protocol_error("MySQL returned a non-UTF-8 decimal value"))?,
        )),
        245 => Ok(Value::Text(
            String::from_utf8(reader.lenenc_bytes()?.to_vec())
                .map_err(|_| protocol_error("MySQL returned non-UTF-8 JSON"))?,
        )),
        15 | 247..=254 => {
            let bytes = reader.lenenc_bytes()?;
            if column.charset == BINARY_CHARSET {
                Ok(Value::Blob(bytes.to_vec()))
            } else {
                Ok(Value::Text(String::from_utf8(bytes.to_vec()).map_err(
                    |_| protocol_error("MySQL returned non-UTF-8 text for a text column"),
                )?))
            }
        }
        16 | 255 => Ok(Value::Blob(reader.lenenc_bytes()?.to_vec())),
        10 => Ok(Value::Text(decode_date(reader)?)),
        7 | 12 | 17 | 18 => Ok(Value::Text(decode_date_time(reader)?)),
        11 | 19 => Ok(Value::Text(decode_time(reader)?)),
        6 => Ok(Value::Null),
        code => Err(protocol_error(format!(
            "unsupported MySQL binary column type {code}"
        ))),
    }
}

fn integer_value(raw: u64, unsigned: bool, bits: u32) -> Result<Value, StatementError> {
    if unsigned {
        return i64::try_from(raw).map(Value::Integer).map_err(|_| {
            protocol_error("unsigned MySQL integer exceeds the supported signed 64-bit range")
        });
    }
    let signed = match bits {
        8 => (raw as u8 as i8) as i64,
        16 => (raw as u16 as i16) as i64,
        32 => (raw as u32 as i32) as i64,
        64 => raw as i64,
        _ => return Err(protocol_error("invalid MySQL integer width")),
    };
    Ok(Value::Integer(signed))
}

fn decode_date_time(reader: &mut Reader<'_>) -> Result<String, StatementError> {
    let length = reader.u8()? as usize;
    if length == 0 {
        return Ok("0000-00-00 00:00:00".to_string());
    }
    if !matches!(length, 4 | 7 | 11) {
        return Err(protocol_error("invalid MySQL date/time length"));
    }
    let year = reader.u16()?;
    let month = reader.u8()?;
    let day = reader.u8()?;
    if length == 4 {
        return Ok(format!("{year:04}-{month:02}-{day:02}"));
    }
    let hour = reader.u8()?;
    let minute = reader.u8()?;
    let second = reader.u8()?;
    let mut value = format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}");
    if length == 11 {
        let microseconds = reader.u32()?;
        write!(value, ".{microseconds:06}").expect("writing to a string cannot fail");
    }
    Ok(value)
}

fn decode_date(reader: &mut Reader<'_>) -> Result<String, StatementError> {
    let length = reader.u8()?;
    if length == 0 {
        return Ok("0000-00-00".to_string());
    }
    if length != 4 {
        return Err(protocol_error("invalid MySQL date length"));
    }
    let year = reader.u16()?;
    let month = reader.u8()?;
    let day = reader.u8()?;
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

fn decode_time(reader: &mut Reader<'_>) -> Result<String, StatementError> {
    let length = reader.u8()? as usize;
    if length == 0 {
        return Ok("00:00:00".to_string());
    }
    if !matches!(length, 8 | 12) {
        return Err(protocol_error("invalid MySQL time length"));
    }
    let negative = reader.u8()? != 0;
    let days = reader.u32()?;
    let hours = days as u64 * 24 + reader.u8()? as u64;
    let minute = reader.u8()?;
    let second = reader.u8()?;
    let mut value = format!(
        "{}{hours:02}:{minute:02}:{second:02}",
        if negative { "-" } else { "" }
    );
    if length == 12 {
        let microseconds = reader.u32()?;
        write!(value, ".{microseconds:06}").expect("writing to a string cannot fail");
    }
    Ok(value)
}

const fn mysql_type_name(code: u8, charset: u16) -> &'static str {
    match code {
        0 | 246 => "DECIMAL",
        1 => "TINYINT",
        2 => "SMALLINT",
        3 => "INT",
        4 => "FLOAT",
        5 => "DOUBLE",
        6 => "NULL",
        7 | 17 => "TIMESTAMP",
        8 => "BIGINT",
        9 => "MEDIUMINT",
        10 => "DATE",
        11 | 19 => "TIME",
        12 | 18 => "DATETIME",
        13 => "YEAR",
        15 | 253 => binary_or_text_type(charset, "VARBINARY", "VARCHAR"),
        16 => "BIT",
        245 => "JSON",
        247 => "ENUM",
        248 => "SET",
        249 => binary_or_text_type(charset, "TINYBLOB", "TINYTEXT"),
        250 => binary_or_text_type(charset, "MEDIUMBLOB", "MEDIUMTEXT"),
        251 => binary_or_text_type(charset, "LONGBLOB", "LONGTEXT"),
        252 => binary_or_text_type(charset, "BLOB", "TEXT"),
        254 => binary_or_text_type(charset, "BINARY", "CHAR"),
        255 => "GEOMETRY",
        _ => "UNKNOWN",
    }
}

const fn binary_or_text_type(
    charset: u16,
    binary_type: &'static str,
    text_type: &'static str,
) -> &'static str {
    if charset == BINARY_CHARSET {
        binary_type
    } else {
        text_type
    }
}

struct OkPacket {
    affected_rows: u64,
    last_insert_id: u64,
    status: ServerStatus,
}

fn parse_ok(packet: &[u8]) -> Result<OkPacket, StatementError> {
    if packet.first() == Some(&0xff) {
        return Err(server_error(packet));
    }
    let mut reader = Reader::new(packet);
    let header = reader.u8()?;
    if !matches!(header, 0x00 | 0xfe) {
        return Err(protocol_error("expected MySQL OK packet"));
    }
    let affected_rows = reader.lenenc_int()?;
    let last_insert_id = reader.lenenc_int()?;
    let status = ServerStatus::from_bits_retain(reader.u16()?);
    let _warnings = reader.u16()?;
    Ok(OkPacket {
        affected_rows,
        last_insert_id,
        status,
    })
}

fn is_result_terminator(packet: &[u8]) -> bool {
    packet.first() == Some(&0xfe) && packet.len() < MAX_PACKET_PAYLOAD
}

fn parse_result_terminator(
    packet: &[u8],
    deprecates_eof: bool,
) -> Result<OkPacket, StatementError> {
    if deprecates_eof {
        return parse_ok(packet);
    }

    let mut reader = Reader::new(packet);
    if reader.u8()? != 0xfe {
        return Err(protocol_error("expected MySQL EOF packet"));
    }
    let _warnings = reader.u16()?;
    let status = ServerStatus::from_bits_retain(reader.u16()?);
    if reader.remaining() != 0 {
        return Err(protocol_error("invalid MySQL EOF packet"));
    }
    Ok(OkPacket {
        affected_rows: 0,
        last_insert_id: 0,
        status,
    })
}

fn server_error(packet: &[u8]) -> StatementError {
    StatementError::new(server_error_text(packet))
}

fn server_error_text(packet: &[u8]) -> String {
    let mut reader = Reader::new(packet);
    if reader.u8().ok() != Some(0xff) {
        return "unexpected MySQL server packet".to_string();
    }
    let code = reader.u16().unwrap_or_default();
    let state = if reader.remaining_bytes().first() == Some(&b'#') {
        _ = reader.u8();
        String::from_utf8_lossy(reader.bytes(5).unwrap_or_default()).into_owned()
    } else {
        "HY000".to_string()
    };
    let message = String::from_utf8_lossy(reader.remaining_bytes());
    format!("MySQL error {code} ({state}): {message}")
}

fn write_packet(stream: &mut dyn Write, sequence: &mut u8, payload: &[u8]) -> io::Result<()> {
    let mut rest = payload;
    loop {
        let length = rest.len().min(MAX_PACKET_PAYLOAD);
        let header = [
            length as u8,
            (length >> 8) as u8,
            (length >> 16) as u8,
            *sequence,
        ];
        stream.write_all(&header)?;
        stream.write_all(&rest[..length])?;
        *sequence = sequence.wrapping_add(1);
        rest = &rest[length..];
        if length < MAX_PACKET_PAYLOAD {
            stream.flush()?;
            return Ok(());
        }
    }
}

fn read_packet(stream: &mut dyn Read, sequence: &mut u8) -> io::Result<Vec<u8>> {
    let mut payload = Vec::new();
    loop {
        let mut header = [0_u8; 4];
        stream.read_exact(&mut header)?;
        let length = header[0] as usize | (header[1] as usize) << 8 | (header[2] as usize) << 16;
        if header[3] != *sequence {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "unexpected MySQL packet sequence {}, expected {}",
                    header[3], *sequence
                ),
            ));
        }
        *sequence = sequence.wrapping_add(1);
        let start = payload.len();
        payload.resize(start + length, 0);
        stream.read_exact(&mut payload[start..])?;
        if length < MAX_PACKET_PAYLOAD {
            return Ok(payload);
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    const fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn remaining_bytes(&self) -> &'a [u8] {
        &self.bytes[self.position..]
    }

    fn skip(&mut self, length: usize) -> Result<(), StatementError> {
        self.bytes(length).map(|_| ())
    }

    fn bytes(&mut self, length: usize) -> Result<&'a [u8], StatementError> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| protocol_error("truncated MySQL packet"))?;
        let bytes = &self.bytes[self.position..end];
        self.position = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, StatementError> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, StatementError> {
        Ok(u16::from_le_bytes(
            self.bytes(2)?.try_into().expect("length checked"),
        ))
    }

    fn u32(&mut self) -> Result<u32, StatementError> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?.try_into().expect("length checked"),
        ))
    }

    fn u64(&mut self) -> Result<u64, StatementError> {
        Ok(u64::from_le_bytes(
            self.bytes(8)?.try_into().expect("length checked"),
        ))
    }

    fn f32(&mut self) -> Result<f32, StatementError> {
        Ok(f32::from_le_bytes(
            self.bytes(4)?.try_into().expect("length checked"),
        ))
    }

    fn f64(&mut self) -> Result<f64, StatementError> {
        Ok(f64::from_le_bytes(
            self.bytes(8)?.try_into().expect("length checked"),
        ))
    }

    fn nul_bytes(&mut self) -> Result<&'a [u8], StatementError> {
        let length = self
            .remaining_bytes()
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| protocol_error("missing NUL terminator in MySQL packet"))?;
        let value = self.bytes(length)?;
        self.skip(1)?;
        Ok(value)
    }

    fn lenenc_int(&mut self) -> Result<u64, StatementError> {
        match self.u8()? {
            value @ 0..=0xfa => Ok(value as u64),
            0xfc => Ok(self.u16()? as u64),
            0xfd => {
                let bytes = self.bytes(3)?;
                Ok(bytes[0] as u64 | (bytes[1] as u64) << 8 | (bytes[2] as u64) << 16)
            }
            0xfe => self.u64(),
            _ => Err(protocol_error("invalid length-encoded MySQL integer")),
        }
    }

    fn lenenc_bytes(&mut self) -> Result<&'a [u8], StatementError> {
        let length = usize::try_from(self.lenenc_int()?)
            .map_err(|_| protocol_error("length-encoded MySQL value is too large"))?;
        self.bytes(length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestStream {
        input: io::Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl Read for TestStream {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.input.read(buffer)
        }
    }

    impl Write for TestStream {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn ok_packet(status: ServerStatus) -> Vec<u8> {
        let payload = [
            0x00,
            0x00,
            0x00,
            status.bits() as u8,
            (status.bits() >> 8) as u8,
            0x00,
            0x00,
        ];
        let mut packet = vec![payload.len() as u8, 0, 0, 1];
        packet.extend_from_slice(&payload);
        packet
    }

    #[test]
    fn client_tracks_transaction_status() {
        let mut input = ok_packet(ServerStatus::IN_TRANS);
        input.extend(ok_packet(ServerStatus::empty()));
        let mut client = Client {
            stream: Box::new(TestStream {
                input: io::Cursor::new(input),
                output: Vec::new(),
            }),
            affected_rows: 0,
            last_insert_id: 0,
            capabilities: Capabilities::PROTOCOL_41,
            in_transaction: false,
        };
        client.execute_script("START TRANSACTION").unwrap();
        assert!(client.in_transaction);
        client.execute_script("COMMIT").unwrap();
        assert!(!client.in_transaction);
    }

    #[test]
    fn transport_errors_mark_connections_as_broken() {
        assert!(statement_io(io::Error::from(io::ErrorKind::BrokenPipe)).connection_broken);
        assert!(protocol_error("invalid packet").connection_broken);
        assert!(!server_error(&[0xff, 1, 0]).connection_broken);
    }

    #[test]
    fn named_parameters_ignore_literals_and_comments() {
        let (query, names) = rewrite_named_parameters(
            "SELECT :one, ':two', 'it\\'s :still_text', `:three`, ? -- :four\n/* :five */ WHERE x = :six",
        )
        .unwrap();
        assert_eq!(
            query,
            "SELECT ?, ':two', 'it\\'s :still_text', `:three`, ? -- :four\n/* :five */ WHERE x = ?"
        );
        assert_eq!(
            names,
            vec![Some(":one".to_string()), None, Some(":six".to_string())]
        );

        let (query, names) = rewrite_named_parameters("SELECT 'cafe', 'café', :naam").unwrap();
        assert_eq!(query, "SELECT 'cafe', 'café', ?");
        assert_eq!(names, vec![Some(":naam".to_string())]);
    }

    #[test]
    fn length_encoded_integers_roundtrip() {
        for value in [0, 250, 251, 65_535, 65_536, 0xff_ffff, u32::MAX as u64] {
            let mut bytes = Vec::new();
            put_lenenc_int(&mut bytes, value);
            assert_eq!(Reader::new(&bytes).lenenc_int().unwrap(), value);
        }
    }

    #[test]
    fn caching_sha2_scramble_matches_known_vector() {
        assert_eq!(
            caching_sha2_scramble("secret", b"12345678901234567890"),
            [
                0x51, 0xec, 0xd6, 0xde, 0xdb, 0xd3, 0x4d, 0x54, 0x45, 0xc0, 0xa1, 0x90, 0xd4, 0xf5,
                0x1a, 0xcf, 0x0d, 0x23, 0xb9, 0x4d, 0xb6, 0x6c, 0x91, 0xf3, 0xf7, 0x89, 0xfa, 0xa9,
                0x19, 0x37, 0x51, 0xcd,
            ]
        );
        assert!(caching_sha2_scramble("", b"nonce").is_empty());
    }

    #[cfg(feature = "mysql-native-password")]
    #[test]
    fn mysql_native_scramble_matches_known_vector() {
        assert_eq!(
            mysql_native_scramble("secret", b"12345678901234567890"),
            [
                0x0f, 0x8b, 0x90, 0x33, 0xe0, 0x89, 0x7c, 0x0a, 0x83, 0x38, 0xeb, 0xe3, 0xde, 0xa9,
                0x01, 0x0d, 0xda, 0x47, 0xab, 0x56,
            ]
        );
        assert!(mysql_native_scramble("", b"nonce").is_empty());
    }

    #[cfg(not(feature = "mysql-native-password"))]
    #[test]
    fn mysql_native_password_reports_disabled_feature() {
        assert_eq!(
            auth_response("mysql_native_password", "secret", b"nonce").unwrap_err(),
            "MySQL authentication plugin 'mysql_native_password' requires the \
             'mysql-native-password' feature"
        );
    }

    #[test]
    fn legacy_eof_packet_preserves_server_status() {
        let packet = [0xfe, 0x02, 0x00, 0x08, 0x80];
        let parsed = parse_result_terminator(&packet, false).unwrap();
        assert_eq!(parsed.affected_rows, 0);
        assert_eq!(parsed.last_insert_id, 0);
        assert!(parsed.status.contains(ServerStatus::MORE_RESULTS_EXISTS));
        assert_eq!(parsed.status.bits(), 0x8008);
    }

    #[test]
    fn ok_packet_requires_protocol_41_status_and_warnings() {
        let packet = [0x00, 0x01, 0x02, 0x08, 0x80, 0x03, 0x00];
        let parsed = parse_ok(&packet).unwrap();
        assert_eq!(parsed.affected_rows, 1);
        assert_eq!(parsed.last_insert_id, 2);
        assert!(parsed.status.contains(ServerStatus::MORE_RESULTS_EXISTS));
        assert_eq!(parsed.status.bits(), 0x8008);
        assert!(parse_ok(&packet[..5]).is_err());
    }

    #[test]
    fn date_and_json_binary_values_keep_their_logical_types() {
        let mut date = Reader::new(&[4, 0xe8, 0x07, 8, 30]);
        assert_eq!(decode_date(&mut date).unwrap(), "2024-08-30");
        let mut zero_date = Reader::new(&[0]);
        assert_eq!(decode_date(&mut zero_date).unwrap(), "0000-00-00");

        let column = Column {
            name: "document".to_string(),
            table: None,
            origin_name: None,
            type_code: 245,
            flags: ColumnFlags::empty(),
            charset: BINARY_CHARSET,
        };
        let row = [0x00, 0x00, 0x07, b'{', b'"', b'a', b'"', b':', b'1', b'}'];
        assert_eq!(
            decode_binary_row(&row, &[column]).unwrap(),
            vec![Value::Text("{\"a\":1}".to_string())]
        );
    }

    #[test]
    fn string_protocol_types_use_the_column_charset() {
        let text_column = Column {
            name: "body".to_string(),
            table: None,
            origin_name: None,
            type_code: 252,
            flags: ColumnFlags::empty(),
            charset: 45,
        };
        let blob_column = Column {
            charset: BINARY_CHARSET,
            ..text_column.clone()
        };
        let row = [0, 0, 3, b'f', b'o', b'o'];

        assert_eq!(text_column.declared_type(), "TEXT");
        assert_eq!(blob_column.declared_type(), "BLOB");
        assert_eq!(
            decode_binary_row(&row, &[text_column]).unwrap(),
            vec![Value::Text("foo".to_string())]
        );
        assert_eq!(
            decode_binary_row(&row, &[blob_column]).unwrap(),
            vec![Value::Blob(b"foo".to_vec())]
        );

        assert_eq!(mysql_type_name(253, 45), "VARCHAR");
        assert_eq!(mysql_type_name(253, BINARY_CHARSET), "VARBINARY");
    }

    #[test]
    fn socket_authentication_sends_an_empty_response() {
        assert!(
            auth_response("auth_socket", "ignored", b"nonce")
                .unwrap()
                .is_empty()
        );
        assert!(
            auth_response("unix_socket", "ignored", b"nonce")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn packet_codec_checks_sequence_numbers() {
        let mut wire = io::Cursor::new(Vec::new());
        let mut sequence = 0;
        write_packet(&mut wire, &mut sequence, b"hello").unwrap();
        assert_eq!(sequence, 1);
        wire.set_position(0);
        sequence = 0;
        assert_eq!(read_packet(&mut wire, &mut sequence).unwrap(), b"hello");

        let mut invalid = io::Cursor::new(vec![0, 0, 0, 3]);
        sequence = 0;
        assert_eq!(
            read_packet(&mut invalid, &mut sequence).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn binary_row_decodes_signed_unsigned_and_null() {
        let columns = vec![
            Column {
                name: "signed".to_string(),
                table: None,
                origin_name: None,
                type_code: 3,
                flags: ColumnFlags::empty(),
                charset: 63,
            },
            Column {
                name: "unsigned".to_string(),
                table: None,
                origin_name: None,
                type_code: 8,
                flags: ColumnFlags::UNSIGNED,
                charset: 63,
            },
            Column {
                name: "empty".to_string(),
                table: None,
                origin_name: None,
                type_code: 253,
                flags: ColumnFlags::empty(),
                charset: 45,
            },
        ];
        let mut packet = vec![0, 1 << 4];
        packet.extend_from_slice(&(-5_i32).to_le_bytes());
        packet.extend_from_slice(&(i64::MAX as u64).to_le_bytes());
        let row = decode_binary_row(&packet, &columns).unwrap();
        assert!(matches!(row[0], Value::Integer(-5)));
        assert!(matches!(row[1], Value::Integer(i64::MAX)));
        assert!(matches!(row[2], Value::Null));
    }

    #[test]
    fn binary_row_rejects_unsigned_integers() {
        let column = Column {
            name: "unsigned".to_string(),
            table: None,
            origin_name: None,
            type_code: 8,
            flags: ColumnFlags::UNSIGNED,
            charset: 63,
        };
        let mut packet = vec![0, 0];
        packet.extend_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(
            decode_binary_row(&packet, &[column])
                .unwrap_err()
                .to_string(),
            "Statement error: MySQL protocol error: unsigned MySQL integer exceeds the supported signed 64-bit range"
        );
    }
}
