/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use objc2_proc_macros::extern_class;

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "RenamedObject"]
    pub struct Object;
);

fn main() {}
