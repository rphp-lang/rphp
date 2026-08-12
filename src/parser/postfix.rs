impl Parser {
    /// Parse a statically named member after a class-like owner. The shared
    /// postfix loop in `parse_power` continues the resulting expression.
    fn parse_named_static_access(&mut self, class_name: String) -> Result<Expr, String> {
        self.expect(&Token::DoubleColon)?;
        if self.peek() == Token::LBrace {
            self.advance();
            let constant = self.parse_expr()?;
            self.expect(&Token::RBrace)?;
            if self.peek() == Token::LParen {
                return Err("Dynamic static method calls are not supported yet".into());
            }
            return Ok(Expr::DynamicNamedClassConstant {
                class_name,
                constant: Box::new(constant),
            });
        }
        if let Token::Variable(_) = self.peek() {
            let property = match self.advance() {
                Token::Variable(name) => name,
                _ => unreachable!(),
            };
            return Ok(Expr::StaticProperty {
                class_name,
                property,
            });
        }

        let member = match self.advance() {
            Token::Identifier(name) => name,
            Token::Class => "class".to_string(),
            other => return Err(format!("Expected member name after ::, got {:?}", other)),
        };
        let generic_args = self.parse_optional_turbofish()?;
        if self.peek() != Token::LParen {
            if !generic_args.is_empty() {
                return Err("Generic type arguments must be followed by a method call".into());
            }
            return Ok(Expr::ClassConstant {
                class_name,
                constant: member,
            });
        }

        self.advance();
        if self.peek() == Token::DotDotDot && self.peek_at(1) == Token::RParen {
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
                    },
                },
                ArrayElement {
                    key: None,
                    value: Expr::StringLiteral(member),
                },
            ]))));
        }
        let args = self.parse_call_args()?;
        Ok(Expr::StaticCall {
            class_name,
            method: member,
            args,
            generic_args,
        })
    }

    /// Parse every PHP postfix family from one left-to-right loop so literals,
    /// calls, arrays, closures and named/static expressions cannot drift apart.
    fn parse_postfix_chain(&mut self, mut expr: Expr) -> Result<Expr, String> {
        loop {
            match self.peek() {
                Token::LBracket => {
                    // An empty dimension is only valid as a write target. Leave
                    // it for the statement parser instead of trying to parse
                    // `]` as an index expression.
                    if self.peek_at(1) == Token::RBracket {
                        break;
                    }
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(&Token::RBracket)?;
                    expr = Expr::ArrayAccess {
                        array: Box::new(expr),
                        index: Box::new(index),
                    };
                }
                Token::LParen => {
                    self.advance();
                    if self.peek() == Token::DotDotDot && self.peek_at(1) == Token::RParen {
                        self.advance();
                        self.advance();
                        expr = Expr::FirstClassCallable(Box::new(expr));
                    } else {
                        let args = self.parse_call_args()?;
                        expr = Expr::DynamicCall {
                            callable: Box::new(expr),
                            args,
                            generic_args: Vec::new(),
                        };
                    }
                }
                Token::DoubleColon if self.peek_at(1) == Token::Less => {
                    let generic_args = self.parse_optional_turbofish()?;
                    self.expect(&Token::LParen)?;
                    let args = self.parse_call_args()?;
                    expr = Expr::DynamicCall {
                        callable: Box::new(expr),
                        args,
                        generic_args,
                    };
                }
                Token::DoubleColon => {
                    self.advance();
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
                    if self.peek() == Token::LParen {
                        self.advance();
                        let args = self.parse_call_args()?;
                        expr = Expr::DynamicCall {
                            callable: Box::new(Expr::ArrayLiteral(vec![
                                ArrayElement {
                                    key: None,
                                    value: expr,
                                },
                                ArrayElement {
                                    key: None,
                                    value: constant,
                                },
                            ])),
                            args,
                            generic_args: Vec::new(),
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
                    let token = self.advance();
                    let member = Self::token_as_named_arg_label(&token).ok_or_else(|| {
                        format!(
                            "Expected property/method name after {}, got {:?}",
                            if nullsafe { "?->" } else { "->" },
                            token
                        )
                    })?;
                    let generic_args = self.parse_optional_turbofish()?;
                    if self.peek() == Token::LParen {
                        self.advance();
                        if self.peek() == Token::DotDotDot && self.peek_at(1) == Token::RParen {
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
                                },
                                ArrayElement {
                                    key: None,
                                    value: Expr::StringLiteral(member),
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
                        };
                    } else {
                        expr = Expr::PropertyAccess {
                            object: Box::new(expr),
                            property: member,
                            nullsafe,
                        };
                    }
                }
                _ => break,
            }
        }
        Ok(expr)
    }
}
