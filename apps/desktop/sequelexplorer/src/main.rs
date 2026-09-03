/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod database;
mod ipc;
mod schema;
mod sql_transfer;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bsql::{Connection, PoolOptions, SqliteMode};
use bwebview::{WebviewBuilder, WebviewEvent};
use bwindow::{
    Accelerator, Event, EventLoopBuilder, FileDialog, KeyCode, LogicalSize, MenuBarBuilder,
    MenuBuilder, MenuItem, MessageDialog, MessageLevel, Modifiers, Theme, WindowBuilder,
};
use preferences::Preferences;
use rust_embed::Embed;
use small_http::Status;
use small_router::RouterBuilder;
use zeroize::Zeroizing;

use crate::database::{
    DatabaseState, MysqlConnectionPendingGuard, OpenMysqlRequest, QueryResponse, State,
    db_databases, db_raw_query, db_table_data, db_table_delete, db_table_insert, db_table_update,
    db_tables, db_users, db_users_create, db_users_delete, db_users_update, open_mysql,
    query_connection, replace_database_state_if_current, run_query,
};
use crate::ipc::{IpcMessage, Preference};
use crate::schema::{db_table_schema, db_table_schema_update};
use crate::sql_transfer::{export_sql, import_sql};

#[derive(Embed)]
#[folder = "web"]
struct WebAssets;

// MARK: Main

pub(crate) enum AppEvent {
    Webview(bwindow::WindowId, WebviewEvent),
    UserEvent(String),
}

const fn background_color(theme: Theme) -> u32 {
    match theme {
        Theme::Light => 0xffffff,
        Theme::Dark => 0x222222,
    }
}

fn main() {
    let startup_path = if cfg!(any(target_os = "macos", windows)) {
        std::env::args().nth(1)
    } else {
        None
    };
    let state: State = Arc::new(Mutex::new(DatabaseState::default()));
    let event_loop = EventLoopBuilder::new()
        .with_user_event::<AppEvent>()
        .app_id("nl", "bplaat", "SequelExplorer")
        .single_instance(false)
        .menu(
            MenuBarBuilder::new()
                .menu(
                    MenuBuilder::new("File")
                        .item(
                            MenuItem::new("Connect to Database...", "open")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyO)),
                        )
                        .separator()
                        .item(MenuItem::new("Import SQL...", "importSql"))
                        .item(MenuItem::new("Export SQL...", "exportSql"))
                        .separator(),
                )
                .menu(
                    MenuBuilder::new("View")
                        .item(
                            MenuItem::new("Data", "showData")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::Digit1)),
                        )
                        .item(
                            MenuItem::new("Schema", "showSchema")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::Digit2)),
                        )
                        .item(
                            MenuItem::new("Query", "showQuery")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::Digit3)),
                        ),
                )
                .menu(
                    MenuBuilder::new("Query")
                        .item(
                            MenuItem::new("Run Query", "runQuery")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::Enter)),
                        )
                        .item(
                            MenuItem::new("Clear Query", "clearQuery")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyK)),
                        ),
                ),
        )
        .build();

    let router = RouterBuilder::<State>::with(Arc::clone(&state))
        .get("/api/databases", db_databases)
        .get("/api/users", db_users)
        .post("/api/users", db_users_create)
        .put("/api/users", db_users_update)
        .delete("/api/users", db_users_delete)
        .get("/api/tables", db_tables)
        .get("/api/table/:name/data", db_table_data)
        .post("/api/table/:name/data", db_table_insert)
        .put("/api/table/:name/data", db_table_update)
        .delete("/api/table/:name/data", db_table_delete)
        .get("/api/table/:name/schema", db_table_schema)
        .put("/api/table/:name/schema", db_table_schema_update)
        .post("/api/query/raw", db_raw_query)
        .build();
    let event_loop_proxy = Arc::new(event_loop.create_proxy());
    let mysql_connection_pending = Arc::new(AtomicBool::new(false));
    let connection_generation = Arc::new(AtomicU64::new(0));

    #[allow(unused_mut)]
    let mut window_builder = WindowBuilder::new()
        .title("Sequel Explorer")
        .size(LogicalSize::new(1200.0, 768.0))
        .min_size(LogicalSize::new(800.0, 480.0))
        .background_color(background_color(event_loop.theme()))
        .center()
        .remember_window_state()
        .allow_file_drop(true);
    #[cfg(target_os = "macos")]
    {
        window_builder = window_builder.macos_titlebar_style(bwindow::MacosTitlebarStyle::Hidden);
    }
    let mut window = window_builder.build();

    let prefs = Preferences::new("nl.bplaat.SequelExplorer").expect("Failed to open preferences");
    let mut webview = WebviewBuilder::new(&window)
        .on_event(event_loop.create_proxy(), AppEvent::Webview)
        .load_rust_embed_with_custom_handler::<WebAssets>(move |req| {
            let res = router.handle(req);
            if res.status != Status::NotFound {
                Some(res)
            } else {
                None
            }
        })
        .build();

    #[cfg(target_os = "macos")]
    webview.add_user_script(
        format!(
            "document.documentElement.style.setProperty('--macos-titlebar-height', '{}px');",
            window.macos_titlebar_size().height
        ),
        bwebview::InjectionTime::DocumentStart,
    );

    let mut page_ready = false;
    let mut pending_menu_action: Option<String> = None;
    let mut pending_open_path = startup_path;
    let mut initial_restore_sent = false;
    event_loop.run(move |event| match event {
        Event::Window(_, bwindow::WindowEvent::ThemeChanged(theme)) => {
            window.set_background_color(background_color(theme));
        }
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::PageLoadStart)) => {
            page_ready = false
        }
        Event::OpenFiles(paths) => {
            if let Some(path) = paths.first() {
                let path = path.to_string_lossy().into_owned();
                if page_ready {
                    webview.send_ipc_message(
                        serde_json::to_string(&IpcMessage::OpenFile { path })
                            .expect("Failed to serialize open file message"),
                    );
                } else {
                    pending_open_path = Some(path);
                }
            }
        }
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::PageTitleChange(title))) => {
            window.set_title(title)
        }
        Event::Action(action) => {
            if page_ready {
                webview.send_ipc_message(
                    serde_json::to_string(&IpcMessage::MenuAction { action })
                        .expect("Failed to serialize menu action"),
                );
            } else {
                pending_menu_action = Some(action);
            }
        }
        Event::Window(_, bwindow::WindowEvent::DroppedFile(path)) => {
            let path = path.to_string_lossy().into_owned();
            if page_ready {
                webview.send_ipc_message(
                    serde_json::to_string(&IpcMessage::OpenFile { path })
                        .expect("Failed to serialize open file message"),
                );
            } else {
                pending_open_path = Some(path);
            }
        }
        #[cfg(target_os = "macos")]
        Event::Window(_, bwindow::WindowEvent::MacosFullscreenChange(is_fullscreen)) => {
            if is_fullscreen {
                webview.evaluate_script("document.body.classList.add('is-fullscreen');");
            } else {
                webview.evaluate_script("document.body.classList.remove('is-fullscreen');");
            }
        }
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::MessageReceive(message))) => {
            let message = Zeroizing::new(message);
            let message = match serde_json::from_str(&message) {
                Ok(message) => message,
                Err(error) => {
                    eprintln!("Ignoring invalid IPC message: {error}");
                    return;
                }
            };
            match message {
                IpcMessage::Ready => {
                    page_ready = true;
                    let values = Preference::ALL
                        .into_iter()
                        .filter_map(|preference| match prefs.get(preference.key()) {
                            Ok(Some(preferences::Value::String(value))) => {
                                Some((preference, value))
                            }
                            _ => None,
                        })
                        .collect();
                    webview.send_ipc_message(
                        serde_json::to_string(&IpcMessage::Preferences { values })
                            .expect("Failed to serialize preferences"),
                    );
                    if let Some(path) = pending_open_path.take() {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::OpenFile { path })
                                .expect("Failed to serialize open file message"),
                        );
                    } else if !initial_restore_sent {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::RestoreLastFile)
                                .expect("Failed to serialize restore last file message"),
                        );
                    }
                    initial_restore_sent = true;
                    if let Some(action) = pending_menu_action.take() {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::MenuAction { action })
                                .expect("Failed to serialize menu action"),
                        );
                    }
                }
                IpcMessage::SetPreference { key, value } => {
                    let result = match value {
                        Some(value) => prefs.set(key.key(), value),
                        None => prefs.remove(key.key()),
                    };
                    if let Err(error) = result {
                        eprintln!("Failed to save preference: {error}");
                    }
                }
                IpcMessage::ShowMessage {
                    request_id,
                    title,
                    description,
                    warning,
                } => {
                    MessageDialog::new()
                        .parent(&window)
                        .title(title)
                        .description(description)
                        .level(if warning {
                            MessageLevel::Warning
                        } else {
                            MessageLevel::Error
                        })
                        .show();
                    webview.send_ipc_message(
                        serde_json::to_string(&IpcMessage::ShowMessageResponse { request_id })
                            .expect("Failed to serialize response"),
                    );
                }
                IpcMessage::OpenFileDialog { request_id } => {
                    let path = FileDialog::new()
                        .parent(&window)
                        .title("Open SQLite Database")
                        .add_filter("SQLite databases", &["db", "sqlite", "sqlite3"])
                        .pick_file()
                        .map(|p| p.to_string_lossy().into_owned());
                    let response = IpcMessage::OpenFileDialogResponse { request_id, path };
                    webview.send_ipc_message(
                        serde_json::to_string(&response).expect("Failed to serialize response"),
                    );
                }
                IpcMessage::OpenDatabase { request_id, path } => {
                    let request_generation =
                        connection_generation.fetch_add(1, Ordering::AcqRel) + 1;
                    let result = Connection::open_sqlite(
                        &path,
                        SqliteMode::ReadWrite,
                        PoolOptions::single_connection(),
                    );
                    let (ok, error) = match result {
                        Ok(conn) => {
                            if replace_database_state_if_current(
                                &state,
                                &connection_generation,
                                request_generation,
                                DatabaseState::sqlite(conn),
                            ) {
                                (true, None)
                            } else {
                                (false, Some("Connection request was superseded".to_string()))
                            }
                        }
                        Err(e) => (false, Some(e.to_string())),
                    };
                    let response = IpcMessage::OpenDatabaseResponse {
                        request_id,
                        ok,
                        error,
                    };
                    webview.send_ipc_message(
                        serde_json::to_string(&response).expect("Failed to serialize response"),
                    );
                }
                IpcMessage::ImportSql { request_id } => {
                    let path = FileDialog::new()
                        .parent(&window)
                        .title("Import SQL")
                        .add_filter("SQL files", &["sql"])
                        .pick_file();
                    if let Some(path) = path {
                        let state = Arc::clone(&state);
                        let event_loop_proxy = Arc::clone(&event_loop_proxy);
                        std::thread::Builder::new()
                            .name("sql-import".to_string())
                            .spawn(move || {
                                let error = match std::fs::read_to_string(&path) {
                                    Ok(sql) => import_sql(&state, &sql).err(),
                                    Err(error) => {
                                        Some(format!("Failed to read {}: {error}", path.display()))
                                    }
                                };
                                let _ = event_loop_proxy.send_user_event(AppEvent::UserEvent(
                                    serde_json::to_string(&IpcMessage::ImportSqlResponse {
                                        request_id,
                                        cancelled: false,
                                        error,
                                    })
                                    .expect("Failed to serialize response"),
                                ));
                            })
                            .expect("Failed to spawn SQL import thread");
                    } else {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::ImportSqlResponse {
                                request_id,
                                cancelled: true,
                                error: None,
                            })
                            .expect("Failed to serialize response"),
                        );
                    }
                }
                IpcMessage::ExportSql {
                    request_id,
                    file_name,
                } => {
                    let path = FileDialog::new()
                        .parent(&window)
                        .title("Export SQL")
                        .file_name(file_name)
                        .add_filter("SQL files", &["sql"])
                        .save_file();
                    if let Some(mut path) = path {
                        if path.extension().is_none() {
                            path.set_extension("sql");
                        }
                        let state = Arc::clone(&state);
                        let event_loop_proxy = Arc::clone(&event_loop_proxy);
                        std::thread::Builder::new()
                            .name("sql-export".to_string())
                            .spawn(move || {
                                let error = export_sql(&state)
                                    .and_then(|sql| {
                                        std::fs::write(&path, sql).map_err(|error| {
                                            format!("Failed to write {}: {error}", path.display())
                                        })
                                    })
                                    .err();
                                let _ = event_loop_proxy.send_user_event(AppEvent::UserEvent(
                                    serde_json::to_string(&IpcMessage::ExportSqlResponse {
                                        request_id,
                                        cancelled: false,
                                        error,
                                    })
                                    .expect("Failed to serialize response"),
                                ));
                            })
                            .expect("Failed to spawn SQL export thread");
                    } else {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::ExportSqlResponse {
                                request_id,
                                cancelled: true,
                                error: None,
                            })
                            .expect("Failed to serialize response"),
                        );
                    }
                }
                IpcMessage::OpenMysql {
                    request_id,
                    transport,
                    host,
                    port,
                    socket,
                    user,
                    password,
                    tls,
                    remember,
                    previous_connection,
                } => {
                    if mysql_connection_pending.swap(true, Ordering::AcqRel) {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::OpenMysqlResponse {
                                request_id,
                                ok: false,
                                error: Some(
                                    "A MySQL connection is already being opened".to_string(),
                                ),
                                credential_saved: false,
                                credential_error: None,
                            })
                            .expect("Failed to serialize response"),
                        );
                        return;
                    }
                    let request = OpenMysqlRequest {
                        request_id,
                        connection_generation: connection_generation.fetch_add(1, Ordering::AcqRel)
                            + 1,
                        transport,
                        host,
                        port,
                        socket,
                        user,
                        password,
                        tls,
                        remember,
                        previous_connection,
                    };
                    let state = Arc::clone(&state);
                    let event_loop_proxy = Arc::clone(&event_loop_proxy);
                    let mysql_connection_pending = Arc::clone(&mysql_connection_pending);
                    let connection_generation = Arc::clone(&connection_generation);
                    std::thread::Builder::new()
                        .name("mysql-connection".to_string())
                        .spawn(move || {
                            let _pending_guard =
                                MysqlConnectionPendingGuard(mysql_connection_pending);
                            let request_id = request.request_id;
                            let response =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    open_mysql(request, &state, &connection_generation)
                                }))
                                .unwrap_or_else(|_| {
                                    IpcMessage::OpenMysqlResponse {
                                        request_id,
                                        ok: false,
                                        error: Some("Failed to open MySQL connection".to_string()),
                                        credential_saved: false,
                                        credential_error: None,
                                    }
                                });
                            let _ = event_loop_proxy.send_user_event(AppEvent::UserEvent(
                                serde_json::to_string(&response)
                                    .expect("Failed to serialize MySQL response"),
                            ));
                        })
                        .expect("Failed to spawn MySQL connection thread");
                }
                IpcMessage::RunQuery { request_id, sql } => {
                    let connection = query_connection(&state);
                    let event_loop_proxy = Arc::clone(&event_loop_proxy);
                    std::thread::Builder::new()
                        .name("query".to_string())
                        .spawn(move || {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    connection.and_then(|connection| run_query(&connection, &sql))
                                }))
                                .unwrap_or_else(|_| Err("Failed to run query".to_string()));
                            let response = IpcMessage::RunQueryResponse {
                                request_id,
                                result: match result {
                                    Ok(result) => QueryResponse::Success(result),
                                    Err(error) => QueryResponse::Error { error },
                                },
                            };
                            let _ = event_loop_proxy.send_user_event(AppEvent::UserEvent(
                                serde_json::to_string(&response)
                                    .expect("Failed to serialize query response"),
                            ));
                        })
                        .expect("Failed to spawn query thread");
                }
                IpcMessage::SelectMysqlDatabase {
                    request_id,
                    database,
                } => {
                    let request_generation =
                        connection_generation.fetch_add(1, Ordering::AcqRel) + 1;
                    let settings = state.lock().expect("mutex poisoned").mysql_settings();
                    let result = settings
                        .ok_or_else(|| "No MySQL connection open".to_string())
                        .and_then(|settings| {
                            settings
                                .connect(Some(&database))
                                .map(|connection| (connection, settings))
                        });
                    let (ok, error) = match result {
                        Ok((connection, settings)) => {
                            if replace_database_state_if_current(
                                &state,
                                &connection_generation,
                                request_generation,
                                DatabaseState::mysql(connection, settings, database),
                            ) {
                                (true, None)
                            } else {
                                (false, Some("Connection request was superseded".to_string()))
                            }
                        }
                        Err(error) => (false, Some(error)),
                    };
                    webview.send_ipc_message(
                        serde_json::to_string(&IpcMessage::SelectMysqlDatabaseResponse {
                            request_id,
                            ok,
                            error,
                        })
                        .expect("Failed to serialize response"),
                    );
                }
                _ => {}
            }
        }
        Event::UserEvent(AppEvent::UserEvent(message)) => webview.send_ipc_message(message),
        _ => {}
    });
}
