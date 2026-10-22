/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(unused)]

use std::ffi::{c_char, c_void};

// MARK: GTK
#[repr(C)]
pub(crate) struct GError([u8; 0]);
#[repr(C)]
pub(crate) struct GVariantType([u8; 0]);
#[repr(C)]
pub(crate) struct GdkDisplay([u8; 0]);
#[repr(C)]
pub(crate) struct GInputStream([u8; 0]);
#[repr(C)]
pub(crate) struct GObject([u8; 0]);
#[repr(C)]
pub(crate) struct GSimpleAction([u8; 0]);
#[repr(C)]
pub(crate) struct GVariant([u8; 0]);
pub(crate) const G_CONNECT_DEFAULT: i32 = 0;
#[repr(C)]
pub(crate) struct GdkDragContext([u8; 0]);
#[repr(C)]
pub(crate) struct GdkRGBA {
    pub(crate) red: f64,
    pub(crate) green: f64,
    pub(crate) blue: f64,
    pub(crate) alpha: f64,
}
#[repr(C)]
pub(crate) struct GtkSelectionData([u8; 0]);
#[repr(C)]
pub(crate) struct GtkWidget([u8; 0]);
#[repr(C)]
pub(crate) struct GtkWindow([u8; 0]);

unsafe extern "C" {
    pub(crate) fn g_action_get_name(action: *mut GSimpleAction) -> *const c_char;
    pub(crate) fn g_action_map_add_action(map: *mut c_void, action: *mut GSimpleAction);
    pub(crate) fn g_action_map_lookup_action(
        map: *mut c_void,
        name: *const c_char,
    ) -> *mut GSimpleAction;
    pub(crate) fn g_input_stream_read_all(
        stream: *mut GInputStream,
        buffer: *mut c_void,
        count: usize,
        bytes_read: *mut usize,
        cancellable: *mut c_void,
        error: *mut *mut GError,
    ) -> bool;
    pub(crate) fn g_memory_input_stream_new_from_data(
        data: *const c_void,
        len: usize,
        destroy: *const c_void,
    ) -> *mut GInputStream;
    pub(crate) fn g_object_new(
        object_type: *mut c_void,
        first_property_name: *const c_char,
        ...
    ) -> *mut GObject;
    pub(crate) fn g_object_ref(object: *mut GObject) -> *mut GObject;
    pub(crate) fn g_object_unref(object: *mut GObject);
    pub(crate) fn g_signal_connect_data(
        instance: *mut GObject,
        detailed_signal: *const c_char,
        c_handler: *const c_void,
        data: *const c_void,
        destroy_data: *const c_void,
        connect_flags: i32,
    );
    pub(crate) fn g_signal_connect_object(
        instance: *mut GObject,
        detailed_signal: *const c_char,
        handler: *const c_void,
        object: *mut GObject,
        flags: i32,
    );
    pub(crate) fn g_signal_handlers_disconnect_matched(
        instance: *mut GObject,
        mask: u32,
        signal_id: u32,
        detail: u32,
        closure: *mut c_void,
        func: *mut c_void,
        data: *mut c_void,
    ) -> u32;
    pub(crate) fn g_signal_lookup(name: *const c_char, type_: usize) -> u32;
    pub(crate) fn g_simple_action_group_new() -> *mut c_void;
    pub(crate) fn g_simple_action_new(
        name: *const c_char,
        parameter_type: *const GVariantType,
    ) -> *mut GSimpleAction;
    pub(crate) fn g_simple_action_set_enabled(action: *mut GSimpleAction, enabled: i32);
    pub(crate) fn g_type_from_name(name: *const c_char) -> usize;
    pub(crate) fn g_type_check_instance_is_a(instance: *mut c_void, type_: usize) -> i32;
    pub(crate) fn gdk_display_get_default() -> *mut GdkDisplay;
    pub(crate) fn gtk_container_add(container: *mut GtkWidget, widget: *mut GtkWidget);
    pub(crate) fn gtk_drag_finish(
        context: *mut GdkDragContext,
        success: i32,
        delete: i32,
        time: u32,
    );
    pub(crate) fn gtk_show_uri_on_window(
        parent: *mut GtkWindow,
        uri: *const c_char,
        timestamp: u32,
        error: *mut *mut GError,
    );
    pub(crate) fn gtk_widget_destroy(widget: *mut GtkWidget);
    pub(crate) fn gtk_widget_get_action_group(
        widget: *mut GtkWidget,
        prefix: *const c_char,
    ) -> *mut c_void;
    pub(crate) fn gtk_widget_hide(widget: *mut GtkWidget);
    pub(crate) fn gtk_widget_insert_action_group(
        widget: *mut GtkWidget,
        prefix: *const c_char,
        group: *mut c_void,
    );
    pub(crate) fn gtk_widget_show_all(window: *mut GtkWidget);
    pub(crate) fn gtk_widget_show(widget: *mut GtkWidget);
}

pub(crate) unsafe fn gdk_display_is_wayland(display: *mut GdkDisplay) -> bool {
    let type_ = unsafe { g_type_from_name(c"GdkWaylandDisplay".as_ptr()) };
    type_ != 0 && unsafe { g_type_check_instance_is_a(display.cast(), type_) != 0 }
}

// MARK: Soup
#[repr(C)]
pub(crate) struct SoupMessageHeaders([u8; 0]);
pub(crate) const SOUP_MESSAGE_HEADERS_RESPONSE: i32 = 1;
// WebKitGTK 4.0 uses libsoup 2.4; WebKitGTK 4.1 uses libsoup 3.0.
unsafe extern "C" {
    pub(crate) fn soup_message_headers_new(r#type: i32) -> *mut SoupMessageHeaders;
    pub(crate) fn soup_message_headers_foreach(
        headers: *mut SoupMessageHeaders,
        func: extern "C" fn(name: *const c_char, value: *const c_char, user_data: *mut c_void),
        user_data: *mut c_void,
    );
    pub(crate) fn soup_message_headers_append(
        headers: *mut SoupMessageHeaders,
        name: *const c_char,
        value: *const c_char,
    );
}

// MARK: WebKitGTK
#[repr(C)]
pub(crate) struct WebKitEditorState([u8; 0]);
#[repr(C)]
pub(crate) struct WebKitWebContext([u8; 0]);
#[repr(C)]
pub(crate) struct WebKitWebView([u8; 0]);
#[repr(C)]
pub(crate) struct WebkitSettings([u8; 0]);
#[repr(C)]
pub(crate) struct WebKitNavigationPolicyDecision([u8; 0]);
#[repr(C)]
pub(crate) struct WebKitURIRequest([u8; 0]);
#[repr(C)]
pub(crate) struct WebKitUserContentManager([u8; 0]);
#[repr(C)]
pub(crate) struct WebKitUserScript([u8; 0]);
#[repr(C)]
pub(crate) struct WebKitJavascriptResult([u8; 0]);
#[repr(C)]
pub(crate) struct WebKitURISchemeRequest([u8; 0]);

// WebKitGTK 4.1
#[repr(C)]
pub(crate) struct WebKitURISchemeResponse([u8; 0]);

pub(crate) const WEBKIT_LOAD_STARTED: i32 = 1;
pub(crate) const WEBKIT_LOAD_FINISHED: i32 = 3;
pub(crate) const WEBKIT_POLICY_DECISION_TYPE_NEW_WINDOW_ACTION: i32 = 1;
pub(crate) const WEBKIT_USER_CONTENT_INJECT_TOP_FRAME: i32 = 1;
pub(crate) const WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START: i32 = 0;
pub(crate) const WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_END: i32 = 1;
unsafe extern "C" {
    pub(crate) fn webkit_web_context_get_default() -> *mut WebKitWebContext;
    pub(crate) fn webkit_web_context_register_uri_scheme(
        context: *mut WebKitWebContext,
        scheme: *const c_char,
        callback: *const c_void,
        user_data: *mut c_void,
        user_data_destroy_func: *const c_void,
    );

    pub(crate) fn webkit_uri_scheme_request_get_uri(
        request: *mut WebKitURISchemeRequest,
    ) -> *const c_char;
    pub(crate) fn webkit_web_view_stop_loading(webview: *mut WebKitWebView);
    pub(crate) fn webkit_web_view_get_editor_state(
        webview: *mut WebKitWebView,
    ) -> *mut WebKitEditorState;
    pub(crate) fn webkit_editor_state_is_undo_available(state: *mut WebKitEditorState) -> i32;
    pub(crate) fn webkit_editor_state_is_redo_available(state: *mut WebKitEditorState) -> i32;
    pub(crate) fn webkit_editor_state_is_cut_available(state: *mut WebKitEditorState) -> i32;
    pub(crate) fn webkit_editor_state_is_copy_available(state: *mut WebKitEditorState) -> i32;
    pub(crate) fn webkit_editor_state_is_paste_available(state: *mut WebKitEditorState) -> i32;
    pub(crate) fn webkit_web_view_execute_editing_command(
        webview: *mut WebKitWebView,
        command: *const c_char,
    );
    pub(crate) fn webkit_web_view_get_type() -> *mut c_void;
    pub(crate) fn webkit_web_view_get_settings(web_view: *mut WebKitWebView)
    -> *mut WebkitSettings;
    pub(crate) fn webkit_web_view_load_uri(web_view: *mut WebKitWebView, uri: *const c_char);
    pub(crate) fn webkit_web_view_load_html(
        web_view: *mut WebKitWebView,
        content: *const c_char,
        base_uri: *const c_char,
    );
    pub(crate) fn webkit_web_view_get_title(web_view: *mut WebKitWebView) -> *mut c_char;
    pub(crate) fn webkit_web_view_get_uri(web_view: *mut WebKitWebView) -> *mut c_char;
    pub(crate) fn webkit_navigation_policy_decision_get_request(
        decision: *mut WebKitNavigationPolicyDecision,
    ) -> *mut WebKitURIRequest;
    pub(crate) fn webkit_uri_request_get_uri(request: *mut WebKitURIRequest) -> *const c_char;
    pub(crate) fn webkit_user_content_manager_new() -> *mut WebKitUserContentManager;
    pub(crate) fn webkit_web_view_get_user_content_manager(
        web_view: *mut WebKitWebView,
    ) -> *mut WebKitUserContentManager;
    pub(crate) fn webkit_user_script_new(
        source: *const c_char,
        injected_frames: i32,
        injection_time: i32,
        whitelist: *const *const c_char,
        blacklist: *const *const c_char,
    ) -> *mut WebKitUserScript;
    pub(crate) fn webkit_user_content_manager_add_script(
        manager: *mut WebKitUserContentManager,
        script: *mut WebKitUserScript,
    );
    pub(crate) fn webkit_user_content_manager_register_script_message_handler(
        manager: *mut WebKitUserContentManager,
        name: *const c_char,
    );
    pub(crate) fn webkit_settings_set_user_agent(
        settings: *mut WebkitSettings,
        user_agent: *const c_char,
    );
    pub(crate) fn webkit_settings_set_enable_developer_extras(
        settings: *mut WebkitSettings,
        enable: bool,
    );
    pub(crate) fn webkit_web_view_set_background_color(
        web_view: *mut WebKitWebView,
        color: *const GdkRGBA,
    );

    // WebKitGTK 4.0 before 2.22
    pub(crate) fn webkit_javascript_result_get_global_context(
        result: *mut WebKitJavascriptResult,
    ) -> *mut c_void;
    pub(crate) fn webkit_javascript_result_get_value(
        result: *mut WebKitJavascriptResult,
    ) -> *mut c_void;

    // WebKitGTK 4.0
    pub(crate) fn webkit_uri_scheme_request_finish(
        request: *mut WebKitURISchemeRequest,
        stream: *mut GInputStream,
        stream_length: i64,
        content_type: *const c_char,
    );
    pub(crate) fn webkit_web_view_run_javascript(
        web_view: *mut WebKitWebView,
        script: *const c_char,
        cancellable: *const c_void,
        callback: *const c_void,
        user_data: *const c_void,
    );

    // WebKitGTK 4.0 2.22+ and WebKitGTK 4.1
    pub(crate) fn webkit_javascript_result_get_js_value(
        result: *mut WebKitJavascriptResult,
    ) -> *mut c_void;

    // WebKitGTK 4.1
    pub(crate) fn webkit_uri_scheme_response_new(
        input_stream: *mut GInputStream,
        stream_length: i64,
    ) -> *mut WebKitURISchemeResponse;
    pub(crate) fn webkit_uri_scheme_response_set_status(
        response: *mut WebKitURISchemeResponse,
        status_code: u32,
        reason_phrase: *const c_char,
    );
    pub(crate) fn webkit_uri_scheme_response_set_http_headers(
        response: *mut WebKitURISchemeResponse,
        headers: *mut SoupMessageHeaders,
    );
    pub(crate) fn webkit_uri_scheme_response_set_content_type(
        response: *mut WebKitURISchemeResponse,
        content_type: *const c_char,
    );
    pub(crate) fn webkit_uri_scheme_request_get_http_method(
        request: *mut WebKitURISchemeRequest,
    ) -> *const c_char;
    pub(crate) fn webkit_uri_scheme_request_get_http_headers(
        request: *mut WebKitURISchemeRequest,
    ) -> *mut SoupMessageHeaders;
    pub(crate) fn webkit_uri_scheme_request_get_http_body(
        request: *mut WebKitURISchemeRequest,
    ) -> *mut GInputStream;
    pub(crate) fn webkit_uri_scheme_request_finish_with_response(
        request: *mut WebKitURISchemeRequest,
        response: *mut WebKitURISchemeResponse,
    );
    pub(crate) fn webkit_web_view_evaluate_javascript(
        web_view: *mut WebKitWebView,
        script: *const c_char,
        length: usize,
        world_name: *const c_char,
        source_uri: *const c_char,
        cancellable: *const c_void,
        callback: *const c_void,
        user_data: *const c_void,
    );
}

unsafe extern "C" {
    // WebKitGTK 4.0 before 2.22
    pub(crate) fn JSValueToStringCopy(
        ctx: *mut c_void,
        value: *mut c_void,
        exception: *mut c_void,
    ) -> *mut c_void;
    pub(crate) fn JSStringGetMaximumUTF8CStringSize(string: *mut c_void) -> usize;
    pub(crate) fn JSStringGetUTF8CString(
        string: *mut c_void,
        buffer: *mut c_char,
        buffer_size: usize,
    ) -> usize;
    pub(crate) fn JSStringRelease(string: *mut c_void);

    // WebKitGTK 4.0 2.22+ and WebKitGTK 4.1
    pub(crate) fn jsc_value_to_string(value: *mut c_void) -> *const c_char;
}
