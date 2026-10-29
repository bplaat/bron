/*
 * Copyright (c) 2023-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::parser::{AstNode, DeclarationType, ObjectProperty};
use crate::value::{ArrayValue, ObjectValue, Value};

enum Scope {
    Function(IndexMap<String, Value>),
    Block(IndexMap<String, Value>),
}

enum Control {
    Error(String),
    Break(Option<String>),
    Continue(Option<String>),
    Return(Value),
}

pub(crate) struct Interpreter {
    global_env: Rc<RefCell<IndexMap<String, Value>>>,
    scopes: Vec<Scope>,
    previous_value: Value,
}

impl Interpreter {
    pub(crate) const fn new(global_env: Rc<RefCell<IndexMap<String, Value>>>) -> Self {
        Interpreter {
            global_env,
            scopes: Vec::new(),
            previous_value: Value::Undefined,
        }
    }

    // MARK: Eval
    pub(crate) fn eval(&mut self, node: &AstNode) -> Result<Value, String> {
        match self.eval_node(node) {
            Ok(val) => Ok(val),
            Err(Control::Error(err)) => Err(err),
            Err(Control::Break(_)) => Err(String::from(
                "Interpreter: 'break' used outside of loop or switch",
            )),
            Err(Control::Continue(_)) => {
                Err(String::from("Interpreter: 'continue' used outside of loop"))
            }
            Err(Control::Return(_)) => Err(String::from(
                "Interpreter: 'return' used outside of function",
            )),
        }
    }

    fn eval_node(&mut self, node: &AstNode) -> Result<Value, Control> {
        match node {
            AstNode::Block { label, nodes } => {
                self.scopes.push(Scope::Block(IndexMap::new()));
                for node in nodes {
                    self.previous_value = match self.eval_node(node) {
                        Ok(val) => val,
                        Err(Control::Break(break_label))
                            if label.is_some() && break_label == *label =>
                        {
                            self.scopes.pop();
                            return Ok(self.previous_value.clone());
                        }
                        Err(control) => {
                            self.scopes.pop();
                            return Err(control);
                        }
                    };
                }
                self.scopes.pop();
                Ok(self.previous_value.clone())
            }
            AstNode::If {
                label,
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_value = self.eval_node(condition)?;
                if cond_value.is_truthy() {
                    match self.eval_node(then_branch) {
                        Err(Control::Break(break_label))
                            if label.is_some() && break_label == *label =>
                        {
                            Ok(self.previous_value.clone())
                        }
                        result => result,
                    }
                } else if let Some(else_branch) = else_branch {
                    match self.eval_node(else_branch) {
                        Err(Control::Break(break_label))
                            if label.is_some() && break_label == *label =>
                        {
                            Ok(self.previous_value.clone())
                        }
                        result => result,
                    }
                } else {
                    Ok(Value::Undefined)
                }
            }
            AstNode::Switch {
                label,
                expression,
                cases,
                default,
            } => {
                let expr_value = self.eval_node(expression)?;
                let mut last_value = Value::Undefined;
                for (case_value, case_body) in cases {
                    let case_eval = self.eval_node(case_value)?;
                    if expr_value.loose_equals(&case_eval) {
                        last_value = match self.eval_node(case_body) {
                            Err(Control::Break(break_label)) if break_label == *label => {
                                return Ok(self.previous_value.clone());
                            }
                            result => result,
                        }?;
                    }
                }
                if let Some(default_body) = default {
                    last_value = match self.eval_node(default_body) {
                        Err(Control::Break(break_label)) if break_label == *label => {
                            return Ok(self.previous_value.clone());
                        }
                        result => result,
                    }?;
                }
                Ok(last_value)
            }
            AstNode::While {
                label,
                condition,
                body,
            } => {
                let mut last_value = Value::Undefined;
                loop {
                    let cond_value = self.eval_node(condition)?;
                    if !cond_value.is_truthy() {
                        break;
                    }

                    last_value = match self.eval_node(body) {
                        Err(Control::Break(break_label)) if break_label == *label => {
                            return Ok(self.previous_value.clone());
                        }
                        Err(Control::Continue(continue_label)) if continue_label == *label => {
                            continue;
                        }
                        result => result,
                    }?;
                }
                Ok(last_value)
            }
            AstNode::DoWhile {
                label,
                body,
                condition,
            } => {
                let mut last_value = Value::Undefined;
                loop {
                    last_value = match self.eval_node(body) {
                        Err(Control::Break(break_label)) if break_label == *label => {
                            return Ok(self.previous_value.clone());
                        }
                        Err(Control::Continue(continue_label)) if continue_label == *label => {
                            if !self.eval_node(condition)?.is_truthy() {
                                break;
                            }
                            continue;
                        }
                        result => result,
                    }?;

                    if !self.eval_node(condition)?.is_truthy() {
                        break;
                    }
                }
                Ok(last_value)
            }
            AstNode::For {
                label,
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_node) = init {
                    self.eval_node(init_node)?;
                }

                let mut last_value = Value::Undefined;
                loop {
                    if let Some(cond_node) = condition {
                        let cond_value = self.eval_node(cond_node)?;
                        if !cond_value.is_truthy() {
                            break;
                        }
                    }

                    last_value = match self.eval_node(body) {
                        Err(Control::Break(break_label)) if break_label == *label => {
                            return Ok(self.previous_value.clone());
                        }
                        Err(Control::Continue(continue_label)) if continue_label == *label => {
                            if let Some(update_node) = update {
                                self.eval_node(update_node)?;
                            }
                            continue;
                        }
                        result => result,
                    }?;

                    if let Some(update_node) = update {
                        self.eval_node(update_node)?;
                    }
                }
                Ok(last_value)
            }
            AstNode::ForIn {
                label,
                variable,
                declaration_type,
                iterable,
                body,
            } => {
                let iterable_value = self.eval_node(iterable)?;

                // Get the keys to iterate over
                let keys = match &iterable_value {
                    Value::Object(obj) => obj.borrow().keys().cloned().collect::<Vec<_>>(),
                    Value::Array(arr) => {
                        let arr_borrowed = arr.borrow();
                        let mut indices = Vec::new();
                        for (idx, val) in arr_borrowed.iter().enumerate() {
                            // for...in only iterates over defined elements
                            if !matches!(val, Value::Undefined) {
                                indices.push(idx.to_string());
                            }
                        }
                        indices
                    }
                    _ => return Ok(Value::Undefined),
                };

                if declaration_type.is_some() {
                    self.scopes.push(Scope::Block(IndexMap::new()));
                    self.set_var(*declaration_type, variable, Value::Undefined);
                }

                let mut last_value = Value::Undefined;
                for key in keys {
                    let key_value = Value::String(key.clone());
                    self.set_var(*declaration_type, variable, key_value);

                    last_value = match self.eval_node(body) {
                        Err(Control::Break(break_label)) if break_label == *label => {
                            if declaration_type.is_some() {
                                self.scopes.pop();
                            }
                            return Ok(self.previous_value.clone());
                        }
                        Err(Control::Continue(continue_label)) if continue_label == *label => {
                            continue;
                        }
                        result => result,
                    }?;
                    self.previous_value = last_value.clone();
                }

                if declaration_type.is_some() {
                    self.scopes.pop();
                }
                Ok(last_value)
            }
            AstNode::ForOf {
                label,
                variable,
                declaration_type,
                iterable,
                body,
            } => {
                let iterable_value = self.eval_node(iterable)?;

                // Get the values to iterate over
                let values = match &iterable_value {
                    Value::Array(arr) => arr.borrow().clone(),
                    _ => return Ok(Value::Undefined),
                };

                if declaration_type.is_some() {
                    self.scopes.push(Scope::Block(IndexMap::new()));
                    self.set_var(*declaration_type, variable, Value::Undefined);
                }

                let mut last_value = Value::Undefined;
                for value in values {
                    self.set_var(*declaration_type, variable, value);

                    last_value = match self.eval_node(body) {
                        Err(Control::Break(break_label)) if break_label == *label => {
                            if declaration_type.is_some() {
                                self.scopes.pop();
                            }
                            return Ok(self.previous_value.clone());
                        }
                        Err(Control::Continue(continue_label)) if continue_label == *label => {
                            continue;
                        }
                        result => result,
                    }?;
                    self.previous_value = last_value.clone();
                }

                if declaration_type.is_some() {
                    self.scopes.pop();
                }
                Ok(last_value)
            }
            AstNode::Continue(continue_label) => Err(Control::Continue(continue_label.clone())),
            AstNode::Break(label) => Err(Control::Break(label.clone())),
            AstNode::Return(value) => {
                let ret_value = if let Some(ret_node) = value {
                    self.eval_node(ret_node)?
                } else {
                    Value::Undefined
                };
                Err(Control::Return(ret_value))
            }
            AstNode::Comma(nodes) => {
                for node in nodes {
                    self.previous_value = self.eval_node(node)?;
                }
                Ok(self.previous_value.clone())
            }

            AstNode::Value(value) => Ok(value.clone()),
            AstNode::ArrayLiteral(nodes) => {
                let mut elements = Vec::new();
                for node in nodes {
                    elements.push(self.eval_node(node)?);
                }
                Ok(Value::Array(ArrayValue {
                    elements: Rc::new(RefCell::new(elements)),
                }))
            }
            AstNode::ObjectLiteral(properties) => {
                let mut obj = IndexMap::new();
                for (property_key, value_node) in properties {
                    let key_str = match property_key {
                        ObjectProperty::Literal(key) => key.clone(),
                        ObjectProperty::Computed(key_expr) => {
                            let computed_key = self.eval_node(key_expr)?;
                            computed_key.to_string()
                        }
                    };
                    let value = self.eval_node(value_node)?;
                    obj.insert(key_str, value);
                }
                Ok(Value::Object(ObjectValue {
                    properties: Rc::new(RefCell::new(obj)),
                }))
            }
            AstNode::Variable(variable) => match self.get_var(variable) {
                Some(value) => Ok(value.clone()),
                None => Err(Control::Error(format!(
                    "Interpreter: variable {variable} doesn't exists"
                ))),
            },
            AstNode::FunctionCall(function, arguments) => {
                // Special handling for array prototype methods
                if let AstNode::GetProperty(object_node, property_node) = &**function
                    && let AstNode::Value(Value::String(method_name)) = &**property_node
                {
                    let object_value = self.eval_node(object_node)?;
                    if let Value::Array(arr) = object_value
                        && method_name.as_str() == "push"
                    {
                        let mut arg_values = Vec::new();
                        for arg in arguments {
                            arg_values.push(self.eval_node(arg)?);
                        }
                        let mut borrowed = arr.borrow_mut();
                        for val in arg_values {
                            borrowed.push(val);
                        }
                        return Ok(Value::Number(borrowed.len() as f64));
                    }
                }

                // Check if this is a method call (object.method())
                let (func_value, this_value) =
                    if let AstNode::GetProperty(object_node, _) = &**function {
                        let this_obj = self.eval_node(object_node)?;
                        let func = self.eval_node(function)?;
                        (func, Some(this_obj))
                    } else {
                        let func = self.eval_node(function)?;
                        (func, None)
                    };

                let mut arg_values = Vec::new();
                for arg in arguments {
                    arg_values.push(self.eval_node(arg)?);
                }
                match func_value {
                    Value::Function(rc) => {
                        let (arg_names, body) = &*rc;
                        let mut func_env = IndexMap::new();

                        // Bind this
                        let this_val = this_value.unwrap_or(Value::Undefined);
                        func_env.insert("this".to_string(), this_val);

                        // Bind arguments
                        for (i, arg_name) in arg_names.iter().enumerate() {
                            let arg_value = arg_values.get(i).cloned().unwrap_or(Value::Undefined);
                            func_env.insert(arg_name.clone(), arg_value.clone());
                        }
                        func_env.insert(
                            "arguments".to_string(),
                            Value::Array(ArrayValue {
                                elements: Rc::new(RefCell::new(arg_values.to_vec())),
                            }),
                        );

                        self.scopes.push(Scope::Function(func_env));
                        match self.eval_node(body) {
                            Ok(_) => {
                                self.scopes.pop();
                                Ok(Value::Undefined)
                            }
                            Err(Control::Return(ret_val)) => {
                                self.scopes.pop();
                                Ok(ret_val)
                            }
                            Err(control) => {
                                self.scopes.pop();
                                Err(control)
                            }
                        }
                    }
                    Value::NativeFunction(func) => Ok(func(&arg_values)),
                    _ => Err(Control::Error(String::from(
                        "Interpreter: trying to call a non-function value",
                    ))),
                }
            }

            AstNode::Assign(declaration_type, lhs, rhs) => self.assign(*declaration_type, lhs, rhs),
            AstNode::AddAssign(lhs, rhs) => self.op_assign(lhs, rhs, |a, b| a + b, "addition"),
            AstNode::SubtractAssign(lhs, rhs) => {
                self.op_assign(lhs, rhs, |a, b| a - b, "subtraction")
            }
            AstNode::MultiplyAssign(lhs, rhs) => {
                self.op_assign(lhs, rhs, |a, b| a * b, "multiplication")
            }
            AstNode::DivideAssign(lhs, rhs) => self.op_assign(
                lhs,
                rhs,
                |a, b| if b != 0.0 { a / b } else { f64::NAN },
                "division",
            ),
            AstNode::RemainderAssign(lhs, rhs) => self.op_assign(lhs, rhs, |a, b| a % b, "modulo"),
            AstNode::ExponentiationAssign(lhs, rhs) => {
                self.op_assign(lhs, rhs, |a, b| a.powf(b), "exponentiation")
            }
            AstNode::BitwiseAndAssign(lhs, rhs) => {
                self.binary_op_assign(lhs, rhs, |a, b| a & b, "bitwise and")
            }
            AstNode::BitwiseOrAssign(lhs, rhs) => {
                self.binary_op_assign(lhs, rhs, |a, b| a | b, "bitwise or")
            }
            AstNode::BitwiseXorAssign(lhs, rhs) => {
                self.binary_op_assign(lhs, rhs, |a, b| a ^ b, "bitwise xor")
            }
            AstNode::LeftShiftAssign(lhs, rhs) => {
                self.binary_op_assign(lhs, rhs, |a, b| a << b, "left shift")
            }
            AstNode::SignedRightShiftAssign(lhs, rhs) => {
                self.binary_op_assign(lhs, rhs, |a, b| a >> b, "signed right shift")
            }
            AstNode::UnsignedRightShiftAssign(lhs, rhs) => self.binary_op_assign(
                lhs,
                rhs,
                |a, b| ((a as u32) >> (b as u32)) as i32,
                "unsigned right shift",
            ),
            AstNode::LogicalOrAssign(lhs, rhs) => {
                let lhs_val = self.eval_node(lhs)?;
                if lhs_val.is_truthy() {
                    return Ok(lhs_val);
                }
                let rhs_val = self.eval_node(rhs)?;
                match &**lhs {
                    AstNode::Variable(variable) => self.set_var(None, variable, rhs_val.clone()),
                    _ => {
                        return Err(Control::Error(String::from(
                            "Interpreter: assign lhs is not a variable",
                        )));
                    }
                }
                Ok(rhs_val)
            }
            AstNode::LogicalAndAssign(lhs, rhs) => {
                let lhs_val = self.eval_node(lhs)?;
                if !lhs_val.is_truthy() {
                    return Ok(lhs_val);
                }
                let rhs_val = self.eval_node(rhs)?;
                match &**lhs {
                    AstNode::Variable(variable) => self.set_var(None, variable, rhs_val.clone()),
                    _ => {
                        return Err(Control::Error(String::from(
                            "Interpreter: assign lhs is not a variable",
                        )));
                    }
                }
                Ok(rhs_val)
            }

            AstNode::Ternary {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_value = self.eval_node(condition)?;
                if cond_value.is_truthy() {
                    self.eval_node(then_branch)
                } else {
                    self.eval_node(else_branch)
                }
            }

            AstNode::UnaryMinus(unary) => match self.eval_node(unary)? {
                Value::Number(n) => Ok(Value::Number(-n)),
                _ => Err(Control::Error(String::from(
                    "Interpreter: negation on non-number",
                ))),
            },
            AstNode::UnaryLogicalNot(unary) => {
                let val = self.eval_node(unary)?;
                Ok(Value::Boolean(!val.is_truthy()))
            }
            AstNode::UnaryPreIncrement(unary) => match &**unary {
                AstNode::Variable(var_name) => {
                    let current_value = self.eval_node(unary)?;
                    match current_value {
                        Value::Number(n) => {
                            let new_value = Value::Number(n + 1.0);
                            self.set_var(None, var_name, new_value.clone());
                            Ok(new_value)
                        }
                        _ => Err(Control::Error(String::from(
                            "Interpreter: increment on non-number",
                        ))),
                    }
                }
                _ => Err(Control::Error(String::from(
                    "Interpreter: pre-increment can only be applied to variables",
                ))),
            },
            AstNode::UnaryPreDecrement(unary) => match &**unary {
                AstNode::Variable(var_name) => {
                    let current_value = self.eval_node(unary)?;
                    match current_value {
                        Value::Number(n) => {
                            let new_value = Value::Number(n - 1.0);
                            self.set_var(None, var_name, new_value.clone());
                            Ok(new_value)
                        }
                        _ => Err(Control::Error(String::from(
                            "Interpreter: decrement on non-number",
                        ))),
                    }
                }
                _ => Err(Control::Error(String::from(
                    "Interpreter: pre-decrement can only be applied to variables",
                ))),
            },
            AstNode::UnaryPostIncrement(unary) => match &**unary {
                AstNode::Variable(var_name) => {
                    let current_value = self.eval_node(unary)?;
                    match current_value {
                        Value::Number(n) => {
                            self.set_var(None, var_name, Value::Number(n + 1.0));
                            Ok(Value::Number(n))
                        }
                        _ => Err(Control::Error(String::from(
                            "Interpreter: increment on non-number",
                        ))),
                    }
                }
                _ => Err(Control::Error(String::from(
                    "Interpreter: post-increment can only be applied to variables",
                ))),
            },
            AstNode::UnaryPostDecrement(unary) => match &**unary {
                AstNode::Variable(var_name) => {
                    let current_value = self.eval_node(unary)?;
                    match current_value {
                        Value::Number(n) => {
                            self.set_var(None, var_name, Value::Number(n - 1.0));
                            Ok(Value::Number(n))
                        }
                        _ => Err(Control::Error(String::from(
                            "Interpreter: decrement on non-number",
                        ))),
                    }
                }
                _ => Err(Control::Error(String::from(
                    "Interpreter: post-decrement can only be applied to variables",
                ))),
            },
            AstNode::UnaryTypeof(unary) => {
                let val = self.eval_node(unary)?;
                Ok(Value::String(val.typeof_string().to_string()))
            }

            AstNode::Add(lhs, rhs) => self.arithmetic_op(lhs, rhs, |a, b| a + b, "addition"),
            AstNode::Subtract(lhs, rhs) => {
                self.arithmetic_op(lhs, rhs, |a, b| a - b, "subtraction")
            }
            AstNode::Multiply(lhs, rhs) => {
                self.arithmetic_op(lhs, rhs, |a, b| a * b, "multiplication")
            }
            AstNode::Divide(lhs, rhs) => self.arithmetic_op(
                lhs,
                rhs,
                |a, b| if b != 0.0 { a / b } else { f64::NAN },
                "division",
            ),
            AstNode::Remainder(lhs, rhs) => self.arithmetic_op(lhs, rhs, |a, b| a % b, "modulo"),
            AstNode::Exponentiation(lhs, rhs) => {
                self.arithmetic_op(lhs, rhs, |a, b| a.powf(b), "exponentiation")
            }
            AstNode::BitwiseAnd(lhs, rhs) => self.binary_op(lhs, rhs, |a, b| a & b, "bitwise and"),
            AstNode::BitwiseOr(lhs, rhs) => self.binary_op(lhs, rhs, |a, b| a | b, "bitwise or"),
            AstNode::BitwiseXor(lhs, rhs) => self.binary_op(lhs, rhs, |a, b| a ^ b, "bitwise xor"),
            AstNode::LeftShift(lhs, rhs) => self.binary_op(lhs, rhs, |a, b| a << b, "left shift"),
            AstNode::SignedRightShift(lhs, rhs) => {
                self.binary_op(lhs, rhs, |a, b| a >> b, "signed right shift")
            }
            AstNode::UnsignedRightShift(lhs, rhs) => self.binary_op(
                lhs,
                rhs,
                |a, b| ((a as u32) >> (b as u32)) as i32,
                "unsigned right shift",
            ),
            AstNode::BitwiseNot(unary) => match self.eval_node(unary)? {
                Value::Number(n) => Ok(Value::Number(!(n as i32) as f64)),
                _ => Err(Control::Error(String::from(
                    "Interpreter: bitwise not on non-number",
                ))),
            },

            AstNode::Equals(lhs, rhs) => {
                let (lhs_val, rhs_val) = (self.eval_node(lhs)?, self.eval_node(rhs)?);
                Ok(Value::Boolean(lhs_val.loose_equals(&rhs_val)))
            }
            AstNode::StrictEquals(lhs, rhs) => {
                let (lhs_val, rhs_val) = (self.eval_node(lhs)?, self.eval_node(rhs)?);
                Ok(Value::Boolean(lhs_val == rhs_val))
            }
            AstNode::NotEquals(lhs, rhs) => {
                let (lhs_val, rhs_val) = (self.eval_node(lhs)?, self.eval_node(rhs)?);
                Ok(Value::Boolean(!lhs_val.loose_equals(&rhs_val)))
            }
            AstNode::StrictNotEquals(lhs, rhs) => {
                let (lhs_val, rhs_val) = (self.eval_node(lhs)?, self.eval_node(rhs)?);
                Ok(Value::Boolean(lhs_val != rhs_val))
            }
            AstNode::LessThen(lhs, rhs) => self.compare_op(lhs, rhs, |a, b| a < b, "less than"),
            AstNode::LessThenEquals(lhs, rhs) => {
                self.compare_op(lhs, rhs, |a, b| a <= b, "less than equals")
            }
            AstNode::GreaterThen(lhs, rhs) => {
                self.compare_op(lhs, rhs, |a, b| a > b, "greater than")
            }
            AstNode::GreaterThenEquals(lhs, rhs) => {
                self.compare_op(lhs, rhs, |a, b| a >= b, "greater than equals")
            }

            AstNode::LogicalAnd(lhs, rhs) => {
                let lhs_val = self.eval_node(lhs)?;
                if !lhs_val.is_truthy() {
                    return Ok(lhs_val);
                }
                self.eval_node(rhs)
            }
            AstNode::LogicalOr(lhs, rhs) => {
                let lhs_val = self.eval_node(lhs)?;
                if lhs_val.is_truthy() {
                    return Ok(lhs_val);
                }
                self.eval_node(rhs)
            }

            AstNode::GetProperty(object_node, property_node) => {
                let object_value = self.eval_node(object_node)?;
                let property_value = self.eval_node(property_node)?;
                match (object_value, property_value) {
                    (Value::Array(elements), Value::Number(index)) => {
                        let idx = index as usize;
                        let arr = elements.borrow();
                        if idx < arr.len() {
                            Ok(arr.get(idx).cloned().unwrap_or(Value::Undefined))
                        } else {
                            Ok(Value::Undefined)
                        }
                    }
                    (Value::Array(elements), Value::String(property)) => {
                        if property == "length" {
                            Ok(Value::Number(elements.borrow().len() as f64))
                        } else if let Ok(index) = property.parse::<usize>() {
                            // String is a numeric index
                            let arr = elements.borrow();
                            if index < arr.len() {
                                Ok(arr.get(index).cloned().unwrap_or(Value::Undefined))
                            } else {
                                Ok(Value::Undefined)
                            }
                        } else {
                            Ok(Value::Undefined)
                        }
                    }
                    (Value::Object(obj), Value::String(property)) => Ok(obj
                        .borrow()
                        .get(&property)
                        .cloned()
                        .unwrap_or(Value::Undefined)),
                    _ => Ok(Value::Undefined),
                }
            }
        }
    }

    // MARK: Var get set
    fn get_var(&mut self, variable: &str) -> Option<Value> {
        for scope in self.scopes.iter_mut().rev() {
            match scope {
                Scope::Block(env) | Scope::Function(env) if env.contains_key(variable) => {
                    return env.get(variable).cloned();
                }
                _ => {}
            }
        }
        self.global_env.borrow().get(variable).cloned()
    }

    fn set_var(&mut self, declaration_type: Option<DeclarationType>, variable: &str, value: Value) {
        for scope in self.scopes.iter_mut().rev() {
            match scope {
                Scope::Block(env)
                    if matches!(
                        declaration_type,
                        Some(DeclarationType::Let | DeclarationType::Const)
                    ) || env.contains_key(variable) =>
                {
                    env.insert(variable.to_string(), value);
                    return;
                }
                Scope::Function(env)
                    if declaration_type.is_some() || env.contains_key(variable) =>
                {
                    env.insert(variable.to_string(), value);
                    return;
                }
                _ => {}
            }
        }
        self.global_env
            .borrow_mut()
            .insert(variable.to_string(), value);
    }

    // MARK: Utils
    fn assign(
        &mut self,
        declaration_type: Option<DeclarationType>,
        lhs: &AstNode,
        rhs: &AstNode,
    ) -> Result<Value, Control> {
        let result = self.eval_node(rhs)?;
        match lhs {
            AstNode::Variable(variable) => {
                self.set_var(declaration_type, variable, result.clone());
                Ok(result)
            }
            AstNode::GetProperty(object_node, property_node) => {
                let object_value = self.eval_node(object_node)?;
                let property_value = self.eval_node(property_node)?;
                match (&object_value, &property_value) {
                    (Value::Array(arr), Value::Number(index)) => {
                        let idx = *index as usize;
                        let mut borrowed_arr = arr.borrow_mut();
                        // Extend array if needed
                        if idx >= borrowed_arr.len() {
                            borrowed_arr.resize(idx + 1, Value::Undefined);
                        }
                        borrowed_arr[idx] = result.clone();
                    }
                    (Value::Array(arr), Value::String(property)) => {
                        if property == "length" {
                            let new_len = result.to_number() as usize;
                            let mut borrowed_arr = arr.borrow_mut();
                            borrowed_arr.truncate(new_len);
                        } else {
                            return Err(Control::Error(String::from(
                                "Interpreter: cannot assign to array property",
                            )));
                        }
                    }
                    (Value::Object(obj), Value::String(property)) => {
                        obj.borrow_mut().insert(property.clone(), result.clone());
                    }
                    _ => {
                        return Err(Control::Error(String::from(
                            "Interpreter: invalid property assignment",
                        )));
                    }
                }
                Ok(result)
            }
            _ => Err(Control::Error(String::from(
                "Interpreter: assign lhs is not a variable",
            ))),
        }
    }

    fn arithmetic_op<F>(
        &mut self,
        lhs: &AstNode,
        rhs: &AstNode,
        op: F,
        op_name: &str,
    ) -> Result<Value, Control>
    where
        F: Fn(f64, f64) -> f64,
    {
        let lhs_val = self.eval_node(lhs)?;
        let rhs_val = self.eval_node(rhs)?;

        // Handle string concatenation for addition
        // FIXME: cleanup
        if op_name == "addition"
            && let (Value::String(a), Value::String(b)) = (&lhs_val, &rhs_val)
        {
            return Ok(Value::String(format!("{a}{b}")));
        }

        match (lhs_val, rhs_val) {
            (Value::Number(a), Value::Number(b)) => Ok(Value::Number(op(a, b))),
            _ => Err(Control::Error(format!(
                "Interpreter: {op_name} on non-numbers"
            ))),
        }
    }

    fn binary_op<F>(
        &mut self,
        lhs: &AstNode,
        rhs: &AstNode,
        op: F,
        op_name: &str,
    ) -> Result<Value, Control>
    where
        F: Fn(i32, i32) -> i32,
    {
        match (self.eval_node(lhs)?, self.eval_node(rhs)?) {
            (Value::Number(a), Value::Number(b)) => {
                Ok(Value::Number(op(a as i32, b as i32) as f64))
            }
            _ => Err(Control::Error(format!(
                "Interpreter: {op_name} on non-numbers"
            ))),
        }
    }

    fn compare_op<F>(
        &mut self,
        lhs: &AstNode,
        rhs: &AstNode,
        op: F,
        op_name: &str,
    ) -> Result<Value, Control>
    where
        F: Fn(f64, f64) -> bool,
    {
        match (self.eval_node(lhs)?, self.eval_node(rhs)?) {
            (Value::Number(a), Value::Number(b)) => Ok(Value::Boolean(op(a, b))),
            _ => Err(Control::Error(format!(
                "Interpreter: {op_name} on non-numbers"
            ))),
        }
    }

    fn op_assign<F>(
        &mut self,
        lhs: &AstNode,
        rhs: &AstNode,
        op: F,
        op_name: &str,
    ) -> Result<Value, Control>
    where
        F: Fn(f64, f64) -> f64,
    {
        let lhs_val = self.eval_node(lhs)?;
        let rhs_val = self.eval_node(rhs)?;

        // Handle string concatenation for addition
        // FIXME: cleanup
        if op_name == "addition"
            && let (Value::String(a), Value::String(b)) = (&lhs_val, &rhs_val)
        {
            let result = Value::String(format!("{a}{b}"));
            match lhs {
                AstNode::Variable(variable) => self.set_var(None, variable, result.clone()),
                _ => {
                    return Err(Control::Error(String::from(
                        "Interpreter: assign lhs is not a variable",
                    )));
                }
            }
            return Ok(result);
        }

        let result = match (lhs_val, rhs_val) {
            (Value::Number(a), Value::Number(b)) => Value::Number(op(a, b)),
            _ => {
                return Err(Control::Error(format!(
                    "Interpreter: {op_name} assign on non-numbers"
                )));
            }
        };
        match lhs {
            AstNode::Variable(variable) => self.set_var(None, variable, result.clone()),
            _ => {
                return Err(Control::Error(String::from(
                    "Interpreter: assign lhs is not a variable",
                )));
            }
        }
        Ok(result)
    }

    fn binary_op_assign<F>(
        &mut self,
        lhs: &AstNode,
        rhs: &AstNode,
        op: F,
        op_name: &str,
    ) -> Result<Value, Control>
    where
        F: Fn(i32, i32) -> i32,
    {
        let lhs_val = self.eval_node(lhs)?;
        let rhs_val = self.eval_node(rhs)?;
        let result = match (lhs_val, rhs_val) {
            (Value::Number(a), Value::Number(b)) => Value::Number(op(a as i32, b as i32) as f64),
            _ => {
                return Err(Control::Error(format!(
                    "Interpreter: {op_name} assign on non-numbers"
                )));
            }
        };
        match lhs {
            AstNode::Variable(variable) => self.set_var(None, variable, result.clone()),
            _ => {
                return Err(Control::Error(String::from(
                    "Interpreter: assign lhs is not a variable",
                )));
            }
        }
        Ok(result)
    }
}
