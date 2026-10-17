/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use bsql::{FromRow, FromValue, Value, ValueError};

#[derive(Default, FromRow)]
struct User<T>
where
    T: Default + Into<Value> + TryFrom<Value, Error = ValueError>,
{
    #[sql(rename = "display_name")]
    name: T,
    nickname: Option<String>,
    #[sql(skip)]
    cached: bool,
}

#[derive(FromValue)]
enum Status {
    Active = 1,
    Disabled = 2,
}

fn main() {
    let _: Value = Status::Active.into();
    let _: Result<Status, ValueError> = Value::Integer(2).try_into();
    assert_eq!(User::<String>::columns(), "display_name, nickname");
    assert_eq!(User::<String>::values(), "?, ?");
}
