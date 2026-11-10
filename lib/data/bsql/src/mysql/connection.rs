/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::io::{Read, Write};
#[cfg(unix)]
use std::path::PathBuf;
use std::time::Duration;

use crate::connection::Connection;
use crate::{ConnectionError, PoolOptions};

/// Transport used to connect to MySQL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MysqlTransport {
    /// TCP connection with optional verified TLS.
    Tcp {
        /// Server hostname.
        host: String,
        /// Server port.
        port: u16,
        /// Whether verified TLS is required.
        tls: bool,
    },
    /// Unix domain socket connection.
    #[cfg(unix)]
    Unix {
        /// Socket path.
        path: PathBuf,
    },
}

impl MysqlTransport {
    /// Create a TCP transport.
    ///
    /// Setting `tls` to `true` requires the `mysql-tls` feature.
    pub fn tcp(host: impl Into<String>, port: u16, tls: bool) -> Self {
        Self::Tcp {
            host: host.into(),
            port,
            tls,
        }
    }

    /// Create a Unix domain socket transport.
    #[cfg(unix)]
    pub fn unix(path: impl Into<PathBuf>) -> Self {
        Self::Unix { path: path.into() }
    }
}

pub(crate) struct MysqlOptions {
    pub(super) transport: MysqlTransport,
    pub(super) user: String,
    pub(super) password: String,
    pub(super) database: Option<String>,
    pub(super) timeout: Duration,
}

impl MysqlOptions {
    fn new(
        transport: MysqlTransport,
        user: impl Into<String>,
        password: impl Into<String>,
        database: Option<&str>,
    ) -> Self {
        Self {
            transport,
            user: user.into(),
            password: password.into(),
            database: database.map(str::to_owned),
            timeout: Duration::from_secs(10),
        }
    }
}

pub(crate) trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

pub(crate) struct Client {
    pub(crate) stream: Box<dyn Stream>,
    pub(crate) affected_rows: u64,
    pub(crate) last_insert_id: u64,
    pub(crate) capabilities: super::Capabilities,
    pub(crate) in_transaction: bool,
}

impl Connection {
    /// Connect to MySQL using the classic client/server protocol.
    pub fn open_mysql(
        transport: MysqlTransport,
        user: impl Into<String>,
        password: impl Into<String>,
        database: Option<&str>,
        pool_options: PoolOptions,
    ) -> Result<Self, ConnectionError> {
        #[cfg(not(feature = "mysql-tls"))]
        if let MysqlTransport::Tcp { tls: true, .. } = transport {
            return Err(ConnectionError::new(
                "MySQL TLS requires the `mysql-tls` feature",
            ));
        }
        Self::from_mysql_options(
            MysqlOptions::new(transport, user, password, database),
            pool_options,
        )
    }
}

#[cfg(all(test, not(feature = "mysql-tls")))]
mod tests {
    use super::*;

    #[test]
    fn tls_transport_requires_mysql_tls_feature() {
        let result = Connection::open_mysql(
            MysqlTransport::tcp("localhost", 3306, true),
            "user",
            "password",
            None,
            PoolOptions::default(),
        );
        let error = match result {
            Ok(_) => panic!("TLS connection unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(
            error.to_string(),
            "Connection error: MySQL TLS requires the `mysql-tls` feature"
        );
    }
}
