/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 * SPDX-License-Identifier: MIT
 */

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::{Bind, FromRow, Statement, StatementError};

/// Configuration for a database connection pool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolOptions {
    /// Maximum number of physical connections in the pool.
    pub max_connections: usize,
}

impl PoolOptions {
    /// Use exactly one physical connection.
    pub const fn single_connection() -> Self {
        Self { max_connections: 1 }
    }
}

/// Metadata produced by one completed write operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExecutionResult {
    /// Number of rows affected by the operation.
    pub affected_rows: u64,
    /// Row identifier produced by the operation, or zero when none was produced.
    pub last_insert_row_id: u64,
}

impl Default for PoolOptions {
    fn default() -> Self {
        let workers = std::thread::available_parallelism().map_or(1, |count| count.get());
        Self {
            max_connections: workers,
        }
    }
}

pub(crate) enum InnerConnection {
    #[cfg(feature = "mysql")]
    Mysql(crate::mysql::Client),
    #[cfg(feature = "sqlite")]
    Sqlite(crate::sqlite::Connection),
}

impl InnerConnection {
    pub(crate) fn execute_script(&mut self, sql: &str) -> Result<(), StatementError> {
        match self {
            #[cfg(feature = "mysql")]
            Self::Mysql(client) => client.execute_script(sql),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(connection) => connection.execute_script(sql),
        }
    }

    fn begin(&mut self) -> Result<(), StatementError> {
        match self {
            #[cfg(feature = "mysql")]
            Self::Mysql(client) => client.execute_script("START TRANSACTION"),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(connection) => connection.begin_transaction(),
        }
    }

    fn finish(&mut self, commit: bool) -> Result<(), StatementError> {
        match self {
            #[cfg(feature = "mysql")]
            Self::Mysql(client) => {
                client.execute_script(if commit { "COMMIT" } else { "ROLLBACK" })
            }
            #[cfg(feature = "sqlite")]
            Self::Sqlite(connection) => {
                if commit {
                    connection.commit()
                } else {
                    connection.rollback()
                }
            }
        }
    }

    #[allow(clippy::missing_const_for_fn)]
    fn is_autocommit(&self) -> bool {
        match self {
            #[cfg(feature = "mysql")]
            Self::Mysql(client) => !client.in_transaction,
            #[cfg(feature = "sqlite")]
            Self::Sqlite(connection) => connection.is_autocommit(),
        }
    }

    #[allow(clippy::missing_const_for_fn)]
    pub(crate) fn affected_rows(&self) -> u64 {
        match self {
            #[cfg(feature = "mysql")]
            Self::Mysql(client) => client.affected_rows,
            #[cfg(feature = "sqlite")]
            Self::Sqlite(connection) => connection.affected_rows(),
        }
    }

    #[allow(clippy::missing_const_for_fn)]
    pub(crate) fn last_insert_row_id(&self) -> u64 {
        match self {
            #[cfg(feature = "mysql")]
            Self::Mysql(client) => client.last_insert_id,
            #[cfg(feature = "sqlite")]
            Self::Sqlite(connection) => connection.last_insert_row_id(),
        }
    }

    #[cfg(feature = "mysql")]
    #[allow(clippy::missing_const_for_fn)]
    pub(crate) fn mysql_mut(&mut self) -> Result<&mut crate::mysql::Client, StatementError> {
        match self {
            Self::Mysql(client) => Ok(client),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(_) => Err(StatementError::new("operation requires a MySQL connection")),
        }
    }

    #[cfg(feature = "sqlite")]
    #[allow(clippy::missing_const_for_fn)]
    fn sqlite(&self) -> Result<&crate::sqlite::Connection, StatementError> {
        match self {
            Self::Sqlite(connection) => Ok(connection),
            #[cfg(feature = "mysql")]
            Self::Mysql(_) => Err(StatementError::new(
                "operation requires a SQLite connection",
            )),
        }
    }
}

pub(crate) struct ConnectionLease {
    connection: Option<Mutex<InnerConnection>>,
    return_to: Option<LeaseReturn>,
    healthy: AtomicBool,
    finished: AtomicBool,
}

impl ConnectionLease {
    fn new(connection: InnerConnection, return_to: LeaseReturn) -> Arc<Self> {
        Arc::new(Self {
            connection: Some(Mutex::new(connection)),
            return_to: Some(return_to),
            healthy: AtomicBool::new(true),
            finished: AtomicBool::new(false),
        })
    }

    pub(crate) fn connection(&self) -> std::sync::MutexGuard<'_, InnerConnection> {
        let connection = self
            .connection
            .as_ref()
            .expect("lease connection is present");
        match connection.lock() {
            Ok(connection) => connection,
            Err(error) => {
                self.mark_broken();
                error.into_inner()
            }
        }
    }

    pub(crate) fn active_connection(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, InnerConnection>, StatementError> {
        let connection = self.connection();
        self.ensure_active()?;
        Ok(connection)
    }

    pub(crate) fn ensure_active(&self) -> Result<(), StatementError> {
        if self.finished.load(Ordering::Acquire) {
            return Err(StatementError::new(
                "transaction connection is no longer active",
            ));
        }
        Ok(())
    }

    pub(crate) fn mark_broken(&self) {
        self.healthy.store(false, Ordering::Release);
    }

    fn begin_transaction(&self) -> Result<(), StatementError> {
        let result = self.connection().begin();
        if let Err(error) = &result
            && error.connection_broken
        {
            self.mark_broken();
        }
        result
    }

    fn finish_transaction(&self, commit: bool) -> Result<(), StatementError> {
        self.finished.store(true, Ordering::Release);
        let result = self.connection().finish(commit);
        if let Err(error) = &result
            && error.connection_broken
        {
            self.mark_broken();
        }
        result
    }
}

impl Drop for ConnectionLease {
    fn drop(&mut self) {
        let (Some(connection), Some(return_to)) = (self.connection.take(), self.return_to.take())
        else {
            return;
        };
        let (connection, lock_healthy) = match connection.into_inner() {
            Ok(connection) => (connection, true),
            Err(error) => (error.into_inner(), false),
        };
        return_to.release(
            connection,
            lock_healthy && self.healthy.load(Ordering::Acquire),
        );
    }
}

enum LeaseReturn {
    #[cfg(feature = "mysql")]
    Mysql(Arc<MysqlPool>),
    #[cfg(feature = "sqlite")]
    SqliteReader(Arc<SqlitePool>),
    #[cfg(feature = "sqlite")]
    SqliteWriter(Arc<SqlitePool>),
}

impl LeaseReturn {
    fn release(self, connection: InnerConnection, healthy: bool) {
        #[cfg(not(feature = "mysql"))]
        let _ = healthy;
        match self {
            #[cfg(feature = "mysql")]
            Self::Mysql(pool) => {
                if healthy {
                    pool.put_back(connection);
                } else {
                    pool.discard(connection);
                }
            }
            #[cfg(feature = "sqlite")]
            Self::SqliteReader(pool) => pool.put_back_reader(connection),
            #[cfg(feature = "sqlite")]
            Self::SqliteWriter(pool) => pool.put_back_writer(connection),
        }
    }
}

#[derive(Clone)]
enum ConnectionPool {
    #[cfg(feature = "mysql")]
    Mysql(Arc<MysqlPool>),
    #[cfg(feature = "sqlite")]
    Sqlite(Arc<SqlitePool>),
}

impl ConnectionPool {
    fn prepare<T: FromRow>(&self, query: &str) -> Result<Statement<T>, StatementError> {
        match self {
            #[cfg(feature = "mysql")]
            Self::Mysql(pool) => prepare_on_lease(pool.acquire()?, query),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(pool) => pool.prepare(query),
        }
    }

    fn acquire_writer(&self) -> Result<Arc<ConnectionLease>, StatementError> {
        match self {
            #[cfg(feature = "mysql")]
            Self::Mysql(pool) => pool.acquire(),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(pool) => pool.acquire_writer(),
        }
    }

    fn acquire_script_connection(&self) -> Result<Arc<ConnectionLease>, StatementError> {
        match self {
            #[cfg(feature = "mysql")]
            Self::Mysql(pool) => pool.acquire(),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(pool) if pool.is_read_only() => pool.acquire_reader(),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(pool) => pool.acquire_writer(),
        }
    }
}

fn prepare_on_lease<T: FromRow>(
    lease: Arc<ConnectionLease>,
    query: &str,
) -> Result<Statement<T>, StatementError> {
    let mut connection = lease.active_connection()?;
    let result = match &mut *connection {
        #[cfg(feature = "mysql")]
        InnerConnection::Mysql(client) => client
            .prepare(query)
            .map(|prepared| Statement::new_mysql(prepared, Arc::clone(&lease))),
        #[cfg(feature = "sqlite")]
        InnerConnection::Sqlite(connection) => connection
            .prepare(query)
            .map(|prepared| Statement::new_sqlite(prepared, Arc::clone(&lease))),
    };
    drop(connection);
    if let Err(error) = &result
        && error.connection_broken
    {
        lease.mark_broken();
    }
    result
}

#[cfg(feature = "mysql")]
struct MysqlPool {
    state: Mutex<MysqlState>,
    available: Condvar,
    options: crate::mysql::MysqlOptions,
    max_connections: usize,
}

#[cfg(feature = "mysql")]
struct MysqlState {
    idle: Vec<InnerConnection>,
    total: usize,
}

#[cfg(feature = "mysql")]
impl MysqlPool {
    fn new(
        options: crate::mysql::MysqlOptions,
        pool: PoolOptions,
    ) -> Result<Arc<Self>, ConnectionError> {
        validate_pool_options(&pool)?;
        let client = crate::mysql::Client::connect(&options).map_err(ConnectionError::new)?;
        Ok(Arc::new(Self {
            state: Mutex::new(MysqlState {
                idle: vec![InnerConnection::Mysql(client)],
                total: 1,
            }),
            available: Condvar::new(),
            options,
            max_connections: pool.max_connections,
        }))
    }

    fn acquire(self: &Arc<Self>) -> Result<Arc<ConnectionLease>, StatementError> {
        let mut state = self.state.lock().map_err(|_| pool_lock_error())?;
        loop {
            if let Some(connection) = state.idle.pop() {
                drop(state);
                return Ok(self.lease(connection));
            }
            if state.total < self.max_connections {
                state.total += 1;
                drop(state);
                match crate::mysql::Client::connect(&self.options) {
                    Ok(client) => return Ok(self.lease(InnerConnection::Mysql(client))),
                    Err(error) => {
                        let mut state = self.state.lock().map_err(|_| pool_lock_error())?;
                        state.total -= 1;
                        self.available.notify_one();
                        return Err(StatementError::new(format!(
                            "failed to open pooled MySQL connection: {error}"
                        )));
                    }
                }
            }
            state = self.available.wait(state).map_err(|_| pool_lock_error())?;
        }
    }

    fn lease(self: &Arc<Self>, connection: InnerConnection) -> Arc<ConnectionLease> {
        ConnectionLease::new(connection, LeaseReturn::Mysql(Arc::clone(self)))
    }

    fn put_back(&self, connection: InnerConnection) {
        if let Ok(mut state) = self.state.lock() {
            state.idle.push(connection);
            self.available.notify_one();
        }
    }

    fn discard(&self, _connection: InnerConnection) {
        if let Ok(mut state) = self.state.lock() {
            state.total -= 1;
            self.available.notify_one();
        }
    }
}

#[cfg(feature = "sqlite")]
pub(crate) struct SqlitePoolOptions {
    pub(crate) path: std::path::PathBuf,
    pub(crate) mode: crate::sqlite::SqliteMode,
}

#[cfg(feature = "sqlite")]
pub(crate) struct SqlitePool {
    state: Mutex<SqliteState>,
    available: Condvar,
    options: SqlitePoolOptions,
    max_connections: usize,
}

#[cfg(feature = "sqlite")]
struct SqliteState {
    readers: Vec<InnerConnection>,
    reader_total: usize,
    writer: Option<InnerConnection>,
}

#[cfg(feature = "sqlite")]
impl SqlitePool {
    fn new(options: SqlitePoolOptions, pool: PoolOptions) -> Result<Arc<Self>, ConnectionError> {
        validate_pool_options(&pool)?;
        if pool.max_connections > 1 && !crate::sqlite::Connection::is_threadsafe() {
            return Err(ConnectionError::new(
                "pooled SQLite connections require a thread-safe SQLite build",
            ));
        }
        let connection = crate::sqlite::Connection::open(&options.path, options.mode)
            .map_err(ConnectionError::new)?;
        let read_only = options.mode == crate::sqlite::SqliteMode::ReadOnly;
        let (readers, reader_total, writer) = if read_only {
            (vec![InnerConnection::Sqlite(connection)], 1, None)
        } else {
            (vec![], 0, Some(InnerConnection::Sqlite(connection)))
        };
        Ok(Arc::new(Self {
            state: Mutex::new(SqliteState {
                readers,
                reader_total,
                writer,
            }),
            available: Condvar::new(),
            options,
            max_connections: pool.max_connections,
        }))
    }

    fn is_read_only(&self) -> bool {
        self.options.mode == crate::sqlite::SqliteMode::ReadOnly
    }

    fn prepare<T: FromRow>(self: &Arc<Self>, query: &str) -> Result<Statement<T>, StatementError> {
        if !self.is_read_only() && self.max_connections == 1 {
            return prepare_on_lease(self.acquire_writer()?, query);
        }
        if !self.is_read_only() && is_obvious_sqlite_write(query) {
            return prepare_on_lease(self.acquire_writer()?, query);
        }
        let reader = self.acquire_reader()?;
        let prepared = reader.connection().sqlite()?.prepare(query)?;
        if self.is_read_only() || prepared.is_read_only() {
            return Ok(Statement::new_sqlite(prepared, reader));
        }
        drop(prepared);
        drop(reader);
        prepare_on_lease(self.acquire_writer()?, query)
    }

    fn acquire_reader(self: &Arc<Self>) -> Result<Arc<ConnectionLease>, StatementError> {
        let capacity = if self.is_read_only() {
            self.max_connections
        } else {
            self.max_connections - 1
        };
        let mut state = self.state.lock().map_err(|_| pool_lock_error())?;
        loop {
            if let Some(connection) = state.readers.pop() {
                drop(state);
                return Ok(self.reader_lease(connection));
            }
            if state.reader_total < capacity {
                state.reader_total += 1;
                drop(state);
                let result = crate::sqlite::Connection::open(&self.options.path, self.options.mode);
                match result {
                    Ok(connection) => {
                        return Ok(self.reader_lease(InnerConnection::Sqlite(connection)));
                    }
                    Err(error) => {
                        let mut state = self.state.lock().map_err(|_| pool_lock_error())?;
                        state.reader_total -= 1;
                        self.available.notify_all();
                        return Err(StatementError::new(error));
                    }
                }
            }
            state = self.available.wait(state).map_err(|_| pool_lock_error())?;
        }
    }

    pub(crate) fn acquire_writer(self: &Arc<Self>) -> Result<Arc<ConnectionLease>, StatementError> {
        if self.is_read_only() {
            return Err(StatementError::new(
                "operation requires a writable SQLite connection",
            ));
        }
        let mut state = self.state.lock().map_err(|_| pool_lock_error())?;
        loop {
            if let Some(connection) = state.writer.take() {
                drop(state);
                return Ok(ConnectionLease::new(
                    connection,
                    LeaseReturn::SqliteWriter(Arc::clone(self)),
                ));
            }
            state = self.available.wait(state).map_err(|_| pool_lock_error())?;
        }
    }

    fn reader_lease(self: &Arc<Self>, connection: InnerConnection) -> Arc<ConnectionLease> {
        ConnectionLease::new(connection, LeaseReturn::SqliteReader(Arc::clone(self)))
    }

    fn put_back_reader(&self, connection: InnerConnection) {
        if let Ok(mut state) = self.state.lock() {
            state.readers.push(connection);
            self.available.notify_one();
        }
    }

    fn put_back_writer(&self, connection: InnerConnection) {
        if let Ok(mut state) = self.state.lock() {
            state.writer = Some(connection);
            self.available.notify_all();
        }
    }
}

#[cfg(feature = "sqlite")]
fn is_obvious_sqlite_write(query: &str) -> bool {
    let keyword = first_sql_keyword(query);
    [
        "ALTER",
        "ANALYZE",
        "ATTACH",
        "BEGIN",
        "COMMIT",
        "CREATE",
        "DELETE",
        "DETACH",
        "DROP",
        "END",
        "INSERT",
        "REINDEX",
        "RELEASE",
        "REPLACE",
        "ROLLBACK",
        "SAVEPOINT",
        "UPDATE",
        "VACUUM",
    ]
    .iter()
    .any(|candidate| keyword.eq_ignore_ascii_case(candidate))
}

fn reject_transaction_control(sql: &str) -> Result<(), StatementError> {
    if sql
        .split(';')
        .map(first_sql_keyword)
        .any(is_transaction_control_keyword)
    {
        return Err(StatementError::new(
            "transaction-control SQL is not allowed; use Connection::transaction",
        ));
    }
    Ok(())
}

fn is_transaction_control_keyword(keyword: &str) -> bool {
    [
        "BEGIN",
        "COMMIT",
        "RELEASE",
        "ROLLBACK",
        "SAVEPOINT",
        "START",
    ]
    .iter()
    .any(|candidate| keyword.eq_ignore_ascii_case(candidate))
}

fn first_sql_keyword(mut sql: &str) -> &str {
    loop {
        sql = sql.trim_start();
        if let Some(rest) = sql.strip_prefix("--") {
            sql = rest.split_once('\n').map_or("", |(_, rest)| rest);
            continue;
        }
        if let Some(rest) = sql.strip_prefix("/*") {
            sql = rest.split_once("*/").map_or("", |(_, rest)| rest);
            continue;
        }
        return sql
            .split_once(|character: char| !character.is_ascii_alphabetic())
            .map_or(sql, |(keyword, _)| keyword);
    }
}

fn pool_lock_error() -> StatementError {
    StatementError::new("connection pool lock is poisoned")
}

fn validate_pool_options(options: &PoolOptions) -> Result<(), ConnectionError> {
    if options.max_connections == 0 {
        return Err(ConnectionError::new(
            "maximum pool connections must be greater than zero",
        ));
    }
    Ok(())
}

/// A connection error.
#[derive(Debug)]
pub struct ConnectionError {
    msg: String,
}

impl ConnectionError {
    pub(crate) fn new(msg: impl Into<String>) -> Self {
        Self { msg: msg.into() }
    }
}

impl Display for ConnectionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Connection error: {}", self.msg)
    }
}
impl Error for ConnectionError {}

/// A pooled database connection.
pub struct Connection {
    source: ConnectionSource,
    rollback_on_drop: bool,
}

#[derive(Clone)]
enum ConnectionSource {
    Pool(ConnectionPool),
    Transaction(Arc<ConnectionLease>),
}

impl Clone for Connection {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            rollback_on_drop: false,
        }
    }
}

impl Connection {
    #[cfg(feature = "mysql")]
    pub(crate) fn from_mysql_options(
        options: crate::mysql::MysqlOptions,
        pool: PoolOptions,
    ) -> Result<Self, ConnectionError> {
        Ok(Self {
            source: ConnectionSource::Pool(ConnectionPool::Mysql(MysqlPool::new(options, pool)?)),
            rollback_on_drop: false,
        })
    }

    #[cfg(feature = "sqlite")]
    pub(crate) fn from_sqlite_options(
        options: SqlitePoolOptions,
        pool: PoolOptions,
    ) -> Result<Self, ConnectionError> {
        Ok(Self {
            source: ConnectionSource::Pool(ConnectionPool::Sqlite(SqlitePool::new(options, pool)?)),
            rollback_on_drop: false,
        })
    }

    /// Prepare a statement.
    pub fn prepare<T: FromRow>(
        &self,
        query: impl AsRef<str>,
    ) -> Result<Statement<T>, StatementError> {
        reject_transaction_control(query.as_ref())?;
        match &self.source {
            ConnectionSource::Pool(pool) => pool.prepare(query.as_ref()),
            ConnectionSource::Transaction(lease) => {
                prepare_on_lease(Arc::clone(lease), query.as_ref())
            }
        }
    }

    /// Prepare and bind a query.
    pub fn query<T: FromRow>(
        &self,
        query: impl AsRef<str>,
        params: impl Bind,
    ) -> Result<Statement<T>, StatementError> {
        let mut statement = self.prepare::<T>(query)?;
        statement.bind(params)?;
        Ok(statement)
    }

    /// Read the first returned row.
    pub fn query_some<T: FromRow>(
        &self,
        query: impl AsRef<str>,
        params: impl Bind,
    ) -> Result<T, StatementError> {
        self.query::<T>(query, params)?
            .next()
            .transpose()?
            .ok_or_else(|| StatementError::new("expected at least one row from query"))
    }

    /// Execute a SQL script.
    pub fn execute_script(&self, sql: &str) -> Result<ExecutionResult, StatementError> {
        if matches!(self.source, ConnectionSource::Transaction(_)) {
            reject_transaction_control(sql)?;
        }
        let lease = match &self.source {
            ConnectionSource::Pool(pool) => pool.acquire_script_connection()?,
            ConnectionSource::Transaction(lease) => Arc::clone(lease),
        };
        let result = lease.active_connection()?.execute_script(sql);
        if let Err(error) = result {
            if error.connection_broken {
                lease.mark_broken();
            }
            // A script that fails after its own BEGIN must not return its open transaction to the pool.
            return Err(match self.source {
                ConnectionSource::Pool(_) => rollback_after_error(&lease, error),
                ConnectionSource::Transaction(_) => error,
            });
        }
        if matches!(self.source, ConnectionSource::Pool(_)) && !lease.connection().is_autocommit() {
            let rollback = lease.finish_transaction(false);
            return Err(match rollback {
                Ok(()) => StatementError::new(
                    "SQL script left a transaction open; use Connection::transaction",
                ),
                Err(error) => StatementError::new(format!(
                    "SQL script left a transaction open and rollback failed: {error}"
                )),
            });
        }
        let connection = lease.connection();
        Ok(ExecutionResult {
            affected_rows: connection.affected_rows(),
            last_insert_row_id: connection.last_insert_row_id(),
        })
    }

    /// Execute a prepared query.
    pub fn execute(
        &self,
        query: impl AsRef<str>,
        params: impl Bind,
    ) -> Result<ExecutionResult, StatementError> {
        let mut statement = self.query::<()>(query, params)?;
        statement.next().transpose()?;
        Ok(statement.execution_result())
    }

    /// Run a closure inside a transaction on one exclusively leased connection.
    pub fn transaction<T, E>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<StatementError> + Display,
    {
        let ConnectionSource::Pool(pool) = &self.source else {
            return Err(E::from(StatementError::new(
                "nested transactions are not supported",
            )));
        };
        let lease = pool.acquire_writer().map_err(E::from)?;
        lease.begin_transaction().map_err(E::from)?;
        let mut transaction = Connection {
            source: ConnectionSource::Transaction(lease),
            rollback_on_drop: true,
        };
        match operation(&transaction) {
            Ok(value) if Arc::strong_count(transaction.transaction_lease()) != 1 => {
                drop(value);
                let error = E::from(StatementError::new(
                    "transaction connections and statements must not escape their closure",
                ));
                let error = rollback_after_error(transaction.transaction_lease(), error);
                transaction.rollback_on_drop = false;
                Err(error)
            }
            Ok(value) => match transaction.transaction_lease().finish_transaction(true) {
                Ok(()) => {
                    transaction.rollback_on_drop = false;
                    Ok(value)
                }
                Err(error) => {
                    let error =
                        rollback_after_error(transaction.transaction_lease(), E::from(error));
                    transaction.rollback_on_drop = false;
                    Err(error)
                }
            },
            Err(error) => {
                let error = rollback_after_error(transaction.transaction_lease(), error);
                transaction.rollback_on_drop = false;
                Err(error)
            }
        }
    }

    fn transaction_lease(&self) -> &Arc<ConnectionLease> {
        let ConnectionSource::Transaction(lease) = &self.source else {
            unreachable!()
        };
        lease
    }

    #[cfg(feature = "sqlite")]
    pub(crate) fn sqlite_pool(&self) -> Result<&Arc<SqlitePool>, StatementError> {
        let ConnectionSource::Pool(pool) = &self.source else {
            return Err(StatementError::new(
                "operation is not available on a transaction connection",
            ));
        };
        match pool {
            ConnectionPool::Sqlite(pool) => Ok(pool),
            #[cfg(feature = "mysql")]
            ConnectionPool::Mysql(_) => Err(StatementError::new(
                "operation requires a SQLite connection",
            )),
        }
    }

    #[cfg(feature = "sqlite")]
    pub(crate) const fn is_sqlite(&self) -> bool {
        matches!(
            &self.source,
            ConnectionSource::Pool(ConnectionPool::Sqlite(_))
        )
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if !self.rollback_on_drop {
            return;
        }
        self.rollback_on_drop = false;
        let ConnectionSource::Transaction(lease) = &self.source else {
            return;
        };
        if !lease.connection().is_autocommit() {
            _ = lease.finish_transaction(false);
        }
    }
}

fn rollback_after_error<E>(lease: &ConnectionLease, error: E) -> E
where
    E: From<StatementError> + Display,
{
    if lease.connection().is_autocommit() {
        return error;
    }
    match lease.finish_transaction(false) {
        Ok(()) => error,
        Err(rollback_error) => E::from(StatementError::new(format!(
            "{error}; additionally failed to roll back transaction: {rollback_error}"
        ))),
    }
}

/// Run a query with named arguments.
#[macro_export]
macro_rules! query_args {
    ($t:tt, $db:expr, $query:expr, Args { $($key:ident : $value:expr),* $(,)? } $(,)?) => {{
        (|| -> std::result::Result<_, $crate::StatementError> {
            let mut statement = $db.prepare::<$t>($query)?;
            $(statement.bind_named_value(concat!(":", stringify!($key)), Into::<$crate::Value>::into($value))?;)*
            Ok(statement)
        })()
    }};
}

/// Execute a query with named arguments.
#[macro_export]
macro_rules! execute_args {
    ($db:expr, $query:expr, Args { $($key:ident : $value:expr),* $(,)? } $(,)?) => {{
        (|| -> std::result::Result<_, $crate::StatementError> {
            let mut statement = $db.prepare::<()>($query)?;
            $(statement.bind_named_value(concat!(":", stringify!($key)), Into::<$crate::Value>::into($value))?;)*
            statement.next().transpose()?;
            Ok(statement.execution_result())
        })()
    }};
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn pool_options_defaults() {
        let workers = std::thread::available_parallelism().map_or(1, |count| count.get());
        assert_eq!(PoolOptions::default().max_connections, workers);
        assert_eq!(PoolOptions::single_connection().max_connections, 1);
    }

    #[test]
    fn zero_connection_pool_is_rejected() {
        let path = std::env::temp_dir().join(format!("bsql-zero-pool-{}.db", std::process::id()));
        let error = Connection::open_sqlite(
            path,
            crate::sqlite::SqliteMode::ReadWrite,
            PoolOptions { max_connections: 0 },
        )
        .err()
        .expect("zero-sized pool should fail");
        assert!(error.to_string().contains("greater than zero"));
    }

    #[test]
    fn poisoned_lease_lock_is_recovered_before_reuse() -> Result<(), StatementError> {
        let database = Connection::open_sqlite_memory().expect("open SQLite");
        let lease = database.sqlite_pool()?.acquire_writer()?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _connection = lease.connection();
            panic!("poison lease lock");
        }));
        assert!(result.is_err());
        drop(lease.connection());
        assert!(!lease.healthy.load(Ordering::Acquire));
        drop(lease);
        database.execute("CREATE TABLE items (id INTEGER PRIMARY KEY) STRICT", ())?;
        Ok(())
    }

    #[test]
    fn sqlite_memory_uses_one_serialized_connection() -> Result<(), StatementError> {
        let error = Connection::open_sqlite(
            ":memory:",
            crate::sqlite::SqliteMode::ReadWrite,
            PoolOptions { max_connections: 2 },
        )
        .err()
        .expect("multi-connection in-memory database should fail");
        assert!(error.to_string().contains("exactly one connection"));

        let database = Connection::open_sqlite_memory().expect("open SQLite");
        database.execute("CREATE TABLE items (id INTEGER PRIMARY KEY) STRICT", ())?;
        database.execute("INSERT INTO items VALUES (1), (2)", ())?;
        let pool = database.sqlite_pool()?;
        assert_eq!(pool.max_connections, 1);
        assert_eq!(pool.state.lock().expect("pool lock").reader_total, 0);
        assert_eq!(
            database.query_some::<i64>("SELECT COUNT(*) FROM items", ())?,
            2
        );
        Ok(())
    }

    #[test]
    fn sqlite_single_connection_uses_one_read_write_handle() -> Result<(), Box<dyn Error>> {
        let path =
            std::env::temp_dir().join(format!("bsql-single-connection-{}.db", std::process::id()));
        let database = Connection::open_sqlite(
            &path,
            crate::sqlite::SqliteMode::ReadWrite,
            PoolOptions::single_connection(),
        )?;
        database.execute("CREATE TABLE items (id INTEGER PRIMARY KEY) STRICT", ())?;
        database.execute("INSERT INTO items VALUES (1)", ())?;
        assert_eq!(database.query_some::<i64>("SELECT id FROM items", ())?, 1);

        let state = database.sqlite_pool()?.state.lock().expect("pool lock");
        assert!(state.writer.is_some());
        assert_eq!(state.reader_total, 0);
        drop(state);
        drop(database);
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn execution_result_belongs_to_the_operation() -> Result<(), StatementError> {
        let database = Connection::open_sqlite_memory().expect("open SQLite");
        database.execute("CREATE TABLE items (id INTEGER PRIMARY KEY) STRICT", ())?;
        let result = database.execute("INSERT INTO items VALUES (10), (11)", ())?;
        assert_eq!(result.affected_rows, 2);
        assert_eq!(result.last_insert_row_id, 11);
        Ok(())
    }

    #[test]
    fn pooled_transaction_control_is_rejected_and_rolled_back() -> Result<(), StatementError> {
        let database = Connection::open_sqlite_memory().expect("open SQLite");
        database.execute("CREATE TABLE items (id INTEGER PRIMARY KEY) STRICT", ())?;
        assert!(database.execute("BEGIN", ()).is_err());
        assert!(
            database
                .execute_script("BEGIN; INSERT INTO items VALUES (1)")
                .is_err()
        );
        assert_eq!(
            database.query_some::<i64>("SELECT COUNT(*) FROM items", ())?,
            0
        );
        database.execute("INSERT INTO items VALUES (2)", ())?;
        Ok(())
    }

    #[test]
    fn escaped_transaction_connection_is_invalidated() -> Result<(), StatementError> {
        let database = Connection::open_sqlite_memory().expect("open SQLite");
        database.execute("CREATE TABLE items (id INTEGER PRIMARY KEY) STRICT", ())?;
        let escaped = Arc::new(Mutex::new(None));
        let output = Arc::clone(&escaped);
        let error = database
            .transaction(|transaction| -> Result<(), StatementError> {
                *output.lock().expect("escaped lock") = Some(transaction.clone());
                Ok(())
            })
            .expect_err("escaping a transaction must fail");
        assert!(error.to_string().contains("must not escape"));
        let escaped = escaped
            .lock()
            .expect("escaped lock")
            .take()
            .expect("escaped connection");
        assert!(escaped.execute("INSERT INTO items VALUES (1)", ()).is_err());
        drop(escaped);
        database.execute("INSERT INTO items VALUES (2)", ())?;
        Ok(())
    }

    #[test]
    fn escaped_transaction_connection_is_invalidated_after_error() -> Result<(), StatementError> {
        let database = Connection::open_sqlite_memory().expect("open SQLite");
        database.execute("CREATE TABLE items (id INTEGER PRIMARY KEY) STRICT", ())?;
        let escaped = Arc::new(Mutex::new(None));
        let output = Arc::clone(&escaped);
        let error = database
            .transaction(|transaction| -> Result<(), StatementError> {
                *output.lock().expect("escaped lock") = Some(transaction.clone());
                Err(StatementError::new("abort"))
            })
            .expect_err("transaction should fail");
        assert!(error.to_string().contains("abort"));
        let escaped = escaped
            .lock()
            .expect("escaped lock")
            .take()
            .expect("escaped connection");
        assert!(escaped.execute("INSERT INTO items VALUES (1)", ()).is_err());
        drop(escaped);
        database.execute("INSERT INTO items VALUES (2)", ())?;
        Ok(())
    }

    #[test]
    fn escaped_transaction_statement_is_rejected() -> Result<(), StatementError> {
        let database = Connection::open_sqlite_memory().expect("open SQLite");
        database.execute("CREATE TABLE items (id INTEGER PRIMARY KEY) STRICT", ())?;
        let error = database
            .transaction(|transaction| transaction.prepare::<()>("INSERT INTO items VALUES (1)"))
            .err()
            .expect("escaping a transaction statement must fail");
        assert!(error.to_string().contains("must not escape"));
        database.execute("INSERT INTO items VALUES (2)", ())?;
        Ok(())
    }

    #[test]
    fn transaction_error_rolls_back_and_rejects_nesting() -> Result<(), StatementError> {
        let database = Connection::open_sqlite_memory().expect("open SQLite");
        database.execute("CREATE TABLE items (id INTEGER PRIMARY KEY) STRICT", ())?;
        let error = database
            .transaction(|transaction| -> Result<(), StatementError> {
                transaction.execute("INSERT INTO items VALUES (1)", ())?;
                assert!(
                    transaction
                        .transaction(|_| -> Result<(), StatementError> { Ok(()) })
                        .is_err()
                );
                Err(StatementError::new("abort"))
            })
            .expect_err("closure error must propagate");
        assert!(error.to_string().contains("abort"));
        assert_eq!(
            database.query_some::<i64>("SELECT COUNT(*) FROM items", ())?,
            0
        );
        database.execute("INSERT INTO items VALUES (2)", ())?;
        Ok(())
    }

    #[test]
    fn failed_script_rolls_back_its_transaction() -> Result<(), StatementError> {
        let database = Connection::open_sqlite_memory().expect("open SQLite");
        database.execute("CREATE TABLE items (id INTEGER PRIMARY KEY) STRICT", ())?;
        let script = "BEGIN TRANSACTION; INSERT INTO items VALUES (1); INSERT INTO missing VALUES (1); COMMIT;";
        let error = database
            .execute_script(script)
            .expect_err("missing table must fail");
        assert!(error.to_string().contains("missing"));
        assert_eq!(
            database.query_some::<i64>("SELECT COUNT(*) FROM items", ())?,
            0
        );
        database.execute_script("BEGIN TRANSACTION; INSERT INTO items VALUES (2); COMMIT;")?;
        assert_eq!(
            database.query_some::<i64>("SELECT COUNT(*) FROM items", ())?,
            1
        );
        Ok(())
    }

    #[test]
    fn read_only_pool_can_execute_read_only_scripts() -> Result<(), Box<dyn Error>> {
        static NEXT_DATABASE: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("bsql-read-only-{}-{id}.db", std::process::id()));
        {
            let database = Connection::open_sqlite(
                &path,
                crate::sqlite::SqliteMode::ReadWrite,
                PoolOptions::default(),
            )?;
            database.execute("CREATE TABLE items (id INTEGER PRIMARY KEY) STRICT", ())?;
        }
        {
            let database = Connection::open_sqlite(
                &path,
                crate::sqlite::SqliteMode::ReadOnly,
                PoolOptions { max_connections: 2 },
            )?;
            database.execute_script("SELECT * FROM items")?;
        }
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn transaction_panic_rolls_back_and_releases_writer() -> Result<(), StatementError> {
        let database = Connection::open_sqlite_memory().expect("open SQLite");
        database.execute("CREATE TABLE items (id INTEGER PRIMARY KEY) STRICT", ())?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = database.transaction(|transaction| -> Result<(), StatementError> {
                transaction.execute("INSERT INTO items VALUES (1)", ())?;
                panic!("abort transaction");
            });
        }));
        assert!(result.is_err());
        assert_eq!(
            database.query_some::<i64>("SELECT COUNT(*) FROM items", ())?,
            0
        );
        database.execute("INSERT INTO items VALUES (2)", ())?;
        Ok(())
    }

    #[test]
    fn sqlite_transactions_serialize_writers() -> Result<(), Box<dyn Error>> {
        let id = std::process::id();
        let path = std::env::temp_dir().join(format!("bsql-pool-{id}.db"));
        let database = Connection::open_sqlite(
            &path,
            crate::sqlite::SqliteMode::ReadWrite,
            PoolOptions { max_connections: 3 },
        )?;
        database.enable_wal_logging()?;
        database.transaction(|writer| -> Result<(), StatementError> {
            assert_eq!(writer.query_some::<i64>("PRAGMA synchronous", ())?, 1);
            Ok(())
        })?;
        database.execute("CREATE TABLE items (value INTEGER NOT NULL) STRICT", ())?;
        database.execute("INSERT INTO items VALUES (0)", ())?;

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first = database.clone();
        let first_writer = std::thread::Builder::new()
            .name("test-first-writer".to_string())
            .spawn(move || {
                first.transaction(|transaction| -> Result<(), StatementError> {
                    transaction.execute("UPDATE items SET value = 1", ())?;
                    started_tx.send(()).expect("signal transaction start");
                    release_rx.recv().expect("release transaction");
                    Ok(())
                })
            })
            .expect("spawn first writer");
        started_rx.recv_timeout(Duration::from_secs(1))?;

        let (attempting_tx, attempting_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let second = database.clone();
        let second_writer = std::thread::Builder::new()
            .name("test-second-writer".to_string())
            .spawn(move || {
                attempting_tx.send(()).expect("signal writer attempt");
                let result = second.execute("UPDATE items SET value = 2", ());
                finished_tx.send(result).expect("signal writer completion");
            })
            .expect("spawn second writer");
        attempting_rx.recv_timeout(Duration::from_secs(1))?;
        assert!(finished_rx.recv_timeout(Duration::from_millis(50)).is_err());
        assert_eq!(
            database.query_some::<i64>("SELECT value FROM items", ())?,
            0
        );

        release_tx.send(())?;
        first_writer.join().expect("first writer")?;
        finished_rx.recv_timeout(Duration::from_secs(1))??;
        second_writer.join().expect("second writer");
        assert_eq!(
            database.query_some::<i64>("SELECT value FROM items", ())?,
            2
        );
        drop(database);
        std::fs::remove_file(&path)?;
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        Ok(())
    }
}
