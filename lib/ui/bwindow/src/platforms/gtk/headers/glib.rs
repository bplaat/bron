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
pub(crate) struct GObject([u8; 0]);
pub(crate) const G_CONNECT_DEFAULT: i32 = 0;
pub(crate) const G_CONNECT_AFTER: i32 = 1;
unsafe extern "C" {
    #[cfg(test)]
    pub(crate) fn g_object_notify(object: *mut GObject, property: *const c_char);
    pub(crate) fn g_object_get_data(object: *mut GObject, key: *const c_char) -> *mut c_void;
    pub(crate) fn g_object_set_data(object: *mut GObject, key: *const c_char, data: *mut c_void);
    pub(crate) fn g_signal_emit_by_name(instance: *mut GObject, signal: *const c_char, ...);
    pub(crate) fn g_object_get(instance: *mut GObject, first_property_name: *const c_char, ...);
    pub(crate) fn g_object_set(instance: *mut GObject, first_property_name: *const c_char, ...);
    pub(crate) fn g_signal_connect_data(
        instance: *mut GObject,
        detailed_signal: *const c_char,
        c_handler: *const c_void,
        data: *const c_void,
        destroy_data: *const c_void,
        connect_flags: i32,
    );
    pub(crate) fn g_object_unref(object: *mut GObject);
    pub(crate) fn g_object_ref(object: *mut GObject) -> *mut GObject;
    pub(crate) fn g_type_from_name(name: *const c_char) -> usize;
    pub(crate) fn g_type_check_instance_is_a(instance: *mut c_void, type_: usize) -> i32;
    pub(crate) fn g_signal_handlers_disconnect_matched(
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
pub(crate) struct GError([u8; 0]);
#[repr(C)]
pub(crate) struct GKeyFile([u8; 0]);
#[repr(C)]
pub(crate) struct GVariant([u8; 0]);
#[repr(C)]
pub(crate) struct GVariantBuilder([u8; 0]);
#[repr(C)]
pub(crate) struct GVariantType([u8; 0]);
#[repr(C)]
pub(crate) struct GList {
    pub(crate) data: *mut c_void,
    pub(crate) next: *mut GList,
    pub(crate) prev: *mut GList,
}
#[repr(C)]
pub(crate) struct GSList {
    pub(crate) data: *mut c_void,
    pub(crate) next: *mut GSList,
}
unsafe extern "C" {
    pub(crate) fn g_set_application_name(name: *const c_char);
    pub(crate) fn g_get_application_name() -> *const c_char;
    pub(crate) fn g_set_prgname(prgname: *const c_char);
    pub(crate) fn g_idle_add_full(
        priority: i32,
        function: extern "C" fn(*mut c_void) -> i32,
        data: *mut c_void,
        notify: extern "C" fn(*mut c_void),
    ) -> u32;
    pub(crate) fn g_source_remove(tag: u32) -> i32;
    pub(crate) fn g_error_free(error: *mut GError);
    pub(crate) fn g_key_file_new() -> *mut GKeyFile;
    pub(crate) fn g_key_file_load_from_file(
        key_file: *mut GKeyFile,
        file: *const c_char,
        flags: i32,
        error: *mut *mut GError,
    ) -> bool;
    pub(crate) fn g_key_file_get_integer(
        key_file: *mut GKeyFile,
        group_name: *const c_char,
        key: *const c_char,
        error: *mut *mut c_void,
    ) -> i32;
    pub(crate) fn g_key_file_get_boolean(
        key_file: *mut GKeyFile,
        group_name: *const c_char,
        key: *const c_char,
        error: *mut *mut c_void,
    ) -> bool;
    pub(crate) fn g_key_file_set_integer(
        key_file: *mut GKeyFile,
        group_name: *const c_char,
        key: *const c_char,
        value: i32,
    );
    pub(crate) fn g_key_file_set_boolean(
        key_file: *mut GKeyFile,
        group_name: *const c_char,
        key: *const c_char,
        value: bool,
    );
    pub(crate) fn g_key_file_save_to_file(
        key_file: *mut GKeyFile,
        file: *const c_char,
        error: *mut *mut c_void,
    ) -> bool;
    pub(crate) fn g_key_file_free(key_file: *mut GKeyFile);
    pub(crate) fn g_free(mem: *mut c_void);
    pub(crate) fn g_filename_from_uri(
        uri: *const c_char,
        hostname: *mut *mut c_char,
        error: *mut *mut GError,
    ) -> *mut c_char;
    pub(crate) fn g_strfreev(str_array: *mut *mut c_char);
    pub(crate) fn g_slist_free_full(
        list: *mut GSList,
        free_func: unsafe extern "C" fn(*mut c_void),
    );
    pub(crate) fn g_variant_type_new(type_string: *const c_char) -> *mut GVariantType;
    pub(crate) fn g_variant_type_free(type_: *mut GVariantType);
    pub(crate) fn g_variant_builder_new(type_: *const GVariantType) -> *mut GVariantBuilder;
    pub(crate) fn g_variant_builder_add(
        builder: *mut GVariantBuilder,
        format_string: *const c_char,
        ...
    );
    pub(crate) fn g_variant_builder_unref(builder: *mut GVariantBuilder);
    pub(crate) fn g_variant_new(format_string: *const c_char, ...) -> *mut GVariant;
    pub(crate) fn g_variant_new_boolean(value: i32) -> *mut GVariant;
    pub(crate) fn g_variant_get_boolean(value: *mut GVariant) -> i32;
    pub(crate) fn g_variant_new_double(value: f64) -> *mut GVariant;
    pub(crate) fn g_variant_ref_sink(value: *mut GVariant) -> *mut GVariant;
    pub(crate) fn g_variant_unref(value: *mut GVariant);
}

// MARK: GIO
#[repr(C)]
pub(crate) struct GDBusConnection([u8; 0]);
#[repr(C)]
pub(crate) struct GDBusInterfaceInfo([u8; 0]);
#[repr(C)]
pub(crate) struct GDBusMethodInvocation([u8; 0]);
#[repr(C)]
pub(crate) struct GDBusNodeInfo([u8; 0]);
pub(crate) const G_BUS_TYPE_SESSION: i32 = 2;
pub(crate) type GDBusMethodCallFunc = extern "C" fn(
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
pub(crate) struct GDBusInterfaceVTable {
    pub(crate) method_call: Option<GDBusMethodCallFunc>,
    pub(crate) get_property: *const c_void,
    pub(crate) set_property: *const c_void,
}
#[repr(C)]
pub(crate) struct GApplication([u8; 0]);
#[repr(C)]
pub(crate) struct GFile([u8; 0]);
#[repr(C)]
pub(crate) struct GSimpleAction([u8; 0]);
pub(crate) const G_APPLICATION_HANDLES_OPEN: u32 = 1 << 2;
pub(crate) const G_APPLICATION_NON_UNIQUE: u32 = 1 << 5;

#[repr(C)]
pub(crate) struct GMenu([u8; 0]);
#[repr(C)]
pub(crate) struct GMenuModel([u8; 0]);

#[repr(C)]
pub(crate) struct GMenuItem([u8; 0]);
unsafe extern "C" {
    pub(crate) fn g_application_id_is_valid(id: *const c_char) -> i32;
    pub(crate) fn g_application_register(
        application: *mut GApplication,
        cancellable: *mut c_void,
        error: *mut *mut GError,
    ) -> i32;
    pub(crate) fn g_application_get_is_remote(application: *mut GApplication) -> i32;
    pub(crate) fn g_application_run(
        application: *mut GApplication,
        argc: i32,
        argv: *mut *mut c_char,
    ) -> i32;
    pub(crate) fn g_application_quit(application: *mut GApplication);
    pub(crate) fn g_application_hold(application: *mut GApplication);
    pub(crate) fn g_application_release(application: *mut GApplication);
    pub(crate) fn g_file_get_path(file: *mut GFile) -> *mut c_char;
    pub(crate) fn g_simple_action_group_new() -> *mut c_void;
    pub(crate) fn g_action_get_name(action: *mut GSimpleAction) -> *const c_char;
    pub(crate) fn g_action_group_get_action_enabled(group: *mut c_void, name: *const c_char)
    -> i32;
    pub(crate) fn g_action_group_activate_action(
        group: *mut c_void,
        name: *const c_char,
        parameter: *mut GVariant,
    );
    pub(crate) fn g_action_map_lookup_action(
        map: *mut c_void,
        name: *const c_char,
    ) -> *mut GSimpleAction;
    pub(crate) fn g_simple_action_set_enabled(action: *mut GSimpleAction, enabled: i32);
    pub(crate) fn g_simple_action_new_stateful(
        name: *const c_char,
        parameter_type: *const GVariantType,
        state: *mut GVariant,
    ) -> *mut GSimpleAction;
    pub(crate) fn g_simple_action_set_state(action: *mut GSimpleAction, state: *mut GVariant);
    pub(crate) fn g_simple_action_new(
        name: *const c_char,
        parameter_type: *const GVariantType,
    ) -> *mut GSimpleAction;
    pub(crate) fn g_action_map_add_action(map: *mut c_void, action: *mut GSimpleAction);
    pub(crate) fn g_menu_model_get_n_items(model: *mut GMenuModel) -> i32;
    pub(crate) fn g_menu_new() -> *mut GMenu;
    pub(crate) fn g_menu_append_section(
        menu: *mut GMenu,
        label: *const c_char,
        section: *mut GMenuModel,
    );
    pub(crate) fn g_menu_append_submenu(
        menu: *mut GMenu,
        label: *const c_char,
        submenu: *mut GMenuModel,
    );
    pub(crate) fn g_menu_item_new(label: *const c_char, action: *const c_char) -> *mut GMenuItem;
    pub(crate) fn g_menu_item_set_attribute(
        item: *mut GMenuItem,
        attribute: *const c_char,
        format: *const c_char,
        ...
    );
    pub(crate) fn g_menu_append_item(menu: *mut GMenu, item: *mut GMenuItem);
    pub(crate) fn g_bus_get_sync(
        bus_type: i32,
        cancellable: *mut c_void,
        error: *mut *mut GError,
    ) -> *mut GDBusConnection;
    pub(crate) fn g_dbus_connection_emit_signal(
        connection: *mut GDBusConnection,
        destination_bus_name: *const c_char,
        object_path: *const c_char,
        interface_name: *const c_char,
        signal_name: *const c_char,
        parameters: *mut GVariant,
        error: *mut *mut GError,
    ) -> i32;
    pub(crate) fn g_bus_watch_name_on_connection(
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
    pub(crate) fn g_dbus_connection_register_object(
        connection: *mut GDBusConnection,
        object_path: *const c_char,
        interface_info: *mut GDBusInterfaceInfo,
        vtable: *const GDBusInterfaceVTable,
        user_data: *mut c_void,
        user_data_free_func: *const c_void,
        error: *mut *mut GError,
    ) -> u32;
    pub(crate) fn g_dbus_method_invocation_return_value(
        invocation: *mut GDBusMethodInvocation,
        parameters: *mut GVariant,
    );
    pub(crate) fn g_dbus_node_info_new_for_xml(
        xml_data: *const c_char,
        error: *mut *mut GError,
    ) -> *mut GDBusNodeInfo;
    pub(crate) fn g_dbus_node_info_lookup_interface(
        info: *mut GDBusNodeInfo,
        name: *const c_char,
    ) -> *mut GDBusInterfaceInfo;
}
