impl Parser {
    /// Parse expression: ternary ? : (lowest precedence, non-associative in PHP 8+)
    fn parse_expr(&mut self) -> Result<Expr, String> {
        // yield has the lowest precedence
        if self.peek() == Token::Yield {
            return self.parse_yield_expr();
        }

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
        if !matches!(
            &target,
            Expr::Variable(_)
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
        ) {
            return Err("Invalid array append target".into());
        }
        self.expect(&Token::LBracket)?;
        self.expect(&Token::RBracket)?;
        self.expect(&Token::Assign)?;
        if self.peek() == Token::Ampersand {
            self.advance();
        }
        let expr = self.parse_expr()?;
        Ok(Expr::ArrayAppendAssign {
            target: Box::new(target),
            expr: Box::new(expr),
        })
    }

    fn finish_compound_assignment_expression(&mut self, target: Expr) -> Result<Expr, String> {
        if !matches!(
            &target,
            Expr::Variable(_)
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
        ) {
            return Err("Invalid compound assignment target".into());
        }
        let op = Self::compound_assign_op(&self.advance())
            .ok_or_else(|| "Expected compound assignment operator".to_string())?;
        let expr = self.parse_expr()?;
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
        let expr = Box::new(self.parse_expr()?);
        match target {
            Expr::Variable(var) if by_reference => Ok(Expr::AssignReference {
                var,
                target: expr,
            }),
            Expr::Variable(var) => Ok(Expr::Assign { var, expr }),
            Expr::ArrayAccess { .. }
            | Expr::PropertyAccess {
                nullsafe: false, ..
            }
            | Expr::DynamicPropertyAccess {
                nullsafe: false, ..
            }
            | Expr::StaticProperty { .. } => Ok(Expr::AssignTarget {
                target: Box::new(target),
                expr,
            }),
            other => Err(format!("Invalid assignment target: {other:?}")),
        }
    }

    fn finish_coalesce_assignment_expression(&mut self, target: Expr) -> Result<Expr, String> {
        if !matches!(
            &target,
            Expr::Variable(_)
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
        ) {
            return Err("Invalid null-coalescing assignment target".into());
        }
        self.expect(&Token::QuestionQuestionAssign)?;
        let expr = self.parse_expr()?;
        Ok(Expr::CoalesceAssign {
            target: Box::new(target),
            expr: Box::new(expr),
        })
    }

    fn parse_yield_expr(&mut self) -> Result<Expr, String> {
        self.advance(); // consume 'yield'

        // yield from <expr>
        if self.peek() == Token::From {
            self.advance(); // consume 'from'
            let expr = self.parse_expr()?;
            return Ok(Expr::YieldFrom(Box::new(expr)));
        }

        // yield; or yield at end of expression context (no value)
        if matches!(
            self.peek(),
            Token::Semicolon
                | Token::RParen
                | Token::RBracket
                | Token::RBrace
                | Token::Comma
                | Token::Eof
        ) {
            return Ok(Expr::Yield {
                value: None,
                key: None,
            });
        }

        // yield <expr> or yield <key> => <value>
        let first = self.parse_ternary()?;
        if self.peek() == Token::DoubleArrow {
            self.advance(); // consume '=>'
            let value = self.parse_ternary()?;
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
        let left = self.parse_logical_xor()?;

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

    /// Keyword `xor`: both operands are evaluated and converted to bool.
    fn parse_logical_xor(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_logical_or()?;
        while self.peek() == Token::LogicalXor {
            self.advance();
            let right = self.parse_logical_or()?;
            left = Expr::BinaryOp {
                op: BinOp::LogicalXor,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
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
        let mut left = self.parse_concat()?;

        loop {
            // instanceof has same precedence as comparison operators
            if self.peek() == Token::Instanceof {
                self.advance();
                left = if self.peek() == Token::Backslash
                    || matches!(self.peek(), Token::Identifier(_))
                {
                    Expr::Instanceof {
                        expr: Box::new(left),
                        class_name: self.parse_qualified_name()?,
                    }
                } else if self.peek() == Token::Static {
                    self.advance();
                    Expr::Instanceof {
                        expr: Box::new(left),
                        class_name: "static".to_string(),
                    }
                } else if matches!(self.peek(), Token::Variable(_)) {
                    let class = match self.advance() {
                        Token::Variable(name) => Expr::Variable(name),
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
        let operand = self.parse_concat()?;
        self.finish_assignment_tail(operand)
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
                        || matches!(self.peek(), Token::Identifier(_))
                    {
                        Expr::Instanceof {
                            expr: Box::new(expr),
                            class_name: self.parse_qualified_name()?,
                        }
                    } else if self.peek() == Token::Static {
                        self.advance();
                        Expr::Instanceof {
                            expr: Box::new(expr),
                            class_name: "static".to_string(),
                        }
                    } else if let Token::Variable(class_name) = self.peek() {
                        self.advance();
                        let class = self.parse_dynamic_new_class_expression(Expr::Variable(class_name))?;
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
            Token::Clone => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::Clone(Box::new(expr)))
            }
            Token::LParen => {
                // Check for type cast: (int), (string), (float), (bool), (array), (object)
                let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
                let cast_type = match &next {
                    Token::Identifier(name) => match name.as_str() {
                        "int" | "integer" => Some(CastType::Int),
                        "float" | "double" | "real" => Some(CastType::Float),
                        "string" => Some(CastType::String),
                        "bool" | "boolean" => Some(CastType::Bool),
                        "object" => Some(CastType::Object),
                        _ => None,
                    },
                    Token::ArrayKw => Some(CastType::Array),
                    _ => None,
                };
                if let Some(ct) = cast_type {
                    if self.tokens.get(self.pos + 2) == Some(&Token::RParen) {
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
        match self.peek() {
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
            Token::Variable(_) => {
                let name = match self.advance() {
                    Token::Variable(n) => n,
                    _ => unreachable!(),
                };
                Ok(Expr::Variable(name))
            }
            Token::PlusPlus => {
                self.advance();
                let target = self.parse_power()?;
                match target {
                    Expr::Variable(name) => Ok(Expr::PreInc(name)),
                    Expr::PropertyAccess {
                        nullsafe: false, ..
                    }
                    | Expr::DynamicPropertyAccess {
                        nullsafe: false, ..
                    }
                    | Expr::StaticProperty { .. }
                    | Expr::ArrayAccess { .. } => Ok(Expr::PreIncTarget(Box::new(target))),
                    other => Err(format!("Invalid increment target: {other:?}")),
                }
            }
            Token::MinusMinus => {
                self.advance();
                let target = self.parse_power()?;
                match target {
                    Expr::Variable(name) => Ok(Expr::PreDec(name)),
                    Expr::PropertyAccess {
                        nullsafe: false, ..
                    }
                    | Expr::DynamicPropertyAccess {
                        nullsafe: false, ..
                    }
                    | Expr::StaticProperty { .. }
                    | Expr::ArrayAccess { .. } => Ok(Expr::PreDecTarget(Box::new(target))),
                    other => Err(format!("Invalid decrement target: {other:?}")),
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
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::Isset => {
                self.advance();
                self.expect(&Token::LParen)?;
                let mut args = Vec::new();
                let arg = self.parse_expr()?;
                if !Self::is_isset_target(&arg) {
                    return Err("Cannot use isset() on the result of an expression".into());
                }
                args.push(arg);
                while self.peek() == Token::Comma {
                    self.advance();
                    let arg = self.parse_expr()?;
                    if !Self::is_isset_target(&arg) {
                        return Err("Cannot use isset() on the result of an expression".into());
                    }
                    args.push(arg);
                }
                self.expect(&Token::RParen)?;
                Ok(Expr::Isset(args))
            }
            Token::Empty => {
                self.advance();
                self.expect(&Token::LParen)?;
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::Empty(Box::new(expr)))
            }
            Token::Backslash => {
                // Fully qualified name: \App\Models\User() or \App\Models\User::method()
                let name = self.parse_qualified_name()?;
                let generic_args = self.parse_optional_turbofish()?;
                if !generic_args.is_empty() {
                    self.expect(&Token::LParen)?;
                    if self.peek() == Token::DotDotDot && self.peek_at(1) == Token::RParen {
                        return Err("Generic first-class function callables are not supported yet"
                            .into());
                    }
                    let args = self.parse_call_args()?;
                    return Ok(Expr::FunctionCall {
                        name,
                        args,
                        generic_args,
                    });
                }
                if self.peek() == Token::DoubleColon {
                    return self.parse_named_static_access(name);
                }
                if self.peek() == Token::LParen {
                    self.advance();
                    if self.peek() == Token::DotDotDot && self.peek_at(1) == Token::RParen {
                        self.advance();
                        self.advance();
                        return Ok(Expr::FirstClassFunctionCallable(name));
                    }
                    let args = self.parse_call_args()?;
                    Ok(Expr::FunctionCall {
                        name,
                        args,
                        generic_args: Vec::new(),
                    })
                } else {
                    Ok(Expr::Constant(name))
                }
            }
            Token::MagicConstant { .. } => match self.advance() {
                Token::MagicConstant { name, line } => Ok(Expr::MagicConstant { name, line }),
                _ => unreachable!(),
            },
            Token::Identifier(_) => {
                let name = if self.peek_at(1) == Token::Backslash {
                    // Qualified name: App\Models\User
                    self.parse_qualified_name()?
                } else {
                    match self.advance() {
                        Token::Identifier(n) => n,
                        _ => unreachable!(),
                    }
                };
                let generic_args = self.parse_optional_turbofish()?;
                if !generic_args.is_empty() {
                    self.expect(&Token::LParen)?;
                    if self.peek() == Token::DotDotDot && self.peek_at(1) == Token::RParen {
                        return Err("Generic first-class function callables are not supported yet"
                            .into());
                    }
                    let args = self.parse_call_args()?;
                    return Ok(Expr::FunctionCall {
                        name,
                        args,
                        generic_args,
                    });
                }
                // Static access: ClassName::method() or ClassName::$prop
                if self.peek() == Token::DoubleColon {
                    return self.parse_named_static_access(name);
                }
                // Check if this is a function call (followed by `(`)
                if self.peek() == Token::LParen {
                    self.advance(); // consume (
                    if self.peek() == Token::DotDotDot && self.peek_at(1) == Token::RParen {
                        self.advance();
                        self.advance();
                        return Ok(Expr::FirstClassFunctionCallable(name));
                    }
                    let args = self.parse_call_args()?;
                    Ok(Expr::FunctionCall {
                        name,
                        args,
                        generic_args: Vec::new(),
                    })
                } else {
                    // Bare identifier — constant reference (e.g., PHP_INT_MAX, FOO)
                    Ok(Expr::Constant(name))
                }
            }
            Token::Static => {
                if self.peek_at(1) == Token::Function {
                    self.advance(); // consume 'static'
                    return self.parse_closure(true);
                }
                if self.peek_at(1) == Token::Fn {
                    self.advance(); // consume 'static'
                    return self.parse_arrow_function(true);
                }
                if !self.class_scope_active {
                    return Err("Cannot use \"static\" when no class scope is active".into());
                }
                self.advance();
                if self.peek() != Token::DoubleColon {
                    return Err(format!(
                        "Expected :: after static, got {:?}",
                        self.peek()
                    ));
                }
                self.parse_named_static_access("static".to_string())
            }
            Token::Match => {
                return self.parse_match_expr();
            }
            Token::Function => {
                // Closure (anonymous function)
                return self.parse_closure(false);
            }
            Token::Fn => {
                // Arrow function: fn($x) => expr
                return self.parse_arrow_function(false);
            }
            Token::New => {
                self.advance(); // consume 'new'
                if self.peek() == Token::Class {
                    self.advance();
                    let args = if self.peek() == Token::LParen {
                        self.advance();
                        self.parse_call_args()?
                    } else {
                        Vec::new()
                    };
                    let parent = if self.peek() == Token::Extends {
                        self.advance();
                        Some(self.parse_generic_ancestor()?)
                    } else {
                        None
                    };
                    let implements = if self.peek() == Token::Implements {
                        self.advance();
                        let mut interfaces = Vec::new();
                        loop {
                            interfaces.push(self.parse_generic_ancestor()?);
                            if self.peek() == Token::Comma {
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
                    let (properties, constants, methods) = self.parse_anonymous_class_body()?;
                    return Ok(Expr::AnonymousNew {
                        args,
                        parent,
                        implements,
                        properties,
                        constants,
                        methods,
                    });
                }
                if let Token::Variable(class_name) = self.peek() {
                    self.advance();
                    let class = self
                        .parse_dynamic_new_class_expression(Expr::Variable(class_name))?;
                    let args = if self.peek() == Token::LParen {
                        self.advance();
                        self.parse_call_args()?
                    } else {
                        Vec::new()
                    };
                    return Ok(Expr::DynamicNew {
                        class: Box::new(class),
                        args,
                    });
                }
                let class_name = match self.peek() {
                    Token::Backslash | Token::Identifier(_) => self.parse_qualified_name()?,
                    Token::Static if self.class_scope_active => {
                        self.advance();
                        "static".to_string()
                    }
                    token => {
                        return Err(format!("Expected class name after 'new', got {token:?}"));
                    }
                };
                let generic_args = self.parse_optional_turbofish()?;
                let args = if self.peek() == Token::LParen {
                    self.advance(); // consume (
                    self.parse_call_args()?
                } else {
                    Vec::new()
                };
                Ok(Expr::New {
                    class_name,
                    args,
                    generic_args,
                })
            }
            Token::Throw => {
                self.advance();
                let expr = self.parse_expr()?;
                return Ok(Expr::Throw(Box::new(expr)));
            }
            Token::LBracket => {
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
                    });
                }
                self.advance(); // consume '['
                let elements = self.parse_array_elements(Token::RBracket)?;
                self.expect(&Token::RBracket)?;
                Ok(Expr::ArrayLiteral(elements))
            }
            Token::ArrayKw => {
                // Long array syntax: array(1, 2, 'a' => 3)
                self.advance(); // consume 'array'
                self.expect(&Token::LParen)?;
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
        if std::mem::discriminant(&self.peek()) == std::mem::discriminant(&end_token) {
            return Ok(elements);
        }
        loop {
            if self.peek() == Token::DotDotDot {
                self.advance();
                elements.push(ArrayElement {
                    key: None,
                    value: self.parse_expr()?,
                    unpack: true,
                });
            } else {
            let value = self.parse_expr()?;
            if self.peek() == Token::DoubleArrow {
                // key => value
                self.advance();
                let actual_value = self.parse_expr()?;
                elements.push(ArrayElement {
                    key: Some(value),
                    value: actual_value,
                    unpack: false,
                });
            } else {
                elements.push(ArrayElement {
                    key: None,
                    value,
                    unpack: false,
                });
            }
            }
            if self.peek() == Token::Comma {
                self.advance();
                // Allow trailing comma
                if std::mem::discriminant(&self.peek()) == std::mem::discriminant(&end_token) {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(elements)
    }

}
