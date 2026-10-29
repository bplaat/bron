/*
 * Copyright (c) 2023-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::RefCell;
use std::fmt::{self, Display, Formatter};
use std::ops::Deref;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::parser::AstNode;

/// MARK: Array value
#[derive(Debug, Clone)]
pub(crate) struct ArrayValue {
    pub elements: Rc<RefCell<Vec<Value>>>,
}
impl Deref for ArrayValue {
    type Target = Rc<RefCell<Vec<Value>>>;

    fn deref(&self) -> &Self::Target {
        &self.elements
    }
}
impl PartialEq for ArrayValue {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.elements, &other.elements)
    }
}

/// MARK: Object value
#[derive(Debug, Clone)]
pub(crate) struct ObjectValue {
    pub properties: Rc<RefCell<IndexMap<String, Value>>>,
}
impl Deref for ObjectValue {
    type Target = Rc<RefCell<IndexMap<String, Value>>>;

    fn deref(&self) -> &Self::Target {
        &self.properties
    }
}
impl PartialEq for ObjectValue {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.properties, &other.properties)
    }
}

/// MARK: Value
#[derive(Debug, Clone)]
#[allow(private_interfaces)]
pub enum Value {
    /// Undefined value
    Undefined,
    /// Null value
    Null,
    /// Boolean value
    Boolean(bool),
    /// Number value
    Number(f64),
    /// String value
    String(String),
    /// Array value
    Array(ArrayValue),
    /// Object value
    Object(ObjectValue),
    /// Function value
    Function(Rc<(Vec<String>, AstNode)>),
    /// Native function
    NativeFunction(fn(&[Value]) -> Value),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a == b,
            (Value::Function(a), Value::Function(b)) => Rc::ptr_eq(a, b),
            (Value::NativeFunction(a), Value::NativeFunction(b)) => {
                let a_ptr: usize = (*a) as usize;
                let b_ptr: usize = (*b) as usize;
                a_ptr == b_ptr
            }
            _ => false,
        }
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Value::Undefined => write!(f, "undefined"),
            Value::Null => write!(f, "null"),
            Value::Boolean(b) => write!(f, "{b}"),
            Value::Number(n) => write!(f, "{n}"),
            Value::String(s) => write!(f, "{s}"),
            Value::Array(a) => {
                let mut elements = vec![];
                for v in a.borrow().iter() {
                    elements.push(v.to_string());
                }
                write!(f, "[{}]", elements.join(", "))
            }
            Value::Object(_) => write!(f, "[object Object]"),
            Value::Function(_) | Value::NativeFunction(_) => {
                write!(f, "function() {{ [native code] }}")
            }
        }
    }
}

impl Value {
    pub(crate) const fn typeof_string(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Boolean(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Null | Value::Array(_) | Value::Object(_) => "object",
            Value::Function(..) | Value::NativeFunction(_) => "function",
        }
    }

    pub(crate) fn is_truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Boolean(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Array(_) | Value::Object(_) | Value::Function(..) | Value::NativeFunction(_) => {
                true
            }
        }
    }

    pub(crate) fn loose_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Undefined | Value::Null, Value::Undefined | Value::Null) => true,
            (Value::Null | Value::Undefined, _) | (_, Value::Null | Value::Undefined) => false,
            (Value::Boolean(a), b) => {
                let a_num = if *a { 1.0 } else { 0.0 };
                a_num == b.to_number()
            }
            (a, Value::Boolean(b)) => {
                let b_num = if *b { 1.0 } else { 0.0 };
                a.to_number() == b_num
            }
            (Value::Number(a), b) => *a == b.to_number(),
            (a, Value::Number(b)) => a.to_number() == *b,
            (Value::String(a), b) => *a == b.to_string(),
            (a, Value::String(b)) => a.to_string() == *b,
            _ => self == other,
        }
    }

    pub(crate) fn to_number(&self) -> f64 {
        match self {
            Value::Null => 0.0,
            Value::Boolean(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Number(n) => *n,
            Value::String(s) => s.parse::<f64>().unwrap_or(f64::NAN),
            Value::Array(a) => {
                let arr = a.borrow();
                if arr.len() == 1 {
                    arr[0].to_number()
                } else if arr.is_empty() {
                    0.0
                } else {
                    f64::NAN
                }
            }
            Value::Undefined
            | Value::Object(_)
            | Value::Function(..)
            | Value::NativeFunction(_) => f64::NAN,
        }
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use std::cell::RefCell;
    use std::rc::Rc;

    use indexmap::IndexMap;

    use super::*;

    fn make_array(vals: Vec<Value>) -> Value {
        Value::Array(ArrayValue {
            elements: Rc::new(RefCell::new(vals)),
        })
    }

    fn make_object() -> Value {
        Value::Object(ObjectValue {
            properties: Rc::new(RefCell::new(IndexMap::new())),
        })
    }

    #[test]
    fn test_is_truthy() {
        assert!(!Value::Undefined.is_truthy());
        assert!(!Value::Null.is_truthy());
        assert!(!Value::Boolean(false).is_truthy());
        assert!(Value::Boolean(true).is_truthy());
        assert!(!Value::Number(0.0).is_truthy());
        assert!(!Value::Number(f64::NAN).is_truthy());
        assert!(Value::Number(1.0).is_truthy());
        assert!(Value::Number(-1.0).is_truthy());
        assert!(!Value::String(String::new()).is_truthy());
        assert!(Value::String("x".to_string()).is_truthy());
        assert!(make_array(vec![]).is_truthy()); // empty array is truthy in JS
        assert!(make_object().is_truthy());
    }

    #[test]
    fn test_to_number() {
        assert!(Value::Undefined.to_number().is_nan());
        assert_eq!(Value::Null.to_number(), 0.0);
        assert_eq!(Value::Boolean(false).to_number(), 0.0);
        assert_eq!(Value::Boolean(true).to_number(), 1.0);
        assert_eq!(Value::Number(3.11).to_number(), 3.11);
        assert_eq!(Value::String("42".to_string()).to_number(), 42.0);
        assert!(Value::String("abc".to_string()).to_number().is_nan());
        assert_eq!(make_array(vec![]).to_number(), 0.0);
        assert_eq!(make_array(vec![Value::Number(7.0)]).to_number(), 7.0);
        assert!(
            make_array(vec![Value::Number(1.0), Value::Number(2.0)])
                .to_number()
                .is_nan()
        );
        assert!(make_object().to_number().is_nan());
    }

    #[test]
    fn test_loose_equals() {
        assert!(Value::Undefined.loose_equals(&Value::Undefined));
        assert!(Value::Null.loose_equals(&Value::Null));
        assert!(Value::Undefined.loose_equals(&Value::Null));
        assert!(Value::Null.loose_equals(&Value::Undefined));
        assert!(!Value::Null.loose_equals(&Value::Number(0.0)));
        assert!(!Value::Undefined.loose_equals(&Value::Number(0.0)));
        assert!(Value::Boolean(true).loose_equals(&Value::Number(1.0)));
        assert!(Value::Number(1.0).loose_equals(&Value::Boolean(true)));
        assert!(Value::Boolean(false).loose_equals(&Value::Number(0.0)));
        assert!(Value::String("1".to_string()).loose_equals(&Value::Number(1.0)));
        assert!(Value::Number(1.0).loose_equals(&Value::String("1".to_string())));
        assert!(!Value::Number(1.0).loose_equals(&Value::Number(2.0)));
    }

    #[test]
    fn test_typeof_string() {
        assert_eq!(Value::Undefined.typeof_string(), "undefined");
        assert_eq!(Value::Null.typeof_string(), "object"); // JS quirk: null is "object"
        assert_eq!(Value::Boolean(true).typeof_string(), "boolean");
        assert_eq!(Value::Number(0.0).typeof_string(), "number");
        assert_eq!(Value::String(String::new()).typeof_string(), "string");
        assert_eq!(make_array(vec![]).typeof_string(), "object");
        assert_eq!(make_object().typeof_string(), "object");
    }

    #[test]
    fn test_display() {
        assert_eq!(Value::Undefined.to_string(), "undefined");
        assert_eq!(Value::Null.to_string(), "null");
        assert_eq!(Value::Boolean(true).to_string(), "true");
        assert_eq!(Value::Boolean(false).to_string(), "false");
        assert_eq!(Value::Number(42.0).to_string(), "42");
        assert_eq!(Value::String("hello".to_string()).to_string(), "hello");
        assert_eq!(make_array(vec![]).to_string(), "[]");
        assert_eq!(
            make_array(vec![
                Value::Number(1.0),
                Value::Number(2.0),
                Value::Number(3.0),
            ])
            .to_string(),
            "[1, 2, 3]"
        );
        assert_eq!(make_object().to_string(), "[object Object]");
    }
}
