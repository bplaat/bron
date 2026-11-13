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
mod sql;
mod sql_transfer;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use bsql::{Connection, PoolOptions, SqliteMode};
use bwebview::{WebviewBuilder, WebviewEvent};
use bwindow::{
    Accelerator, Event, EventLoopBuilder, EventLoopProxy, FileDialog, KeyCode, LogicalSize,
    MenuBarBuilder, MenuBuilder, MenuItem, MessageButtons, MessageDialog, MessageDialogResult,
    MessageLevel, Modifiers, Theme, WindowBuilder,
};
use preferences::Preferences;
use rust_embed::Embed;
use serde_json::json;
use small_http::{Method, Request, Status};
use small_router::{Router, RouterBuilder};
use zeroize::Zeroizing;

use crate::database::{
    DatabaseState, MysqlConnectionPendingGuard, OpenMysqlRequest, QueryResponse, RawQueryResponse,
    State, db_databases, db_databases_create, db_databases_drop, db_table_data, db_table_delete,
    db_table_insert, db_table_update, db_tables, db_users, db_users_create, db_users_delete,
    db_users_update, error_response, open_mysql, query_connection,
    replace_database_state_if_current, run_query, run_raw_query,
};
use crate::ipc::{IpcMessage, MessageKind, Preference};
use crate::schema::{
    db_table_operation, db_table_schema, db_table_schema_update, db_tables_create,
};
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

fn build_router(state: State) -> Router<State> {
    RouterBuilder::<State>::with(state)
        .get("/api/databases", db_databases)
        .post("/api/databases", db_databases_create)
        .delete("/api/databases", db_databases_drop)
        .get("/api/users", db_users)
        .post("/api/users", db_users_create)
        .put("/api/users", db_users_update)
        .delete("/api/users", db_users_delete)
        .get("/api/tables", db_tables)
        .post("/api/tables", db_tables_create)
        .get("/api/table/:name/data", db_table_data)
        .post("/api/table/:name/data", db_table_insert)
        .put("/api/table/:name/data", db_table_update)
        .delete("/api/table/:name/data", db_table_delete)
        .get("/api/table/:name/schema", db_table_schema)
        .put("/api/table/:name/schema", db_table_schema_update)
        .post("/api/table/:name/operation", db_table_operation)
        .error(|_, _, error| error_response(error))
        .build()
}

struct ApiRequest {
    request_id: u64,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
}

/// Runs API requests one at a time in order, like the event loop did, but off the UI thread
fn spawn_api_worker(
    router: Router<State>,
    event_loop_proxy: Arc<EventLoopProxy<AppEvent>>,
) -> mpsc::Sender<ApiRequest> {
    let (sender, receiver) = mpsc::channel::<ApiRequest>();
    thread::Builder::new()
        .name("api".to_string())
        .spawn(move || {
            for request in receiver {
                let data = catch_unwind(AssertUnwindSafe(|| {
                    handle_api_request(&router, &request.method, &request.path, request.body)
                }))
                .unwrap_or_else(|_| json!({ "error": "The request failed" }));
                let _ = event_loop_proxy.send_user_event(AppEvent::UserEvent(
                    serde_json::to_string(&IpcMessage::ApiRequestResponse {
                        request_id: request.request_id,
                        data,
                    })
                    .expect("Failed to serialize API response"),
                ));
            }
        })
        .expect("Failed to spawn API thread");
    sender
}

fn handle_api_request(
    router: &Router<State>,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> serde_json::Value {
    let Ok(method) = method.parse::<Method>() else {
        return json!({ "error": "Invalid request method" });
    };
    if !path.starts_with("/api/") {
        return json!({ "error": "Invalid request path" });
    }
    let mut request = Request::with_method_and_url(method, format!("http://localhost{path}"));
    request.body = body.map(|body| body.to_string().into_bytes());
    let response = router.handle(&request);
    if response.status == Status::NotFound {
        return json!({ "error": "Not found" });
    }
    serde_json::from_slice(&response.body)
        .unwrap_or_else(|_| json!({ "error": "Invalid response from the database layer" }))
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
                        .item(
                            MenuItem::new("New Table...", "newTable")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyN)),
                        )
                        .item(MenuItem::new("New Database...", "newDatabase"))
                        .separator()
                        .item(MenuItem::new("Import SQL...", "importSql"))
                        .item(MenuItem::new("Export SQL...", "exportSql"))
                        .separator(),
                )
                .menu(
                    MenuBuilder::new("Database")
                        .item(MenuItem::new("Delete Database...", "dropDatabase"))
                        .separator()
                        .item(MenuItem::new("Manage Users...", "manageUsers")),
                )
                .menu(
                    MenuBuilder::new("Table")
                        .item(MenuItem::new("Duplicate Table...", "duplicateTable"))
                        .item(MenuItem::new("Truncate Table...", "truncateTable"))
                        .item(MenuItem::new("Delete Table...", "dropTable")),
                )
                .menu(
                    MenuBuilder::new("Row")
                        .item(
                            MenuItem::new("Add Row...", "addRow").accelerator(Accelerator::new(
                                Modifiers::PRIMARY | Modifiers::SHIFT,
                                KeyCode::KeyN,
                            )),
                        )
                        .item(
                            MenuItem::new("Edit Row...", "editRow")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyE)),
                        )
                        .item(
                            MenuItem::new("Duplicate Rows", "duplicateRows")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyD)),
                        )
                        .item(MenuItem::new("Delete Rows...", "deleteRows"))
                        .separator()
                        .item(MenuItem::new("Copy Rows", "copyRows"))
                        .item(
                            MenuItem::new("Copy Rows as SQL INSERT", "copyRowsAsInsert")
                                .accelerator(Accelerator::new(
                                    Modifiers::PRIMARY | Modifiers::SHIFT,
                                    KeyCode::KeyC,
                                )),
                        ),
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
                        )
                        .separator()
                        .item(
                            MenuItem::new("Refresh", "refresh")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyR)),
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

    let router = build_router(Arc::clone(&state));
    let event_loop_proxy = Arc::new(event_loop.create_proxy());
    let api_requests = spawn_api_worker(router, Arc::clone(&event_loop_proxy));
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
        // The page draws its own titlebar, the window buttons are centered in its 42px top bars
        // with the same 14px inset on the left
        window_builder = window_builder
            .macos_titlebar_style(bwindow::MacosTitlebarStyle::Hidden)
            .macos_traffic_light_position(bwindow::LogicalPoint::new(14.0, 14.0));
    }
    let mut window = window_builder.build();

    let prefs = Preferences::new("nl.bplaat.SequelExplorer").expect("Failed to open preferences");
    let mut webview = WebviewBuilder::new(&window)
        .on_event(event_loop.create_proxy(), AppEvent::Webview)
        .load_rust_embed::<WebAssets>()
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
                #[cfg(target_os = "macos")]
                IpcMessage::WindowDrag => window.macos_start_window_drag(),
                #[cfg(target_os = "macos")]
                IpcMessage::WindowDoubleClick => window.macos_perform_titlebar_double_click(),
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
                    level,
                    confirm,
                } => {
                    let mut dialog = MessageDialog::new()
                        .parent(&window)
                        .title(title)
                        .description(description)
                        .level(match level {
                            MessageKind::Info => MessageLevel::Info,
                            MessageKind::Warning => MessageLevel::Warning,
                            MessageKind::Error => MessageLevel::Error,
                        });
                    if let Some(label) = &confirm {
                        dialog = dialog.buttons(MessageButtons::OkCancelCustom(
                            label.clone(),
                            "Cancel".to_string(),
                        ));
                    }
                    let result = dialog.show();
                    let confirmed = match &confirm {
                        Some(label) => result == MessageDialogResult::Custom(label.clone()),
                        None => true,
                    };
                    webview.send_ipc_message(
                        serde_json::to_string(&IpcMessage::ShowMessageResponse {
                            request_id,
                            confirmed,
                        })
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
                        thread::Builder::new()
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
                        thread::Builder::new()
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
                    thread::Builder::new()
                        .name("mysql-connection".to_string())
                        .spawn(move || {
                            let _pending_guard =
                                MysqlConnectionPendingGuard(mysql_connection_pending);
                            let request_id = request.request_id;
                            let response = catch_unwind(AssertUnwindSafe(|| {
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
                    thread::Builder::new()
                        .name("query".to_string())
                        .spawn(move || {
                            let result = catch_unwind(AssertUnwindSafe(|| {
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
                IpcMessage::ApiRequest {
                    request_id,
                    method,
                    path,
                    body,
                } => {
                    let _ = api_requests.send(ApiRequest {
                        request_id,
                        method,
                        path,
                        body,
                    });
                }
                IpcMessage::RunRawQuery { request_id, sql } => {
                    let connection = query_connection(&state);
                    let state = Arc::clone(&state);
                    let event_loop_proxy = Arc::clone(&event_loop_proxy);
                    thread::Builder::new()
                        .name("raw-query".to_string())
                        .spawn(move || {
                            let result = catch_unwind(AssertUnwindSafe(|| {
                                connection.and_then(|connection| run_raw_query(&connection, &sql))
                            }))
                            .unwrap_or_else(|_| Err("Failed to run query".to_string()));
                            // Raw statements can change any table, so forget cached table metadata
                            if let Ok(mut state) = state.lock() {
                                state.clear_table_metadata();
                            }
                            let response = IpcMessage::RunRawQueryResponse {
                                request_id,
                                result: match result {
                                    Ok(result) => RawQueryResponse::Success(result),
                                    Err(error) => RawQueryResponse::Error { error },
                                },
                            };
                            let _ = event_loop_proxy.send_user_event(AppEvent::UserEvent(
                                serde_json::to_string(&response)
                                    .expect("Failed to serialize query response"),
                            ));
                        })
                        .expect("Failed to spawn raw query thread");
                }
                IpcMessage::SelectMysqlDatabase {
                    request_id,
                    database,
                } => {
                    let request_generation =
                        connection_generation.fetch_add(1, Ordering::AcqRel) + 1;
                    let state = Arc::clone(&state);
                    let event_loop_proxy = Arc::clone(&event_loop_proxy);
                    let connection_generation = Arc::clone(&connection_generation);
                    // Connecting and waiting for the state lock can block, so keep it off the UI thread
                    thread::Builder::new()
                        .name("mysql-select-database".to_string())
                        .spawn(move || {
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
                                        (
                                            false,
                                            Some("Connection request was superseded".to_string()),
                                        )
                                    }
                                }
                                Err(error) => (false, Some(error)),
                            };
                            let _ = event_loop_proxy.send_user_event(AppEvent::UserEvent(
                                serde_json::to_string(&IpcMessage::SelectMysqlDatabaseResponse {
                                    request_id,
                                    ok,
                                    error,
                                })
                                .expect("Failed to serialize response"),
                            ));
                        })
                        .expect("Failed to spawn MySQL database thread");
                }
                _ => {}
            }
        }
        Event::UserEvent(AppEvent::UserEvent(message)) => webview.send_ipc_message(message),
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_requests_carry_methods_and_bodies_over_ipc() {
        let connection = Connection::open_sqlite_memory().expect("open test database");
        connection
            .execute("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT)", ())
            .expect("create fixture");
        let router = build_router(Arc::new(Mutex::new(DatabaseState::sqlite(connection))));

        let inserted = handle_api_request(
            &router,
            "POST",
            "/api/table/items/data",
            Some(json!({ "rows": [{ "id": null, "name": { "kind": "text", "value": "a" } }] })),
        );
        assert_eq!(inserted["inserted"], 1);

        let data = handle_api_request(&router, "GET", "/api/table/items/data?sort=id", None);
        assert_eq!(data["rows"][0][1]["value"], "a");

        assert_eq!(
            handle_api_request(&router, "GET", "/api/missing", None)["error"],
            "Not found"
        );
        assert_eq!(
            handle_api_request(&router, "GET", "/index.html", None)["error"],
            "Invalid request path"
        );
    }
}
