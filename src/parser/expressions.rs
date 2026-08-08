impl Parser {
    /// Parse expression: ternary ? : (lowest precedence, non-associative in PHP 8+)
    fn parse_expr(&mut self) -> Result<Expr, String> {
        // yield has the lowest precedence
        if self.peek() == Token::Yield {
            return self.parse_yield_expr();
        }

        let expr = self.parse_ternary()?;

        // Handle assignment as expression: $var = expr
        if self.peek() == Token::Assign {
            if let Expr::Variable(var) = expr {
                self.advance(); // consume '='
                let rhs = self.parse_expr()?;
                return Ok(Expr::Assign {
                    var,
                    expr: Box::new(rhs),
                });
            }
        }

        Ok(expr)
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
                return Ok(Expr::Elvis {
                    left: Box::new(expr),
                    right: Box::new(right),
                });
            }

            let then_expr = self.parse_ternary()?;
            self.expect(&Token::Colon)?;
            let else_expr = self.parse_null_coalesce()?;

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
            let right = self.parse_logical_and()?;
            left = Expr::BinaryOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Logical AND: && (left-associative)
    fn parse_logical_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bitwise_or()?;

        while self.peek() == Token::AmpAmp {
            self.advance();
            let right = self.parse_bitwise_or()?;
            left = Expr::BinaryOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
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
                let class_name = if self.peek() == Token::Backslash
                    || matches!(self.peek(), Token::Identifier(_))
                {
                    self.parse_qualified_name()?
                } else {
                    return Err(format!(
                        "Expected class name after instanceof, got {:?}",
                        self.peek()
                    ));
                };
                left = Expr::Instanceof {
                    expr: Box::new(left),
                    class_name,
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
            let right = self.parse_concat()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Concat: . (left-associative, lower than additive in PHP 8)
    fn parse_concat(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_shift()?;

        while self.peek() == Token::Dot {
            self.advance();
            let right = self.parse_shift()?;
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
            Token::Minus => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::UnaryMinus(Box::new(expr)))
            }
            Token::Tilde => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::BitwiseNot(Box::new(expr)))
            }
            Token::LParen => {
                // Check for type cast: (int), (string), (float), (bool), (array)
                let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
                let cast_type = match &next {
                    Token::Identifier(name) => match name.as_str() {
                        "int" | "integer" => Some(CastType::Int),
                        "float" | "double" | "real" => Some(CastType::Float),
                        "string" => Some(CastType::String),
                        "bool" | "boolean" => Some(CastType::Bool),
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
        let base = self.parse_primary()?;

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

    fn parse_primary(&mut self) -> Result<Expr, String> {
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
                // Check for postfix ++ / --
                if self.peek() == Token::PlusPlus {
                    self.advance();
                    return Ok(Expr::PostInc(name));
                } else if self.peek() == Token::MinusMinus {
                    self.advance();
                    return Ok(Expr::PostDec(name));
                }
                let expr = Expr::Variable(name);
                let expr = self.parse_postfix_chain(expr)?;
                Ok(expr)
            }
            Token::PlusPlus => {
                self.advance();
                let name = match self.advance() {
                    Token::Variable(n) => n,
                    other => return Err(format!("Expected variable after ++, got {:?}", other)),
                };
                Ok(Expr::PreInc(name))
            }
            Token::MinusMinus => {
                self.advance();
                let name = match self.advance() {
                    Token::Variable(n) => n,
                    other => return Err(format!("Expected variable after --, got {:?}", other)),
                };
                Ok(Expr::PreDec(name))
            }
            Token::Bang => {
                self.advance();
                let expr = self.parse_primary()?;
                Ok(Expr::Not(Box::new(expr)))
            }
            Token::Print => {
                self.advance();
                let expr = self.parse_expr()?;
                Ok(Expr::Print(Box::new(expr)))
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
                if !Self::is_variable_like(&arg) {
                    return Err("Cannot use isset() on the result of an expression".into());
                }
                args.push(arg);
                while self.peek() == Token::Comma {
                    self.advance();
                    let arg = self.parse_expr()?;
                    if !Self::is_variable_like(&arg) {
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
                if self.peek() == Token::DoubleColon {
                    self.advance();
                    if let Token::Variable(_) = self.peek() {
                        let prop = match self.advance() {
                            Token::Variable(n) => n,
                            _ => unreachable!(),
                        };
                        return Ok(Expr::StaticProperty {
                            class_name: name,
                            property: prop,
                        });
                    }
                    let member = match self.advance() {
                        Token::Identifier(n) => n,
                        other => {
                            return Err(format!("Expected member name after ::, got {:?}", other));
                        }
                    };
                    if self.peek() == Token::LParen {
                        self.advance();
                        let args = self.parse_call_args()?;
                        return Ok(Expr::StaticCall {
                            class_name: name,
                            method: member,
                            args,
                        });
                    } else {
                        return Ok(Expr::StaticProperty {
                            class_name: name,
                            property: member,
                        });
                    }
                }
                if self.peek() == Token::LParen {
                    self.advance();
                    let args = self.parse_call_args()?;
                    Ok(Expr::FunctionCall { name, args })
                } else {
                    Ok(Expr::Constant(name))
                }
            }
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
                // Static access: ClassName::method() or ClassName::$prop
                if self.peek() == Token::DoubleColon {
                    self.advance(); // consume ::
                    if let Token::Variable(_) = self.peek() {
                        let prop = match self.advance() {
                            Token::Variable(n) => n,
                            _ => unreachable!(),
                        };
                        let expr = Expr::StaticProperty {
                            class_name: name,
                            property: prop,
                        };
                        return Ok(self.parse_postfix_chain(expr)?);
                    }
                    let member = match self.advance() {
                        Token::Identifier(n) => n,
                        Token::Class => "class".to_string(),
                        other => {
                            return Err(format!("Expected member name after ::, got {:?}", other));
                        }
                    };
                    if self.peek() == Token::LParen {
                        self.advance();
                        let args = self.parse_call_args()?;
                        let expr = Expr::StaticCall {
                            class_name: name,
                            method: member,
                            args,
                        };
                        return Ok(self.parse_postfix_chain(expr)?);
                    } else {
                        // Static constant/enum case access: ClassName::CONSTANT
                        let expr = Expr::StaticProperty {
                            class_name: name,
                            property: member,
                        };
                        return Ok(self.parse_postfix_chain(expr)?);
                    }
                }
                // Check if this is a function call (followed by `(`)
                if self.peek() == Token::LParen {
                    self.advance(); // consume (
                    let args = self.parse_call_args()?;
                    Ok(Expr::FunctionCall { name, args })
                } else {
                    // Bare identifier — constant reference (e.g., PHP_INT_MAX, FOO)
                    Ok(Expr::Constant(name))
                }
            }
            Token::Match => {
                return self.parse_match_expr();
            }
            Token::Function => {
                // Closure (anonymous function)
                return self.parse_closure();
            }
            Token::Fn => {
                // Arrow function: fn($x) => expr
                return self.parse_arrow_function();
            }
            Token::New => {
                self.advance(); // consume 'new'
                let class_name = if self.peek() == Token::Backslash
                    || matches!(self.peek(), Token::Identifier(_))
                {
                    self.parse_qualified_name()?
                } else {
                    return Err(format!(
                        "Expected class name after 'new', got {:?}",
                        self.peek()
                    ));
                };
                let args = if self.peek() == Token::LParen {
                    self.advance(); // consume (
                    self.parse_call_args()?
                } else {
                    Vec::new()
                };
                let mut expr = Expr::New { class_name, args };
                // Handle ->method() / ->prop chains on new
                expr = self.parse_postfix_chain(expr)?;
                return Ok(expr);
            }
            Token::Throw => {
                self.advance();
                let expr = self.parse_expr()?;
                return Ok(Expr::Throw(Box::new(expr)));
            }
            Token::Clone => {
                self.advance(); // consume 'clone'
                let expr = self.parse_unary()?;
                return Ok(Expr::Clone(Box::new(expr)));
            }
            Token::LBracket => {
                // Short array syntax: [1, 2, 'a' => 3]
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
            let value = self.parse_expr()?;
            if self.peek() == Token::DoubleArrow {
                // key => value
                self.advance();
                let actual_value = self.parse_expr()?;
                elements.push(ArrayElement {
                    key: Some(value),
                    value: actual_value,
                });
            } else {
                elements.push(ArrayElement { key: None, value });
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

    /// Parse postfix chains: [idx], ->prop, ->method()
    fn parse_postfix_chain(&mut self, mut expr: Expr) -> Result<Expr, String> {
        loop {
            match self.peek() {
                Token::LBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(&Token::RBracket)?;
                    expr = Expr::ArrayAccess {
                        array: Box::new(expr),
                        index: Box::new(index),
                    };
                }
                Token::LParen => {
                    // Dynamic call: $var(...), $arr[0](...), etc.
                    self.advance(); // consume '('
                    let args = self.parse_call_args()?;
                    expr = Expr::DynamicCall {
                        callable: Box::new(expr),
                        args,
                    };
                }
                Token::Arrow | Token::NullSafe => {
                    let nullsafe = matches!(self.peek(), Token::NullSafe);
                    self.advance();
                    let member = match self.advance() {
                        Token::Identifier(n) => n,
                        other => {
                            return Err(format!(
                                "Expected property/method name after {}, got {:?}",
                                if nullsafe { "?->" } else { "->" },
                                other
                            ));
                        }
                    };
                    if self.peek() == Token::LParen {
                        self.advance();
                        let args = self.parse_call_args()?;
                        expr = Expr::MethodCall {
                            object: Box::new(expr),
                            method: member,
                            args,
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
