/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

pub(crate) use crate::platforms::gtk::headers::gdk::*;
pub(crate) use crate::platforms::gtk::headers::glib::*;
pub(crate) use crate::platforms::gtk::headers::gtk::*;

mod gdk;
mod glib;
mod gtk;
