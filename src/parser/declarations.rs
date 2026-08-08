impl Parser {
    /// Parse try { } catch (Type $e) { } finally { }
    fn parse_try_catch(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'try'
        self.expect(&Token::LBrace)?;
        let mut try_body = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            try_body.push(self.parse_stmt()?);
        }
        self.expect(&Token::RBrace)?;

        let mut catches = Vec::new();
        while self.peek() == Token::Catch {
            self.advance(); // consume 'catch'
            self.expect(&Token::LParen)?;
            // Parse exception type(s): ExA | ExB
            let mut types = Vec::new();
            let type_name = self.parse_qualified_name()?;
            types.push(type_name);
            while self.peek() == Token::Pipe {
                self.advance();
                let t = self.parse_qualified_name()?;
                types.push(t);
            }
            let var = match self.advance() {
                Token::Variable(n) => n,
                other => return Err(format!("Expected variable in catch, got {:?}", other)),
            };
            self.expect(&Token::RParen)?;
            self.expect(&Token::LBrace)?;
            let mut body = Vec::new();
            while self.peek() != Token::RBrace && !self.at_eof() {
                body.push(self.parse_stmt()?);
            }
            self.expect(&Token::RBrace)?;
            catches.push(CatchClause { types, var, body });
        }

        let finally_body = if self.peek() == Token::Finally {
            self.advance();
            self.expect(&Token::LBrace)?;
            let mut body = Vec::new();
            while self.peek() != Token::RBrace && !self.at_eof() {
                body.push(self.parse_stmt()?);
            }
            self.expect(&Token::RBrace)?;
            Some(body)
        } else {
            None
        };

        if catches.is_empty() && finally_body.is_none() {
            return Err("Cannot use try without catch or finally".into());
        }

        Ok(Stmt::TryCatch {
            try_body,
            catches,
            finally_body,
        })
    }

    /// Parse class declaration
    fn parse_class(&mut self) -> Result<Stmt, String> {
        let mut is_abstract = false;
        let mut is_final = false;
        // Consume leading modifiers (abstract, final) in any order before 'class'
        loop {
            match self.peek() {
                Token::Abstract => {
                    self.advance();
                    is_abstract = true;
                }
                Token::Final => {
                    self.advance();
                    is_final = true;
                }
                _ => break,
            }
        }
        if is_abstract && is_final {
            return Err("Cannot use the final modifier on an abstract class".into());
        }
        self.advance(); // consume 'class'
        let name = match self.advance() {
            Token::Identifier(n) => n,
            other => return Err(format!("Expected class name, got {:?}", other)),
        };
        let parent = if self.peek() == Token::Extends {
            self.advance();
            Some(self.parse_qualified_name()?)
        } else {
            None
        };
        let implements = if self.peek() == Token::Implements {
            self.advance();
            let mut ifaces = Vec::new();
            loop {
                ifaces.push(self.parse_qualified_name()?);
                if self.peek() == Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            ifaces
        } else {
            Vec::new()
        };
        self.expect(&Token::LBrace)?;

        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut uses = Vec::new();

        let prev_in_class = self.in_class_body;
        self.in_class_body = true;

        while self.peek() != Token::RBrace && !self.at_eof() {
            // Trait `use` statements: use Foo, Bar;
            if self.peek() == Token::Use {
                self.advance(); // consume 'use'
                loop {
                    uses.push(self.parse_qualified_name()?);
                    if self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&Token::Semicolon)?;
                continue;
            }

            let (vis, is_static, is_final, is_readonly) = self.parse_visibility_and_static();

            if self.peek() == Token::Function {
                // Method
                self.advance(); // consume 'function'
                let method_name = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected method name, got {:?}", other)),
                };
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
                methods.push(ClassMethod {
                    visibility: vis,
                    name: method_name,
                    params,
                    body,
                    is_static,
                    is_final,
                    return_type,
                });
            } else if matches!(self.peek(), Token::Variable(_)) || self.is_type_hint_start() {
                // Property — possibly with type hint: `private int $x = 0;`
                // Skip type hint if present (we don't enforce property types at runtime yet)
                let _type_hint = self.try_parse_type_hint()?;
                let prop_name = match self.advance() {
                    Token::Variable(n) => n,
                    other => return Err(format!("Expected property variable, got {:?}", other)),
                };
                let default = if self.peek() == Token::Assign {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(&Token::Semicolon)?;
                properties.push(ClassProperty {
                    visibility: vis,
                    name: prop_name,
                    default,
                    is_static,
                    is_readonly,
                });
            } else if matches!(self.peek(), Token::Const) {
                // Class constants — not yet implemented
                return Err(format!("Unexpected token in class body: {:?}", self.peek()));
            } else {
                return Err(format!("Unexpected token in class body: {:?}", self.peek()));
            }
        }
        self.in_class_body = prev_in_class;
        self.expect(&Token::RBrace)?;

        Ok(Stmt::Class {
            name,
            parent,
            implements,
            is_abstract,
            is_final,
            properties,
            methods,
            uses,
        })
    }

    /// Parse trait declaration
    fn parse_trait(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'trait'
        let name = match self.advance() {
            Token::Identifier(n) => n,
            other => return Err(format!("Expected trait name, got {:?}", other)),
        };
        self.expect(&Token::LBrace)?;

        let mut properties = Vec::new();
        let mut methods = Vec::new();

        while self.peek() != Token::RBrace && !self.at_eof() {
            let (vis, is_static, is_final, is_readonly) = self.parse_visibility_and_static();

            if self.peek() == Token::Function {
                self.advance();
                let method_name = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected method name, got {:?}", other)),
                };
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
                methods.push(ClassMethod {
                    visibility: vis,
                    name: method_name,
                    params,
                    body,
                    is_static,
                    is_final,
                    return_type,
                });
            } else if matches!(self.peek(), Token::Variable(_)) || self.is_type_hint_start() {
                // Property — possibly with type hint
                let _type_hint = self.try_parse_type_hint()?;
                let prop_name = match self.advance() {
                    Token::Variable(n) => n,
                    other => return Err(format!("Expected property variable, got {:?}", other)),
                };
                let default = if self.peek() == Token::Assign {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(&Token::Semicolon)?;
                properties.push(ClassProperty {
                    visibility: vis,
                    name: prop_name,
                    default,
                    is_static,
                    is_readonly,
                });
            } else {
                return Err(format!("Unexpected token in trait body: {:?}", self.peek()));
            }
        }
        self.expect(&Token::RBrace)?;

        Ok(Stmt::Trait {
            name,
            properties,
            methods,
        })
    }

    /// Parse interface declaration
    fn parse_interface(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'interface'
        let name = match self.advance() {
            Token::Identifier(n) => n,
            other => return Err(format!("Expected interface name, got {:?}", other)),
        };
        // interface Foo extends Bar, Baz { ... }
        let extends = if self.peek() == Token::Extends {
            self.advance();
            let mut parents = Vec::new();
            loop {
                parents.push(self.parse_qualified_name()?);
                if self.peek() == Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            parents
        } else {
            Vec::new()
        };
        self.expect(&Token::LBrace)?;

        let mut methods = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            let (vis, is_static, _is_final, _is_readonly) = self.parse_visibility_and_static();
            if self.peek() == Token::Function {
                self.advance(); // consume 'function'
                let method_name = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected method name, got {:?}", other)),
                };
                // Interface methods must be public (PHP rule)
                if vis != Visibility::Public {
                    let vis_str = match vis {
                        Visibility::Protected => "protected",
                        Visibility::Private => "private",
                        _ => "public",
                    };
                    return Err(format!(
                        "Access type for interface method {}::{}() must be public (got {})",
                        name, method_name, vis_str
                    ));
                }
                self.expect(&Token::LParen)?;
                let params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                let return_type = self.parse_return_type()?;
                self.expect(&Token::Semicolon)?; // interface methods end with ;
                methods.push(ClassMethod {
                    visibility: vis,
                    name: method_name,
                    params,
                    body: vec![],
                    is_static,
                    is_final: false,
                    return_type,
                });
            } else {
                return Err(format!(
                    "Unexpected token in interface body: {:?}",
                    self.peek()
                ));
            }
        }
        self.expect(&Token::RBrace)?;

        Ok(Stmt::Interface {
            name,
            extends,
            methods,
        })
    }

    /// Parse enum declaration
    fn parse_enum(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'enum'
        let name = match self.advance() {
            Token::Identifier(n) => n,
            other => return Err(format!("Expected enum name, got {:?}", other)),
        };
        // Optional backing type: enum Foo: string { ... }
        let backing_type = if self.peek() == Token::Colon {
            self.advance(); // consume ':'
            Some(self.parse_base_type_hint()?)
        } else {
            None
        };
        self.expect(&Token::LBrace)?;

        let mut cases = Vec::new();
        let mut methods = Vec::new();

        let prev_in_class = self.in_class_body;
        self.in_class_body = true;

        while self.peek() != Token::RBrace && !self.at_eof() {
            if self.peek() == Token::Case {
                self.advance(); // consume 'case'
                let case_name = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected enum case name, got {:?}", other)),
                };
                let value = if self.peek() == Token::Assign {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(&Token::Semicolon)?;
                cases.push((case_name, value));
            } else {
                // Method in enum
                let (vis, is_static, is_final, _is_readonly) = self.parse_visibility_and_static();
                if self.peek() == Token::Function {
                    self.advance();
                    let method_name = match self.advance() {
                        Token::Identifier(n) => n,
                        other => return Err(format!("Expected method name, got {:?}", other)),
                    };
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
                    methods.push(ClassMethod {
                        visibility: vis,
                        name: method_name,
                        params,
                        body,
                        is_static,
                        is_final,
                        return_type,
                    });
                } else {
                    return Err(format!("Unexpected token in enum body: {:?}", self.peek()));
                }
            }
        }
        self.in_class_body = prev_in_class;
        self.expect(&Token::RBrace)?;

        Ok(Stmt::Enum {
            name,
            backing_type,
            cases,
            methods,
        })
    }

    fn parse_visibility_and_static(&mut self) -> (Visibility, bool, bool, bool) {
        let mut vis = Visibility::Public;
        let mut is_static = false;
        let mut is_final = false;
        let mut is_readonly = false;

        loop {
            match self.peek() {
                Token::Public => {
                    self.advance();
                    vis = Visibility::Public;
                }
                Token::Protected => {
                    self.advance();
                    vis = Visibility::Protected;
                }
                Token::Private => {
                    self.advance();
                    vis = Visibility::Private;
                }
                Token::Static => {
                    self.advance();
                    is_static = true;
                }
                Token::Final => {
                    self.advance();
                    is_final = true;
                }
                Token::Abstract => {
                    self.advance(); /* absorbed for abstract methods */
                }
                Token::Identifier(ref s) if s == "readonly" => {
                    self.advance();
                    is_readonly = true;
                }
                _ => break,
            }
        }
        (vis, is_static, is_final, is_readonly)
    }

    /// Parse match expression
    fn parse_match_expr(&mut self) -> Result<Expr, String> {
        self.advance(); // consume 'match'
        self.expect(&Token::LParen)?;
        let expr = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::LBrace)?;

        let mut arms = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            if self.peek() == Token::Default {
                self.advance();
                self.expect(&Token::DoubleArrow)?;
                let body = self.parse_expr()?;
                arms.push(MatchArm {
                    conditions: None,
                    body,
                });
            } else {
                // One or more comma-separated conditions
                let mut conditions = Vec::new();
                conditions.push(self.parse_expr()?);
                while self.peek() == Token::Comma {
                    // Peek ahead: if next is => or }, this comma terminates the arm
                    let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
                    if next == Token::DoubleArrow || next == Token::RBrace {
                        break;
                    }
                    self.advance(); // consume comma
                    conditions.push(self.parse_expr()?);
                }
                self.expect(&Token::DoubleArrow)?;
                let body = self.parse_expr()?;
                arms.push(MatchArm {
                    conditions: Some(conditions),
                    body,
                });
            }
            // Optional trailing comma between arms
            if self.peek() == Token::Comma {
                self.advance();
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::Match {
            expr: Box::new(expr),
            arms,
        })
    }

    /// Parse arrow function: fn($x, $y) => expr
    /// Desugars to Closure with auto-captured use vars and body = [Return(expr)]
    fn parse_arrow_function(&mut self) -> Result<Expr, String> {
        self.advance(); // consume 'fn'
        self.expect(&Token::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(&Token::RParen)?;
        let return_type = self.parse_return_type()?;
        self.expect(&Token::DoubleArrow)?;
        let expr = self.parse_expr()?;

        // Auto-capture: collect free variables from expr that aren't params
        let param_names: std::collections::HashSet<&str> =
            params.iter().map(|p| p.name.as_str()).collect();
        let mut free_vars = Vec::new();
        Self::collect_free_vars(&expr, &param_names, &mut free_vars);

        let body = vec![Stmt::Return(Some(expr))];
        Ok(Expr::Closure {
            params,
            use_vars: free_vars,
            body,
            return_type,
        })
    }

    /// Collect variable names referenced in an expression that are not in `bound`.
    fn collect_free_vars(
        expr: &Expr,
        bound: &std::collections::HashSet<&str>,
        out: &mut Vec<String>,
    ) {
        match expr {
            Expr::Variable(name) => {
                if !bound.contains(name.as_str()) && !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Expr::BinaryOp { left, right, .. } => {
                Self::collect_free_vars(left, bound, out);
                Self::collect_free_vars(right, bound, out);
            }
            Expr::UnaryMinus(inner)
            | Expr::Not(inner)
            | Expr::Throw(inner)
            | Expr::Empty(inner)
            | Expr::Print(inner)
            | Expr::BitwiseNot(inner) => {
                Self::collect_free_vars(inner, bound, out);
            }
            Expr::Assign { var, expr: inner } => {
                if !bound.contains(var.as_str()) && !out.contains(var) {
                    out.push(var.clone());
                }
                Self::collect_free_vars(inner, bound, out);
            }
            Expr::FunctionCall { args, .. } | Expr::StaticCall { args, .. } => {
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::DynamicCall { callable, args } => {
                Self::collect_free_vars(callable, bound, out);
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::Isset(exprs) => {
                for e in exprs {
                    Self::collect_free_vars(e, bound, out);
                }
            }
            Expr::PostInc(name) | Expr::PostDec(name) | Expr::PreInc(name) | Expr::PreDec(name) => {
                if !bound.contains(name.as_str()) && !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                Self::collect_free_vars(condition, bound, out);
                Self::collect_free_vars(then_expr, bound, out);
                Self::collect_free_vars(else_expr, bound, out);
            }
            Expr::NullCoalesce { left, right } | Expr::Elvis { left, right } => {
                Self::collect_free_vars(left, bound, out);
                Self::collect_free_vars(right, bound, out);
            }
            Expr::ArrayLiteral(elements) => {
                for elem in elements {
                    if let Some(k) = &elem.key {
                        Self::collect_free_vars(k, bound, out);
                    }
                    Self::collect_free_vars(&elem.value, bound, out);
                }
            }
            Expr::ArrayAccess { array, index } => {
                Self::collect_free_vars(array, bound, out);
                Self::collect_free_vars(index, bound, out);
            }
            Expr::PropertyAccess { object, .. } => {
                Self::collect_free_vars(object, bound, out);
            }
            Expr::MethodCall { object, args, .. } => {
                Self::collect_free_vars(object, bound, out);
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::Closure { use_vars, .. } => {
                // Nested closure — only capture its explicit use vars
                for v in use_vars {
                    if !bound.contains(v.as_str()) && !out.contains(v) {
                        out.push(v.clone());
                    }
                }
            }
            Expr::Cast { expr: inner, .. } => {
                Self::collect_free_vars(inner, bound, out);
            }
            Expr::Instanceof { expr: inner, .. } => {
                Self::collect_free_vars(inner, bound, out);
            }
            Expr::New { args, .. } => {
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::Match { expr: inner, arms } => {
                Self::collect_free_vars(inner, bound, out);
                for arm in arms {
                    if let Some(conds) = &arm.conditions {
                        for cond in conds {
                            Self::collect_free_vars(cond, bound, out);
                        }
                    }
                    Self::collect_free_vars(&arm.body, bound, out);
                }
            }
            Expr::StaticProperty { .. } => {}
            // Literals and constants — no variables
            Expr::Integer(_)
            | Expr::Float(_)
            | Expr::StringLiteral(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Constant(_) => {}
            // Yield — collect vars from value/key expressions
            Expr::Yield { value, key } => {
                if let Some(v) = value {
                    Self::collect_free_vars(v, bound, out);
                }
                if let Some(k) = key {
                    Self::collect_free_vars(k, bound, out);
                }
            }
            Expr::YieldFrom(sub) => {
                Self::collect_free_vars(sub, bound, out);
            }
            Expr::Clone(inner) => {
                Self::collect_free_vars(inner, bound, out);
            }
        }
    }

    /// Parse closure: function($a, $b) use($c) { ... }
    fn parse_closure(&mut self) -> Result<Expr, String> {
        self.advance(); // consume 'function'
        self.expect(&Token::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(&Token::RParen)?;

        let mut use_vars = Vec::new();
        if self.peek() == Token::Use {
            self.advance();
            self.expect(&Token::LParen)?;
            let v = match self.advance() {
                Token::Variable(n) => n,
                other => return Err(format!("Expected variable in use, got {:?}", other)),
            };
            use_vars.push(v);
            while self.peek() == Token::Comma {
                self.advance();
                let v = match self.advance() {
                    Token::Variable(n) => n,
                    other => return Err(format!("Expected variable in use, got {:?}", other)),
                };
                use_vars.push(v);
            }
            self.expect(&Token::RParen)?;
        }

        let return_type = self.parse_return_type()?;

        self.expect(&Token::LBrace)?;
        let mut body = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            body.push(self.parse_stmt()?);
        }
        self.expect(&Token::RBrace)?;

        Ok(Expr::Closure {
            params,
            use_vars,
            body,
            return_type,
        })
    }
}
