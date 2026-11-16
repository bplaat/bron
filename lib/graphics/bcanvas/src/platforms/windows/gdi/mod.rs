/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The GDI+ backend, for Windows without Direct2D. GDI+ draws antialiased paths, text and
//! images into premultiplied GDI bitmaps, while gradients and composite operations other
//! than source over are painted in software.

pub(crate) use self::context::Context;
pub(crate) use self::target::{OffscreenTarget, WindowTarget};

mod composite;
mod context;
mod gradient;
mod headers;
mod target;
