/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::database::{QueryResponse, RawQueryResponse};

// MARK: IPC messages
#[derive(Deserialize, Serialize)]
#[allow(clippy::enum_variant_names)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum IpcMessage {
    Ready,
    /// Starts a native window drag from a custom titlebar area
    WindowDrag,
    /// Performs the titlebar double click action from a custom titlebar area
    WindowDoubleClick,
    ShowMessage {
        request_id: u64,
        title: String,
        description: String,
        #[serde(default)]
        level: MessageKind,
        /// Shows a confirmation dialog with this button label next to Cancel
        #[serde(default)]
        confirm: Option<String>,
    },
    ShowMessageResponse {
        request_id: u64,
        confirmed: bool,
    },
    MenuAction {
        action: String,
    },
    RestoreLastFile,
    OpenFile {
        path: String,
    },
    OpenFileDialog {
        request_id: u64,
    },
    OpenFileDialogResponse {
        request_id: u64,
        path: Option<String>,
    },
    OpenDatabase {
        request_id: u64,
        path: String,
    },
    OpenDatabaseResponse {
        request_id: u64,
        ok: bool,
        error: Option<String>,
    },
    ImportSql {
        request_id: u64,
    },
    ImportSqlResponse {
        request_id: u64,
        cancelled: bool,
        error: Option<String>,
    },
    ExportSql {
        request_id: u64,
        file_name: String,
    },
    ExportSqlResponse {
        request_id: u64,
        cancelled: bool,
        error: Option<String>,
    },
    OpenMysql {
        request_id: u64,
        transport: String,
        host: String,
        port: u16,
        socket: String,
        user: String,
        password: Option<Zeroizing<String>>,
        tls: bool,
        remember: bool,
        previous_connection: Option<MysqlCredentialIdentity>,
    },
    OpenMysqlResponse {
        request_id: u64,
        ok: bool,
        error: Option<String>,
        credential_saved: bool,
        credential_error: Option<String>,
    },
    SelectMysqlDatabase {
        request_id: u64,
        database: String,
    },
    SelectMysqlDatabaseResponse {
        request_id: u64,
        ok: bool,
        error: Option<String>,
    },
    RunQuery {
        request_id: u64,
        sql: String,
    },
    #[serde(skip_deserializing)]
    RunQueryResponse {
        request_id: u64,
        #[serde(flatten)]
        result: QueryResponse,
    },
    ApiRequest {
        request_id: u64,
        method: String,
        path: String,
        #[serde(default)]
        body: Option<serde_json::Value>,
    },
    #[serde(skip_deserializing)]
    ApiRequestResponse {
        request_id: u64,
        data: serde_json::Value,
    },
    RunRawQuery {
        request_id: u64,
        sql: String,
    },
    #[serde(skip_deserializing)]
    RunRawQueryResponse {
        request_id: u64,
        #[serde(flatten)]
        result: RawQueryResponse,
    },
    Preferences {
        values: HashMap<Preference, String>,
    },
    SetPreference {
        key: Preference,
        value: Option<String>,
    },
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MessageKind {
    Info,
    Warning,
    #[default]
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Preference {
    DbPath,
    TableName,
    MysqlConnection,
    MysqlDatabase,
}

impl Preference {
    pub(crate) const ALL: [Self; 4] = [
        Self::DbPath,
        Self::TableName,
        Self::MysqlConnection,
        Self::MysqlDatabase,
    ];

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::DbPath => "last_db_path",
            Self::TableName => "last_table_name",
            Self::MysqlConnection => "last_mysql_connection",
            Self::MysqlDatabase => "last_mysql_database",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MysqlCredentialIdentity {
    pub(crate) transport: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) socket: String,
    pub(crate) user: String,
}
