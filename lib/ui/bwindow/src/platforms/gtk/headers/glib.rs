/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]
#![allow(unused)]

use std::ffi::{c_char, c_void};

// MARK: GObject
#[repr(C)]
pub struct GObject([u8; 0]);
pub const G_CONNECT_DEFAULT: i32 = 0;
unsafe extern "C" {
    #[cfg(test)]
    pub fn g_object_notify(object: *mut GObject, property: *const c_char);
    pub fn g_object_new(
        object_type: *mut c_void,
        first_property_name: *const c_char,
        ...
    ) -> *mut GObject;
    pub fn g_object_get(instance: *mut GObject, first_property_name: *const c_char, ...);
    pub fn g_object_set(instance: *mut GObject, first_property_name: *const c_char, ...);
    pub fn g_signal_connect_data(
        instance: *mut GObject,
        detailed_signal: *const c_char,
        c_handler: *const c_void,
        data: *const c_void,
        destroy_data: *const c_void,
        connect_flags: i32,
    );
    pub fn g_object_unref(object: *mut GObject);
    pub fn g_object_ref(object: *mut GObject) -> *mut GObject;
    pub fn g_type_from_name(name: *const c_char) -> usize;
    pub fn g_type_check_instance_is_a(instance: *mut c_void, type_: usize) -> i32;
    pub fn g_signal_handlers_disconnect_matched(
        instance: *mut GObject,
        mask: u32,
        signal_id: u32,
        detail: u32,
        closure: *mut c_void,
        func: *mut c_void,
        data: *mut c_void,
    ) -> u32;
}

// MARK: GLib
#[repr(C)]
pub struct GError([u8; 0]);
#[repr(C)]
pub struct GKeyFile([u8; 0]);
#[repr(C)]
pub struct GVariant([u8; 0]);
#[repr(C)]
pub struct GVariantBuilder([u8; 0]);
#[repr(C)]
pub struct GVariantType([u8; 0]);
#[repr(C)]
pub struct GSList {
    pub data: *mut c_void,
    pub next: *mut GSList,
}
unsafe extern "C" {
    pub fn g_set_prgname(prgname: *const c_char);
    pub fn g_idle_add_full(
        priority: i32,
        function: extern "C" fn(*mut c_void) -> i32,
        data: *mut c_void,
        notify: extern "C" fn(*mut c_void),
    ) -> u32;
    pub fn g_source_remove(tag: u32) -> i32;
    pub fn g_error_free(error: *mut GError);
    pub fn g_key_file_new() -> *mut GKeyFile;
    pub fn g_key_file_load_from_file(
        key_file: *mut GKeyFile,
        file: *const c_char,
        flags: i32,
        error: *mut *mut GError,
    ) -> bool;
    pub fn g_key_file_get_integer(
        key_file: *mut GKeyFile,
        group_name: *const c_char,
        key: *const c_char,
        error: *mut *mut c_void,
    ) -> i32;
    pub fn g_key_file_get_boolean(
        key_file: *mut GKeyFile,
        group_name: *const c_char,
        key: *const c_char,
        error: *mut *mut c_void,
    ) -> bool;
    pub fn g_key_file_set_integer(
        key_file: *mut GKeyFile,
        group_name: *const c_char,
        key: *const c_char,
        value: i32,
    );
    pub fn g_key_file_set_boolean(
        key_file: *mut GKeyFile,
        group_name: *const c_char,
        key: *const c_char,
        value: bool,
    );
    pub fn g_key_file_save_to_file(
        key_file: *mut GKeyFile,
        file: *const c_char,
        error: *mut *mut c_void,
    ) -> bool;
    pub fn g_key_file_free(key_file: *mut GKeyFile);
    pub fn g_idle_add(function: extern "C" fn(*mut c_void) -> i32, data: *mut c_void) -> u32;
    pub fn g_free(mem: *mut c_void);
    pub fn g_filename_from_uri(
        uri: *const c_char,
        hostname: *mut *mut c_char,
        error: *mut *mut GError,
    ) -> *mut c_char;
    pub fn g_strfreev(str_array: *mut *mut c_char);
    pub fn g_slist_free_full(list: *mut GSList, free_func: unsafe extern "C" fn(*mut c_void));
    pub fn g_variant_type_new(type_string: *const c_char) -> *mut GVariantType;
    pub fn g_variant_type_free(type_: *mut GVariantType);
    pub fn g_variant_builder_new(type_: *const GVariantType) -> *mut GVariantBuilder;
    pub fn g_variant_builder_add(builder: *mut GVariantBuilder, format_string: *const c_char, ...);
    pub fn g_variant_builder_unref(builder: *mut GVariantBuilder);
    pub fn g_variant_new(format_string: *const c_char, ...) -> *mut GVariant;
    pub fn g_variant_new_boolean(value: i32) -> *mut GVariant;
    pub fn g_variant_new_double(value: f64) -> *mut GVariant;
    pub fn g_variant_ref_sink(value: *mut GVariant) -> *mut GVariant;
    pub fn g_variant_unref(value: *mut GVariant);
}

// MARK: GIO
#[repr(C)]
pub struct GDBusConnection([u8; 0]);
#[repr(C)]
pub struct GDBusInterfaceInfo([u8; 0]);
#[repr(C)]
pub struct GDBusMethodInvocation([u8; 0]);
#[repr(C)]
pub struct GDBusNodeInfo([u8; 0]);
pub const G_BUS_TYPE_SESSION: i32 = 2;
pub type GDBusMethodCallFunc = extern "C" fn(
    *mut GDBusConnection,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut GVariant,
    *mut GDBusMethodInvocation,
    *mut c_void,
);
#[repr(C)]
pub struct GDBusInterfaceVTable {
    pub method_call: Option<GDBusMethodCallFunc>,
    pub get_property: *const c_void,
    pub set_property: *const c_void,
}
#[repr(C)]
pub struct GInputStream([u8; 0]);
#[repr(C)]
pub struct GApplication([u8; 0]);
#[repr(C)]
pub struct GFile([u8; 0]);
#[repr(C)]
pub struct GSimpleAction([u8; 0]);
pub const G_APPLICATION_HANDLES_OPEN: u32 = 1 << 2;
pub const G_APPLICATION_NON_UNIQUE: u32 = 1 << 5;

#[repr(C)]
pub struct GMenu([u8; 0]);
#[repr(C)]
pub struct GMenuModel([u8; 0]);

#[repr(C)]
pub struct GMenuItem([u8; 0]);
unsafe extern "C" {
    pub fn g_application_id_is_valid(id: *const c_char) -> i32;
    pub fn g_application_register(
        application: *mut GApplication,
        cancellable: *mut c_void,
        error: *mut *mut GError,
    ) -> i32;
    pub fn g_application_get_is_remote(application: *mut GApplication) -> i32;
    pub fn g_application_run(
        application: *mut GApplication,
        argc: i32,
        argv: *mut *mut c_char,
    ) -> i32;
    pub fn g_application_quit(application: *mut GApplication);
    pub fn g_file_get_path(file: *mut GFile) -> *mut c_char;
    pub fn g_simple_action_new(
        name: *const c_char,
        parameter_type: *const GVariantType,
    ) -> *mut GSimpleAction;
    pub fn g_action_map_add_action(map: *mut c_void, action: *mut GSimpleAction);
    #[cfg(test)]
    pub fn g_menu_model_get_n_items(model: *mut GMenuModel) -> i32;
    pub fn g_menu_new() -> *mut GMenu;
    pub fn g_menu_append_section(menu: *mut GMenu, label: *const c_char, section: *mut GMenuModel);
    pub fn g_menu_append_submenu(menu: *mut GMenu, label: *const c_char, submenu: *mut GMenuModel);
    pub fn g_menu_item_new(label: *const c_char, action: *const c_char) -> *mut GMenuItem;
    pub fn g_menu_item_set_attribute(
        item: *mut GMenuItem,
        attribute: *const c_char,
        format: *const c_char,
        ...
    );
    pub fn g_menu_append_item(menu: *mut GMenu, item: *mut GMenuItem);
    pub fn g_bus_get_sync(
        bus_type: i32,
        cancellable: *mut c_void,
        error: *mut *mut GError,
    ) -> *mut GDBusConnection;
    pub fn g_dbus_connection_emit_signal(
        connection: *mut GDBusConnection,
        destination_bus_name: *const c_char,
        object_path: *const c_char,
        interface_name: *const c_char,
        signal_name: *const c_char,
        parameters: *mut GVariant,
        error: *mut *mut GError,
    ) -> i32;
    pub fn g_bus_watch_name_on_connection(
        connection: *mut GDBusConnection,
        name: *const c_char,
        flags: u32,
        name_appeared_handler: Option<
            extern "C" fn(*mut GDBusConnection, *const c_char, *const c_char, *mut c_void),
        >,
        name_vanished_handler: Option<
            extern "C" fn(*mut GDBusConnection, *const c_char, *mut c_void),
        >,
        user_data: *mut c_void,
        user_data_free_func: *const c_void,
    ) -> u32;
    pub fn g_dbus_connection_register_object(
        connection: *mut GDBusConnection,
        object_path: *const c_char,
        interface_info: *mut GDBusInterfaceInfo,
        vtable: *const GDBusInterfaceVTable,
        user_data: *mut c_void,
        user_data_free_func: *const c_void,
        error: *mut *mut GError,
    ) -> u32;
    pub fn g_dbus_method_invocation_return_value(
        invocation: *mut GDBusMethodInvocation,
        parameters: *mut GVariant,
    );
    pub fn g_dbus_node_info_new_for_xml(
        xml_data: *const c_char,
        error: *mut *mut GError,
    ) -> *mut GDBusNodeInfo;
    pub fn g_dbus_node_info_lookup_interface(
        info: *mut GDBusNodeInfo,
        name: *const c_char,
    ) -> *mut GDBusInterfaceInfo;
    pub fn g_memory_input_stream_new_from_data(
        data: *const c_void,
        len: usize,
        destroy: *const c_void,
    ) -> *mut GInputStream;
    pub fn g_input_stream_read_all(
        stream: *mut GInputStream,
        buffer: *mut c_void,
        count: usize,
        bytes_read: *mut usize,
        cancellable: *mut c_void,
        error: *mut *mut GError,
    ) -> bool;
}
