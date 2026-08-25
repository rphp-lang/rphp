impl Parser {
    /// Keep recursively nested grammar below the native Rust stack limit.
    /// PHP reports an ordinary parser memory-exhaustion diagnostic when its
    /// generated parser cannot grow its stack; RPHP must likewise reject a
    /// hostile source unit instead of aborting the process.
    const MAX_SYNTAX_NESTING: usize = 256;
    const DEDICATED_STACK_NESTING: usize = 16;
    const DEDICATED_STACK_SIZE: usize = 64 * 1024 * 1024;

    fn parse_foreach_destructure(&mut self) -> Result<Option<Vec<ListTarget>>, String> {
        let end = if matches!(self.peek(), Token::LBracket(_)) {
            self.advance();
            Token::RBracket
        } else if matches!(self.peek(), Token::Identifier(ref name, _) if name.eq_ignore_ascii_case("list"))
            && matches!(self.peek_at(1), Token::LParen(_))
        {
            self.advance();
            self.expect_lparen()?;
            Token::RParen
        } else {
            return Ok(None);
        };
        let targets = self.parse_list_targets(&end)?;
        self.expect(&end)?;
        Ok(Some(targets))
    }

    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            source_name: None,
            in_class_body: false,
            class_scope_active: false,
            generic_scopes: Vec::new(),
            deferred_compile_error: None,
            deferred_compile_deprecations: Vec::new(),
            strict_types_allowed: true,
            empty_dimension_unset_context: false,
            preserve_empty_dimension_suffix: false,
            new_postfix_error_suffix: None,
            last_primary_line: None,
            outermost_scope: true,
            halted: false,
        }
    }

    pub fn with_source_name(mut self, source_name: impl Into<String>) -> Self {
        self.source_name = Some(source_name.into());
        self
    }

    pub(crate) fn with_class_scope_active(mut self, active: bool) -> Self {
        self.class_scope_active = active;
        self
    }

    pub fn parse(&mut self) -> Result<Vec<Stmt>, String> {
        let (max_depth, deepest_line) = self.check_syntax_nesting()?;
        if max_depth > Self::DEDICATED_STACK_NESTING {
            let spawn_error = self.memory_exhausted(deepest_line);
            return std::thread::scope(|scope| {
                let parser = std::thread::Builder::new()
                    .name("rphp-parser".to_string())
                    .stack_size(Self::DEDICATED_STACK_SIZE)
                    .spawn_scoped(scope, || self.parse_inner())
                    .map_err(|_| spawn_error)?;
                match parser.join() {
                    Ok(result) => result,
                    Err(panic) => std::panic::resume_unwind(panic),
                }
            });
        }

        self.parse_inner()
    }

    fn parse_inner(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(&Token::OpenTag)?;
        let mut stmts = Vec::new();

        while !self.at_eof() {
            stmts.push(self.parse_stmt_in_scope(true)?);
        }

        stmts.extend(self.deferred_compile_deprecations.drain(..).map(
            |(message, line)| Stmt::ExprStmt(Expr::CompileDeprecation { message, line }),
        ));

        if let Some((message, line)) = self.deferred_compile_error.take() {
            stmts.push(Stmt::ExprStmt(Expr::CompileError { message, line }));
        }

        Ok(stmts)
    }

    fn check_syntax_nesting(&self) -> Result<(usize, usize), String> {
        let mut depth = 0usize;
        let mut max_depth = 0usize;
        let mut line = 1usize;
        let mut deepest_line = line;

        for token in &self.tokens {
            line = match token {
                Token::This(token_line)
                | Token::Variable(_, token_line)
                | Token::LBracket(token_line)
                | Token::ParseError(_, token_line)
                | Token::MagicConstant {
                    line: token_line, ..
                }
                | Token::HaltCompiler {
                    line: token_line, ..
                }
                | Token::Exit {
                    line: token_line, ..
                }
                | Token::Goto {
                    line: token_line, ..
                }
                | Token::Case(token_line)
                | Token::Default(token_line)
                | Token::Echo { line: token_line } => *token_line,
                _ => line,
            };

            match token {
                Token::LParen(_) | Token::LBrace | Token::LBracket(_) => {
                    depth += 1;
                    if depth > max_depth {
                        max_depth = depth;
                        deepest_line = line;
                    }
                    if depth > Self::MAX_SYNTAX_NESTING {
                        return Err(self.memory_exhausted(line));
                    }
                }
                Token::RParen | Token::RBrace | Token::RBracket => {
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }

        Ok((max_depth, deepest_line))
    }

    fn memory_exhausted(&self, line: usize) -> String {
        self.source_error("memory exhausted", line)
    }

    fn source_error(&self, message: &str, line: usize) -> String {
        let location = self
            .source_name
            .as_deref()
            .filter(|source_name| !source_name.is_empty())
            .map(|source_name| format!(" in {source_name}"))
            .unwrap_or_default();
        format!("{message}{location} on line {line}")
    }

    fn group_use_missing_item_error(
        &self,
        kind: UseKind,
        token: &str,
        line: usize,
    ) -> String {
        let expectation = if kind == UseKind::Class {
            "identifier or namespaced name or \"function\" or \"const\""
        } else {
            "identifier or namespaced name"
        };
        self.source_error(
            &format!("syntax error, unexpected token \"{token}\", expecting {expectation}"),
            line,
        )
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        let strict_types_allowed = self.strict_types_allowed;
        if !matches!(self.peek(), Token::Declare | Token::Semicolon(_)) {
            self.strict_types_allowed = false;
        }
        if let Token::Identifier(name, _) = self.peek() {
            if self.peek_at(1) == Token::Colon {
                self.advance();
                self.advance();
                return Ok(Stmt::Label(name));
            }
        }
        if let Token::Exit { line, .. } = self.peek()
            && self.peek_at(1) == Token::Colon
        {
            return Err(self.source_error("syntax error, unexpected token \":\"", line));
        }
        if let Token::Goto { line, .. } = self.peek() {
            self.advance();
            let name = match self.advance() {
                Token::Identifier(label, _) => label,
                Token::Exit { .. } => {
                    return Err(self.source_error(
                        "syntax error, unexpected token \"exit\", expecting identifier",
                        line,
                    ));
                }
                token => return Err(format!("Expected label after goto, got {token:?}")),
            };
            self.expect(&Token::Semicolon(0))?;
            return Ok(Stmt::Goto { name, line });
        }
        match self.peek() {
            Token::HaltCompiler { offset, line } => {
                self.advance();
                self.halted = true;
                if !self.outermost_scope {
                    let _ = self.compile_error(
                        "__HALT_COMPILER() can only be used from the outermost scope",
                        line,
                    );
                }
                Ok(Stmt::HaltCompiler { offset, line })
            }
            Token::LBrace => {
                self.advance();
                let mut body = Vec::new();
                while self.peek() != Token::RBrace && !self.at_eof() {
                    body.push(self.parse_stmt_in_scope(false)?);
                }
                self.expect(&Token::RBrace)?;
                Ok(Stmt::Block(body))
            }
            Token::ParseError(message, line) => {
                self.advance();
                Err(self.source_error(&message, line))
            }
            Token::CompileError(message, line) => {
                self.advance();
                self.compile_error(message, line);
                Ok(Stmt::Noop)
            }
            Token::CompileWarning(message, line) => {
                self.advance();
                Ok(Stmt::ExprStmt(Expr::CompileWarning { message, line }))
            }
            Token::CompileDeprecation(message, line) => {
                self.advance();
                Ok(Stmt::ExprStmt(Expr::CompileDeprecation { message, line }))
            }
            Token::Semicolon(_) => {
                self.advance();
                Ok(Stmt::Noop)
            }
            Token::Declare => {
                self.advance(); // consume 'declare'
                self.expect_lparen()?;
                let (directive, directive_line) = match self.advance() {
                    Token::Identifier(n, line) => (n, line),
                    other => {
                        return Err(format!(
                            "Expected directive name in declare(), got {:?}",
                            other
                        ));
                    }
                };
                let invalid_strict_placement = directive.eq_ignore_ascii_case("strict_types")
                    && !strict_types_allowed;
                if invalid_strict_placement {
                    let _ = self.compile_error(
                        "strict_types declaration must be the very first statement in the script",
                        directive_line,
                    );
                }
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
                let invalid_strict_block = directive.eq_ignore_ascii_case("strict_types")
                    && self.peek() == Token::LBrace;
                if invalid_strict_block {
                    let _ = self.compile_error(
                        "strict_types declaration must not use block mode",
                        directive_line,
                    );
                }
                if (invalid_strict_placement || invalid_strict_block)
                    && self.peek() == Token::LBrace
                {
                    self.advance();
                    while self.peek() != Token::RBrace && !self.at_eof() {
                        let _ = self.parse_stmt_in_scope(false)?;
                    }
                    self.expect(&Token::RBrace)?;
                    return Ok(Stmt::Noop);
                }
                self.expect(&Token::Semicolon(0))?;
                if invalid_strict_placement || invalid_strict_block {
                    Ok(Stmt::Noop)
                } else {
                    Ok(Stmt::Declare { directive, value })
                }
            }
            Token::Namespace => {
                self.advance(); // consume 'namespace'
                // The bracketed global namespace has no name: `namespace { ... }`.
                // Keep the empty spelling in the AST so compilation can restore
                // global resolution while retaining the namespace block boundary.
                let name = if self.peek() == Token::LBrace {
                    String::new()
                } else {
                    self.parse_qualified_name()?
                };
                if self.peek() == Token::LBrace {
                    // Braced namespace: namespace App\Models { ... }
                    self.advance(); // consume '{'
                    let mut body = Vec::new();
                    while self.peek() != Token::RBrace && self.peek() != Token::Eof {
                        body.push(self.parse_stmt_in_scope(false)?);
                    }
                    if self.halted && self.at_eof() {
                        return Err(self.source_error("Unclosed '{'", 1));
                    }
                    self.expect(&Token::RBrace)?;
                    Ok(Stmt::Namespace { name, body })
                } else {
                    // Unbraced namespace: namespace App\Models; (rest of file belongs to this namespace)
                    self.expect(&Token::Semicolon(0))?;
                    let mut body = Vec::new();
                    while self.peek() != Token::Eof && self.peek() != Token::Namespace {
                        body.push(self.parse_stmt_in_scope(true)?);
                    }
                    Ok(Stmt::Namespace { name, body })
                }
            }
            Token::Use(use_line) if !self.in_class_body => {
                // Top-level class/function import. Their alias tables are
                // separate in PHP even when the source alias is identical.
                self.advance(); // consume 'use'
                let kind = if matches!(self.peek(), Token::Function(_)) {
                    self.advance();
                    UseKind::Function
                } else if self.peek() == Token::Const {
                    self.advance();
                    UseKind::Const
                } else {
                    UseKind::Class
                };
                let (first_name, grouped) = self.parse_use_name()?;
                let mut imports = Vec::new();
                if grouped {
                    if self.peek() == Token::RBrace {
                        return Err(self.group_use_missing_item_error(kind, "}", use_line));
                    }
                    if matches!(self.peek(), Token::Comma(_)) {
                        return Err(self.group_use_missing_item_error(kind, ",", use_line));
                    }
                    loop {
                        let item_kind = if matches!(self.peek(), Token::Function(_)) {
                            if kind != UseKind::Class {
                                return Err(
                                    "Typed group use declaration cannot override its kind"
                                        .to_string(),
                                );
                            }
                            self.advance();
                            UseKind::Function
                        } else if self.peek() == Token::Const {
                            if kind != UseKind::Class {
                                return Err(
                                    "Typed group use declaration cannot override its kind"
                                        .to_string(),
                                );
                            }
                            self.advance();
                            UseKind::Const
                        } else {
                            kind
                        };
                        if self.peek() == Token::RBrace {
                            return Err(self.group_use_missing_item_error(
                                item_kind, "}", use_line,
                            ));
                        }
                        if matches!(self.peek(), Token::Comma(_)) {
                            return Err(self.group_use_missing_item_error(
                                item_kind, ",", use_line,
                            ));
                        }
                        if self.peek() == Token::Backslash {
                            return Err(
                                "Group use item cannot start with a namespace separator"
                                    .to_string(),
                            );
                        }
                        let (relative_name, nested_group) = self.parse_use_name()?;
                        if nested_group {
                            return Err("Nested group use declaration is not allowed".to_string());
                        }
                        let alias = if self.consume_use_alias_keyword() {
                            match self.advance() {
                                Token::Identifier(name, _) => name,
                                other => {
                                    return Err(format!(
                                        "Expected alias name after 'as', got {:?}",
                                        other
                                    ));
                                }
                            }
                        } else {
                            relative_name
                                .rsplit('\\')
                                .next()
                                .unwrap_or(&relative_name)
                                .to_string()
                        };
                        imports.push((item_kind, format!("{first_name}\\{relative_name}"), alias));
                        if !matches!(self.peek(), Token::Comma(_)) {
                            break;
                        }
                        self.advance();
                        if self.peek() == Token::RBrace {
                            break;
                        }
                        if matches!(self.peek(), Token::Comma(_)) {
                            return Err(self.source_error(
                                "syntax error, unexpected token \",\", expecting \"}\"",
                                use_line,
                            ));
                        }
                    }
                    self.expect(&Token::RBrace)?;
                } else {
                    let mut fqn = first_name;
                    loop {
                        let alias = if self.consume_use_alias_keyword() {
                            match self.advance() {
                                Token::Identifier(name, _) => name,
                                other => {
                                    return Err(format!(
                                        "Expected alias name after 'as', got {:?}",
                                        other
                                    ));
                                }
                            }
                        } else {
                            fqn.rsplit('\\').next().unwrap_or(&fqn).to_string()
                        };
                        imports.push((kind, fqn, alias));
                        if !matches!(self.peek(), Token::Comma(_)) {
                            break;
                        }
                        self.advance();
                        let (next_name, nested_group) = self.parse_use_name()?;
                        if nested_group {
                            return Err("Group use prefix must start a use declaration".to_string());
                        }
                        fqn = next_name;
                    }
                }
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::UseDecl {
                    line: use_line,
                    imports,
                })
            }
            Token::Const => {
                self.advance(); // consume 'const'
                let mut declarations = Vec::new();
                let mut const_line = 0;
                loop {
                    let (name, line) = match self.advance() {
                        Token::Identifier(name, line)
                        | Token::MagicConstant { name, line }
                        | Token::Goto { name, line } => (name, line),
                        Token::Exit { line, .. } => {
                            return Err(self.source_error(
                                "syntax error, unexpected token \"exit\", expecting identifier",
                                line,
                            ));
                        }
                        other => {
                            return Err(format!(
                                "Expected constant name after 'const', got {:?}",
                                other
                            ));
                        }
                    };
                    if const_line == 0 {
                        const_line = line;
                    }
                    self.expect(&Token::Assign)?;
                    declarations.push((name, self.parse_expr()?));
                    if !matches!(self.peek(), Token::Comma(_)) {
                        break;
                    }
                    self.advance();
                }
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Const {
                    line: const_line,
                    attributes: Vec::new(),
                    declarations,
                })
            }
            Token::Echo { line } => {
                self.advance();
                let mut expressions = vec![self.with_new_postfix_error_suffix(
                    Some(", expecting \",\" or \";\""),
                    |parser| parser.parse_expr(),
                )?];
                while matches!(self.peek(), Token::Comma(_)) {
                    self.advance();
                    expressions.push(self.with_new_postfix_error_suffix(
                        Some(", expecting \",\" or \";\""),
                        |parser| parser.parse_expr(),
                    )?);
                }
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Echo { expressions, line })
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
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Include {
                    path,
                    is_require,
                    is_once,
                })
            }
            Token::Variable(_, _) => {
                // Peek ahead to determine statement type
                let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
                if matches!(next, Token::LBracket(_)) {
                    // Could be $a[] = ..., $a[idx] = ..., or expression
                    // Check for $a[] = (array push)
                    // Reference appends need the general expression AST so
                    // the compiler can preserve the source reference cell.
                    let is_push = self.tokens.get(self.pos + 2) == Some(&Token::RBracket)
                        && self.tokens.get(self.pos + 3) == Some(&Token::Assign)
                        && self.tokens.get(self.pos + 4) != Some(&Token::Ampersand);
                    if is_push {
                        let (var_name, line) = match self.advance() {
                            Token::Variable(name, line) => (name, line),
                            _ => unreachable!(),
                        };
                        self.advance(); // consume '['
                        self.advance(); // consume ']'
                        self.advance(); // consume '='
                        let expr = self.parse_expr()?;
                        self.expect(&Token::Semicolon(0))?;
                        if var_name == "GLOBALS" {
                            return Ok(Stmt::ExprStmt(
                                self.compile_error("Cannot append to $GLOBALS", line),
                            ));
                        }
                        return Ok(Stmt::ArrayPush {
                            var: var_name,
                            expr,
                            line,
                        });
                    }
                    // Parse a complete $a[idx]...[idx] write. Keeping every
                    // dimension in one AST node lets the compiler evaluate
                    // keys once and rebuild COW parents from the leaf.
                    if self.is_array_assign() {
                        let (var_name, line) = match self.advance() {
                            Token::Variable(name, line) => (name, line),
                            _ => unreachable!(),
                        };
                        let mut indices = Vec::new();
                        while matches!(self.peek(), Token::LBracket(_)) {
                            self.expect_lbracket()?;
                            indices.push(self.parse_expr()?);
                            self.expect(&Token::RBracket)?;
                        }
                        self.expect(&Token::Assign)?;
                        let expr = self.parse_expr()?;
                        self.expect(&Token::Semicolon(0))?;
                        return Ok(if indices.len() == 1 {
                            Stmt::ArrayAssign {
                                var: var_name,
                                index: indices.pop().unwrap(),
                                expr,
                                line,
                            }
                        } else {
                            Stmt::NestedArrayAssign {
                                root: Self::variable_expression(var_name, line),
                                indices,
                                expr,
                                line,
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
                    let (var_name, line) = match self.advance() {
                        Token::Variable(name, line) => (name, line),
                        _ => unreachable!(),
                    };
                    self.expect(&Token::Assign)?;
                    if self.peek() == Token::Ampersand {
                        self.advance();
                        let target = self.parse_empty_dimension_target_prefix()?;
                        if !self.is_empty_array_dimension_suffix() {
                            self.expect(&Token::Semicolon(0))?;
                            if var_name == "GLOBALS" {
                                return Ok(Stmt::ExprStmt(self.globals_modification_error(line)));
                            }
                            if let Expr::Globals { line } = &target {
                                return Ok(Stmt::ExprStmt(
                                    self.compile_error(
                                        "Cannot acquire reference to $GLOBALS",
                                        *line,
                                    ),
                                ));
                            }
                            if let Some(line) = Self::nullsafe_chain_line(&target) {
                                return Ok(Stmt::ExprStmt(self.nullsafe_reference_error(line)));
                            }
                            if matches!(&target, Expr::ArrayAccess { .. })
                                && let Some((message, line)) =
                                    self.array_write_root_error(&target)
                            {
                                return Ok(Stmt::ExprStmt(self.compile_error(message, line)));
                            }
                            return Ok(Stmt::ExprStmt(Expr::AssignReference {
                                var: var_name,
                                target: Box::new(target),
                            }));
                        }
                        if !matches!(
                            &target,
                            Expr::Variable { .. }
                                | Expr::ArrayAccess { .. }
                                | Expr::PropertyAccess {
                                    nullsafe: false,
                                    ..
                                }
                                | Expr::StaticProperty { .. }
                                | Expr::DynamicNamedStaticProperty { .. }
                                | Expr::DynamicStaticProperty { .. }
                        ) {
                            return Err("Invalid array reference target".into());
                        }
                        self.expect_lbracket()?;
                        self.expect(&Token::RBracket)?;
                        self.expect(&Token::Semicolon(0))?;
                        return Ok(Stmt::BindArrayAppendReference {
                            var: var_name,
                            target,
                        });
                    }
                    let expr = self.parse_assignment_or_yield()?;
                    if let Expr::Cast {
                        cast_type: CastType::Void,
                        line: void_line,
                        ..
                    } = &expr
                    {
                        return Err(self.source_error(
                            "syntax error, unexpected token \"(void)\"",
                            *void_line,
                        ));
                    }
                    let assignment = if var_name == "GLOBALS" {
                        self.globals_modification_error(line)
                    } else {
                        Expr::Assign {
                            var: var_name.clone(),
                            expr: Box::new(expr),
                        }
                    };
                    let expression = self.finish_keyword_logical_tail(assignment)?;
                    // PHP treats the end of the source unit like a closing PHP
                    // tag, so the final statement may omit its semicolon.
                    if !self.at_eof() {
                        self.expect(&Token::Semicolon(0))?;
                    }
                    match expression {
                        Expr::Assign { var, expr } => Ok(Stmt::Assign { var, expr: *expr }),
                        expression => Ok(Stmt::ExprStmt(expression)),
                    }
                } else if let Some(bin_op) = Self::compound_assign_op(&next) {
                    let (var_name, line) = match self.advance() {
                        Token::Variable(name, line) => (name, line),
                        _ => unreachable!(),
                    };
                    self.advance(); // consume the compound operator
                    let rhs = self.parse_expr()?;
                    self.expect(&Token::Semicolon(0))?;
                    if var_name == "GLOBALS" {
                        return Ok(Stmt::ExprStmt(self.globals_modification_error(line)));
                    }
                    Ok(Stmt::CompoundAssign {
                        target: Expr::Variable {
                            name: var_name,
                            line,
                        },
                        op: bin_op,
                        expr: rhs,
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
                                object,
                                property,
                                line,
                                ..
                            } = expr
                            {
                                self.advance(); // consume '='
                                let rhs = self.parse_expr()?;
                                self.expect(&Token::Semicolon(0))?;
                                return Ok(Stmt::AssignProp {
                                    object: *object,
                                    property,
                                    expr: rhs,
                                    line,
                                });
                            }
                        } else if matches!(expr, Expr::ArrayAccess { .. }) {
                            let line = match &expr {
                                Expr::ArrayAccess { line, .. } => *line,
                                _ => unreachable!(),
                            };
                            let (root, mut indices) = Self::split_array_access(expr);
                            self.advance(); // consume '='
                            let rhs = self.parse_expr()?;
                            self.expect(&Token::Semicolon(0))?;
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
                                    line,
                                });
                            }
                            if matches!(
                                root,
                                Expr::Variable { .. }
                                    | Expr::PropertyAccess { .. }
                                    | Expr::StaticProperty { .. }
                                    | Expr::DynamicNamedStaticProperty { .. }
                                    | Expr::DynamicStaticProperty { .. }
                            ) {
                                return Ok(Stmt::NestedArrayAssign {
                                    root,
                                    indices,
                                    expr: rhs,
                                    line,
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
                self.expect_lparen()?;
                let condition = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                let body = self.parse_control_body(Token::EndWhile)?;
                Ok(Stmt::While { condition, body })
            }
            Token::Do => {
                self.advance(); // consume 'do'
                let body = self.parse_block_or_stmt()?;
                if self.halted {
                    return Ok(Stmt::DoWhile {
                        condition: Expr::Bool(false),
                        body,
                    });
                }
                self.expect(&Token::While)?;
                self.expect_lparen()?;
                let condition = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::DoWhile { condition, body })
            }
            Token::Break { line } => {
                self.advance();
                let level = self.parse_break_continue_level("break", line)?;
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Break { level, line })
            }
            Token::Continue { line } => {
                self.advance();
                let level = self.parse_break_continue_level("continue", line)?;
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Continue { level, line })
            }
            Token::Switch => {
                self.advance(); // consume 'switch'
                self.expect_lparen()?;
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                let alternative = match self.peek() {
                    Token::LBrace => {
                        self.advance();
                        false
                    }
                    Token::Colon => {
                        self.advance();
                        true
                    }
                    token => return Err(format!("Expected switch body, got {token:?}")),
                };
                let mut cases = Vec::new();
                let mut has_default = false;
                while !matches!(self.peek(), Token::RBrace | Token::EndSwitch) && !self.at_eof() {
                    match self.peek() {
                        Token::Case(_) => {
                            self.advance();
                            let value = self.parse_expr()?;
                            self.consume_switch_label_separator()?;
                            let mut body = Vec::new();
                            while !matches!(
                                self.peek(),
                                Token::Case(_)
                                    | Token::Default(_)
                                    | Token::RBrace
                                    | Token::EndSwitch
                            ) && !self.at_eof()
                            {
                                body.push(self.parse_stmt_in_scope(false)?);
                            }
                            cases.push(SwitchCase {
                                value: Some(value),
                                body,
                            });
                        }
                        Token::Default(_) => {
                            if has_default {
                                return Err(
                                    "Switch statements may only contain one default clause".into(),
                                );
                            }
                            has_default = true;
                            self.advance();
                            self.consume_switch_label_separator()?;
                            let mut body = Vec::new();
                            while !matches!(
                                self.peek(),
                                Token::Case(_)
                                    | Token::Default(_)
                                    | Token::RBrace
                                    | Token::EndSwitch
                            ) && !self.at_eof()
                            {
                                body.push(self.parse_stmt_in_scope(false)?);
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
                if alternative {
                    self.expect(&Token::EndSwitch)?;
                    self.expect(&Token::Semicolon(0))?;
                } else {
                    self.expect(&Token::RBrace)?;
                }
                Ok(Stmt::Switch { expr, cases })
            }
            Token::For => {
                self.advance(); // consume 'for'
                self.expect_lparen()?;

                // Init: optional assignment or expression before first ;
                let mut init = Vec::new();
                while !matches!(self.peek(), Token::Semicolon(_)) {
                    init.push(self.parse_for_init()?);
                    if !matches!(self.peek(), Token::Comma(_)) {
                        break;
                    }
                    self.advance();
                }
                self.expect(&Token::Semicolon(0))?;

                // Every comma-separated condition is evaluated; the last one
                // determines whether the loop continues.
                let mut condition = Vec::new();
                while !matches!(self.peek(), Token::Semicolon(_)) {
                    condition.push(self.parse_expr()?);
                    if !matches!(self.peek(), Token::Comma(_)) {
                        break;
                    }
                    self.advance();
                }
                if let Some(Expr::Cast {
                    cast_type: CastType::Void,
                    line,
                    ..
                }) = condition.last()
                {
                    return Err(self.source_error(
                        "syntax error, unexpected token \";\", expecting \",\"",
                        *line,
                    ));
                }
                self.expect(&Token::Semicolon(0))?;

                let mut update = Vec::new();
                while self.peek() != Token::RParen {
                    update.push(self.parse_expr()?);
                    if !matches!(self.peek(), Token::Comma(_)) {
                        break;
                    }
                    self.advance();
                }
                self.expect(&Token::RParen)?;

                let body = self.parse_control_body(Token::EndFor)?;
                Ok(Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                })
            }
            Token::Foreach { line } => {
                self.advance(); // consume 'foreach'
                self.expect_lparen()?;
                let array = self.parse_expr()?;
                self.expect(&Token::As)?;
                // foreach ($arr as $key => $val), foreach ($arr as $val),
                // and the corresponding destructuring value forms.
                let first_by_ref = if self.peek() == Token::Ampersand {
                    self.advance();
                    true
                } else {
                    false
                };
                if let Some(targets) = self.parse_foreach_destructure()? {
                    if first_by_ref {
                        return Err("Foreach destructuring target cannot be a reference".into());
                    }
                    if self.peek() == Token::DoubleArrow {
                        return Err("Cannot use list as key element".to_string());
                    }
                    self.expect(&Token::RParen)?;
                    let body = self.parse_control_body(Token::EndForeach)?;
                   return Ok(Stmt::Foreach {
                       line,
                       array,
                       value: ForeachTarget::Destructure(targets),
                        key: None,
                       by_ref: false,
                       body,
                   });
               }
                let first_expr = self.parse_foreach_target_expression()?;
                let first = self.into_foreach_target(first_expr)?;
                let (key, value, by_ref) = if self.peek() == Token::DoubleArrow {
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
                    let value = if let Some(targets) = self.parse_foreach_destructure()? {
                        if by_ref {
                            return Err(
                                "Foreach destructuring target cannot be a reference".into()
                            );
                        }
                       ForeachTarget::Destructure(targets)
                   } else {
                        let value_expr = self.parse_foreach_target_expression()?;
                        self.into_foreach_target(value_expr)?
                   };
                    (Some(first), value, by_ref)
               } else {
                    (None, first, first_by_ref)
               };
                self.expect(&Token::RParen)?;
                let body = self.parse_control_body(Token::EndForeach)?;
                Ok(Stmt::Foreach {
                   line,
                   array,
                   value,
                    key,
                    by_ref,
                    body,
                })
            }
            Token::Function(line)
                if matches!(
                    self.peek_at(1),
                    Token::Identifier(_, _) | Token::From | Token::Exit { .. }
                )
                    || (self.peek_at(1) == Token::Ampersand
                        && matches!(
                            self.peek_at(2),
                            Token::Identifier(_, _) | Token::From | Token::Exit { .. }
                        )) =>
            {
                self.advance(); // consume 'function'
                // Accept the PHP reference-return declaration marker. Return
                // aliasing itself remains outside the current execution
                // contract, matching the closure parser's bounded handling.
                let returns_by_ref = self.peek() == Token::Ampersand;
                self.consume_reference_return_marker();
                let name = match self.advance() {
                    Token::Identifier(n, _) => n,
                    Token::From => "from".to_string(),
                    Token::Exit { .. } => {
                        return Err(self.source_error(
                            "syntax error, unexpected token \"exit\", expecting \"(\"",
                            line,
                        ));
                    }
                    other => return Err(format!("Expected function name, got {:?}", other)),
                };
                if name.eq_ignore_ascii_case("assert") {
                    self.compile_error(
                        "Defining a custom assert() function is not allowed, as the function has special semantics",
                        line,
                    );
                }
                // A named function never inherits the surrounding method's
                // class scope. Closures deliberately do.
                let previous_class_scope = self.class_scope_active;
                self.class_scope_active = false;
                let generic_params = self.parse_generic_parameters()?;
                self.push_generic_scope(&generic_params);
                self.expect_lparen()?;
                let params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                let return_type = self.parse_return_type(line, false)?;
                self.expect(&Token::LBrace)?;
                let mut body = Vec::new();
                while self.peek() != Token::RBrace && !self.at_eof() {
                    body.push(self.parse_stmt_in_scope(false)?);
                }
                self.expect(&Token::RBrace)?;
                self.pop_generic_scope();
                self.class_scope_active = previous_class_scope;
                Ok(Stmt::Function {
                    line,
                    attributes: Vec::new(),
                    name,
                    returns_by_ref,
                    params,
                    body,
                    return_type,
                    generic_params,
                })
            }
            Token::Return { line } => {
                self.advance(); // consume 'return'
                if matches!(self.peek(), Token::Semicolon(_)) {
                    self.advance();
                    Ok(Stmt::Return { expr: None, line })
                } else {
                    let expr = self.with_new_postfix_error_suffix(
                        Some(", expecting \";\""),
                        |parser| parser.parse_expr(),
                    )?;
                    self.expect(&Token::Semicolon(0))?;
                    Ok(Stmt::Return {
                        expr: Some(expr),
                        line,
                    })
                }
            }
            Token::Unset => {
                self.advance();
                let list_line = self.expect_lparen()?;
                if matches!(self.peek(), Token::Comma(_)) {
                    return Err(self.comma_list_error(list_line, false));
                }
                let mut targets = Vec::new();
                let expr = self.parse_unset_target()?;
                let expr = self.normalize_unset_target(expr)?;
                targets.push(expr);
                while self.comma_list_has_next(list_line)? {
                    let expr = self.parse_unset_target()?;
                    let expr = self.normalize_unset_target(expr)?;
                    targets.push(expr);
                }
                self.expect(&Token::RParen)?;
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Unset(targets))
            }
            Token::Try => self.parse_try_catch(),
            Token::Throw(line) => {
                let line = line as usize;
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Throw { expr, line })
            }
            Token::AttributeStart(_) => {
                let attributes = self.parse_attribute_groups()?;
                let declaration = self.parse_stmt()?;
                self.attach_attributes(declaration, attributes)
            }
            Token::Class | Token::Abstract | Token::Final => self.parse_class(),
            Token::Identifier(ref name, _) if name.eq_ignore_ascii_case("class") => {
                self.parse_class()
            }
            Token::Identifier(ref name, _)
                if name.eq_ignore_ascii_case("readonly")
                    && (matches!(
                        self.peek_at(1),
                        Token::Class | Token::Abstract | Token::Final
                    ) || matches!(
                        self.peek_at(1),
                        Token::Identifier(ref keyword, _)
                            if keyword.eq_ignore_ascii_case("class")
                    )) =>
            {
                self.parse_class()
            }
            Token::Enum => self.parse_enum(),
            Token::Interface => self.parse_interface(),
            Token::Trait => self.parse_trait(),
            Token::Static(_) if self.peek_at(1) == Token::DoubleColon => {
                let expr = self.parse_expr()?;
                self.finish_static_property_statement(expr)
            }
            Token::Static(_)
                if matches!(self.peek_at(1), Token::Function(_) | Token::Fn(_)) =>
            {
                self.parse_expression_statement()
            }
            Token::Isset
            | Token::Empty
            | Token::Exit { .. }
            | Token::Match(_)
            | Token::New(_)
            | Token::Yield(_)
            | Token::Clone(_)
            | Token::Print
            | Token::LParen(_)
            | Token::Fn(_)
            | Token::Function(_)
            | Token::Integer(_)
            | Token::Float(_)
            | Token::StringLiteral(_)
            | Token::BinaryStringLiteral(_)
            | Token::MagicConstant { .. }
            | Token::Dollar(_)
            | Token::This(_)
            | Token::Null
            | Token::True
            | Token::False
            | Token::Bang
            | Token::Plus
            | Token::Minus
            | Token::At
            | Token::Tilde
            | Token::PlusPlus
            | Token::MinusMinus
            | Token::ArrayKw => self.parse_expression_statement(),
            Token::Identifier(_, _) | Token::Backslash | Token::From => {
                // Check for list() destructuring: list($a, $b) = expr;
                if let Token::Identifier(ref name, _) = self.peek() {
                    if name.eq_ignore_ascii_case("list")
                        && matches!(self.peek_at(1), Token::LParen(_))
                    {
                        return self.parse_list_assign();
                    }
                }
                let expr = self.parse_expr()?;
                self.finish_static_property_statement(expr)
            }
            Token::LBracket(_) => {
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
                    match self.peek() {
                        Token::Variable(_, _) => match self.advance() {
                            Token::Variable(name, _) => {
                                vars.push(GlobalTarget::Variable(name))
                            }
                            _ => unreachable!(),
                        },
                        Token::Dollar(_) => {
                            self.advance();
                            let name = if self.peek() == Token::LBrace {
                                self.advance();
                                let name = self.parse_expr()?;
                                self.expect(&Token::RBrace)?;
                                name
                            } else {
                                self.parse_primary_atom()?
                            };
                            vars.push(GlobalTarget::Dynamic(name));
                        }
                        other => {
                            return Err(format!(
                                "Expected variable after 'global', got {:?}",
                                other
                            ));
                        }
                    }
                    if matches!(self.peek(), Token::Comma(_)) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Global(vars))
            }
            Token::Static(_)
                if matches!(self.peek_at(1), Token::Variable(_, _) | Token::This(_)) =>
            {
                // static $var = expr; (function-level static variable)
                self.advance(); // consume 'static'
                let line = match self.peek() {
                    Token::Variable(_, line) | Token::This(line) => line,
                    _ => unreachable!("static-variable lookahead was already validated"),
                };
                let mut vars = Vec::new();
                loop {
                    let var_name = match self.advance() {
                        Token::Variable(name, _) => name,
                        Token::This(this_line) => {
                            self.compile_error("Cannot use $this as static variable", this_line);
                            "this".to_string()
                        }
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
                    if matches!(self.peek(), Token::Comma(_)) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::StaticVar { vars, line })
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
            let line = match &expr {
                Expr::ArrayAccess { line, .. } => *line,
                _ => unreachable!(),
            };
            let (root, indices) = Self::split_array_access(expr);
            if !matches!(
                root,
                Expr::StaticProperty { .. }
                    | Expr::DynamicNamedStaticProperty { .. }
                    | Expr::DynamicStaticProperty { .. }
            ) {
                return Err("Unsupported static array assignment target".into());
            }
            self.advance();
            let value = self.parse_expr()?;
            self.expect(&Token::Semicolon(0))?;
            return Ok(Stmt::NestedArrayAssign {
                root,
                indices,
                expr: value,
                line,
            });
        }
        let static_property = match &expr {
            Expr::StaticProperty {
                class_name,
                property,
                line,
                ..
            } => Some((class_name.clone(), property.clone(), *line)),
            _ => None,
        };
        if let Some((class_name, property, line)) = static_property {
            if self.peek() == Token::Assign {
                self.advance();
                let value = self.parse_expr()?;
                self.expect(&Token::Semicolon(0))?;
                return Ok(Stmt::AssignStaticProp {
                    class_name,
                    property,
                    expr: value,
                    line,
                });
            }
            if let Some(op) = Self::compound_assign_op(&self.peek()) {
                self.advance();
                let right = self.parse_expr()?;
                self.expect(&Token::Semicolon(0))?;
                return Ok(Stmt::AssignStaticProp {
                    class_name,
                    property,
                    expr: Expr::BinaryOp {
                        op,
                        left: Box::new(expr),
                        right: Box::new(right),
                    },
                    line,
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
        matches!(self.peek(), Token::LBracket(_)) && self.peek_at(1) == Token::RBracket
    }

    fn finish_array_append_statement(&mut self, target: Expr) -> Result<Stmt, String> {
        let nullsafe_line = Self::nullsafe_chain_line(&target);
        let write_root_error = nullsafe_line
            .is_none()
            .then(|| self.array_write_root_error(&target))
            .flatten();
        self.expect_lbracket()?;
        self.expect(&Token::RBracket)?;
        self.expect(&Token::Assign)?;
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon(0))?;
        if let Some(line) = nullsafe_line {
            return Ok(Stmt::ExprStmt(self.nullsafe_write_error(line)));
        }
        if let Some((message, line)) = write_root_error {
            return Ok(Stmt::ExprStmt(self.compile_error(message, line)));
        }
        if let Expr::Globals { line } = target {
            return Ok(Stmt::ExprStmt(
                self.compile_error("Cannot append to $GLOBALS", line),
            ));
        }
        Ok(Stmt::ArrayAppend { target, expr })
    }

    fn finish_coalesce_assign_statement(&mut self, target: Expr) -> Result<Stmt, String> {
        let write_root_error = if Self::nullsafe_chain_line(&target).is_none()
            && matches!(&target, Expr::ArrayAccess { .. } | Expr::ArrayAppendArgument { .. })
        {
            self.array_write_root_error(&target)
        } else {
            None
        };
        let valid_target = matches!(
            &target,
            Expr::Variable { .. }
                | Expr::DynamicVariable { .. }
                | Expr::Globals { .. }
                | Expr::ArrayAccess { .. }
                | Expr::ArrayAppendArgument { .. }
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
        );
        if !valid_target {
            return Err("Invalid null-coalescing assignment target".into());
        }
        self.expect(&Token::QuestionQuestionAssign)?;
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon(0))?;
        if let Some((message, line)) = write_root_error {
            return Ok(Stmt::ExprStmt(self.compile_error(message, line)));
        }
        if let Expr::Globals { line } = target {
            return Ok(Stmt::ExprStmt(self.globals_modification_error(line)));
        }
        Ok(Stmt::CoalesceAssign { target, expr })
    }

    fn finish_compound_assign_statement(&mut self, target: Expr) -> Result<Stmt, String> {
        let write_root_error = if Self::nullsafe_chain_line(&target).is_none()
            && matches!(&target, Expr::ArrayAccess { .. })
        {
            self.array_write_root_error(&target)
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
        ) {
            return Err("Invalid compound assignment target".into());
        }
        let op = Self::compound_assign_op(&self.advance())
            .ok_or_else(|| "Expected compound assignment operator".to_string())?;
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon(0))?;
        if let Some((message, line)) = write_root_error {
            return Ok(Stmt::ExprStmt(self.compile_error(message, line)));
        }
        if let Expr::Globals { line } = target {
            return Ok(Stmt::ExprStmt(self.globals_modification_error(line)));
        }
        Ok(Stmt::CompoundAssign { target, op, expr })
    }

    /// Parse if / elseif / else chain.
    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'if' or 'elseif'
        self.expect_lparen()?;
        let condition = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        if self.peek() == Token::Colon {
            return self.parse_alternative_if_after_condition(condition);
        }
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

    fn parse_alternative_if_after_condition(&mut self, condition: Expr) -> Result<Stmt, String> {
        self.expect(&Token::Colon)?;
        let then_body = self.parse_statements_until(|token| {
            matches!(token, Token::ElseIf | Token::Else | Token::EndIf)
        })?;
        let else_body = match self.peek() {
            Token::ElseIf => {
                self.advance();
                self.expect_lparen()?;
                let elseif_condition = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                vec![self.parse_alternative_if_after_condition(elseif_condition)?]
            }
            Token::Else => {
                self.advance();
                self.expect(&Token::Colon)?;
                let body = self.parse_statements_until(|token| *token == Token::EndIf)?;
                self.expect(&Token::EndIf)?;
                self.expect(&Token::Semicolon(0))?;
                body
            }
            Token::EndIf => {
                self.advance();
                self.expect(&Token::Semicolon(0))?;
                Vec::new()
            }
            token => return Err(format!("Expected elseif, else, or endif, got {token:?}")),
        };
        Ok(Stmt::If {
            condition,
            then_body,
            else_body,
        })
    }

    /// Parse for-loop init: either `$var = expr`, `$var op= expr`, or an expression.
    fn parse_for_init(&mut self) -> Result<Stmt, String> {
        if let Token::Variable(_, _) = self.peek() {
            let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
            if next == Token::Assign {
                let var_name = match self.advance() {
                    Token::Variable(name, _) => name,
                    _ => unreachable!(),
                };
                self.expect(&Token::Assign)?;
                let expr = self.parse_expr()?;
                return Ok(Stmt::Assign {
                    var: var_name,
                    expr,
                });
            } else if let Some(bin_op) = Self::compound_assign_op(&next) {
                let (var_name, line) = match self.advance() {
                    Token::Variable(name, line) => (name, line),
                    _ => unreachable!(),
                };
                self.advance(); // consume compound operator
                let rhs = self.parse_expr()?;
                return Ok(Stmt::CompoundAssign {
                    target: Expr::Variable {
                        name: var_name,
                        line,
                    },
                    op: bin_op,
                    expr: rhs,
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
                stmts.push(self.parse_stmt_in_scope(false)?);
            }
            self.expect(&Token::RBrace)?;
            Ok(stmts)
        } else {
            // Single statement (no braces)
            let stmt = self.parse_stmt_in_scope(false)?;
            Ok(vec![stmt])
        }
    }

    fn parse_statements_until(
        &mut self,
        mut is_terminator: impl FnMut(&Token) -> bool,
    ) -> Result<Vec<Stmt>, String> {
        let mut statements = Vec::new();
        while !self.at_eof() {
            let token = self.peek();
            if is_terminator(&token) {
                break;
            }
            statements.push(self.parse_stmt_in_scope(false)?);
        }
        Ok(statements)
    }

    fn parse_control_body(&mut self, end_token: Token) -> Result<Vec<Stmt>, String> {
        if self.peek() != Token::Colon {
            return self.parse_block_or_stmt();
        }
        self.advance();
        let body = self.parse_statements_until(|token| *token == end_token)?;
        self.expect(&end_token)?;
        self.expect(&Token::Semicolon(0))?;
        Ok(body)
    }

    fn parse_stmt_in_scope(&mut self, outermost: bool) -> Result<Stmt, String> {
        let previous = self.outermost_scope;
        self.outermost_scope = outermost;
        let result = self.parse_stmt();
        self.outermost_scope = previous;
        result
    }

    fn consume_switch_label_separator(&mut self) -> Result<(), String> {
        match self.peek() {
            Token::Colon => {
                self.advance();
                Ok(())
            }
            Token::Semicolon(line) => {
                self.advance();
                self.deferred_compile_deprecations.push((
                    "Case statements followed by a semicolon (;) are deprecated, use a colon (:) instead"
                        .to_string(),
                    line,
                ));
                Ok(())
            }
            token => Err(format!("Expected ':' or ';' after switch label, got {token:?}")),
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
        self.expect(&Token::Semicolon(0))?;
        match expr {
            Expr::CoalesceAssign { target, expr } => Ok(Stmt::CoalesceAssign {
                target: *target,
                expr: *expr,
            }),
            Expr::ArrayAppendAssign {
                target,
                expr,
                by_ref,
            } if by_ref => Ok(Stmt::ExprStmt(Expr::ArrayAppendAssign {
                target,
                expr,
                by_ref,
            })),
            Expr::ArrayAppendAssign {
                target,
                expr,
                by_ref,
            } => match *target {
                target @ Expr::Variable { .. } => Ok(Stmt::ExprStmt(Expr::ArrayAppendAssign {
                    target: Box::new(target),
                    expr,
                    by_ref,
                })),
                target => Ok(Stmt::ArrayAppend {
                    target,
                    expr: *expr,
                }),
            },
            Expr::AssignTarget { target, expr } => match *target {
                target @ Expr::DynamicVariable { .. } => {
                    Ok(Stmt::ExprStmt(Expr::AssignTarget {
                        target: Box::new(target),
                        expr,
                    }))
                }
                Expr::PropertyAccess {
                    object,
                    property,
                    nullsafe: false,
                    line,
                } => Ok(Stmt::AssignProp {
                    object: *object,
                    property,
                    expr: *expr,
                    line,
                }),
                Expr::StaticProperty {
                    class_name,
                    property,
                    line,
                    ..
                } => Ok(Stmt::AssignStaticProp {
                    class_name,
                    property,
                    expr: *expr,
                    line,
                }),
                target @ (Expr::DynamicNamedStaticProperty { .. }
                | Expr::DynamicStaticProperty { .. }) => {
                    Ok(Stmt::ExprStmt(Expr::AssignTarget {
                        target: Box::new(target),
                        expr,
                    }))
                }
                target @ Expr::DynamicPropertyAccess {
                    nullsafe: false, ..
                } => Ok(Stmt::ExprStmt(Expr::AssignTarget {
                    target: Box::new(target),
                    expr,
                })),
                target @ Expr::ArrayAccess { .. } => {
                    let line = match &target {
                        Expr::ArrayAccess { line, .. } => *line,
                        _ => unreachable!(),
                    };
                    let (root, mut indices) = Self::split_array_access(target);
                    if indices.len() == 1
                        && let Expr::PropertyAccess {
                            object,
                            property,
                            nullsafe: false,
                            ..
                        } = root
                    {
                        return Ok(Stmt::AssignObjArrayDim {
                            object: *object,
                            property,
                            index: indices.pop().unwrap(),
                            expr: *expr,
                            line,
                        });
                    }
                    Ok(Stmt::NestedArrayAssign {
                        root,
                        indices,
                        expr: *expr,
                        line,
                    })
                }
                _ => Err("Invalid assignment target".into()),
            },
            expr => Ok(Stmt::ExprStmt(expr)),
        }
    }
}
