/*
 * Copyright (c) 2023-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::rc::Rc;

use crate::Value;
use crate::lexer::Token;

#[derive(Debug, Clone, Copy)]
pub(crate) enum DeclarationType {
    Var,
    Let,
    Const,
}

#[derive(Debug)]
pub(crate) enum ObjectProperty {
    Literal(String),
    Computed(Box<AstNode>),
}

#[derive(Debug)]
pub(crate) enum AstNode {
    Block {
        label: Option<String>,
        nodes: Vec<AstNode>,
    },
    If {
        label: Option<String>,
        condition: Box<AstNode>,
        then_branch: Box<AstNode>,
        else_branch: Option<Box<AstNode>>,
    },
    Switch {
        label: Option<String>,
        expression: Box<AstNode>,
        cases: Vec<(AstNode, AstNode)>,
        default: Option<Box<AstNode>>,
    },
    While {
        label: Option<String>,
        condition: Box<AstNode>,
        body: Box<AstNode>,
    },
    DoWhile {
        label: Option<String>,
        body: Box<AstNode>,
        condition: Box<AstNode>,
    },
    For {
        label: Option<String>,
        init: Option<Box<AstNode>>,
        condition: Option<Box<AstNode>>,
        update: Option<Box<AstNode>>,
        body: Box<AstNode>,
    },
    ForIn {
        label: Option<String>,
        variable: String,
        declaration_type: Option<DeclarationType>,
        iterable: Box<AstNode>,
        body: Box<AstNode>,
    },
    ForOf {
        label: Option<String>,
        variable: String,
        declaration_type: Option<DeclarationType>,
        iterable: Box<AstNode>,
        body: Box<AstNode>,
    },
    Continue(Option<String>),
    Break(Option<String>),
    Return(Option<Box<AstNode>>),
    Comma(Vec<AstNode>),

    Value(Value),
    ArrayLiteral(Vec<AstNode>),
    ObjectLiteral(Vec<(ObjectProperty, AstNode)>),
    Variable(String),
    FunctionCall(Box<AstNode>, Vec<AstNode>),

    Assign(Option<DeclarationType>, Box<AstNode>, Box<AstNode>),
    AddAssign(Box<AstNode>, Box<AstNode>),
    SubtractAssign(Box<AstNode>, Box<AstNode>),
    MultiplyAssign(Box<AstNode>, Box<AstNode>),
    DivideAssign(Box<AstNode>, Box<AstNode>),
    RemainderAssign(Box<AstNode>, Box<AstNode>),
    ExponentiationAssign(Box<AstNode>, Box<AstNode>),
    BitwiseAndAssign(Box<AstNode>, Box<AstNode>),
    BitwiseOrAssign(Box<AstNode>, Box<AstNode>),
    BitwiseXorAssign(Box<AstNode>, Box<AstNode>),
    LeftShiftAssign(Box<AstNode>, Box<AstNode>),
    SignedRightShiftAssign(Box<AstNode>, Box<AstNode>),
    UnsignedRightShiftAssign(Box<AstNode>, Box<AstNode>),
    LogicalOrAssign(Box<AstNode>, Box<AstNode>),
    LogicalAndAssign(Box<AstNode>, Box<AstNode>),

    Ternary {
        condition: Box<AstNode>,
        then_branch: Box<AstNode>,
        else_branch: Box<AstNode>,
    },

    Add(Box<AstNode>, Box<AstNode>),
    Subtract(Box<AstNode>, Box<AstNode>),
    Multiply(Box<AstNode>, Box<AstNode>),
    Divide(Box<AstNode>, Box<AstNode>),
    Remainder(Box<AstNode>, Box<AstNode>),
    Exponentiation(Box<AstNode>, Box<AstNode>),
    BitwiseAnd(Box<AstNode>, Box<AstNode>),
    BitwiseOr(Box<AstNode>, Box<AstNode>),
    BitwiseXor(Box<AstNode>, Box<AstNode>),
    BitwiseNot(Box<AstNode>),
    LeftShift(Box<AstNode>, Box<AstNode>),
    SignedRightShift(Box<AstNode>, Box<AstNode>),
    UnsignedRightShift(Box<AstNode>, Box<AstNode>),
    Equals(Box<AstNode>, Box<AstNode>),
    StrictEquals(Box<AstNode>, Box<AstNode>),
    NotEquals(Box<AstNode>, Box<AstNode>),
    StrictNotEquals(Box<AstNode>, Box<AstNode>),
    LessThen(Box<AstNode>, Box<AstNode>),
    LessThenEquals(Box<AstNode>, Box<AstNode>),
    GreaterThen(Box<AstNode>, Box<AstNode>),
    GreaterThenEquals(Box<AstNode>, Box<AstNode>),
    LogicalAnd(Box<AstNode>, Box<AstNode>),
    LogicalOr(Box<AstNode>, Box<AstNode>),
    UnaryMinus(Box<AstNode>),
    UnaryLogicalNot(Box<AstNode>),
    UnaryTypeof(Box<AstNode>),
    UnaryPreIncrement(Box<AstNode>),
    UnaryPreDecrement(Box<AstNode>),
    UnaryPostIncrement(Box<AstNode>),
    UnaryPostDecrement(Box<AstNode>),

    GetProperty(Box<AstNode>, Box<AstNode>),
}

pub(crate) struct Parser<'a> {
    tokens: &'a Vec<Token>,
    position: usize,
}

impl<'a> Parser<'a> {
    pub(crate) const fn new(tokens: &'a Vec<Token>) -> Self {
        Parser {
            tokens,
            position: 0,
        }
    }

    pub(crate) fn parse(&mut self) -> Result<AstNode, String> {
        self.statements()
    }

    fn skip_whitespace(&mut self) {
        while let Token::Newline = self.tokens[self.position] {
            self.position += 1;
        }
    }

    fn peek(&mut self) -> &Token {
        self.skip_whitespace();
        &self.tokens[self.position]
    }

    fn peek_without_whitespace_skip(&mut self) -> &Token {
        &self.tokens[self.position]
    }

    fn next(&mut self) {
        self.skip_whitespace();
        self.position += 1;
    }

    fn statement_nodes(&mut self) -> Result<Vec<AstNode>, String> {
        let mut nodes = Vec::new();
        loop {
            match self.peek() {
                Token::Case | Token::Default | Token::RightBrace | Token::Eof => break,
                _ => nodes.push(self.statement()?),
            }

            // Automatic Semicolon Insertion (ASI) rules
            match self.peek_without_whitespace_skip() {
                Token::Semicolon | Token::Newline => {
                    self.next();
                }
                Token::RightBrace | Token::Eof => {
                    // ASI: semicolon inserted before }, EOF
                    break;
                }
                _ => {
                    // ASI: semicolon inserted at end of line (we assume line breaks here)
                    continue;
                }
            }
        }
        Ok(nodes)
    }

    fn statements(&mut self) -> Result<AstNode, String> {
        let nodes = self.statement_nodes()?;
        Ok(AstNode::Block { label: None, nodes })
    }

    fn block(&mut self) -> Result<AstNode, String> {
        let nodes = if let Token::LeftBrace = self.peek() {
            self.next();
            let nodes = self.statement_nodes()?;
            if let Token::RightBrace = self.peek() {
                self.next();
            } else {
                return Err(String::from("Parser: expected '}' at end of block"));
            }
            nodes
        } else {
            vec![self.statement()?]
        };
        Ok(AstNode::Block { label: None, nodes })
    }

    fn statement(&mut self) -> Result<AstNode, String> {
        let label = if let Token::Variable(var_name) = self.peek() {
            let label_name = var_name.clone();
            let saved_position = self.position;
            self.next();
            if let Token::Colon = self.peek() {
                self.next();
                Some(label_name)
            } else {
                self.position = saved_position;
                None
            }
        } else {
            None
        };

        match self.peek() {
            Token::LeftBrace => {
                self.next();
                let nodes = self.statement_nodes()?;
                if let Token::RightBrace = self.peek() {
                    self.next();
                } else {
                    return Err(String::from("Parser: expected '}' at end of block"));
                }
                Ok(AstNode::Block { label, nodes })
            }
            Token::If => {
                self.next();
                if let Token::LeftParen = self.peek() {
                    self.next();
                    let condition = self.comma()?;
                    if let Token::RightParen = self.peek() {
                        self.next();
                        let then_branch = self.block()?;
                        let else_branch = if let Token::Else = self.peek() {
                            self.next();
                            Some(Box::new(self.block()?))
                        } else {
                            None
                        };
                        Ok(AstNode::If {
                            label,
                            condition: Box::new(condition),
                            then_branch: Box::new(then_branch),
                            else_branch,
                        })
                    } else {
                        Err(String::from("Parser: expected ')' after if condition"))
                    }
                } else {
                    Err(String::from("Parser: expected '(' after 'if'"))
                }
            }
            Token::Switch => {
                self.next();
                if let Token::LeftParen = self.peek() {
                    self.next();
                    let expression = self.comma()?;
                    if let Token::RightParen = self.peek() {
                        self.next();
                        if let Token::LeftBrace = self.peek() {
                            self.next();
                            let mut cases = Vec::new();
                            let mut default = None;
                            loop {
                                match self.peek() {
                                    Token::Case => {
                                        self.next();
                                        let case_expr = self.comma()?;
                                        if let Token::Colon = self.peek() {
                                            self.next();
                                            let case_body = if let Token::LeftBrace = self.peek() {
                                                self.block()?
                                            } else {
                                                self.statements()?
                                            };
                                            cases.push((case_expr, case_body));
                                        } else {
                                            return Err(String::from(
                                                "Parser: expected ':' after case expression",
                                            ));
                                        }
                                    }
                                    Token::Default => {
                                        self.next();
                                        if let Token::Colon = self.peek() {
                                            self.next();
                                            let default_body = if let Token::LeftBrace = self.peek()
                                            {
                                                self.block()?
                                            } else {
                                                self.statements()?
                                            };
                                            default = Some(Box::new(default_body));
                                        } else {
                                            return Err(String::from(
                                                "Parser: expected ':' after default",
                                            ));
                                        }
                                    }
                                    Token::RightBrace => {
                                        self.next();
                                        break;
                                    }
                                    _ => {
                                        return Err(String::from(
                                            "Parser: expected 'case', 'default', or '}' in switch statement",
                                        ));
                                    }
                                }
                            }
                            Ok(AstNode::Switch {
                                label,
                                expression: Box::new(expression),
                                cases,
                                default,
                            })
                        } else {
                            Err(String::from("Parser: expected '{' after switch expression"))
                        }
                    } else {
                        Err(String::from("Parser: expected ')' after switch expression"))
                    }
                } else {
                    Err(String::from("Parser: expected '(' after 'switch'"))
                }
            }
            Token::While => {
                self.next();
                if let Token::LeftParen = self.peek() {
                    self.next();
                    let condition = self.comma()?;
                    if let Token::RightParen = self.peek() {
                        self.next();
                        let body = self.block()?;
                        Ok(AstNode::While {
                            label,
                            condition: Box::new(condition),
                            body: Box::new(body),
                        })
                    } else {
                        Err(String::from("Parser: expected ')' after while condition"))
                    }
                } else {
                    Err(String::from("Parser: expected '(' after 'while'"))
                }
            }
            Token::Do => {
                self.next();
                let body = self.block()?;
                if let Token::While = self.peek() {
                    self.next();
                    if let Token::LeftParen = self.peek() {
                        self.next();
                        let condition = self.comma()?;
                        if let Token::RightParen = self.peek() {
                            self.next();
                            Ok(AstNode::DoWhile {
                                label,
                                body: Box::new(body),
                                condition: Box::new(condition),
                            })
                        } else {
                            Err(String::from(
                                "Parser: expected ')' after do-while condition",
                            ))
                        }
                    } else {
                        Err(String::from(
                            "Parser: expected '(' after 'while' in do-while",
                        ))
                    }
                } else {
                    Err(String::from("Parser: expected 'while' after do block"))
                }
            }
            Token::For => {
                self.next();
                if let Token::LeftParen = self.peek() {
                    self.next();

                    // Check if it's for...in or for...of
                    // First, look ahead to see if we have a variable declaration or identifier followed by in/of
                    let checkpoint = self.position;
                    let mut is_for_in_of = false;
                    let mut decl_type = None;
                    let mut var_name = String::new();

                    // Check for var/let/const declaration or identifier
                    if matches!(self.peek(), Token::Var | Token::Let | Token::Const) {
                        let dt = match self.peek() {
                            Token::Var => Some(DeclarationType::Var),
                            Token::Let => Some(DeclarationType::Let),
                            Token::Const => Some(DeclarationType::Const),
                            _ => None,
                        };
                        self.next();
                        if let Token::Variable(name) = self.peek() {
                            var_name = name.clone();
                            self.next();
                            if matches!(self.peek(), Token::In | Token::Of) {
                                is_for_in_of = true;
                                decl_type = dt;
                            }
                        }
                    } else if let Token::Variable(name) = self.peek() {
                        var_name = name.clone();
                        self.next();
                        if matches!(self.peek(), Token::In | Token::Of) {
                            is_for_in_of = true;
                        }
                    }

                    // Reset to checkpoint if not for...in/of
                    if !is_for_in_of {
                        self.position = checkpoint;
                    }

                    if is_for_in_of {
                        // Parse for...in or for...of
                        let is_of = matches!(self.peek(), Token::Of);
                        self.next(); // consume 'in' or 'of'

                        let iterable = self.comma()?;

                        if let Token::RightParen = self.peek() {
                            self.next();
                            let body = self.block()?;

                            Ok(if is_of {
                                AstNode::ForOf {
                                    label,
                                    variable: var_name,
                                    declaration_type: decl_type,
                                    iterable: Box::new(iterable),
                                    body: Box::new(body),
                                }
                            } else {
                                AstNode::ForIn {
                                    label,
                                    variable: var_name,
                                    declaration_type: decl_type,
                                    iterable: Box::new(iterable),
                                    body: Box::new(body),
                                }
                            })
                        } else {
                            Err(String::from("Parser: expected ')' after for...in/of"))
                        }
                    } else {
                        // Parse traditional for loop
                        let init = if let Token::Semicolon = self.peek() {
                            None
                        } else {
                            Some(self.comma()?)
                        };
                        if let Token::Semicolon = self.peek() {
                            self.next();
                            let condition = if let Token::Semicolon = self.peek() {
                                None
                            } else {
                                Some(self.comma()?)
                            };
                            if let Token::Semicolon = self.peek() {
                                self.next();
                                let update = if let Token::RightParen = self.peek() {
                                    None
                                } else {
                                    Some(self.comma()?)
                                };
                                if let Token::RightParen = self.peek() {
                                    self.next();
                                    let body = self.block()?;

                                    Ok(AstNode::For {
                                        label,
                                        init: init.map(Box::new),
                                        condition: condition.map(Box::new),
                                        update: update.map(Box::new),
                                        body: Box::new(body),
                                    })
                                } else {
                                    Err(String::from("Parser: expected ')' after for loop"))
                                }
                            } else {
                                Err(String::from("Parser: expected ';' after for condition"))
                            }
                        } else {
                            Err(String::from("Parser: expected ';' after for init"))
                        }
                    }
                } else {
                    Err(String::from("Parser: expected '(' after 'for'"))
                }
            }
            Token::Break => {
                self.next();
                if let Token::Variable(var_name) = self.peek() {
                    let label_name = var_name.clone();
                    self.next();
                    Ok(AstNode::Break(Some(label_name)))
                } else {
                    Ok(AstNode::Break(None))
                }
            }
            Token::Continue => {
                self.next();
                if let Token::Variable(var_name) = self.peek() {
                    let label_name = var_name.clone();
                    self.next();
                    Ok(AstNode::Continue(Some(label_name)))
                } else {
                    Ok(AstNode::Continue(None))
                }
            }
            Token::Function => {
                self.next();
                let name = if let Token::Variable(var_name) = self.peek() {
                    let name = var_name.clone();
                    self.next();
                    name
                } else {
                    return Err(String::from(
                        "Parser: expected function name after 'function'",
                    ));
                };

                if let Token::LeftParen = self.peek() {
                    self.next();
                    let mut arguments = Vec::new();
                    if let Token::RightParen = self.peek() {
                        // No arguments
                        self.next();
                    } else {
                        loop {
                            if let Token::Variable(arg_name) = self.peek() {
                                arguments.push(arg_name.clone());
                                self.next();
                            } else {
                                return Err(String::from(
                                    "Parser: expected argument name in function definition",
                                ));
                            }

                            match self.peek() {
                                Token::Comma => {
                                    self.next();
                                }
                                Token::RightParen => {
                                    self.next();
                                    break;
                                }
                                _ => {
                                    return Err(String::from(
                                        "Parser: expected ',' or ')' in function arguments",
                                    ));
                                }
                            }
                        }
                    }
                    let body = self.block()?;
                    Ok(AstNode::Assign(
                        Some(DeclarationType::Var),
                        Box::new(AstNode::Variable(name)),
                        Box::new(AstNode::Value(Value::Function(Rc::new((arguments, body))))),
                    ))
                } else {
                    Err(String::from("Parser: expected '(' after function name"))
                }
            }
            Token::Return => {
                self.next();
                let expr =
                    if let Token::Semicolon | Token::Newline | Token::RightBrace | Token::Eof =
                        self.peek_without_whitespace_skip()
                    {
                        None
                    } else {
                        Some(Box::new(self.comma()?))
                    };
                Ok(AstNode::Return(expr))
            }
            _ => self.comma(),
        }
    }

    fn comma(&mut self) -> Result<AstNode, String> {
        let node = self.assign()?;
        if let Token::Comma = self.peek() {
            let mut nodes = vec![node];
            while let Token::Comma = self.peek() {
                self.next();
                nodes.push(self.assign()?);
            }
            Ok(AstNode::Comma(nodes))
        } else {
            Ok(node)
        }
    }

    fn assign(&mut self) -> Result<AstNode, String> {
        let declaration_type = match self.peek() {
            Token::Var => {
                self.next();
                Some(DeclarationType::Var)
            }
            Token::Let => {
                self.next();
                Some(DeclarationType::Let)
            }
            Token::Const => {
                self.next();
                Some(DeclarationType::Const)
            }
            _ => None,
        };

        let lhs = self.ternary()?;
        match self.peek() {
            Token::Assign => {
                self.next();
                Ok(AstNode::Assign(
                    declaration_type,
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::AddAssign => {
                self.next();
                Ok(AstNode::AddAssign(Box::new(lhs), Box::new(self.assign()?)))
            }
            Token::SubtractAssign => {
                self.next();
                Ok(AstNode::SubtractAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::MultiplyAssign => {
                self.next();
                Ok(AstNode::MultiplyAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::DivideAssign => {
                self.next();
                Ok(AstNode::DivideAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::RemainderAssign => {
                self.next();
                Ok(AstNode::RemainderAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::ExponentiationAssign => {
                self.next();
                Ok(AstNode::ExponentiationAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::BitwiseAndAssign => {
                self.next();
                Ok(AstNode::BitwiseAndAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::BitwiseOrAssign => {
                self.next();
                Ok(AstNode::BitwiseOrAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::BitwiseXorAssign => {
                self.next();
                Ok(AstNode::BitwiseXorAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::LeftShiftAssign => {
                self.next();
                Ok(AstNode::LeftShiftAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::SignedRightShiftAssign => {
                self.next();
                Ok(AstNode::SignedRightShiftAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::UnsignedRightShiftAssign => {
                self.next();
                Ok(AstNode::UnsignedRightShiftAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::LogicalOrAssign => {
                self.next();
                Ok(AstNode::LogicalOrAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            Token::LogicalAndAssign => {
                self.next();
                Ok(AstNode::LogicalAndAssign(
                    Box::new(lhs),
                    Box::new(self.assign()?),
                ))
            }
            _ => {
                if declaration_type.is_some() {
                    return Err(String::from(
                        "Parser: expected '=' after variable declaration",
                    ));
                }
                Ok(lhs)
            }
        }
    }

    fn ternary(&mut self) -> Result<AstNode, String> {
        let condition = self.logical()?;
        if let Token::Question = self.peek() {
            self.next();
            let if_branch = self.ternary()?;
            if let Token::Colon = self.peek() {
                self.next();
                let else_branch = self.ternary()?;
                Ok(AstNode::Ternary {
                    condition: Box::new(condition),
                    then_branch: Box::new(if_branch),
                    else_branch: Box::new(else_branch),
                })
            } else {
                Err(String::from("Parser: expected ':' in ternary expression"))
            }
        } else {
            Ok(condition)
        }
    }

    fn logical(&mut self) -> Result<AstNode, String> {
        let mut node = self.relational()?;
        loop {
            match self.peek() {
                Token::LogicalAnd => {
                    self.next();
                    node = AstNode::LogicalAnd(Box::new(node), Box::new(self.relational()?));
                }
                Token::LogicalOr => {
                    self.next();
                    node = AstNode::LogicalOr(Box::new(node), Box::new(self.relational()?));
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn relational(&mut self) -> Result<AstNode, String> {
        let mut node = self.equality()?;
        loop {
            match self.peek() {
                Token::LessThen => {
                    self.next();
                    node = AstNode::LessThen(Box::new(node), Box::new(self.equality()?));
                }
                Token::LessThenEquals => {
                    self.next();
                    node = AstNode::LessThenEquals(Box::new(node), Box::new(self.equality()?));
                }
                Token::GreaterThen => {
                    self.next();
                    node = AstNode::GreaterThen(Box::new(node), Box::new(self.equality()?));
                }
                Token::GreaterThenEquals => {
                    self.next();
                    node = AstNode::GreaterThenEquals(Box::new(node), Box::new(self.equality()?));
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn equality(&mut self) -> Result<AstNode, String> {
        let mut node = self.shift()?;
        loop {
            match self.peek() {
                Token::Equals => {
                    self.next();
                    node = AstNode::Equals(Box::new(node), Box::new(self.shift()?));
                }
                Token::StrictEquals => {
                    self.next();
                    node = AstNode::StrictEquals(Box::new(node), Box::new(self.shift()?));
                }
                Token::NotEquals => {
                    self.next();
                    node = AstNode::NotEquals(Box::new(node), Box::new(self.shift()?));
                }
                Token::StrictNotEquals => {
                    self.next();
                    node = AstNode::StrictNotEquals(Box::new(node), Box::new(self.shift()?));
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn shift(&mut self) -> Result<AstNode, String> {
        let mut node = self.bitwise()?;
        loop {
            match self.peek() {
                Token::LeftShift => {
                    self.next();
                    node = AstNode::LeftShift(Box::new(node), Box::new(self.bitwise()?));
                }
                Token::SignedRightShift => {
                    self.next();
                    node = AstNode::SignedRightShift(Box::new(node), Box::new(self.bitwise()?));
                }
                Token::UnsignedRightShift => {
                    self.next();
                    node = AstNode::UnsignedRightShift(Box::new(node), Box::new(self.bitwise()?));
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn bitwise(&mut self) -> Result<AstNode, String> {
        let mut node = self.add()?;
        loop {
            match self.peek() {
                Token::BitwiseAnd => {
                    self.next();
                    node = AstNode::BitwiseAnd(Box::new(node), Box::new(self.add()?));
                }
                Token::BitwiseOr => {
                    self.next();
                    node = AstNode::BitwiseOr(Box::new(node), Box::new(self.add()?));
                }
                Token::BitwiseXor => {
                    self.next();
                    node = AstNode::BitwiseXor(Box::new(node), Box::new(self.add()?));
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn add(&mut self) -> Result<AstNode, String> {
        let mut node = self.mul()?;
        loop {
            match self.peek() {
                Token::Add => {
                    self.next();
                    node = AstNode::Add(Box::new(node), Box::new(self.mul()?));
                }
                Token::Subtract => {
                    self.next();
                    node = AstNode::Subtract(Box::new(node), Box::new(self.mul()?));
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn mul(&mut self) -> Result<AstNode, String> {
        let mut node = self.unary()?;
        loop {
            match self.peek() {
                Token::Multiply => {
                    self.next();
                    node = AstNode::Multiply(Box::new(node), Box::new(self.unary()?));
                }
                Token::Divide => {
                    self.next();
                    node = AstNode::Divide(Box::new(node), Box::new(self.unary()?));
                }
                Token::Remainder => {
                    self.next();
                    node = AstNode::Remainder(Box::new(node), Box::new(self.unary()?));
                }
                Token::Exponentiation => {
                    self.next();
                    node = AstNode::Exponentiation(Box::new(node), Box::new(self.unary()?));
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn unary(&mut self) -> Result<AstNode, String> {
        match self.peek() {
            Token::Add => {
                self.next();
                self.unary()
            }
            Token::Subtract => {
                self.next();
                Ok(AstNode::UnaryMinus(Box::new(self.unary()?)))
            }
            Token::BitwiseNot => {
                self.next();
                Ok(AstNode::BitwiseNot(Box::new(self.unary()?)))
            }
            Token::LogicalNot => {
                self.next();
                Ok(AstNode::UnaryLogicalNot(Box::new(self.unary()?)))
            }
            Token::Increment => {
                self.next();
                Ok(AstNode::UnaryPreIncrement(Box::new(self.unary()?)))
            }
            Token::Decrement => {
                self.next();
                Ok(AstNode::UnaryPreDecrement(Box::new(self.unary()?)))
            }
            Token::Typeof => {
                self.next();
                Ok(AstNode::UnaryTypeof(Box::new(self.unary()?)))
            }
            _ => {
                let node = self.primary();
                self.postfix(node)
            }
        }
    }

    fn postfix(&mut self, node_result: Result<AstNode, String>) -> Result<AstNode, String> {
        let mut node = node_result?;
        loop {
            match self.peek() {
                Token::Period => {
                    self.next();
                    if let Token::Variable(prop_name) = self.peek() {
                        let property = AstNode::Value(Value::String(prop_name.clone()));
                        self.next();
                        node = AstNode::GetProperty(Box::new(node), Box::new(property));
                    } else {
                        return Err(String::from("Parser: expected property name after '.'"));
                    }
                }
                Token::LeftBlock => {
                    self.next();
                    let index_expr = self.comma()?;
                    if let Token::RightBlock = self.peek() {
                        self.next();
                        node = AstNode::GetProperty(Box::new(node), Box::new(index_expr));
                    } else {
                        return Err(String::from("Parser: expected ']' after property access"));
                    }
                }
                Token::LeftParen => {
                    self.next();
                    let mut call_args = Vec::new();
                    if let Token::RightParen = self.peek() {
                        // No arguments
                        self.next();
                    } else {
                        loop {
                            call_args.push(self.ternary()?);
                            match self.peek() {
                                Token::Comma => {
                                    self.next();
                                }
                                Token::RightParen => {
                                    self.next();
                                    break;
                                }
                                _ => {
                                    return Err(String::from(
                                        "Parser: expected ',' or ')' in function call",
                                    ));
                                }
                            }
                        }
                    }
                    node = AstNode::FunctionCall(Box::new(node), call_args);
                }
                Token::Increment => {
                    self.next();
                    return Ok(AstNode::UnaryPostIncrement(Box::new(node)));
                }
                Token::Decrement => {
                    self.next();
                    return Ok(AstNode::UnaryPostDecrement(Box::new(node)));
                }
                _ => return Ok(node),
            }
        }
    }

    fn arrow_function_body(&mut self) -> Result<AstNode, String> {
        if let Token::LeftBrace = self.peek() {
            Ok(self.block()?)
        } else {
            Ok(AstNode::Return(Some(Box::new(self.ternary()?))))
        }
    }

    fn primary(&mut self) -> Result<AstNode, String> {
        match self.peek() {
            Token::LeftParen => {
                self.next();
                let node = self.comma()?;
                if let Token::RightParen = self.peek() {
                    self.next();
                } else {
                    return Err(String::from("Parser: expected ')' after expression"));
                }

                // Arrow function
                if let Token::Arrow = self.peek() {
                    self.next();
                    let mut function_args = Vec::new();
                    if let AstNode::Variable(var_name) = node {
                        function_args.push(var_name);
                    } else if let AstNode::Comma(arg_nodes) = node {
                        for arg_node in arg_nodes {
                            if let AstNode::Variable(var_name) = arg_node {
                                function_args.push(var_name);
                            } else {
                                return Err(String::from(
                                    "Parser: expected variable name in arrow function arguments",
                                ));
                            }
                        }
                    }
                    let body = self.arrow_function_body()?;
                    return Ok(AstNode::Value(Value::Function(Rc::new((
                        function_args,
                        body,
                    )))));
                }

                Ok(node)
            }
            Token::Null => {
                self.next();
                Ok(AstNode::Value(Value::Null))
            }
            Token::Boolean(boolean) => {
                let node = AstNode::Value(Value::Boolean(*boolean));
                self.next();
                Ok(node)
            }
            Token::Number(number) => {
                let node = AstNode::Value(Value::Number(*number));
                self.next();
                Ok(node)
            }
            Token::String(string) => {
                let node = AstNode::Value(Value::String(string.clone()));
                self.next();
                Ok(node)
            }
            Token::LeftBlock => {
                self.next();
                let mut nodes = Vec::new();
                if let Token::RightBlock = self.peek() {
                    // Empty array
                    self.next();
                } else {
                    loop {
                        nodes.push(self.assign()?);
                        match self.peek() {
                            Token::Comma => {
                                self.next();
                            }
                            Token::RightBlock => {
                                self.next();
                                break;
                            }
                            _ => {
                                return Err(String::from(
                                    "Parser: expected ',' or ']' in array literal",
                                ));
                            }
                        }
                    }
                }
                Ok(AstNode::ArrayLiteral(nodes))
            }
            Token::LeftBrace => {
                self.next();
                let mut properties = Vec::new();
                if let Token::RightBrace = self.peek() {
                    // Empty object
                    self.next();
                } else {
                    loop {
                        // Handle computed property names: [expr]: value
                        if let Token::LeftBlock = self.peek() {
                            self.next();
                            let key_expr = self.assign()?;
                            if let Token::RightBlock = self.peek() {
                                self.next();
                            } else {
                                return Err(String::from(
                                    "Parser: expected ']' after computed property name",
                                ));
                            }
                            if let Token::Colon = self.peek() {
                                self.next();
                                let property_value = self.assign()?;
                                let property_key = ObjectProperty::Computed(Box::new(key_expr));
                                properties.push((property_key, property_value));
                            } else {
                                return Err(String::from(
                                    "Parser: expected ':' after computed property name",
                                ));
                            }
                        } else if let Token::Variable(prop_name) = self.peek() {
                            let name = prop_name.clone();
                            self.next();

                            // Handle method shorthand: methodName() { ... }
                            if let Token::LeftParen = self.peek() {
                                self.next();
                                let mut params = vec![];
                                if let Token::RightParen = self.peek() {
                                    self.next();
                                } else {
                                    loop {
                                        if let Token::Variable(param) = self.peek() {
                                            params.push(param.clone());
                                            self.next();
                                        } else {
                                            return Err(String::from(
                                                "Parser: expected parameter name in method",
                                            ));
                                        }
                                        match self.peek() {
                                            Token::Comma => {
                                                self.next();
                                            }
                                            Token::RightParen => {
                                                self.next();
                                                break;
                                            }
                                            _ => {
                                                return Err(String::from(
                                                    "Parser: expected ',' or ')' in method parameters",
                                                ));
                                            }
                                        }
                                    }
                                }
                                let property_value = if let Token::LeftBrace = self.peek() {
                                    let body = self.block()?;
                                    AstNode::Value(Value::Function(Rc::new((params, body))))
                                } else {
                                    return Err(String::from(
                                        "Parser: expected '{' for method body",
                                    ));
                                };
                                let property_key = ObjectProperty::Literal(name);
                                properties.push((property_key, property_value));
                            } else if let Token::Colon = self.peek() {
                                self.next();
                                let property_value = self.assign()?;
                                let property_key = ObjectProperty::Literal(name);
                                properties.push((property_key, property_value));
                            } else {
                                return Err(String::from(
                                    "Parser: expected ':' or '(' after property name in object literal",
                                ));
                            }
                        } else {
                            return Err(String::from(
                                "Parser: expected property name or computed property in object literal",
                            ));
                        }

                        match self.peek() {
                            Token::Comma => {
                                self.next();
                            }
                            Token::RightBrace => {
                                self.next();
                                break;
                            }
                            _ => {
                                return Err(String::from(
                                    "Parser: expected ',' or '}' in object literal",
                                ));
                            }
                        }
                    }
                }
                Ok(AstNode::ObjectLiteral(properties))
            }
            Token::Variable(variable) => {
                let variable = variable.clone();
                self.next();

                // Arrow function
                if let Token::Arrow = self.peek() {
                    self.next();
                    let body = self.arrow_function_body()?;
                    Ok(AstNode::Value(Value::Function(Rc::new((
                        vec![variable],
                        body,
                    )))))
                } else {
                    Ok(AstNode::Variable(variable))
                }
            }
            _ => Err(format!("Parser: unknown node type: {:?}", self.peek())),
        }
    }
}
