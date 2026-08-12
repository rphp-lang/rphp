impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            in_class_body: false,
            class_scope_active: false,
            generic_scopes: Vec::new(),
        }
    }

    pub fn parse(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(&Token::OpenTag)?;
        let mut stmts = Vec::new();

        while !self.at_eof() {
            stmts.push(self.parse_stmt()?);
        }

        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek() {
            Token::Semicolon => {
                self.advance();
                Ok(Stmt::Noop)
            }
            Token::Declare => {
                self.advance(); // consume 'declare'
                self.expect(&Token::LParen)?;
                let directive = match self.advance() {
                    Token::Identifier(n) => n,
                    other => {
                        return Err(format!(
                            "Expected directive name in declare(), got {:?}",
                            other
                        ));
                    }
                };
                self.expect(&Token::Assign)?;
                let value = match self.advance() {
                    Token::Integer(n) => n,
                    Token::True => 1,
                    Token::False => 0,
                    other => {
                        return Err(format!(
                            "Expected integer value in declare(), got {:?}",
                            other
                        ));
                    }
                };
                self.expect(&Token::RParen)?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Declare { directive, value })
            }
            Token::Namespace => {
                self.advance(); // consume 'namespace'
                let name = self.parse_qualified_name()?;
                if self.peek() == Token::LBrace {
                    // Braced namespace: namespace App\Models { ... }
                    self.advance(); // consume '{'
                    let mut body = Vec::new();
                    while self.peek() != Token::RBrace && self.peek() != Token::Eof {
                        body.push(self.parse_stmt()?);
                    }
                    self.expect(&Token::RBrace)?;
                    Ok(Stmt::Namespace { name, body })
                } else {
                    // Unbraced namespace: namespace App\Models; (rest of file belongs to this namespace)
                    self.expect(&Token::Semicolon)?;
                    let mut body = Vec::new();
                    while self.peek() != Token::Eof && self.peek() != Token::Namespace {
                        body.push(self.parse_stmt()?);
                    }
                    Ok(Stmt::Namespace { name, body })
                }
            }
            Token::Use if !self.in_class_body => {
                // Top-level class/function import. Their alias tables are
                // separate in PHP even when the source alias is identical.
                self.advance(); // consume 'use'
                let kind = if self.peek() == Token::Function {
                    self.advance();
                    UseKind::Function
                } else {
                    UseKind::Class
                };
                let mut imports = Vec::new();
                loop {
                    let fqn = self.parse_qualified_name()?;
                    let alias = if self.peek() == Token::As {
                        self.advance(); // consume 'as'
                        match self.advance() {
                            Token::Identifier(n) => n,
                            other => {
                                return Err(format!(
                                    "Expected alias name after 'as', got {:?}",
                                    other
                                ));
                            }
                        }
                    } else {
                        // Default alias = last segment
                        fqn.rsplit('\\').next().unwrap_or(&fqn).to_string()
                    };
                    imports.push((fqn, alias));
                    if self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::UseDecl { kind, imports })
            }
            Token::Const => {
                self.advance(); // consume 'const'
                let name = match self.advance() {
                    Token::Identifier(n) | Token::MagicConstant { name: n, .. } => n,
                    other => {
                        return Err(format!(
                            "Expected constant name after 'const', got {:?}",
                            other
                        ));
                    }
                };
                self.expect(&Token::Assign)?;
                let value = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Const { name, value })
            }
            Token::Echo => {
                self.advance();
                let mut expressions = vec![self.parse_expr()?];
                while self.peek() == Token::Comma {
                    self.advance();
                    expressions.push(self.parse_expr()?);
                }
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Echo(expressions))
            }
            Token::Include | Token::IncludeOnce | Token::Require | Token::RequireOnce => {
                let tok = self.advance();
                let (is_require, is_once) = match tok {
                    Token::Include => (false, false),
                    Token::IncludeOnce => (false, true),
                    Token::Require => (true, false),
                    Token::RequireOnce => (true, true),
                    _ => unreachable!(),
                };
                let path = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Include {
                    path,
                    is_require,
                    is_once,
                })
            }
            Token::Variable(_) => {
                // Peek ahead to determine statement type
                let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
                if next == Token::LBracket {
                    // Could be $a[] = ..., $a[idx] = ..., or expression
                    // Check for $a[] = (array push)
                    let is_push = self.tokens.get(self.pos + 2) == Some(&Token::RBracket)
                        && self.tokens.get(self.pos + 3) == Some(&Token::Assign);
                    if is_push {
                        let var_name = match self.advance() {
                            Token::Variable(name) => name,
                            _ => unreachable!(),
                        };
                        self.advance(); // consume '['
                        self.advance(); // consume ']'
                        self.advance(); // consume '='
                        let expr = self.parse_expr()?;
                        self.expect(&Token::Semicolon)?;
                        return Ok(Stmt::ArrayPush {
                            var: var_name,
                            expr,
                        });
                    }
                    // Parse a complete $a[idx]...[idx] write. Keeping every
                    // dimension in one AST node lets the compiler evaluate
                    // keys once and rebuild COW parents from the leaf.
                    if self.is_array_assign() {
                        let var_name = match self.advance() {
                            Token::Variable(name) => name,
                            _ => unreachable!(),
                        };
                        let mut indices = Vec::new();
                        while self.peek() == Token::LBracket {
                            self.advance();
                            indices.push(self.parse_expr()?);
                            self.expect(&Token::RBracket)?;
                        }
                        self.expect(&Token::Assign)?;
                        let expr = self.parse_expr()?;
                        self.expect(&Token::Semicolon)?;
                        return Ok(if indices.len() == 1 {
                            Stmt::ArrayAssign {
                                var: var_name,
                                index: indices.pop().unwrap(),
                                expr,
                            }
                        } else {
                            Stmt::NestedArrayAssign {
                                root: Expr::Variable(var_name),
                                indices,
                                expr,
                            }
                        });
                    }
                    // Otherwise fall through to expression parsing
                    let expr = self.parse_expr()?;
                    if self.is_array_append_suffix() {
                        return self.finish_array_append_statement(expr);
                    }
                    if self.peek() == Token::QuestionQuestionAssign {
                        return self.finish_coalesce_assign_statement(expr);
                    }
                    if Self::compound_assign_op(&self.peek()).is_some() {
                        return self.finish_compound_assign_statement(expr);
                    }
                    self.finish_value_expression_statement(expr)
                } else if next == Token::Assign {
                    let var_name = match self.advance() {
                        Token::Variable(name) => name,
                        _ => unreachable!(),
                    };
                    self.expect(&Token::Assign)?;
                    if self.peek() == Token::Ampersand {
                        self.advance();
                        let target = self.parse_expr()?;
                        if !self.is_empty_array_dimension_suffix() {
                            return Err(
                                "Only an appended array element can currently be bound by reference"
                                    .into(),
                            );
                        }
                        if !matches!(
                            &target,
                            Expr::Variable(_)
                                | Expr::ArrayAccess { .. }
                                | Expr::PropertyAccess {
                                    nullsafe: false,
                                    ..
                                }
                                | Expr::StaticProperty { .. }
                        ) {
                            return Err("Invalid array reference target".into());
                        }
                        self.expect(&Token::LBracket)?;
                        self.expect(&Token::RBracket)?;
                        self.expect(&Token::Semicolon)?;
                        return Ok(Stmt::BindArrayAppendReference {
                            var: var_name,
                            target,
                        });
                    }
                    let expr = self.parse_expr()?;
                    self.expect(&Token::Semicolon)?;
                    Ok(Stmt::Assign {
                        var: var_name,
                        expr,
                    })
                } else if let Some(bin_op) = Self::compound_assign_op(&next) {
                    // Compound assignment: $x += expr  →  $x = $x + expr
                    let var_name = match self.advance() {
                        Token::Variable(name) => name,
                        _ => unreachable!(),
                    };
                    self.advance(); // consume the compound operator
                    let rhs = self.parse_expr()?;
                    self.expect(&Token::Semicolon)?;
                    Ok(Stmt::Assign {
                        var: var_name.clone(),
                        expr: Expr::BinaryOp {
                            op: bin_op,
                            left: Box::new(Expr::Variable(var_name)),
                            right: Box::new(rhs),
                        },
                    })
                } else {
                    let expr = self.parse_expr()?;
                    if self.is_array_append_suffix() {
                        return self.finish_array_append_statement(expr);
                    }
                    if self.peek() == Token::QuestionQuestionAssign {
                        return self.finish_coalesce_assign_statement(expr);
                    }
                    if Self::compound_assign_op(&self.peek()).is_some() {
                        return self.finish_compound_assign_statement(expr);
                    }
                    // Check for property/array-dim assignment: $obj->prop = expr or $obj->prop[$key] = expr
                    if self.peek() == Token::Assign {
                        // Check structure without consuming
                        let is_prop_assign = matches!(&expr, Expr::PropertyAccess { .. });

                        if is_prop_assign {
                            if let Expr::PropertyAccess {
                                object, property, ..
                            } = expr
                            {
                                self.advance(); // consume '='
                                let rhs = self.parse_expr()?;
                                self.expect(&Token::Semicolon)?;
                                return Ok(Stmt::AssignProp {
                                    object: *object,
                                    property,
                                    expr: rhs,
                                });
                            }
                        } else if matches!(expr, Expr::ArrayAccess { .. }) {
                            let (root, mut indices) = Self::split_array_access(expr);
                            self.advance(); // consume '='
                            let rhs = self.parse_expr()?;
                            self.expect(&Token::Semicolon)?;
                            if indices.len() == 1
                                && let Expr::PropertyAccess {
                                    object, property, ..
                                } = root
                            {
                                return Ok(Stmt::AssignObjArrayDim {
                                    object: *object,
                                    property,
                                    index: indices.pop().unwrap(),
                                    expr: rhs,
                                });
                            }
                            if matches!(
                                root,
                                Expr::Variable(_)
                                    | Expr::PropertyAccess { .. }
                                    | Expr::StaticProperty { .. }
                            ) {
                                return Ok(Stmt::NestedArrayAssign {
                                    root,
                                    indices,
                                    expr: rhs,
                                });
                            }
                            return Err("Unsupported array assignment target".into());
                        }
                    }
                    self.finish_value_expression_statement(expr)
                }
            }
            Token::If => self.parse_if(),
            Token::ElseIf => {
                // elseif at statement level (shouldn't happen normally, but handle gracefully)
                self.parse_if()
            }
            Token::While => {
                self.advance(); // consume 'while'
                self.expect(&Token::LParen)?;
                let condition = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                let body = self.parse_block_or_stmt()?;
                Ok(Stmt::While { condition, body })
            }
            Token::Do => {
                self.advance(); // consume 'do'
                let body = self.parse_block_or_stmt()?;
                self.expect(&Token::While)?;
                self.expect(&Token::LParen)?;
                let condition = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::DoWhile { condition, body })
            }
            Token::Break => {
                self.advance();
                let level = self.parse_break_continue_level()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Break(level))
            }
            Token::Continue => {
                self.advance();
                let level = self.parse_break_continue_level()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Continue(level))
            }
            Token::Switch => {
                self.advance(); // consume 'switch'
                self.expect(&Token::LParen)?;
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                self.expect(&Token::LBrace)?;
                let mut cases = Vec::new();
                let mut has_default = false;
                while self.peek() != Token::RBrace && !self.at_eof() {
                    match self.peek() {
                        Token::Case => {
                            self.advance();
                            let value = self.parse_expr()?;
                            self.expect(&Token::Colon)?;
                            let mut body = Vec::new();
                            while !matches!(
                                self.peek(),
                                Token::Case | Token::Default | Token::RBrace
                            ) && !self.at_eof()
                            {
                                body.push(self.parse_stmt()?);
                            }
                            cases.push(SwitchCase {
                                value: Some(value),
                                body,
                            });
                        }
                        Token::Default => {
                            if has_default {
                                return Err(
                                    "Switch statements may only contain one default clause".into(),
                                );
                            }
                            has_default = true;
                            self.advance();
                            self.expect(&Token::Colon)?;
                            let mut body = Vec::new();
                            while !matches!(
                                self.peek(),
                                Token::Case | Token::Default | Token::RBrace
                            ) && !self.at_eof()
                            {
                                body.push(self.parse_stmt()?);
                            }
                            cases.push(SwitchCase { value: None, body });
                        }
                        other => {
                            return Err(format!(
                                "Expected 'case' or 'default' in switch, got {:?}",
                                other
                            ));
                        }
                    }
                }
                self.expect(&Token::RBrace)?;
                Ok(Stmt::Switch { expr, cases })
            }
            Token::For => {
                self.advance(); // consume 'for'
                self.expect(&Token::LParen)?;

                // Init: optional assignment or expression before first ;
                let init = if self.peek() == Token::Semicolon {
                    vec![]
                } else {
                    let stmt = self.parse_for_init()?;
                    vec![stmt]
                };
                self.expect(&Token::Semicolon)?;

                // Condition: optional expression before second ;
                let condition = if self.peek() == Token::Semicolon {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect(&Token::Semicolon)?;

                // Update: optional expression before )
                let update = if self.peek() == Token::RParen {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect(&Token::RParen)?;

                let body = self.parse_block_or_stmt()?;
                Ok(Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                })
            }
            Token::Foreach => {
                self.advance(); // consume 'foreach'
                self.expect(&Token::LParen)?;
                let array = self.parse_expr()?;
                self.expect(&Token::As)?;
                // foreach ($arr as $key => $val) or foreach ($arr as $val)
                let first_by_ref = if self.peek() == Token::Ampersand {
                    self.advance();
                    true
                } else {
                    false
                };
                let first_var = match self.advance() {
                    Token::Variable(name) => name,
                    other => return Err(format!("Expected variable after 'as', got {:?}", other)),
                };
                let (key_var, value_var, by_ref) = if self.peek() == Token::DoubleArrow {
                    if first_by_ref {
                        return Err("Foreach key cannot be a reference".into());
                    }
                    self.advance(); // consume '=>'
                    let by_ref = if self.peek() == Token::Ampersand {
                        self.advance();
                        true
                    } else {
                        false
                    };
                    let val = match self.advance() {
                        Token::Variable(name) => name,
                        other => {
                            return Err(format!("Expected variable after '=>', got {:?}", other));
                        }
                    };
                    (Some(first_var), val, by_ref)
                } else {
                    (None, first_var, first_by_ref)
                };
                self.expect(&Token::RParen)?;
                let body = self.parse_block_or_stmt()?;
                Ok(Stmt::Foreach {
                    array,
                    value_var,
                    key_var,
                    by_ref,
                    body,
                })
            }
            Token::Function => {
                self.advance(); // consume 'function'
                let name = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected function name, got {:?}", other)),
                };
                // A named function never inherits the surrounding method's
                // class scope. Closures deliberately do.
                let previous_class_scope = self.class_scope_active;
                self.class_scope_active = false;
                let generic_params = self.parse_generic_parameters()?;
                self.push_generic_scope(&generic_params);
                self.expect(&Token::LParen)?;
                let params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                let return_type = self.parse_return_type()?;
                self.expect(&Token::LBrace)?;
                let mut body = Vec::new();
                while self.peek() != Token::RBrace && !self.at_eof() {
                    body.push(self.parse_stmt()?);
                }
                self.expect(&Token::RBrace)?;
                self.pop_generic_scope();
                self.class_scope_active = previous_class_scope;
                Ok(Stmt::Function {
                    name,
                    params,
                    body,
                    return_type,
                    generic_params,
                })
            }
            Token::Return => {
                self.advance(); // consume 'return'
                if self.peek() == Token::Semicolon {
                    self.advance();
                    Ok(Stmt::Return(None))
                } else {
                    let expr = self.parse_expr()?;
                    self.expect(&Token::Semicolon)?;
                    Ok(Stmt::Return(Some(expr)))
                }
            }
            Token::Unset => {
                self.advance();
                self.expect(&Token::LParen)?;
                let mut targets = Vec::new();
                let expr = self.parse_expr()?;
                if !Self::is_variable_like(&expr) {
                    return Err("Cannot use unset() on the result of an expression".into());
                }
                targets.push(expr);
                while self.peek() == Token::Comma {
                    self.advance();
                    let expr = self.parse_expr()?;
                    if !Self::is_variable_like(&expr) {
                        return Err("Cannot use unset() on the result of an expression".into());
                    }
                    targets.push(expr);
                }
                self.expect(&Token::RParen)?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Unset(targets))
            }
            Token::Try => self.parse_try_catch(),
            Token::Throw => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Throw(expr))
            }
            Token::Class | Token::Abstract | Token::Final => self.parse_class(),
            Token::Enum => self.parse_enum(),
            Token::Interface => self.parse_interface(),
            Token::Trait => self.parse_trait(),
            Token::Static if self.peek_at(1) == Token::DoubleColon => {
                let expr = self.parse_expr()?;
                self.finish_static_property_statement(expr)
            }
            Token::Static
                if matches!(self.peek_at(1), Token::Function | Token::Fn) =>
            {
                self.parse_expression_statement()
            }
            Token::Isset
            | Token::Empty
            | Token::Match
            | Token::New
            | Token::Yield
            | Token::Clone
            | Token::Print
            | Token::LParen
            | Token::Fn
            | Token::Integer(_)
            | Token::Float(_)
            | Token::StringLiteral(_)
            | Token::Null
            | Token::True
            | Token::False
            | Token::Bang
            | Token::Minus
            | Token::At
            | Token::Tilde
            | Token::PlusPlus
            | Token::MinusMinus
            | Token::ArrayKw => self.parse_expression_statement(),
            Token::Identifier(_) | Token::Backslash => {
                // Check for list() destructuring: list($a, $b) = expr;
                if let Token::Identifier(ref name) = self.peek() {
                    if name == "list" && self.peek_at(1) == Token::LParen {
                        return self.parse_list_assign();
                    }
                }
                let expr = self.parse_expr()?;
                self.finish_static_property_statement(expr)
            }
            Token::LBracket => {
                // Try short destructuring: [$a, $b] = expr;
                if self.is_short_list_assign() {
                    return self.parse_short_list_assign();
                }
                // Otherwise treat as expression statement (array literal)
                self.parse_expression_statement()
            }
            Token::Global => {
                self.advance(); // consume 'global'
                let mut vars = Vec::new();
                loop {
                    match self.advance() {
                        Token::Variable(name) => vars.push(name),
                        other => {
                            return Err(format!(
                                "Expected variable after 'global', got {:?}",
                                other
                            ));
                        }
                    }
                    if self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Global(vars))
            }
            Token::Static
                if !self.in_class_body && matches!(self.peek_at(1), Token::Variable(_)) =>
            {
                // static $var = expr; (function-level static variable)
                self.advance(); // consume 'static'
                let mut vars = Vec::new();
                loop {
                    let var_name = match self.advance() {
                        Token::Variable(name) => name,
                        other => {
                            return Err(format!(
                                "Expected variable after 'static', got {:?}",
                                other
                            ));
                        }
                    };
                    let default = if self.peek() == Token::Assign {
                        self.advance();
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    vars.push((var_name, default));
                    if self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::StaticVar { vars })
            }
            other => Err(format!("Unexpected token: {:?}", other)),
        }
    }

    /// Map compound assignment token to BinOp, or None.
    fn compound_assign_op(tok: &Token) -> Option<BinOp> {
        match tok {
            Token::PlusAssign => Some(BinOp::Add),
            Token::MinusAssign => Some(BinOp::Sub),
            Token::StarAssign => Some(BinOp::Mul),
            Token::StarStarAssign => Some(BinOp::Pow),
            Token::SlashAssign => Some(BinOp::Div),
            Token::PercentAssign => Some(BinOp::Mod),
            Token::DotAssign => Some(BinOp::Concat),
            Token::AmpAssign => Some(BinOp::BitwiseAnd),
            Token::PipeAssign => Some(BinOp::BitwiseOr),
            Token::CaretAssign => Some(BinOp::BitwiseXor),
            Token::ShiftLeftAssign => Some(BinOp::ShiftLeft),
            Token::ShiftRightAssign => Some(BinOp::ShiftRight),
            _ => None,
        }
    }

    /// Finish a named/self/parent/static property expression statement. Basic
    /// and compound writes share this path so pseudo-class resolution cannot
    /// drift between their parser branches.
    fn finish_static_property_statement(&mut self, expr: Expr) -> Result<Stmt, String> {
        if self.is_array_append_suffix() {
            return self.finish_array_append_statement(expr);
        }
        if self.peek() == Token::QuestionQuestionAssign {
            return self.finish_coalesce_assign_statement(expr);
        }
        if Self::compound_assign_op(&self.peek()).is_some() {
            return self.finish_compound_assign_statement(expr);
        }
        if self.peek() == Token::Assign && matches!(expr, Expr::ArrayAccess { .. }) {
            let (root, indices) = Self::split_array_access(expr);
            if !matches!(root, Expr::StaticProperty { .. }) {
                return Err("Unsupported static array assignment target".into());
            }
            self.advance();
            let value = self.parse_expr()?;
            self.expect(&Token::Semicolon)?;
            return Ok(Stmt::NestedArrayAssign {
                root,
                indices,
                expr: value,
            });
        }
        let static_property = match &expr {
            Expr::StaticProperty {
                class_name,
                property,
            } => Some((class_name.clone(), property.clone())),
            _ => None,
        };
        if let Some((class_name, property)) = static_property {
            if self.peek() == Token::Assign {
                self.advance();
                let value = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                return Ok(Stmt::AssignStaticProp {
                    class_name,
                    property,
                    expr: value,
                });
            }
            if let Some(op) = Self::compound_assign_op(&self.peek()) {
                self.advance();
                let right = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                return Ok(Stmt::AssignStaticProp {
                    class_name,
                    property,
                    expr: Expr::BinaryOp {
                        op,
                        left: Box::new(expr),
                        right: Box::new(right),
                    },
                });
            }
        }
        self.finish_value_expression_statement(expr)
    }

    fn is_array_append_suffix(&self) -> bool {
        self.is_empty_array_dimension_suffix()
            && self.peek_at(2) == Token::Assign
    }

    fn is_empty_array_dimension_suffix(&self) -> bool {
        self.peek() == Token::LBracket && self.peek_at(1) == Token::RBracket
    }

    fn finish_array_append_statement(&mut self, target: Expr) -> Result<Stmt, String> {
        if !matches!(
            &target,
            Expr::Variable(_)
                | Expr::ArrayAccess { .. }
                | Expr::PropertyAccess {
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
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon)?;
        Ok(Stmt::ArrayAppend { target, expr })
    }

    fn finish_coalesce_assign_statement(&mut self, target: Expr) -> Result<Stmt, String> {
        let valid_target = matches!(
            &target,
            Expr::Variable(_)
                | Expr::ArrayAccess { .. }
                | Expr::PropertyAccess {
                    nullsafe: false,
                    ..
                }
                | Expr::StaticProperty { .. }
        );
        if !valid_target {
            return Err("Invalid null-coalescing assignment target".into());
        }
        self.expect(&Token::QuestionQuestionAssign)?;
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon)?;
        Ok(Stmt::CoalesceAssign { target, expr })
    }

    fn finish_compound_assign_statement(&mut self, target: Expr) -> Result<Stmt, String> {
        if !matches!(
            &target,
            Expr::Variable(_)
                | Expr::ArrayAccess { .. }
                | Expr::PropertyAccess {
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
        self.expect(&Token::Semicolon)?;
        Ok(Stmt::CompoundAssign { target, op, expr })
    }

    /// Parse if / elseif / else chain.
    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'if' or 'elseif'
        self.expect(&Token::LParen)?;
        let condition = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        let then_body = self.parse_block_or_stmt()?;
        let else_body = if self.peek() == Token::ElseIf {
            // elseif desugars to else { if (...) { ... } }
            vec![self.parse_if()?]
        } else if self.peek() == Token::Else {
            self.advance();
            // Check for "else if" (two tokens) which is equivalent to "elseif"
            if self.peek() == Token::If {
                vec![self.parse_if()?]
            } else {
                self.parse_block_or_stmt()?
            }
        } else {
            vec![]
        };
        Ok(Stmt::If {
            condition,
            then_body,
            else_body,
        })
    }

    /// Parse for-loop init: either `$var = expr`, `$var op= expr`, or an expression.
    fn parse_for_init(&mut self) -> Result<Stmt, String> {
        if let Token::Variable(_) = self.peek() {
            let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
            if next == Token::Assign {
                let var_name = match self.advance() {
                    Token::Variable(name) => name,
                    _ => unreachable!(),
                };
                self.expect(&Token::Assign)?;
                let expr = self.parse_expr()?;
                return Ok(Stmt::Assign {
                    var: var_name,
                    expr,
                });
            } else if let Some(bin_op) = Self::compound_assign_op(&next) {
                let var_name = match self.advance() {
                    Token::Variable(name) => name,
                    _ => unreachable!(),
                };
                self.advance(); // consume compound operator
                let rhs = self.parse_expr()?;
                return Ok(Stmt::Assign {
                    var: var_name.clone(),
                    expr: Expr::BinaryOp {
                        op: bin_op,
                        left: Box::new(Expr::Variable(var_name)),
                        right: Box::new(rhs),
                    },
                });
            }
        }
        let expr = self.parse_expr()?;
        Ok(Stmt::ExprStmt(expr))
    }

    /// Parse either { stmts } or a single stmt
    fn parse_block_or_stmt(&mut self) -> Result<Vec<Stmt>, String> {
        if self.peek() == Token::LBrace {
            self.advance(); // consume {
            let mut stmts = Vec::new();
            while self.peek() != Token::RBrace && !self.at_eof() {
                stmts.push(self.parse_stmt()?);
            }
            self.expect(&Token::RBrace)?;
            Ok(stmts)
        } else {
            // Single statement (no braces)
            let stmt = self.parse_stmt()?;
            Ok(vec![stmt])
        }
    }

    /// Parse an expression used only for its side effects. Keeping all simple
    /// expression-statement entry points on this path prevents the statement
    /// grammar from lagging behind expressions already accepted by
    /// `parse_primary`.
    fn parse_expression_statement(&mut self) -> Result<Stmt, String> {
        let expr = self.parse_expr()?;
        if self.is_array_append_suffix() {
            return self.finish_array_append_statement(expr);
        }
        if self.peek() == Token::QuestionQuestionAssign {
            return self.finish_coalesce_assign_statement(expr);
        }
        if Self::compound_assign_op(&self.peek()).is_some() {
            return self.finish_compound_assign_statement(expr);
        }
        self.finish_value_expression_statement(expr)
    }

    fn finish_value_expression_statement(&mut self, expr: Expr) -> Result<Stmt, String> {
        self.expect(&Token::Semicolon)?;
        match expr {
            Expr::CoalesceAssign { target, expr } => Ok(Stmt::CoalesceAssign {
                target: *target,
                expr: *expr,
            }),
            Expr::AssignTarget { target, expr } => match *target {
                Expr::PropertyAccess {
                    object,
                    property,
                    nullsafe: false,
                } => Ok(Stmt::AssignProp {
                    object: *object,
                    property,
                    expr: *expr,
                }),
                Expr::StaticProperty {
                    class_name,
                    property,
                } => Ok(Stmt::AssignStaticProp {
                    class_name,
                    property,
                    expr: *expr,
                }),
                target @ Expr::ArrayAccess { .. } => {
                    let (root, mut indices) = Self::split_array_access(target);
                    if indices.len() == 1
                        && let Expr::PropertyAccess {
                            object,
                            property,
                            nullsafe: false,
                        } = root
                    {
                        return Ok(Stmt::AssignObjArrayDim {
                            object: *object,
                            property,
                            index: indices.pop().unwrap(),
                            expr: *expr,
                        });
                    }
                    Ok(Stmt::NestedArrayAssign {
                        root,
                        indices,
                        expr: *expr,
                    })
                }
                _ => Err("Invalid assignment target".into()),
            },
            expr => Ok(Stmt::ExprStmt(expr)),
        }
    }
}
