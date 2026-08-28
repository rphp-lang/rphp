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

    fn consume_first_class_callable_placeholder(&mut self) -> bool {
        if !matches!(self.peek(), Token::DotDotDot(_)) || self.peek_at(1) != Token::RParen {
            return false;
        }
        self.advance();
        self.advance();
        true
    }

    fn first_class_callable(callable: Expr, line: usize) -> Expr {
        Expr::FirstClassCallable {
            callable: Box::new(callable),
            line,
        }
    }

    fn first_class_member_callable(
        owner: Expr,
        member: Expr,
        static_syntax: bool,
        line: usize,
    ) -> Expr {
        Expr::FirstClassMemberCallable {
            owner: Box::new(owner),
            member: Box::new(member),
            static_syntax,
            line,
        }
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
        let name = if matches!(self.peek(), Token::LBrace(_)) {
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
                    if matches!(self.peek(), Token::LBrace(_)) {
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
                Token::DoubleColon
                    if matches!(self.peek_at(1), Token::Dollar(_) | Token::Variable(_, _)) =>
                {
                    self.advance();
                    let (property, property_line) = if matches!(self.peek(), Token::Dollar(_)) {
                        self.parse_indirect_static_member_name()?
                    } else {
                        match self.advance() {
                            Token::Variable(name, line) => (Expr::StringLiteral(name), line),
                            _ => unreachable!(),
                        }
                    };
                    expr = Expr::DynamicStaticProperty {
                        class: Box::new(expr),
                        property: Box::new(property),
                        line: property_line,
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// A static property following an unparenthesized class name belongs to
    /// the class-name expression: `new A::$selected` means
    /// `new (A::$selected)`, not `(new A)::$selected`.
    fn parse_named_new_class_expression(
        &mut self,
        class_name: String,
    ) -> Result<Option<Expr>, String> {
        if self.peek() != Token::DoubleColon
            || !matches!(self.peek_at(1), Token::Dollar(_) | Token::Variable(_, _))
        {
            return Ok(None);
        }
        self.advance();
        let expr = if matches!(self.peek(), Token::Dollar(_)) {
            let (property, line) = self.parse_indirect_static_member_name()?;
            Expr::DynamicNamedStaticProperty {
                class_name,
                property: Box::new(property),
                line,
            }
        } else {
            match self.advance() {
                Token::Variable(property, line) => Expr::StaticProperty {
                    class_name,
                    property,
                    parenthesized: false,
                    line,
                },
                _ => unreachable!(),
            }
        };
        self.parse_dynamic_new_class_expression(expr).map(Some)
    }

    fn validate_new_expression_suffix(
        &self,
        postfix_allowed: bool,
        named_class_syntax: bool,
        line: usize,
    ) -> Result<(), String> {
        if self.peek() == Token::Assign {
            return Err(self.source_error("syntax error, unexpected token \"=\"", line));
        }
        if self.empty_dimension_unset_context && self.peek() == Token::RParen {
            return Err(self.source_error(
                "syntax error, unexpected token \")\", expecting \"->\" or \"?->\" or \"[\"",
                line,
            ));
        }
        if postfix_allowed {
            return Ok(());
        }

        let suffix = self.new_postfix_error_suffix.unwrap_or("");
        match self.peek() {
            Token::Arrow => Err(self.source_error(
                &format!("syntax error, unexpected token \"->\"{suffix}"),
                line,
            )),
            Token::NullSafe => Err(self.source_error(
                &format!("syntax error, unexpected token \"?->\"{suffix}"),
                line,
            )),
            Token::LBracket(bracket_line) => Err(self.source_error(
                &format!("syntax error, unexpected token \"[\"{suffix}"),
                bracket_line,
            )),
            Token::DoubleColon if named_class_syntax => {
                if let Token::Identifier(name, member_line) = self.peek_at(1) {
                    Err(self.source_error(
                        &format!(
                            "syntax error, unexpected identifier \"{name}\", expecting variable or \"$\""
                        ),
                        member_line,
                    ))
                } else {
                    Err(self.source_error(
                        &format!("syntax error, unexpected token \"::\"{suffix}"),
                        line,
                    ))
                }
            }
            Token::DoubleColon => Err(self.source_error(
                &format!("syntax error, unexpected token \"::\"{suffix}"),
                line,
            )),
            _ => Ok(()),
        }
    }

    /// Parse a statically named member after a class-like owner. The shared
    /// postfix loop in `parse_power` continues the resulting expression.
    fn parse_named_static_access(&mut self, class_name: String) -> Result<Expr, String> {
        self.expect(&Token::DoubleColon)?;
        if matches!(self.peek(), Token::LBrace(_)) {
            self.advance();
            let constant = self.parse_expr()?;
            self.expect(&Token::RBrace)?;
            if matches!(self.peek(), Token::LParen(_)) {
                let line = self.expect_lparen()?;
                if self.consume_first_class_callable_placeholder() {
                    return Ok(Self::first_class_member_callable(
                        Expr::ClassConstant {
                            class_name,
                            constant: "class".to_string(),
                            line,
                        },
                        constant,
                        true,
                        line,
                    ));
                }
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
                    method_syntax: true,
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
                if self.consume_first_class_callable_placeholder() {
                    return Ok(Self::first_class_member_callable(
                        Expr::ClassConstant {
                            class_name,
                            constant: "class".to_string(),
                            line,
                        },
                        Expr::DynamicVariable {
                            name: Box::new(property),
                            line: dollar_line,
                        },
                        true,
                        line,
                    ));
                }
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
                    method_syntax: true,
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
                if self.consume_first_class_callable_placeholder() {
                    return Ok(Self::first_class_member_callable(
                        Expr::ClassConstant {
                            class_name,
                            constant: "class".to_string(),
                            line,
                        },
                        Expr::Variable {
                            name: property,
                            line: property_line,
                        },
                        true,
                        line,
                    ));
                }
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
                    method_syntax: true,
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

        let token = self.advance();
        let member_line = match &token {
            Token::Identifier(_, line)
            | Token::Enum { line, .. }
            | Token::Goto { line, .. }
            | Token::Echo { line }
            | Token::MagicConstant { line, .. } => Some(*line),
            Token::New(line) | Token::Throw(line) => Some(*line as usize),
            _ => None,
        };
        let member = Self::token_as_named_arg_label(&token)
            .ok_or_else(|| format!("Expected member name after ::, got {token:?}"))?;
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
            return Ok(Self::first_class_callable(Expr::ArrayLiteral(vec![
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
            ]), line));
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
                        if matches!(
                            self.peek_at(2),
                            Token::LBracket(_)
                                | Token::Arrow
                                | Token::NullSafe
                                | Token::PlusPlus
                                | Token::MinusMinus
                        ) {
                            self.advance();
                            self.advance();
                            expr = Expr::ArrayAppendArgument {
                                target: Box::new(expr),
                                line: expression_line.unwrap_or(line),
                            };
                            continue;
                        }
                        if self.preserve_empty_dimension_suffix
                            || self.peek_at(2) == Token::Assign
                            || Self::compound_assign_op(&self.peek_at(2)).is_some()
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
                    if self.consume_first_class_callable_placeholder() {
                        expr = Self::first_class_callable(expr, line);
                    } else {
                        let args = self.parse_call_args()?;
                        expr = Expr::DynamicCall {
                            callable: Box::new(expr),
                            args,
                            generic_args: Vec::new(),
                            method_syntax: false,
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
                        method_syntax: false,
                        line,
                    };
                }
                Token::DoubleColon => {
                    self.advance();
                    if matches!(self.peek(), Token::Dollar(_)) {
                        let (property, dollar_line) = self.parse_indirect_static_member_name()?;
                        if matches!(self.peek(), Token::LParen(_)) {
                            let line = self.expect_lparen()?;
                            let method = Expr::DynamicVariable {
                                name: Box::new(property),
                                line: dollar_line,
                            };
                            if self.consume_first_class_callable_placeholder() {
                                expr = Self::first_class_member_callable(expr, method, true, line);
                            } else {
                                let args = self.parse_call_args()?;
                                expr = Expr::DynamicStaticCall {
                                    class: Box::new(expr),
                                    method: Box::new(method),
                                    args,
                                    generic_args: Vec::new(),
                                    line,
                                };
                            }
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
                            let method = Expr::Variable {
                                name: member_name,
                                line: member_line,
                            };
                            if self.consume_first_class_callable_placeholder() {
                                expr = Self::first_class_member_callable(expr, method, true, line);
                            } else {
                                let args = self.parse_call_args()?;
                                expr = Expr::DynamicStaticCall {
                                    class: Box::new(expr),
                                    method: Box::new(method),
                                    args,
                                    generic_args: Vec::new(),
                                    line,
                                };
                            }
                        } else {
                            expr = Expr::DynamicStaticProperty {
                                class: Box::new(expr),
                                property: Box::new(Expr::StringLiteral(member_name)),
                                line: member_line,
                            };
                        }
                        continue;
                    }
                    let dynamic_name = matches!(self.peek(), Token::LBrace(_));
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
                        if self.consume_first_class_callable_placeholder() {
                            expr = Self::first_class_member_callable(expr, constant, true, line);
                        } else {
                            let args = self.parse_call_args()?;
                            expr = Expr::DynamicStaticCall {
                                class: Box::new(expr),
                                method: Box::new(constant),
                                args,
                                generic_args: Vec::new(),
                                line,
                            };
                        }
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
                    if matches!(self.peek(), Token::LBrace(_)) {
                        self.advance();
                        let member = self.parse_expr()?;
                        self.expect(&Token::RBrace)?;
                        if matches!(self.peek(), Token::LParen(_)) {
                            let line = self.expect_lparen()?;
                            if self.consume_first_class_callable_placeholder() {
                                expr = if nullsafe {
                                    self.compile_error(
                                        "Cannot combine nullsafe operator with Closure creation",
                                        line,
                                    )
                                } else {
                                    Self::first_class_member_callable(expr, member, false, line)
                                };
                            } else {
                                if nullsafe {
                                    return Err(
                                        "Dynamic nullsafe method calls are not supported yet".into()
                                    );
                                }
                                let args = self.parse_call_args()?;
                                expr = Expr::DynamicCall {
                                    callable: Box::new(Self::dynamic_member_callable(expr, member)),
                                    args,
                                    generic_args: Vec::new(),
                                    method_syntax: true,
                                    line,
                                };
                            }
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
                            let line = self.expect_lparen()?;
                            if self.consume_first_class_callable_placeholder() {
                                expr = if nullsafe {
                                    self.compile_error(
                                        "Cannot combine nullsafe operator with Closure creation",
                                        line,
                                    )
                                } else {
                                    Self::first_class_member_callable(expr, member, false, line)
                                };
                            } else {
                                if nullsafe {
                                    return Err(
                                        "Dynamic nullsafe method calls are not supported yet".into(),
                                    );
                                }
                                let args = self.parse_call_args()?;
                                expr = Expr::DynamicCall {
                                    callable: Box::new(Self::dynamic_member_callable(expr, member)),
                                    args,
                                    generic_args: Vec::new(),
                                    method_syntax: true,
                                    line,
                                };
                            }
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
                    if let Token::Variable(member_name, member_line) = self.peek() {
                        self.advance();
                        let member = Expr::Variable {
                            name: member_name,
                            line: member_line,
                        };
                        if matches!(self.peek(), Token::LParen(_)) {
                            let line = self.expect_lparen()?;
                            if self.consume_first_class_callable_placeholder() {
                                expr = if nullsafe {
                                    self.compile_error(
                                        "Cannot combine nullsafe operator with Closure creation",
                                        line,
                                    )
                                } else {
                                    Self::first_class_member_callable(expr, member, false, line)
                                };
                            } else {
                                if nullsafe {
                                    return Err(
                                        "Dynamic nullsafe method calls are not supported yet".into()
                                    );
                                }
                                let args = self.parse_call_args()?;
                                expr = Expr::DynamicCall {
                                    callable: Box::new(Self::dynamic_member_callable(expr, member)),
                                    args,
                                    generic_args: Vec::new(),
                                    method_syntax: true,
                                    line,
                                };
                            }
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
                        Token::Identifier(_, line) | Token::Enum { line, .. } => Some(*line),
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
                            if !generic_args.is_empty() {
                                return Err(
                                    "Generic first-class method callables are not supported yet"
                                        .into(),
                                );
                            }
                            self.advance();
                            self.advance();
                            expr = if nullsafe {
                                self.compile_error(
                                    "Cannot combine nullsafe operator with Closure creation",
                                    line,
                                )
                            } else {
                                Self::first_class_member_callable(
                                    expr,
                                    Expr::StringLiteral(member),
                                    false,
                                    line,
                                )
                            };
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
                    if matches!(&expr, Expr::ArrayAccess { .. })
                        && let Some((message, line)) = self.array_write_root_error(&expr)
                    {
                        expr = self.compile_error(message, line);
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
                        | Expr::ArrayAccess { .. }
                        | Expr::ArrayAppendArgument { .. } => {
                            if increment {
                                Expr::PostIncTarget(Box::new(expr))
                            } else {
                                Expr::PostDecTarget(Box::new(expr))
                            }
                        }
                        other => {
                            if let Some(error) = self.call_write_error(&other) {
                                error
                            } else {
                                return Err(format!("Invalid increment target: {other:?}"));
                            }
                        }
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }
}
