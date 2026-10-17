/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::marker::PhantomData;

use validate::{Error, Validate};

struct Context;

fn allowed(value: &String, _context: &Context) -> Result<(), Error> {
    if value.is_empty() {
        Err(Error::new("empty"))
    } else {
        Ok(())
    }
}

#[derive(Validate)]
#[validate(context(Context))]
struct Form<'a, T> {
    #[validate(length(min = 2), ascii, custom(allowed))]
    name: String,
    #[validate(range(min = 1, max = 10))]
    count: Option<i32>,
    marker: PhantomData<&'a T>,
}

fn main() {
    let form = Form::<u8> {
        name: "ok".into(),
        count: None,
        marker: PhantomData,
    };
    let _ = form.validate_with(&Context);
}
