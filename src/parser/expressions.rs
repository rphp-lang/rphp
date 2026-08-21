impl Parser {
    /// Parse PHP's keyword logical operators below assignment and yield.
    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_keyword_or()
    }

    fn parse_keyword_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_keyword_xor()?;
        while self.peek() == Token::LogicalOr {
            self.advance();
            let right = self.parse_keyword_xor()?;
            left = Expr::BinaryOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn finish_keyword_logical_tail(&mut self, mut left: Expr) -> Result<Expr, String> {
        while self.peek() == Token::LogicalAnd {
            self.advance();
            let right = self.parse_assignment_or_yield()?;
            left = Expr::BinaryOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        while self.peek() == Token::LogicalXor {
            self.advance();
            let right = self.parse_keyword_and()?;
            left = Expr::BinaryOp {
                op: BinOp::LogicalXor,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        while self.peek() == Token::LogicalOr {
            self.advance();
            let right = self.parse_keyword_xor()?;
            left = Expr::BinaryOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_keyword_xor(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_keyword_and()?;
        while self.peek() == Token::LogicalXor {
            self.advance();
            let right = self.parse_keyword_and()?;
            left = Expr::BinaryOp {
                op: BinOp::LogicalXor,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_keyword_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_assignment_or_yield()?;
        while self.peek() == Token::LogicalAnd {
            self.advance();
            let right = self.parse_assignment_or_yield()?;
            left = Expr::BinaryOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_assignment_or_yield(&mut self) -> Result<Expr, String> {
        let expr = self.parse_ternary()?;
        self.finish_assignment_tail(expr)
    }

    fn finish_assignment_tail(&mut self, expr: Expr) -> Result<Expr, String> {
        if self.peek() == Token::Assign && self.peek_at(1) == Token::Ampersand {
            // Reference assignment has lower precedence than bitwise AND. It
            // must be recognized before parse_bitwise_and consumes `&` as an
            // infix operator and asks for an expression after `$left =`.
            self.finish_assignment_expression(expr)
        } else if self.is_array_append_suffix() {
            self.finish_array_append_assignment_expression(expr)
        } else if self.peek() == Token::QuestionQuestionAssign {
            self.finish_coalesce_assignment_expression(expr)
        } else if self.peek() == Token::Assign {
            // Handle assignment as expression: $var = expr
            self.finish_assignment_expression(expr)
        } else if Self::compound_assign_op(&self.peek()).is_some() {
            self.finish_compound_assignment_expression(expr)
        } else {
            Ok(expr)
        }
    }

    fn finish_array_append_assignment_expression(
        &mut self,
        target: Expr,
    ) -> Result<Expr, String> {
        let nullsafe_line = Self::nullsafe_chain_line(&target);
        let invalid_temporary_line = (!Self::is_array_append_write_target(&target)
            && nullsafe_line.is_none())
        .then(|| {
            self.last_primary_line.unwrap_or_else(|| match self.peek() {
                Token::LBracket(line) => line,
                _ => 0,
            })
        });
        self.expect_lbracket()?;
        self.expect(&Token::RBracket)?;
        self.expect(&Token::Assign)?;
        let by_ref = if self.peek() == Token::Ampersand {
            self.advance();
            true
        } else {
            false
        };
        let expr = self.parse_assignment_or_yield()?;
        if let Some(line) = nullsafe_line {
            return Ok(self.nullsafe_write_error(line));
        }
        if let Some(line) = invalid_temporary_line {
            return Ok(self.compile_error(
                "Cannot use temporary expression in write context",
                line,
            ));
        }
        if let Expr::Globals { line } = target {
            return Ok(self.compile_error("Cannot append to $GLOBALS", line));
        }
        Ok(Expr::ArrayAppendAssign {
            target: Box::new(target),
            expr: Box::new(expr),
            by_ref,
        })
    }

    fn finish_compound_assignment_expression(&mut self, target: Expr) -> Result<Expr, String> {
        let nullsafe_line = Self::nullsafe_chain_line(&target);
        let call_write_error = if nullsafe_line.is_none() {
            self.call_write_error(&target)
        } else {
            None
        };
        if !matches!(
            &target,
            Expr::Variable { .. }
                | Expr::DynamicVariable { .. }
                | Expr::Globals { .. }
                | Expr::ArrayAccess { .. }
                | Expr::PropertyAccess {
                    nullsafe: false,
                    ..
                }
                | Expr::DynamicPropertyAccess {
                    nullsafe: false,
                    ..
                }
                | Expr::StaticProperty { .. }
                | Expr::DynamicNamedStaticProperty { .. }
                | Expr::DynamicStaticProperty { .. }
        ) && nullsafe_line.is_none()
            && call_write_error.is_none()
        {
            return Err("Invalid compound assignment target".into());
        }
        let op = Self::compound_assign_op(&self.advance())
            .ok_or_else(|| "Expected compound assignment operator".to_string())?;
        let expr = self.parse_assignment_or_yield()?;
        if let Some(line) = nullsafe_line {
            return Ok(self.nullsafe_write_error(line));
        }
        if let Some(error) = call_write_error {
            return Ok(error);
        }
        if let Expr::Globals { line } = target {
            return Ok(self.globals_modification_error(line));
        }
        Ok(Expr::CompoundAssignExpression {
            target: Box::new(target),
            op,
            expr: Box::new(expr),
        })
    }

    fn finish_assignment_expression(&mut self, target: Expr) -> Result<Expr, String> {
        self.expect(&Token::Assign)?;
        let by_reference = if self.peek() == Token::Ampersand {
            self.advance();
            true
        } else {
            false
        };
        let nullsafe_line = Self::nullsafe_chain_line(&target);
        let call_write_error = if nullsafe_line.is_none() {
            self.call_write_error(&target)
        } else {
            None
        };
        let this_reassignment = match &target {
            Expr::Variable { name, line } if name == "this" => {
                Some(self.compile_error("Cannot re-assign $this", *line))
            }
            _ => None,
        };
        let expr = Box::new(self.parse_assignment_or_yield()?);
        if let Expr::Cast {
            cast_type: CastType::Void,
            line,
            ..
        } = expr.as_ref()
        {
            return Err(self.source_error(
                "syntax error, unexpected token \"(void)\"",
                *line,
            ));
        }
        if let Some(line) = nullsafe_line {
            return Ok(self.nullsafe_write_error(line));
        }
        if let Some(error) = this_reassignment {
            return Ok(error);
        }
        if let Some(error) = call_write_error {
            return Ok(error);
        }
        if let Expr::Globals { line } = target {
            return Ok(self.globals_modification_error(line));
        }
        if by_reference && let Expr::Globals { line } = expr.as_ref() {
            return Ok(self.compile_error("Cannot acquire reference to $GLOBALS", *line));
        }
        if by_reference && let Some(line) = Self::nullsafe_chain_line(expr.as_ref()) {
            return Ok(self.nullsafe_reference_error(line));
        }
        if by_reference
            && matches!(
                &target,
                Expr::DynamicVariable { .. }
                    | Expr::ArrayAccess { .. }
                    | Expr::PropertyAccess {
                        nullsafe: false,
                        ..
                    }
                    | Expr::DynamicPropertyAccess {
                        nullsafe: false,
                        ..
                    }
                    | Expr::StaticProperty { .. }
                    | Expr::DynamicNamedStaticProperty { .. }
                    | Expr::DynamicStaticProperty { .. }
            )
        {
            return Ok(Expr::AssignTargetReference {
                target: Box::new(target),
                source: expr,
            });
        }
        match target {
            Expr::Variable { name: var, .. } if by_reference => Ok(Expr::AssignReference {
                var,
                target: expr,
            }),
            Expr::Variable { name: var, .. } => Ok(Expr::Assign { var, expr }),
            Expr::DynamicVariable { .. }
            | Expr::ArrayAccess { .. }
            | Expr::PropertyAccess {
                nullsafe: false, ..
            }
            | Expr::DynamicPropertyAccess {
                nullsafe: false, ..
            }
            | Expr::StaticProperty { .. }
            | Expr::DynamicNamedStaticProperty { .. }
            | Expr::DynamicStaticProperty { .. } => Ok(Expr::AssignTarget {
                target: Box::new(target),
                expr,
            }),
            other => Err(format!("Invalid assignment target: {other:?}")),
        }
    }

    fn finish_coalesce_assignment_expression(&mut self, target: Expr) -> Result<Expr, String> {
        let nullsafe_line = Self::nullsafe_chain_line(&target);
        let call_write_error = if nullsafe_line.is_none() {
            self.call_write_error(&target)
        } else {
            None
        };
        if !matches!(
            &target,
            Expr::Variable { .. }
                | Expr::DynamicVariable { .. }
                | Expr::Globals { .. }
                | Expr::ArrayAccess { .. }
                | Expr::PropertyAccess {
                    nullsafe: false,
                    ..
                }
                | Expr::DynamicPropertyAccess {
                    nullsafe: false,
                    ..
                }
                | Expr::StaticProperty { .. }
                | Expr::DynamicNamedStaticProperty { .. }
                | Expr::DynamicStaticProperty { .. }
        ) && nullsafe_line.is_none()
            && call_write_error.is_none()
        {
            return Err("Invalid null-coalescing assignment target".into());
        }
        self.expect(&Token::QuestionQuestionAssign)?;
        let this_reassignment = match &target {
            Expr::Variable { name, line } if name == "this" => {
                Some(self.compile_error("Cannot re-assign $this", *line))
            }
            _ => None,
        };
        let expr = self.parse_assignment_or_yield()?;
        if let Some(line) = nullsafe_line {
            return Ok(self.nullsafe_write_error(line));
        }
        if let Some(error) = this_reassignment {
            return Ok(error);
        }
        if let Some(error) = call_write_error {
            return Ok(error);
        }
        if let Expr::Globals { line } = target {
            return Ok(self.globals_modification_error(line));
        }
        Ok(Expr::CoalesceAssign {
            target: Box::new(target),
            expr: Box::new(expr),
        })
    }

    fn parse_yield_expr(&mut self) -> Result<Expr, String> {
        let line = match self.advance() {
            Token::Yield(line) => line,
            token => return Err(format!("Expected yield, got {token:?}")),
        };

        // yield from <expr>
        if self.peek() == Token::From {
            self.advance(); // consume 'from'
            let expr = self.parse_expr()?;
            return Ok(Expr::YieldFrom {
                expr: Box::new(expr),
                line,
            });
        }

        // yield; or yield at end of expression context (no value)
        if matches!(
            self.peek(),
            Token::Semicolon(_)
                | Token::RParen
                | Token::RBracket
                | Token::RBrace
                | Token::Comma(_)
                | Token::Star
                | Token::Eof
        ) {
            return Ok(Expr::Yield {
                value: None,
                key: None,
            });
        }

        // yield <expr> or yield <key> => <value>
        let first = self.parse_assignment_or_yield()?;
        if self.peek() == Token::DoubleArrow {
            self.advance(); // consume '=>'
            let value = self.parse_assignment_or_yield()?;
            Ok(Expr::Yield {
                key: Some(Box::new(first)),
                value: Some(Box::new(value)),
            })
        } else {
            Ok(Expr::Yield {
                value: Some(Box::new(first)),
                key: None,
            })
        }
    }

    fn parse_ternary(&mut self) -> Result<Expr, String> {
        let expr = self.parse_null_coalesce()?;

        if self.peek() == Token::Question {
            self.advance(); // consume ?

            // Elvis operator: $x ?: $y  (evaluates lhs once)
            if self.peek() == Token::Colon {
                self.advance(); // consume :
                let right = self.parse_null_coalesce()?;
                let mut result = Expr::Elvis {
                    left: Box::new(expr),
                    right: Box::new(right),
                };
                while self.peek() == Token::Question && self.peek_at(1) == Token::Colon {
                    self.advance();
                    self.advance();
                    let right = self.parse_null_coalesce()?;
                    result = Expr::Elvis {
                        left: Box::new(result),
                        right: Box::new(right),
                    };
                }
                return Ok(result);
            }

            // PHP parses the middle arm as a complete expression. In
            // particular, assignments such as `$value ??= fallback()` are
            // valid here even though assignment binds below the ternary
            // operator in the surrounding expression.
            let then_expr = self.parse_ternary()?;
            let then_expr = self.finish_assignment_tail(then_expr)?;
            self.expect(&Token::Colon)?;
            let else_expr = self.parse_null_coalesce()?;
            let else_expr = self.finish_assignment_tail(else_expr)?;

            if self.peek() == Token::Question {
                return Err("Unparenthesized `a ? b : c ? d : e` is not supported. Use explicit parentheses.".into());
            }

            Ok(Expr::Ternary {
                condition: Box::new(expr),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            })
        } else {
            Ok(expr)
        }
    }

    /// Null coalesce: ?? (right-associative)
    fn parse_null_coalesce(&mut self) -> Result<Expr, String> {
        let left = self.parse_logical_or()?;

        if self.peek() == Token::QuestionQuestion {
            self.advance();
            let right = self.parse_null_coalesce()?; // right-associative
            // Assignment is lower precedence than `??`, but it may start on
            // the recursively parsed right-hand side (`$a ?? $b ??= $c`).
            let right = self.finish_assignment_tail(right)?;
            Ok(Expr::NullCoalesce {
                left: Box::new(left),
                right: Box::new(right),
            })
        } else {
            Ok(left)
        }
    }

    /// Logical OR: || (left-associative)
    fn parse_logical_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_logical_and()?;

        while self.peek() == Token::PipePipe {
            self.advance();
            let right = self.parse_logical_or_operand()?;
            left = Expr::BinaryOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_logical_or_operand(&mut self) -> Result<Expr, String> {
        let operand = self.parse_logical_and()?;
        self.finish_variable_assignment(operand)
    }

    /// Logical AND: && (left-associative)
    fn parse_logical_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bitwise_or()?;

        while self.peek() == Token::AmpAmp {
            self.advance();
            let right = self.parse_logical_and_operand()?;
            left = Expr::BinaryOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// PHP's expression grammar admits an assignment as the right operand of
    /// `&&`/`||` without another pair of parentheses. This is common in
    /// guarded lookup code such as `$enabled && $file = find_file()`.
    fn parse_logical_and_operand(&mut self) -> Result<Expr, String> {
        let operand = self.parse_bitwise_or()?;
        self.finish_variable_assignment(operand)
    }

    fn finish_variable_assignment(&mut self, operand: Expr) -> Result<Expr, String> {
        self.finish_assignment_tail(operand)
    }

    /// Bitwise OR: | (left-associative)
    fn parse_bitwise_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bitwise_xor()?;

        while self.peek() == Token::Pipe {
            self.advance();
            let right = self.parse_bitwise_xor()?;
            let right = self.finish_assignment_tail(right)?;
            left = Expr::BinaryOp {
                op: BinOp::BitwiseOr,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Bitwise XOR: ^ (left-associative)
    fn parse_bitwise_xor(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bitwise_and()?;

        while self.peek() == Token::Caret {
            self.advance();
            let right = self.parse_bitwise_and()?;
            let right = self.finish_assignment_tail(right)?;
            left = Expr::BinaryOp {
                op: BinOp::BitwiseXor,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Bitwise AND: & (left-associative)
    fn parse_bitwise_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_comparison()?;

        while self.peek() == Token::Ampersand {
            self.advance();
            let right = self.parse_comparison()?;
            let right = self.finish_assignment_tail(right)?;
            left = Expr::BinaryOp {
                op: BinOp::BitwiseAnd,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Comparison: ==, !=, <, <=, >, >=, <=>, instanceof
    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_pipe()?;

        loop {
            // instanceof has same precedence as comparison operators
            if self.peek() == Token::Instanceof {
                self.advance();
                left = if self.peek() == Token::Backslash
                    || matches!(self.peek(), Token::Identifier(_, _))
                {
                    Expr::Instanceof {
                        expr: Box::new(left),
                        class_name: self.parse_qualified_name()?,
                    }
                } else if matches!(self.peek(), Token::Static(_)) {
                    self.advance();
                    Expr::Instanceof {
                        expr: Box::new(left),
                        class_name: "static".to_string(),
                    }
                } else if matches!(self.peek(), Token::Variable(_, _) | Token::This(_)) {
                    let class = match self.advance() {
                        Token::Variable(name, line) => Self::variable_expression(name, line),
                        Token::This(line) => Expr::Variable {
                            name: "this".to_string(),
                            line,
                        },
                        _ => unreachable!(),
                    };
                    let class = self.parse_dynamic_new_class_expression(class)?;
                    Expr::DynamicInstanceof {
                        expr: Box::new(left),
                        class: Box::new(class),
                    }
                } else {
                    return Err(format!(
                        "Expected class name after instanceof, got {:?}",
                        self.peek()
                    ));
                };
                continue;
            }
            let op = match self.peek() {
                Token::EqualEqual => BinOp::Equal,
                Token::NotEqual => BinOp::NotEqual,
                Token::IdenticalEqual => BinOp::Identical,
                Token::NotIdentical => BinOp::NotIdentical,
                Token::Less => BinOp::Less,
                Token::LessEqual => BinOp::LessEqual,
                Token::Greater => BinOp::Greater,
                Token::GreaterEqual => BinOp::GreaterEqual,
                Token::Spaceship => BinOp::Spaceship,
                _ => break,
            };
            self.advance();
            let right = self.parse_comparison_operand()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// PHP admits assignment on the right side of a comparison without
    /// requiring extra parentheses. Composer's canonical loader relies on
    /// forms such as `false !== $position = strrpos(...)`.
    fn parse_comparison_operand(&mut self) -> Result<Expr, String> {
        let operand = self.parse_pipe()?;
        self.finish_assignment_tail(operand)
    }

    /// PHP 8.5 pipe: lower precedence than concatenation/addition, higher than
    /// comparisons, and left-associative. A bare function name denotes that
    /// callable rather than a constant; all other RHS callable expressions
    /// retain their ordinary expression semantics.
    fn parse_pipe(&mut self) -> Result<Expr, String> {
        let mut input = self.parse_concat()?;
        while let Token::PipeGreater(line) = self.peek() {
            let line = line;
            self.advance();
            if matches!(self.peek(), Token::Fn(_)) {
                self.compile_error(
                    "Arrow functions on the right hand side of |> must be parenthesized",
                    line,
                );
            }
            let callable = match self.parse_concat()? {
                Expr::Constant(name) => Expr::FirstClassFunctionCallable { name, line },
                callable => callable,
            };
            input = Expr::Pipe {
                input: Box::new(input),
                callable: Box::new(callable),
                line,
            };
        }
        Ok(input)
    }

    /// Concat: . (left-associative, lower than additive in PHP 8)
    fn parse_concat(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_shift()?;

        while self.peek() == Token::Dot {
            self.advance();
            let right = self.parse_shift()?;
            let right = self.finish_assignment_tail(right)?;
            left = Expr::BinaryOp {
                op: BinOp::Concat,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Shift: <<, >> (left-associative)
    fn parse_shift(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_additive()?;

        loop {
            let op = match self.peek() {
                Token::ShiftLeft => BinOp::ShiftLeft,
                Token::ShiftRight => BinOp::ShiftRight,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            let right = self.finish_assignment_tail(right)?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Additive: + and - (left-associative)
    fn parse_additive(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_multiplicative()?;

        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            let right = self.finish_assignment_tail(right)?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Multiplicative: *, /, % (left-associative)
    fn parse_multiplicative(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;

        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            let right = self.finish_assignment_tail(right)?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Unary: -expr, (int)expr, (string)expr, etc.
    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Token::Bang => {
                self.advance();
                let mut expr = self.parse_unary()?;
                // `instanceof` binds tighter than logical negation in PHP:
                // `!$value instanceof Type` means `!($value instanceof Type)`.
                if self.peek() == Token::Instanceof {
                    self.advance();
                    expr = if self.peek() == Token::Backslash
                        || matches!(self.peek(), Token::Identifier(_, _))
                    {
                        Expr::Instanceof {
                            expr: Box::new(expr),
                            class_name: self.parse_qualified_name()?,
                        }
                    } else if matches!(self.peek(), Token::Static(_)) {
                        self.advance();
                        Expr::Instanceof {
                            expr: Box::new(expr),
                            class_name: "static".to_string(),
                        }
                    } else if matches!(self.peek(), Token::Variable(_, _) | Token::This(_)) {
                        let class = match self.advance() {
                            Token::Variable(name, line) => Self::variable_expression(name, line),
                            Token::This(line) => Expr::Variable {
                                name: "this".to_string(),
                                line,
                            },
                            _ => unreachable!(),
                        };
                        let class = self.parse_dynamic_new_class_expression(class)?;
                        Expr::DynamicInstanceof {
                            expr: Box::new(expr),
                            class: Box::new(class),
                        }
                    } else {
                        return Err(format!(
                            "Expected class name after instanceof, got {:?}",
                            self.peek()
                        ));
                    };
                }
                // PHP permits `!$value ??= $fallback` and applies `!` to the
                // value produced by the coalescing assignment.
                if self.peek() == Token::QuestionQuestionAssign {
                    expr = self.finish_coalesce_assignment_expression(expr)?;
                } else if self.peek() == Token::Assign {
                    expr = self.finish_assignment_expression(expr)?;
                }
                Ok(Expr::Not(Box::new(expr)))
            }
            Token::Minus => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::UnaryMinus(Box::new(expr)))
            }
            Token::Plus => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::UnaryPlus(Box::new(expr)))
            }
            Token::At => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::ErrorSuppress(Box::new(expr)))
            }
            Token::Tilde => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::BitwiseNot(Box::new(expr)))
            }
            Token::Clone(line) => {
                self.advance();
                let (expr, with_properties) = if matches!(self.peek(), Token::LParen(_)) {
                    self.advance();
                    let mut expr = self.parse_expr()?;
                    let has_argument_separator = matches!(self.peek(), Token::Comma(_));
                    let with_properties = if has_argument_separator {
                        self.advance();
                        if self.peek() == Token::RParen {
                            None
                        } else {
                            let properties = Some(Box::new(self.parse_expr()?));
                            if matches!(self.peek(), Token::Comma(_)) {
                                self.advance();
                            }
                            properties
                        }
                    } else {
                        None
                    };
                    self.expect(&Token::RParen)?;
                    // Before PHP 8.5, whitespace-parenthesized clone operands
                    // could continue with property/method postfixes outside
                    // the grouping parentheses: `clone (new C)->property`.
                    // A comma identifies the new argument-list form instead.
                    if !has_argument_separator {
                        expr = self.parse_postfix_chain(expr)?;
                    }
                    (expr, with_properties)
                } else {
                    (self.parse_unary()?, None)
                };
                // Assignment binds inside clone's operand in PHP's grammar:
                // `clone $copy = new C` means `clone ($copy = new C)`.
                // Finishing the tail at each recursive clone level also keeps
                // `clone clone $copy = new C` right-associated.
                let expr = self.finish_assignment_tail(expr)?;
                Ok(Expr::Clone {
                    expr: Box::new(expr),
                    with_properties,
                    line,
                })
            }
            Token::LParen(line) => {
                // Check for a PHP type cast, including PHP 8.5's explicit
                // discard marker `(void)`.
                let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
                let (cast_type, deprecation) = match &next {
                    Token::Identifier(name, _) => match name.to_ascii_lowercase().as_str() {
                        "int" => (Some(CastType::Int), None),
                        "integer" => (
                            Some(CastType::Int),
                            Some(
                                "Non-canonical cast (integer) is deprecated, use the (int) cast instead",
                            ),
                        ),
                        "float" => (Some(CastType::Float), None),
                        "double" => (
                            Some(CastType::Float),
                            Some(
                                "Non-canonical cast (double) is deprecated, use the (float) cast instead",
                            ),
                        ),
                        "real" => (Some(CastType::Float), None),
                        "string" => (Some(CastType::String), None),
                        "binary" => (
                            Some(CastType::String),
                            Some(
                                "Non-canonical cast (binary) is deprecated, use the (string) cast instead",
                            ),
                        ),
                        "bool" => (Some(CastType::Bool), None),
                        "boolean" => (
                            Some(CastType::Bool),
                            Some(
                                "Non-canonical cast (boolean) is deprecated, use the (bool) cast instead",
                            ),
                        ),
                        "object" => (Some(CastType::Object), None),
                        "void" => (Some(CastType::Void), None),
                        _ => (None, None),
                    },
                    Token::ArrayKw => (Some(CastType::Array), None),
                    _ => (None, None),
                };
                if let Some(ct) = cast_type {
                    if self.tokens.get(self.pos + 2) == Some(&Token::RParen) {
                        if matches!(&next, Token::Identifier(name, _) if name.eq_ignore_ascii_case("real")) {
                            return Err(self.source_error(
                                "The (real) cast has been removed, use (float) instead",
                                line,
                            ));
                        }
                        if let Some(message) = deprecation {
                            self.deferred_compile_deprecations
                                .push((message.to_string(), line));
                        }
                        self.advance(); // (
                        self.advance(); // type keyword
                        self.advance(); // )
                        let expr = self.parse_unary()?;
                        // PHP casts wrap a following assignment expression:
                        // `(bool) $value = source()` assigns first and casts
                        // the value produced for the surrounding expression.
                        let expr = self.finish_assignment_tail(expr)?;
                        return Ok(Expr::Cast {
                            cast_type: ct,
                            expr: Box::new(expr),
                            line,
                        });
                    }
                }
                self.parse_power()
            }
            _ => self.parse_power(),
        }
    }

    /// Power: ** (right-associative, higher precedence than unary)
    fn parse_power(&mut self) -> Result<Expr, String> {
        let atom = self.parse_primary_atom()?;
        let base = self.parse_postfix_chain(atom)?;

        if self.peek() == Token::StarStar {
            self.advance();
            let exp = self.parse_unary()?; // right-associative: recurse through unary
            let exp = self.finish_assignment_tail(exp)?;
            Ok(Expr::BinaryOp {
                op: BinOp::Pow,
                left: Box::new(base),
                right: Box::new(exp),
            })
        } else {
            Ok(base)
        }
    }

    /// Parse exactly one primary atom. Postfix operators are deliberately
    /// applied by `parse_power`, so every atom gets the same chaining grammar.
    fn parse_primary_atom(&mut self) -> Result<Expr, String> {
        self.last_primary_line = None;
        match self.peek() {
            Token::ParseError(message, line) => {
                self.advance();
                Err(self.source_error(&message, line))
            }
            Token::Integer(_) => {
                let val = match self.advance() {
                    Token::Integer(n) => n,
                    _ => unreachable!(),
                };
                Ok(Expr::Integer(val))
            }
            Token::Float(_) => {
                let val = match self.advance() {
                    Token::Float(f) => f,
                    _ => unreachable!(),
                };
                Ok(Expr::Float(val))
            }
            Token::StringLiteral(_) => {
                let val = match self.advance() {
                    Token::StringLiteral(s) => s,
                    _ => unreachable!(),
                };
                Ok(Expr::StringLiteral(val))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Null)
            }
            Token::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Token::Yield(_) => self.parse_yield_expr(),
            Token::Variable(_, _) => {
                let (name, line) = match self.advance() {
                    Token::Variable(n, line) => (n, line),
                    _ => unreachable!(),
                };
                self.last_primary_line = Some(line);
                Ok(Self::variable_expression(name, line))
            }
            Token::Dollar(line) => {
                self.advance();
                self.last_primary_line = Some(line);
                let name = if self.peek() == Token::LBrace {
                    self.advance();
                    let name = self.parse_expr()?;
                    self.expect(&Token::RBrace)?;
                    name
                } else {
                    // The outer postfix loop owns calls, dimensions and
                    // properties. This makes `$$name()` mean `${$name}()`.
                    self.parse_primary_atom()?
                };
                Ok(Expr::DynamicVariable {
                    name: Box::new(name),
                    line,
                })
            }
            Token::This(line) => {
                self.last_primary_line = Some(line);
                self.advance();
                Ok(Expr::Variable {
                    name: "this".to_string(),
                    line,
                })
            }
            Token::PlusPlus => {
                self.advance();
                let target = self.parse_power()?;
                if let Some(line) = Self::nullsafe_chain_line(&target) {
                    return Ok(self.nullsafe_write_error(line));
                }
                match target {
                    Expr::Variable { name, line } => Ok(Expr::PreInc { name, line }),
                    Expr::DynamicVariable { .. } => Ok(Expr::PreIncTarget(Box::new(target))),
                    Expr::Globals { line } => Ok(self.globals_modification_error(line)),
                    Expr::PropertyAccess {
                        nullsafe: false, ..
                    }
                    | Expr::DynamicPropertyAccess {
                        nullsafe: false, ..
                    }
                    | Expr::StaticProperty { .. }
                    | Expr::DynamicNamedStaticProperty { .. }
                    | Expr::DynamicStaticProperty { .. }
                    | Expr::ArrayAccess { .. } => Ok(Expr::PreIncTarget(Box::new(target))),
                    other => self
                        .call_write_error(&other)
                        .map_or_else(|| Err(format!("Invalid increment target: {other:?}")), Ok),
                }
            }
            Token::MinusMinus => {
                self.advance();
                let target = self.parse_power()?;
                if let Some(line) = Self::nullsafe_chain_line(&target) {
                    return Ok(self.nullsafe_write_error(line));
                }
                match target {
                    Expr::Variable { name, line } => Ok(Expr::PreDec { name, line }),
                    Expr::DynamicVariable { .. } => Ok(Expr::PreDecTarget(Box::new(target))),
                    Expr::Globals { line } => Ok(self.globals_modification_error(line)),
                    Expr::PropertyAccess {
                        nullsafe: false, ..
                    }
                    | Expr::DynamicPropertyAccess {
                        nullsafe: false, ..
                    }
                    | Expr::StaticProperty { .. }
                    | Expr::DynamicNamedStaticProperty { .. }
                    | Expr::DynamicStaticProperty { .. }
                    | Expr::ArrayAccess { .. } => Ok(Expr::PreDecTarget(Box::new(target))),
                    other => self
                        .call_write_error(&other)
                        .map_or_else(|| Err(format!("Invalid decrement target: {other:?}")), Ok),
                }
            }
            Token::Print => {
                self.advance();
                let expr = self.parse_expr()?;
                Ok(Expr::Print(Box::new(expr)))
            }
            Token::Include | Token::IncludeOnce | Token::Require | Token::RequireOnce => {
                let token = self.advance();
                let (is_require, is_once) = match token {
                    Token::Include => (false, false),
                    Token::IncludeOnce => (false, true),
                    Token::Require => (true, false),
                    Token::RequireOnce => (true, true),
                    _ => unreachable!(),
                };
                let path = self.parse_expr()?;
                Ok(Expr::Include {
                    path: Box::new(path),
                    is_require,
                    is_once,
                })
            }
            Token::LParen(_) => {
                self.advance();
                let mut expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                if let Expr::StaticProperty { parenthesized, .. } = &mut expr {
                    *parenthesized = true;
                }
                Ok(expr)
            }
            Token::Isset => {
                self.advance();
                let list_line = self.expect_lparen()?;
                if matches!(self.peek(), Token::Comma(_)) {
                    return Err(self.comma_list_error(list_line, false));
                }
                let mut args = Vec::new();
                let arg = self.parse_expr()?;
                if !Self::is_isset_target(&arg) {
                    self.compile_error(
                        "Cannot use isset() on the result of an expression (you can use \"null !== expression\" instead)",
                        self.last_primary_line.unwrap_or(list_line),
                    );
                }
                args.push(arg);
                while self.comma_list_has_next(list_line)? {
                    let arg = self.parse_expr()?;
                    if !Self::is_isset_target(&arg) {
                        self.compile_error(
                            "Cannot use isset() on the result of an expression (you can use \"null !== expression\" instead)",
                            self.last_primary_line.unwrap_or(list_line),
                        );
                    }
                    args.push(arg);
                }
                self.expect(&Token::RParen)?;
                Ok(Expr::Isset(args))
            }
            Token::Empty => {
                self.advance();
                self.expect_lparen()?;
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::Empty(Box::new(expr)))
            }
            Token::Backslash => {
                // Fully qualified name: \App\Models\User() or \App\Models\User::method()
                let name = self.parse_qualified_name()?;
                let named_line = self.last_primary_line;
                let generic_args = self.parse_optional_turbofish()?;
                if !generic_args.is_empty() {
                    let paren_line = self.expect_lparen()?;
                    let line = named_line.unwrap_or(paren_line);
                    if matches!(self.peek(), Token::DotDotDot(_))
                        && self.peek_at(1) == Token::RParen
                    {
                        return Err("Generic first-class function callables are not supported yet"
                            .into());
                    }
                    let args = self.parse_call_args()?;
                    return Ok(Expr::FunctionCall {
                        name,
                        args,
                        generic_args,
                        line,
                    });
                }
                if self.peek() == Token::DoubleColon {
                    return self.parse_named_static_access(name);
                }
                if matches!(self.peek(), Token::LParen(_)) {
                    let paren_line = self.expect_lparen()?;
                    let line = named_line.unwrap_or(paren_line);
                    if matches!(self.peek(), Token::DotDotDot(_))
                        && self.peek_at(1) == Token::RParen
                    {
                        self.advance();
                        self.advance();
                        return Ok(Expr::FirstClassFunctionCallable { name, line });
                    }
                    let args = self.parse_call_args()?;
                    Ok(Expr::FunctionCall {
                        name,
                        args,
                        generic_args: Vec::new(),
                        line,
                    })
                } else {
                    Ok(Expr::Constant(name))
                }
            }
            Token::MagicConstant { .. } => match self.advance() {
                Token::MagicConstant { name, line } => Ok(Expr::MagicConstant { name, line }),
                _ => unreachable!(),
            },
            Token::Namespace if self.peek_at(1) == Token::Backslash => {
                let name = self.parse_namespace_relative_name()?;
                let named_line = self.last_primary_line;
                if self.peek() == Token::DoubleColon {
                    return self.parse_named_static_access(name);
                }
                if matches!(self.peek(), Token::LParen(_)) {
                    let paren_line = self.expect_lparen()?;
                    let line = named_line.unwrap_or(paren_line);
                    if self.consume_first_class_callable_placeholder() {
                        return Ok(Expr::FirstClassFunctionCallable { name, line });
                    }
                    let args = self.parse_call_args()?;
                    Ok(Expr::FunctionCall {
                        name,
                        args,
                        generic_args: Vec::new(),
                        line,
                    })
                } else {
                    Ok(Expr::Constant(name))
                }
            }
            Token::Identifier(_, _) | Token::From => {
                let name = if matches!(self.peek(), Token::Identifier(_, _))
                    && self.peek_at(1) == Token::Backslash
                {
                    // Qualified name: App\Models\User
                    self.parse_qualified_name()?
                } else {
                    match self.advance() {
                        Token::Identifier(n, line) => {
                            self.last_primary_line = Some(line);
                            n
                        }
                        Token::From => "from".to_string(),
                        _ => unreachable!(),
                    }
                };
                let named_line = self.last_primary_line;
                let generic_args = self.parse_optional_turbofish()?;
                if !generic_args.is_empty() {
                    let paren_line = self.expect_lparen()?;
                    let line = named_line.unwrap_or(paren_line);
                    if matches!(self.peek(), Token::DotDotDot(_))
                        && self.peek_at(1) == Token::RParen
                    {
                        return Err("Generic first-class function callables are not supported yet"
                            .into());
                    }
                    let args = self.parse_call_args()?;
                    return Ok(Expr::FunctionCall {
                        name,
                        args,
                        generic_args,
                        line,
                    });
                }
                // Static access: ClassName::method() or ClassName::$prop
                if self.peek() == Token::DoubleColon {
                    return self.parse_named_static_access(name);
                }
                // Check if this is a function call (followed by `(`)
                if matches!(self.peek(), Token::LParen(_)) {
                    let paren_line = self.expect_lparen()?;
                    let line = named_line.unwrap_or(paren_line);
                    if matches!(self.peek(), Token::DotDotDot(_))
                        && self.peek_at(1) == Token::RParen
                    {
                        self.advance();
                        self.advance();
                        return Ok(Expr::FirstClassFunctionCallable { name, line });
                    }
                    let args = self.parse_call_args()?;
                    if name.eq_ignore_ascii_case("eval") {
                        let mut args = args.into_iter();
                        let source = match (args.next(), args.next()) {
                            (Some(CallArg::Positional(source)), None) => source,
                            _ => return Err("eval() expects exactly one positional argument".into()),
                        };
                        Ok(Expr::Eval {
                            source: Box::new(source),
                            line,
                        })
                    } else {
                        Ok(Expr::FunctionCall {
                            name,
                            args,
                            generic_args: Vec::new(),
                            line,
                        })
                    }
                } else {
                    // Bare identifier — constant reference (e.g., PHP_INT_MAX, FOO)
                    Ok(Expr::Constant(name))
                }
            }
            Token::Static(line) => {
                if matches!(self.peek_at(1), Token::Function(_)) {
                    self.advance(); // consume 'static'
                    return self.parse_closure(true);
                }
                if matches!(self.peek_at(1), Token::Fn(_)) {
                    self.advance(); // consume 'static'
                    return self.parse_arrow_function(true);
                }
                self.advance();
                self.last_primary_line = Some(line);
                if self.peek() != Token::DoubleColon {
                    return Err(format!(
                        "Expected :: after static, got {:?}",
                        self.peek()
                    ));
                }
                self.parse_named_static_access("static".to_string())
            }
            Token::Match(_) => {
                return self.parse_match_expr();
            }
            Token::Function(_) => {
                // Closure (anonymous function)
                return self.parse_closure(false);
            }
            Token::Fn(_) => {
                // Arrow function: fn($x) => expr
                return self.parse_arrow_function(false);
            }
            Token::AttributeStart(_) => {
                let attributes = self.parse_attribute_groups()?;
                let mut expression = match self.peek() {
                    Token::Function(_) => self.parse_closure(false)?,
                    Token::Fn(_) => self.parse_arrow_function(false)?,
                    Token::Static(_)
                        if matches!(self.peek_at(1), Token::Function(_) | Token::Fn(_)) =>
                    {
                        self.advance();
                        if matches!(self.peek(), Token::Function(_)) {
                            self.parse_closure(true)?
                        } else {
                            self.parse_arrow_function(true)?
                        }
                    }
                    other => {
                        return Err(format!(
                            "Attribute group must precede a declaration, got {other:?}"
                        ));
                    }
                };
                match &mut expression {
                    Expr::Closure {
                        attributes: target,
                        ..
                    } => *target = attributes,
                    _ => unreachable!("attribute expression parser only accepts closures"),
                }
                return Ok(expression);
            }
            Token::New(line) => {
                let line = line as usize;
                self.advance(); // consume 'new'
                let mut anonymous_readonly = false;
                let mut allow_dynamic_properties = false;
                let mut allow_dynamic_properties_line = line;
                let mut attributes = Vec::new();
                loop {
                    match self.peek() {
                        Token::AttributeStart(attribute_line) => {
                            let mut group = self.parse_attribute_groups()?;
                            allow_dynamic_properties |= group.iter().any(|attribute| {
                                attribute
                                    .name
                                    .strip_prefix('\\')
                                    .unwrap_or(&attribute.name)
                                    .eq_ignore_ascii_case("AllowDynamicProperties")
                            });
                            allow_dynamic_properties_line = attribute_line;
                            attributes.append(&mut group);
                        }
                        Token::Identifier(ref name, _)
                            if name.eq_ignore_ascii_case("readonly")
                                && matches!(
                                    self.peek_at(1),
                                    Token::Class
                                        | Token::Abstract
                                        | Token::Final
                                        | Token::AttributeStart(_)
                                )
                                || name.eq_ignore_ascii_case("readonly")
                                    && matches!(
                                        self.peek_at(1),
                                        Token::Identifier(ref next, _)
                                            if next.eq_ignore_ascii_case("readonly")
                                    ) =>
                        {
                            if anonymous_readonly {
                                self.compile_error(
                                    "Multiple readonly modifiers are not allowed",
                                    line,
                                );
                            }
                            anonymous_readonly = true;
                            self.advance();
                        }
                        Token::Abstract => {
                            self.compile_error(
                                "Cannot use the abstract modifier on an anonymous class",
                                line,
                            );
                            self.advance();
                        }
                        Token::Final => {
                            self.compile_error(
                                "Cannot use the final modifier on an anonymous class",
                                line,
                            );
                            self.advance();
                        }
                        _ => break,
                    }
                }
                if self.peek() == Token::Class {
                    self.advance();
                    if anonymous_readonly && allow_dynamic_properties {
                        self.compile_error(
                            "Cannot apply #[\\AllowDynamicProperties] to readonly class class@anonymous",
                            allow_dynamic_properties_line,
                        );
                    }
                    let args = if matches!(self.peek(), Token::LParen(_)) {
                        self.expect_lparen()?;
                        if matches!(self.peek(), Token::DotDotDot(_))
                            && self.peek_at(1) == Token::RParen
                        {
                            self.advance();
                            self.advance();
                            self.compile_error("Cannot create Closure for new expression", line);
                            Vec::new()
                        } else {
                            self.parse_call_args()?
                        }
                    } else {
                        Vec::new()
                    };
                    let parent = if self.peek() == Token::Extends {
                        self.advance();
                        Some(self.parse_generic_ancestor_with_reserved_static(
                            ReservedStaticRole::Class,
                            Some(line),
                        )?)
                    } else {
                        None
                    };
                    let implements = if self.peek() == Token::Implements {
                        self.advance();
                        let mut interfaces = Vec::new();
                        loop {
                            interfaces.push(self.parse_generic_ancestor_with_reserved_static(
                                ReservedStaticRole::Interface,
                                Some(line),
                            )?);
                            if matches!(self.peek(), Token::Comma(_)) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        interfaces
                    } else {
                        Vec::new()
                    };
                    self.expect(&Token::LBrace)?;
                    let (properties, constants, methods, uses, trait_aliases) =
                        self.parse_anonymous_class_body()?;
                    return Ok(Expr::AnonymousNew {
                        attributes,
                        args,
                        is_readonly: anonymous_readonly,
                        allow_dynamic_properties,
                        parent,
                        implements,
                        properties,
                        constants,
                        methods,
                        uses,
                        trait_aliases,
                        line,
                        call_line: line,
                    });
                }
                if matches!(self.peek(), Token::Variable(_, _) | Token::This(_)) {
                    let (class, call_line) = match self.advance() {
                        Token::Variable(name, variable_line) => {
                            (Self::variable_expression(name, variable_line), variable_line)
                        }
                        Token::This(this_line) => {
                            (
                                Expr::Variable {
                                    name: "this".to_string(),
                                    line: this_line,
                                },
                                this_line,
                            )
                        }
                        _ => unreachable!(),
                    };
                    let class = self.parse_dynamic_new_class_expression(class)?;
                    let args = if matches!(self.peek(), Token::LParen(_)) {
                        self.expect_lparen()?;
                        if self.consume_first_class_callable_placeholder() {
                            self.compile_error("Cannot create Closure for new expression", line);
                            Vec::new()
                        } else {
                            self.parse_call_args()?
                        }
                    } else {
                        Vec::new()
                    };
                    return Ok(Expr::DynamicNew {
                        class: Box::new(class),
                        args,
                        line,
                        call_line,
                    });
                }
                let (class_name, call_line) = match self.peek() {
                    Token::Backslash | Token::Identifier(_, _) | Token::Namespace => {
                        let class_name = if self.peek() == Token::Namespace {
                            self.parse_namespace_relative_name()?
                        } else {
                            self.parse_qualified_name()?
                        };
                        (class_name, self.last_primary_line.unwrap_or(line))
                    }
                    Token::Static(_) => {
                        self.advance();
                        ("static".to_string(), line)
                    }
                    token => {
                        return Err(format!("Expected class name after 'new', got {token:?}"));
                    }
                };
                let generic_args = self.parse_optional_turbofish()?;
                let args = if matches!(self.peek(), Token::LParen(_)) {
                    self.expect_lparen()?;
                    if self.consume_first_class_callable_placeholder() {
                        self.compile_error("Cannot create Closure for new expression", line);
                        Vec::new()
                    } else {
                        self.parse_call_args()?
                    }
                } else {
                    Vec::new()
                };
                Ok(Expr::New {
                    class_name,
                    args,
                    generic_args,
                    line,
                    call_line,
                })
            }
            Token::Throw(line) => {
                let line = line as usize;
                self.advance();
                let expr = self.parse_expr()?;
                return Ok(Expr::Throw {
                    expr: Box::new(expr),
                    line,
                });
            }
            Token::LBracket(line) => {
                // Short array syntax: [1, 2, 'a' => 3]
                if self.is_short_list_assign() {
                    self.advance();
                    let targets = self.parse_list_targets(&Token::RBracket)?;
                    self.expect(&Token::RBracket)?;
                    self.expect(&Token::Assign)?;
                    let expr = self.parse_expr()?;
                    return Ok(Expr::ListAssign {
                        targets,
                        expr: Box::new(expr),
                        line,
                    });
                }
                self.advance(); // consume '['
                let elements = self.parse_array_elements(Token::RBracket)?;
                self.expect(&Token::RBracket)?;
                self.last_primary_line = Some(line);
                Ok(Expr::ArrayLiteral(elements))
            }
            Token::ArrayKw => {
                // Long array syntax: array(1, 2, 'a' => 3)
                self.advance(); // consume 'array'
                self.expect_lparen()?;
                let elements = self.parse_array_elements(Token::RParen)?;
                self.expect(&Token::RParen)?;
                Ok(Expr::ArrayLiteral(elements))
            }
            other => Err(format!("Expected expression, got {:?}", other)),
        }
    }

    /// Parse comma-separated array elements until `end_token`.
    fn parse_array_elements(&mut self, end_token: Token) -> Result<Vec<ArrayElement>, String> {
        let mut elements = Vec::new();
        let mut separator_line = None;
        if std::mem::discriminant(&self.peek()) == std::mem::discriminant(&end_token) {
            return Ok(elements);
        }
        loop {
            if let Token::Comma(comma_line) = self.peek() {
                let diagnostic_line = separator_line.unwrap_or(comma_line);
                self.compile_error("Cannot use empty array elements in arrays", diagnostic_line);
                self.advance();
                separator_line = Some(comma_line);
                if std::mem::discriminant(&self.peek()) == std::mem::discriminant(&end_token) {
                    break;
                }
                continue;
            }
            if let Token::DotDotDot(unpack_line) = self.peek() {
                self.advance();
                elements.push(ArrayElement {
                    key: None,
                    value: self.parse_expr()?,
                    unpack: true,
                    unpack_line: Some(unpack_line),
                    by_reference: false,
                });
            } else {
                let leading_reference = if self.peek() == Token::Ampersand {
                    self.advance();
                    true
                } else {
                    false
                };
                let value = self.parse_expr()?;
                if self.peek() == Token::DoubleArrow {
                    if leading_reference {
                        return Err("Array keys cannot be references".into());
                    }
                    self.advance();
                    let by_reference = if self.peek() == Token::Ampersand {
                        self.advance();
                        true
                    } else {
                        false
                    };
                    let actual_value = self.parse_expr()?;
                    elements.push(ArrayElement {
                        key: Some(value),
                        value: actual_value,
                        unpack: false,
                        unpack_line: None,
                        by_reference,
                    });
                } else {
                    elements.push(ArrayElement {
                        key: None,
                        value,
                        unpack: false,
                        unpack_line: None,
                        by_reference: leading_reference,
                    });
                }
            }
            if let Token::Comma(comma_line) = self.peek() {
                self.advance();
                // Allow trailing comma
                if std::mem::discriminant(&self.peek()) == std::mem::discriminant(&end_token) {
                    break;
                }
                separator_line = Some(comma_line);
            } else {
                break;
            }
        }
        Ok(elements)
    }

}
