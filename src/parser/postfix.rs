impl Parser {
    fn dynamic_member_callable(owner: Expr, member: Expr) -> Expr {
        Expr::ArrayLiteral(vec![
            ArrayElement {
                key: None,
                value: owner,
                unpack: false,
                unpack_line: None,
                by_reference: false,
            },
            ArrayElement {
                key: None,
                value: member,
                unpack: false,
                unpack_line: None,
                by_reference: false,
            },
        ])
    }

    /// After `::`, one leading `$` is the static-member indirection marker,
    /// not another ordinary variable-variable layer. Thus `C::$$name` names
    /// the property selected by `$name`, while `C::$$$name` selects it by
    /// `$$name`.
    fn parse_indirect_static_member_name(&mut self) -> Result<(Expr, usize), String> {
        let line = match self.advance() {
            Token::Dollar(line) => line,
            token => return Err(format!("Expected dynamic static member, got {token:?}")),
        };
        let name = if self.peek() == Token::LBrace {
            self.advance();
            let name = self.parse_expr()?;
            self.expect(&Token::RBrace)?;
            name
        } else {
            self.parse_primary_atom()?
        };
        Ok((name, line))
    }

    /// Parse the variable/property/dimension expression that supplies a
    /// dynamic class name. A following `(` belongs to the constructor, not to
    /// the final property as an ordinary dynamic call.
    fn parse_dynamic_new_class_expression(&mut self, mut expr: Expr) -> Result<Expr, String> {
        loop {
            match self.peek() {
                Token::LBracket(line) if self.peek_at(1) != Token::RBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(&Token::RBracket)?;
                    expr = Expr::ArrayAccess {
                        array: Box::new(expr),
                        index: Box::new(index),
                        line,
                    };
                }
                Token::Arrow | Token::NullSafe => {
                    let nullsafe = matches!(self.peek(), Token::NullSafe);
                    self.advance();
                    if self.peek() == Token::LBrace {
                        self.advance();
                        let property = self.parse_expr()?;
                        self.expect(&Token::RBrace)?;
                        expr = Expr::DynamicPropertyAccess {
                            object: Box::new(expr),
                            property: Box::new(property),
                            nullsafe,
                            line: self.last_primary_line.unwrap_or(0),
                        };
                        continue;
                    }
                    if let Token::Variable(property, _) = self.peek() {
                        self.advance();
                        expr = Expr::DynamicPropertyAccess {
                            object: Box::new(expr),
                            property: Box::new(Expr::Variable {
                                name: property,
                                line: 0,
                            }),
                            nullsafe,
                            line: self.last_primary_line.unwrap_or(0),
                        };
                        continue;
                    }
                    if matches!(self.peek(), Token::Dollar(_)) {
                        let property = self.parse_primary_atom()?;
                        expr = Expr::DynamicPropertyAccess {
                            object: Box::new(expr),
                            property: Box::new(property),
                            nullsafe,
                            line: self.last_primary_line.unwrap_or(0),
                        };
                        continue;
                    }
                    let token = self.advance();
                    let property = Self::token_as_named_arg_label(&token).ok_or_else(|| {
                        format!("Expected property name in dynamic class expression, got {token:?}")
                    })?;
                    expr = Expr::PropertyAccess {
                        object: Box::new(expr),
                        property,
                        nullsafe,
                        line: self.last_primary_line.unwrap_or(0),
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// Parse a statically named member after a class-like owner. The shared
    /// postfix loop in `parse_power` continues the resulting expression.
    fn parse_named_static_access(&mut self, class_name: String) -> Result<Expr, String> {
        self.expect(&Token::DoubleColon)?;
        if self.peek() == Token::LBrace {
            self.advance();
            let constant = self.parse_expr()?;
            self.expect(&Token::RBrace)?;
            if matches!(self.peek(), Token::LParen(_)) {
                let line = self.expect_lparen()?;
                let args = self.parse_call_args()?;
                return Ok(Expr::DynamicCall {
                    callable: Box::new(Self::dynamic_member_callable(
                        Expr::ClassConstant {
                            class_name,
                            constant: "class".to_string(),
                            line,
                        },
                        constant,
                    )),
                    args,
                    generic_args: Vec::new(),
                    line,
                });
            }
            return Ok(Expr::DynamicNamedClassConstant {
                class_name,
                constant: Box::new(constant),
            });
        }
        if matches!(self.peek(), Token::Dollar(_)) {
            let (property, dollar_line) = self.parse_indirect_static_member_name()?;
            if matches!(self.peek(), Token::LParen(_)) {
                let line = self.expect_lparen()?;
                let args = self.parse_call_args()?;
                return Ok(Expr::DynamicCall {
                    callable: Box::new(Self::dynamic_member_callable(
                        Expr::ClassConstant {
                            class_name,
                            constant: "class".to_string(),
                            line,
                        },
                        Expr::DynamicVariable {
                            name: Box::new(property),
                            line: dollar_line,
                        },
                    )),
                    args,
                    generic_args: Vec::new(),
                    line,
                });
            }
            return Ok(Expr::DynamicNamedStaticProperty {
                class_name,
                property: Box::new(property),
                line: dollar_line,
            });
        }
        if let Token::Variable(_, _) = self.peek() {
            let (property, property_line) = match self.advance() {
                Token::Variable(name, line) => (name, line),
                _ => unreachable!(),
            };
            if matches!(self.peek(), Token::LParen(_)) {
                let line = self.expect_lparen()?;
                let args = self.parse_call_args()?;
                return Ok(Expr::DynamicCall {
                    callable: Box::new(Self::dynamic_member_callable(
                        Expr::ClassConstant {
                            class_name,
                            constant: "class".to_string(),
                            line,
                        },
                        Expr::Variable {
                            name: property,
                            line: property_line,
                        },
                    )),
                    args,
                    generic_args: Vec::new(),
                    line,
                });
            }
            return Ok(Expr::StaticProperty {
                class_name,
                property,
                parenthesized: false,
                line: property_line,
            });
        }

        let (member, member_line) = match self.advance() {
            Token::Identifier(name, line) => (name, Some(line)),
            Token::Class => ("class".to_string(), None),
            Token::From => ("from".to_string(), None),
            other => return Err(format!("Expected member name after ::, got {:?}", other)),
        };
        let generic_args = self.parse_optional_turbofish()?;
        if !matches!(self.peek(), Token::LParen(_)) {
            if !generic_args.is_empty() {
                return Err("Generic type arguments must be followed by a method call".into());
            }
            return Ok(Expr::ClassConstant {
                class_name,
                constant: member,
                line: member_line.unwrap_or(self.last_primary_line.unwrap_or(0)),
            });
        }

        let paren_line = self.expect_lparen()?;
        let line = member_line.unwrap_or(paren_line);
        if matches!(self.peek(), Token::DotDotDot(_)) && self.peek_at(1) == Token::RParen {
            if !generic_args.is_empty() {
                return Err("Generic first-class static callables are not supported yet".into());
            }
            self.advance();
            self.advance();
            return Ok(Expr::FirstClassCallable(Box::new(Expr::ArrayLiteral(vec![
                ArrayElement {
                    key: None,
                    value: Expr::ClassConstant {
                        class_name,
                        constant: "class".to_string(),
                        line,
                    },
                    unpack: false,
                    unpack_line: None,
                    by_reference: false,
                },
                ArrayElement {
                    key: None,
                    value: Expr::StringLiteral(member),
                    unpack: false,
                    unpack_line: None,
                    by_reference: false,
                },
            ]))));
        }
        let args = self.parse_call_args()?;
        Ok(Expr::StaticCall {
            class_name,
            method: member,
            args,
            generic_args,
            line,
        })
    }

    /// Parse every PHP postfix family from one left-to-right loop so literals,
    /// calls, arrays, closures and named/static expressions cannot drift apart.
    fn parse_postfix_chain(&mut self, mut expr: Expr) -> Result<Expr, String> {
        let expression_line = self.last_primary_line;
        loop {
            match self.peek() {
                Token::LBracket(line) => {
                    // An empty dimension is only valid as a write target. Leave
                    // it for the statement parser instead of trying to parse
                    // `]` as an index expression.
                    if self.peek_at(1) == Token::RBracket {
                        if self.preserve_empty_dimension_suffix
                            || self.peek_at(2) == Token::Assign
                        {
                            break;
                        }
                        self.advance();
                        self.advance();
                        let message = if self.empty_dimension_unset_context {
                            "Cannot use [] for unsetting"
                        } else {
                            "Cannot use [] for reading"
                        };
                        expr = self.compile_error(message, expression_line.unwrap_or(line));
                        continue;
                    }
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(&Token::RBracket)?;
                    expr = Expr::ArrayAccess {
                        array: Box::new(expr),
                        index: Box::new(index),
                        line,
                    };
                }
                Token::LParen(line) => {
                    self.advance();
                    if matches!(self.peek(), Token::DotDotDot(_))
                        && self.peek_at(1) == Token::RParen
                    {
                        self.advance();
                        self.advance();
                        expr = Expr::FirstClassCallable(Box::new(expr));
                    } else {
                        let args = self.parse_call_args()?;
                        expr = Expr::DynamicCall {
                            callable: Box::new(expr),
                            args,
                            generic_args: Vec::new(),
                            line,
                        };
                    }
                }
                Token::DoubleColon if self.peek_at(1) == Token::Less => {
                    let generic_args = self.parse_optional_turbofish()?;
                    let line = self.expect_lparen()?;
                    let args = self.parse_call_args()?;
                    expr = Expr::DynamicCall {
                        callable: Box::new(expr),
                        args,
                        generic_args,
                        line,
                    };
                }
                Token::DoubleColon => {
                    self.advance();
                    if matches!(self.peek(), Token::Dollar(_)) {
                        let (property, dollar_line) = self.parse_indirect_static_member_name()?;
                        if matches!(self.peek(), Token::LParen(_)) {
                            let line = self.expect_lparen()?;
                            let args = self.parse_call_args()?;
                            expr = Expr::DynamicStaticCall {
                                class: Box::new(expr),
                                method: Box::new(Expr::DynamicVariable {
                                    name: Box::new(property),
                                    line: dollar_line,
                                }),
                                args,
                                generic_args: Vec::new(),
                                line,
                            };
                        } else {
                            expr = Expr::DynamicStaticProperty {
                                class: Box::new(expr),
                                property: Box::new(property),
                                line: dollar_line,
                            };
                        }
                        continue;
                    }
                    if let Token::Variable(member_name, member_line) = self.peek() {
                        self.advance();
                        if matches!(self.peek(), Token::LParen(_)) {
                            let line = self.expect_lparen()?;
                            let args = self.parse_call_args()?;
                            expr = Expr::DynamicStaticCall {
                                class: Box::new(expr),
                                method: Box::new(Expr::Variable {
                                    name: member_name,
                                    line: member_line,
                                }),
                                args,
                                generic_args: Vec::new(),
                                line,
                            };
                        } else {
                            expr = Expr::DynamicStaticProperty {
                                class: Box::new(expr),
                                property: Box::new(Expr::StringLiteral(member_name)),
                                line: member_line,
                            };
                        }
                        continue;
                    }
                    let dynamic_name = self.peek() == Token::LBrace;
                    let constant = if dynamic_name {
                        self.advance();
                        let constant = self.parse_expr()?;
                        self.expect(&Token::RBrace)?;
                        constant
                    } else {
                        let token = self.advance();
                        let name = Self::token_as_named_arg_label(&token).ok_or_else(|| {
                            format!("Expected member name after dynamic ::, got {token:?}")
                        })?;
                        Expr::StringLiteral(name)
                    };
                    if matches!(self.peek(), Token::LParen(_)) {
                        let line = self.expect_lparen()?;
                        let args = self.parse_call_args()?;
                        expr = Expr::DynamicStaticCall {
                            class: Box::new(expr),
                            method: Box::new(constant),
                            args,
                            generic_args: Vec::new(),
                            line,
                        };
                        continue;
                    }
                    expr = Expr::DynamicClassConstant {
                        class: Box::new(expr),
                        constant: Box::new(constant),
                        dynamic_name,
                    };
                }
                Token::Arrow | Token::NullSafe => {
                    let nullsafe = matches!(self.peek(), Token::NullSafe);
                    self.advance();
                    if self.peek() == Token::LBrace {
                        self.advance();
                        let member = self.parse_expr()?;
                        self.expect(&Token::RBrace)?;
                        if matches!(self.peek(), Token::LParen(_)) {
                            if nullsafe {
                                return Err(
                                    "Dynamic nullsafe method calls are not supported yet".into()
                                );
                            }
                            let line = self.expect_lparen()?;
                            let args = self.parse_call_args()?;
                            expr = Expr::DynamicCall {
                                callable: Box::new(Expr::ArrayLiteral(vec![
                                    ArrayElement {
                                        key: None,
                                        value: expr,
                                        unpack: false,
                                        unpack_line: None,
                                        by_reference: false,
                                    },
                                    ArrayElement {
                                        key: None,
                                        value: member,
                                        unpack: false,
                                        unpack_line: None,
                                        by_reference: false,
                                    },
                                ])),
                                args,
                                generic_args: Vec::new(),
                                line,
                            };
                        } else {
                            expr = Expr::DynamicPropertyAccess {
                                object: Box::new(expr),
                                property: Box::new(member),
                                nullsafe,
                                line: expression_line.unwrap_or(0),
                            };
                        }
                        continue;
                    }
                    if matches!(self.peek(), Token::Dollar(_)) {
                        let member = self.parse_primary_atom()?;
                        if matches!(self.peek(), Token::LParen(_)) {
                            if nullsafe {
                                return Err(
                                    "Dynamic nullsafe method calls are not supported yet".into(),
                                );
                            }
                            let line = self.expect_lparen()?;
                            let args = self.parse_call_args()?;
                            expr = Expr::DynamicCall {
                                callable: Box::new(Self::dynamic_member_callable(expr, member)),
                                args,
                                generic_args: Vec::new(),
                                line,
                            };
                        } else {
                            expr = Expr::DynamicPropertyAccess {
                                object: Box::new(expr),
                                property: Box::new(member),
                                nullsafe,
                                line: expression_line.unwrap_or(0),
                            };
                        }
                        continue;
                    }
                    if let Token::Variable(member_name, _) = self.peek() {
                        self.advance();
                        let member = Expr::Variable {
                            name: member_name,
                            line: 0,
                        };
                        if matches!(self.peek(), Token::LParen(_)) {
                            if nullsafe {
                                return Err(
                                    "Dynamic nullsafe method calls are not supported yet".into()
                                );
                            }
                            let line = self.expect_lparen()?;
                            let args = self.parse_call_args()?;
                            expr = Expr::DynamicCall {
                                callable: Box::new(Expr::ArrayLiteral(vec![
                                    ArrayElement {
                                        key: None,
                                        value: expr,
                                        unpack: false,
                                        unpack_line: None,
                                        by_reference: false,
                                    },
                                    ArrayElement {
                                        key: None,
                                        value: member,
                                        unpack: false,
                                        unpack_line: None,
                                        by_reference: false,
                                    },
                                ])),
                                args,
                                generic_args: Vec::new(),
                                line,
                            };
                        } else {
                            expr = Expr::DynamicPropertyAccess {
                                object: Box::new(expr),
                                property: Box::new(member),
                                nullsafe,
                                line: expression_line.unwrap_or(0),
                            };
                        }
                        continue;
                    }
                    let token = self.advance();
                    let member_line = match &token {
                        Token::Identifier(_, line) => Some(*line),
                        Token::Goto { line, .. }
                        | Token::Echo { line }
                        | Token::MagicConstant { line, .. } => Some(*line),
                        Token::New(line) | Token::Throw(line) => Some(*line as usize),
                        _ => None,
                    };
                    let member = Self::token_as_named_arg_label(&token).ok_or_else(|| {
                        format!(
                            "Expected property/method name after {}, got {:?}",
                            if nullsafe { "?->" } else { "->" },
                            token
                        )
                    })?;
                    let generic_args = self.parse_optional_turbofish()?;
                    if matches!(self.peek(), Token::LParen(_)) {
                        let paren_line = self.expect_lparen()?;
                        let line = member_line.unwrap_or(paren_line);
                        if matches!(self.peek(), Token::DotDotDot(_))
                            && self.peek_at(1) == Token::RParen
                        {
                            if nullsafe {
                                return Err(
                                    "Cannot create a first-class callable from nullsafe method syntax"
                                        .into(),
                                );
                            }
                            if !generic_args.is_empty() {
                                return Err(
                                    "Generic first-class method callables are not supported yet"
                                        .into(),
                                );
                            }
                            self.advance();
                            self.advance();
                            expr = Expr::FirstClassCallable(Box::new(Expr::ArrayLiteral(vec![
                                ArrayElement {
                                    key: None,
                                    value: expr,
                                    unpack: false,
                                    unpack_line: None,
                                    by_reference: false,
                                },
                                ArrayElement {
                                    key: None,
                                    value: Expr::StringLiteral(member),
                                    unpack: false,
                                    unpack_line: None,
                                    by_reference: false,
                                },
                            ])));
                            continue;
                        }
                        let args = self.parse_call_args()?;
                        expr = Expr::MethodCall {
                            object: Box::new(expr),
                            method: member,
                            args,
                            generic_args,
                            nullsafe,
                            line,
                        };
                    } else {
                        expr = Expr::PropertyAccess {
                            object: Box::new(expr),
                            property: member,
                            nullsafe,
                            line: member_line.or(expression_line).unwrap_or(0),
                        };
                    }
                }
                Token::PlusPlus | Token::MinusMinus => {
                    let increment = self.peek() == Token::PlusPlus;
                    self.advance();
                    if let Some(line) = Self::nullsafe_chain_line(&expr) {
                        expr = self.nullsafe_write_error(line);
                        continue;
                    }
                    expr = match expr {
                        Expr::Variable { name, line } if increment => {
                            Expr::PostInc { name, line }
                        }
                        Expr::Variable { name, line } => Expr::PostDec { name, line },
                        Expr::Globals { line } => self.globals_modification_error(line),
                        Expr::DynamicVariable { .. }
                        | Expr::PropertyAccess {
                            nullsafe: false, ..
                        }
                        | Expr::DynamicPropertyAccess {
                            nullsafe: false, ..
                        }
                        | Expr::StaticProperty { .. }
                        | Expr::DynamicNamedStaticProperty { .. }
                        | Expr::DynamicStaticProperty { .. }
                        | Expr::ArrayAccess { .. } => {
                            if increment {
                                Expr::PostIncTarget(Box::new(expr))
                            } else {
                                Expr::PostDecTarget(Box::new(expr))
                            }
                        }
                        other => return Err(format!("Invalid increment target: {other:?}")),
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }
}
