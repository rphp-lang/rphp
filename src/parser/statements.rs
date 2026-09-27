impl Parser {
    /// Keep recursively nested grammar below the native Rust stack limit.
    /// PHP reports an ordinary parser memory-exhaustion diagnostic when its
    /// generated parser cannot grow its stack; RPHP must likewise reject a
    /// hostile source unit instead of aborting the process.
    const MAX_SYNTAX_NESTING: usize = 256;
    // The statement grammar has a deliberately broad match frame. Move
    // moderately nested sources before a six-level closure/block chain can
    // exhaust a small caller-provided thread stack.
    const DEDICATED_STACK_NESTING: usize = 5;
    const DEDICATED_STACK_SIZE: usize = 64 * 1024 * 1024;

    fn parse_foreach_destructure(&mut self) -> Result<Option<Vec<ListTarget>>, String> {
        let end = if matches!(self.peek_ref(), Token::LBracket(_)) {
            self.skip();
            Token::RBracket
        } else if matches!(self.peek_ref(), Token::Identifier(name, _) if name.eq_ignore_ascii_case("list"))
            && matches!(self.peek_at_ref(1), Token::LParen(_))
        {
            self.skip();
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
        // Doc comments leave the syntax stream in place; moving every other
        // token into a fresh vector was a measurable share of parsing.
        let mut syntax_tokens = tokens;
        let mut doc_comments = Vec::new();
        let mut kept = 0usize;
        syntax_tokens.retain_mut(|token| {
            if matches!(token, Token::DocComment(_)) {
                if let Token::DocComment(comment) = std::mem::replace(token, Token::Eof) {
                    doc_comments.push((kept, comment));
                }
                false
            } else {
                kept += 1;
                true
            }
        });
        Self {
            tokens: syntax_tokens,
            pos: 0,
            doc_comments,
            source_name: None,
            in_class_body: false,
            class_scope_active: false,
            reference_return_context: false,
            generic_scopes: Vec::new(),
            auto_global_scopes: Vec::new(),
            statement_start: 0,
            pending_statement_start: None,
            deferred_compile_error: None,
            deferred_compile_deprecations: Vec::new(),
            strict_types_allowed: true,
            namespace_style: None,
            empty_dimension_unset_context: false,
            preserve_empty_dimension_suffix: false,
            new_postfix_error_suffix: None,
            last_primary_line: None,
            outermost_scope: true,
            global_constant_scope: true,
            assertion_source_capture: false,
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
                | Token::LBrace(token_line)
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
                | Token::Enum {
                    line: token_line, ..
                }
                | Token::Case(token_line)
                | Token::Default(token_line)
                | Token::Echo { line: token_line }
                | Token::ShortEcho { line: token_line } => *token_line,
                _ => line,
            };

            match token {
                Token::LParen(_) | Token::LBrace(_) | Token::LBracket(_) => {
                    depth += 1;
                    if depth > max_depth {
                        max_depth = depth;
                        deepest_line = line;
                    }
                    if depth > Self::MAX_SYNTAX_NESTING {
                        return Err(self.memory_exhausted(line));
                    }
                }
                Token::RParen | Token::RBrace(_) | Token::RBracket => {
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

    #[cold]
    #[inline(never)]
    fn parse_tick_interval(&mut self, line: usize) -> Result<i64, String> {
        let mut offset = 0;
        while matches!(self.peek_at_ref(offset), Token::LParen(_)) { offset += 1; }
        let literal = matches!(self.peek_at_ref(offset), Token::Integer(_) | Token::Float(_)
            | Token::StringLiteral(_) | Token::BinaryStringLiteral(_));
        let expression = self.parse_expr()?;
        let value = if literal { match expression {
            Expr::Integer(value) => Some(crate::value::Value::long(value)),
            Expr::Float(value) => Some(crate::value::Value::double(value)),
            Expr::StringLiteral(value) | Expr::BinaryStringLiteral(value) => Some(crate::value::Value::string(value)),
            _ => None,
        } } else { None };
        let Some(value) = value else {
            self.compile_error("declare(ticks) value must be a literal", line);
            return Ok(0);
        };
        if let Some(message) = crate::vm::execute::explicit_numeric_cast_warning(
            &value, crate::vm::execute::ExplicitNumericCastTarget::Int,
        ) {
            // Use the same trailing source-unit warning tokens as the lexer.
            // They survive dead branches without adding ordinary parser state.
            let end = self.tokens.len() - usize::from(matches!(self.tokens.last(), Some(Token::Eof)));
            self.tokens.insert(end, Token::CompileWarning(message, line));
        }
        Ok(crate::vm::execute::explicit_long_conversion(&value))
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        let strict_types_allowed = self.strict_types_allowed;
        self.statement_start = self.pending_statement_start.take().unwrap_or(self.pos);
        if self.outermost_scope
            && self.namespace_style == Some(NamespaceDeclarationStyle::Bracketed)
            && !matches!(
                self.peek_ref(),
                Token::Namespace | Token::Semicolon(_) | Token::HaltCompiler { .. } | Token::Eof
            )
        {
            let line = self.current_token_source_line();
            self.compile_error("No code may exist outside of namespace {}", line);
        }
        if !matches!(self.peek_ref(), Token::Declare | Token::Semicolon(_)) {
            self.strict_types_allowed = false;
        }
        if let Token::Identifier(name, _) | Token::Enum { name, .. } = self.peek_ref()
            && *self.peek_at_ref(1) == Token::Colon
        {
            let name = name.clone();
            let line = self.current_token_source_line();
            self.skip();
            self.skip();
            return Ok(Stmt::Label { name, line });
        }
        if let Token::Exit { line, .. } = *self.peek_ref()
            && *self.peek_at_ref(1) == Token::Colon
        {
            return Err(self.source_error("syntax error, unexpected token \":\"", line));
        }
        if let Token::Goto { line, .. } = *self.peek_ref() {
            self.skip();
            let name = match self.advance() {
                Token::Identifier(label, _) | Token::Enum { name: label, .. } => label,
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
                self.skip();
                self.halted = true;
                if !self.outermost_scope {
                    let _ = self.compile_error(
                        "__HALT_COMPILER() can only be used from the outermost scope",
                        line,
                    );
                }
                Ok(Stmt::HaltCompiler { offset, line })
            }
            Token::LBrace(_) => {
                self.skip();
                let mut body = Vec::new();
                while !matches!(self.peek_ref(), Token::RBrace(_)) && !self.at_eof() {
                    body.push(self.parse_stmt_in_scope(false)?);
                }
                self.expect(&Token::RBrace(0))?;
                Ok(Stmt::Block(body))
            }
            Token::ParseError(message, line) => {
                self.skip();
                Err(self.source_error(&message, line))
            }
            Token::CompileError(message, line) => {
                self.skip();
                self.compile_error(message, line);
                Ok(Stmt::Noop)
            }
            Token::CompileWarning(message, line) => {
                self.skip();
                Ok(Stmt::ExprStmt(Expr::CompileWarning { message, line }))
            }
            Token::CompileDeprecation(message, line) => {
                self.skip();
                Ok(Stmt::ExprStmt(Expr::CompileDeprecation { message, line }))
            }
            Token::Semicolon(_) => {
                self.skip();
                Ok(Stmt::Noop)
            }
            Token::Declare => {
                self.skip(); // consume 'declare'
                self.expect_lparen()?;
                let mut directives = Vec::new();
                let mut invalid_strict_placement = false;
                let mut strict_line = None;
                loop {
                    let (directive, directive_line) = match self.advance() {
                        Token::Identifier(n, line) => (n, line),
                        other => {
                            return Err(format!(
                                "Expected directive name in declare(), got {:?}",
                                other
                            ));
                        }
                    };
                    if directive.eq_ignore_ascii_case("strict_types") {
                        strict_line = Some(directive_line);
                        invalid_strict_placement |= !strict_types_allowed;
                        if invalid_strict_placement {
                            let _ = self.compile_error(
                                "strict_types declaration must be the very first statement in the script",
                                directive_line,
                            );
                        }
                    }
                    self.expect(&Token::Assign)?;
                    let value = if directive.eq_ignore_ascii_case("encoding") {
                        match self.peek() {
                            Token::StringLiteral(_) | Token::BinaryStringLiteral(_) => {
                                self.skip();
                                0
                            }
                            _ => {
                                let line = self.closest_token_source_line();
                                let _ = self.compile_error("Encoding must be a literal", line);
                                let _ = self.parse_expr()?;
                                0
                            }
                        }
                    } else if directive.eq_ignore_ascii_case("ticks") {
                        self.parse_tick_interval(directive_line)?
                    } else {
                        match self.advance() {
                            Token::Integer(n) => n,
                            Token::True => 1,
                            Token::False => 0,
                            other => {
                                return Err(format!(
                                    "Expected integer value in declare(), got {:?}",
                                    other
                                ));
                            }
                        }
                    };
                    directives.push((directive, value));
                    if !matches!(self.peek_ref(), Token::Comma(_)) {
                        break;
                    }
                    self.skip();
                }
                self.expect(&Token::RParen)?;
                let invalid_strict_block = strict_line.is_some()
                    && !matches!(self.peek_ref(), Token::Semicolon(_));
                if invalid_strict_block {
                    let _ = self.compile_error(
                        "strict_types declaration must not use block mode",
                        strict_line.unwrap_or(1),
                    );
                }
                let body = if matches!(self.peek_ref(), Token::LBrace(_)) {
                    self.skip();
                    let mut body = Vec::new();
                    while !matches!(self.peek_ref(), Token::RBrace(_)) && !self.at_eof() {
                        body.push(self.parse_stmt_in_scope(false)?);
                    }
                    self.expect(&Token::RBrace(0))?;
                    Some(body)
                } else if *self.peek_ref() == Token::Colon {
                    self.skip();
                    let body = self.parse_statements_until(|token| {
                        matches!(token, Token::Identifier(name, _) if name.eq_ignore_ascii_case("enddeclare"))
                    })?;
                    self.skip();
                    self.expect(&Token::Semicolon(0))?;
                    Some(body)
                } else if matches!(self.peek_ref(), Token::Semicolon(_)) {
                    self.expect(&Token::Semicolon(0))?;
                    None
                } else {
                    let start = self.pos;
                    let statement = self.parse_stmt_in_scope(false);
                    if statement.is_err() && self.pos == start && self.source_name.is_none() {
                        // Preserve the source-less terminator diagnostic when
                        // no body began, reusing the existing cold error path.
                        // An entered body retains its own diagnostic instead.
                        self.expect(&Token::Semicolon(0))?;
                    }
                    Some(vec![statement?])
                };
                if invalid_strict_placement || invalid_strict_block {
                    Ok(Stmt::Noop)
                } else {
                    Ok(Stmt::Declare { directives, body })
                }
            }
            Token::Namespace if *self.peek_at_ref(1) != Token::Backslash => {
                self.skip(); // consume 'namespace'
                // The bracketed global namespace has no name: `namespace { ... }`.
                // Keep the empty spelling in the AST so compilation can restore
                // global resolution while retaining the namespace block boundary.
                let name = if matches!(self.peek_ref(), Token::LBrace(_)) {
                    String::new()
                } else {
                    self.parse_namespace_declaration_name()?
                };
                let line = self
                    .last_primary_line
                    .unwrap_or_else(|| self.current_token_source_line());
                let style = if matches!(self.peek_ref(), Token::LBrace(_)) {
                    NamespaceDeclarationStyle::Bracketed
                } else {
                    NamespaceDeclarationStyle::Unbracketed
                };
                if self.namespace_style.is_none() && !strict_types_allowed {
                    self.compile_error(
                        "Namespace declaration statement has to be the very first statement or after any declare call in the script",
                        line,
                    );
                }
                if let Some(previous) = self.namespace_style
                    && previous != style
                {
                    self.compile_error(
                        "Cannot mix bracketed namespace declarations with unbracketed namespace declarations",
                        line,
                    );
                } else if !self.outermost_scope {
                    self.compile_error("Namespace declarations cannot be nested", line);
                }
                self.namespace_style.get_or_insert(style);
                if name.eq_ignore_ascii_case("namespace") {
                    self.compile_error(format!("Cannot use '{name}' as namespace name"), line);
                } else if name
                    .split_once('\\')
                    .is_some_and(|(prefix, _)| prefix.eq_ignore_ascii_case("namespace"))
                {
                    return Err(self.source_error(
                        &format!(
                            "syntax error, unexpected namespace-relative name \"{name}\", expecting \"{{\""
                        ),
                        line,
                    ));
                }
                if style == NamespaceDeclarationStyle::Bracketed {
                    // Braced namespace: namespace App\Models { ... }
                    self.skip(); // consume '{'
                    let mut body = Vec::new();
                    while !matches!(self.peek_ref(), Token::RBrace(_)) && *self.peek_ref() != Token::Eof {
                        body.push(self.parse_stmt_in_namespace_scope()?);
                    }
                    if self.halted && self.at_eof() {
                        return Err(self.source_error("Unclosed '{'", 1));
                    }
                    self.expect(&Token::RBrace(0))?;
                    Ok(Stmt::Namespace { name, body })
                } else {
                    // Unbraced namespace: namespace App\Models; (rest of file belongs to this namespace)
                    self.expect(&Token::Semicolon(0))?;
                    let mut body = Vec::new();
                    while *self.peek_ref() != Token::Eof
                        && *self.peek_ref() != Token::Namespace
                        && !matches!(self.peek_ref(), Token::RBrace(_))
                    {
                        body.push(self.parse_stmt_in_scope(true)?);
                    }
                    Ok(Stmt::Namespace { name, body })
                }
            }
            Token::Use(use_line) if !self.in_class_body => {
                // Top-level class/function import. Their alias tables are
                // separate in PHP even when the source alias is identical.
                self.skip(); // consume 'use'
                let kind = if matches!(self.peek_ref(), Token::Function(_)) {
                    self.skip();
                    UseKind::Function
                } else if *self.peek_ref() == Token::Const {
                    self.skip();
                    UseKind::Const
                } else {
                    UseKind::Class
                };
                let (first_name, grouped, name_line) = self.parse_use_name()?;
                let mut imports = Vec::new();
                if grouped {
                    if matches!(self.peek_ref(), Token::RBrace(_)) {
                        return Err(self.group_use_missing_item_error(kind, "}", use_line));
                    }
                    if matches!(self.peek_ref(), Token::Comma(_)) {
                        return Err(self.group_use_missing_item_error(kind, ",", use_line));
                    }
                    loop {
                        let item_kind = if matches!(self.peek_ref(), Token::Function(_)) {
                            if kind != UseKind::Class {
                                return Err(self.source_error(
                                    "syntax error, unexpected token \"function\", expecting \"}\"",
                                    self.current_token_source_line(),
                                ));
                            }
                            self.skip();
                            UseKind::Function
                        } else if *self.peek_ref() == Token::Const {
                            if kind != UseKind::Class {
                                return Err(self.source_error(
                                    "syntax error, unexpected token \"const\", expecting \"}\"",
                                    self.current_token_source_line(),
                                ));
                            }
                            self.skip();
                            UseKind::Const
                        } else {
                            kind
                        };
                        if matches!(self.peek_ref(), Token::RBrace(_)) {
                            return Err(self.group_use_missing_item_error(
                                item_kind, "}", use_line,
                            ));
                        }
                        if matches!(self.peek_ref(), Token::Comma(_)) {
                            return Err(self.group_use_missing_item_error(
                                item_kind, ",", use_line,
                            ));
                        }
                        if *self.peek_ref() == Token::Backslash {
                            let line = self.current_token_source_line();
                            let name = Self::token_as_named_arg_label(self.peek_at_ref(1))
                                .unwrap_or_default();
                            return Err(self.source_error(
                                &format!(
                                    "syntax error, unexpected fully qualified name \"\\{name}\", expecting identifier or namespaced name or \"function\" or \"const\""
                                ),
                                line,
                            ));
                        }
                        let (relative_name, nested_group, _) = self.parse_use_name()?;
                        if nested_group {
                            return Err("Nested group use declaration is not allowed".to_string());
                        }
                        let explicit_alias = self.consume_as_keyword();
                        let alias = if explicit_alias {
                            match self.advance() {
                                Token::Identifier(name, _) | Token::Enum { name, .. } => name,
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
                        imports.push((
                            item_kind,
                            format!("{first_name}\\{relative_name}"),
                            alias,
                            explicit_alias,
                        ));
                        if !matches!(self.peek_ref(), Token::Comma(_)) {
                            break;
                        }
                        self.skip();
                        if matches!(self.peek_ref(), Token::RBrace(_)) {
                            break;
                        }
                        if matches!(self.peek_ref(), Token::Comma(_)) {
                            return Err(self.source_error(
                                "syntax error, unexpected token \",\", expecting \"}\"",
                                use_line,
                            ));
                        }
                    }
                    if let Token::LBrace(line) = self.peek() {
                        return Err(self.source_error(
                            "syntax error, unexpected token \"{\", expecting \"}\"",
                            line,
                        ));
                    }
                    self.expect(&Token::RBrace(0))?;
                } else {
                    let mut fqn = first_name;
                    loop {
                        let explicit_alias = self.consume_as_keyword();
                        let alias = if explicit_alias {
                            match self.advance() {
                                Token::Identifier(name, _) | Token::Enum { name, .. } => name,
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
                        imports.push((kind, fqn, alias, explicit_alias));
                        if !matches!(self.peek_ref(), Token::Comma(_)) {
                            break;
                        }
                        self.skip();
                        let (next_name, nested_group, _) = self.parse_use_name()?;
                        if nested_group {
                            return Err("Group use prefix must start a use declaration".to_string());
                        }
                        fqn = next_name;
                    }
                }
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::UseDecl {
                    line: use_line,
                    name_line,
                    imports,
                })
            }
            Token::Const => {
                if !self.global_constant_scope {
                    return Err(self.source_error(
                        "syntax error, unexpected token \"const\"",
                        self.current_token_source_line(),
                    ));
                }
                self.skip(); // consume 'const'
                let mut declarations = Vec::new();
                let mut const_line = 0;
                loop {
                    let (name, line) = match self.advance() {
                        Token::Identifier(name, line)
                            if ["null", "true", "false"].iter().any(|word| name.eq_ignore_ascii_case(word)) => {
                            self.compile_error(
                                format!("Cannot redeclare constant '{}'", name.to_ascii_uppercase()),
                                line,
                            );
                            (name, line)
                        }
                        Token::Identifier(ref name, line) if Self::reserved_identifier(name).is_some() => {
                            return Err(self.unexpected_token_error(&self.tokens[self.pos - 1], "identifier", line));
                        }
                        Token::Identifier(name, line)
                        | Token::Enum { name, line }
                        | Token::MagicConstant { name, line }
                        | Token::Goto { name, line } => (name, line),
                        Token::Exit { line, .. } => {
                            return Err(self.source_error(
                                "syntax error, unexpected token \"exit\", expecting identifier",
                                line,
                            ));
                        }
                        Token::Null | Token::True | Token::False => {
                            let line = self
                                .following_semicolon_source_line()
                                .unwrap_or_else(|| self.closest_token_source_line());
                            let name = match self.tokens[self.pos - 1] {
                                Token::Null => "NULL",
                                Token::True => "TRUE",
                                Token::False => "FALSE",
                                _ => unreachable!(),
                            };
                            self.compile_error(
                                format!("Cannot redeclare constant '{name}'"),
                                line,
                            );
                            (name.to_string(), line)
                        }
                        other => {
                            return Err(self.unexpected_token_error(&other, "identifier", self.closest_token_source_line()));
                        }
                    };
                    if const_line == 0 {
                        const_line = line;
                    }
                    self.expect(&Token::Assign)?;
                    declarations.push((name, self.parse_expr()?));
                    if !matches!(self.peek_ref(), Token::Comma(_)) {
                        break;
                    }
                    self.skip();
                }
                if let Token::LBrace(line) = self.peek() {
                    return Err(self.source_error(
                        "syntax error, unexpected token \"{\", expecting \",\" or \";\"",
                        line,
                    ));
                }
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Const {
                    line: const_line,
                    attributes: Vec::new(),
                    declarations,
                })
            }
            Token::Echo { line } | Token::ShortEcho { line } => {
                self.skip();
                let mut expressions = vec![self.with_new_postfix_error_suffix(
                    Some(", expecting \",\" or \";\""),
                    |parser| parser.parse_expr(),
                )?];
                while matches!(self.peek_ref(), Token::Comma(_)) {
                    self.skip();
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
                let line = self
                    .following_semicolon_source_line()
                    .unwrap_or_else(|| self.closest_token_source_line());
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Include {
                    path,
                    is_require,
                    is_once,
                    line,
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
                        && !matches!(
                            self.tokens.get(self.pos + 4),
                            Some(Token::Ampersand(_))
                        );
                    if is_push {
                        let (var_name, line) = match self.advance() {
                            Token::Variable(name, line) => (name, line),
                            _ => unreachable!(),
                        };
                        self.skip(); // consume '['
                        self.skip(); // consume ']'
                        self.skip(); // consume '='
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
                        while matches!(self.peek_ref(), Token::LBracket(_)) {
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
                    if *self.peek_ref() == Token::QuestionQuestionAssign {
                        return self.finish_coalesce_assign_statement(expr);
                    }
                    if Self::compound_assign_op(self.peek_ref()).is_some() {
                        return self.finish_compound_assign_statement(expr);
                    }
                    self.finish_value_expression_statement(expr)
                } else if next == Token::Assign {
                    let (var_name, line) = match self.advance() {
                        Token::Variable(name, line) => (name, line),
                        _ => unreachable!(),
                    };
                    self.expect(&Token::Assign)?;
                    if matches!(self.peek_ref(), Token::Ampersand(_)) {
                        self.skip();
                        let target = self.parse_empty_dimension_target_prefix()?;
                        if !self.is_empty_array_dimension_suffix() {
                            self.expect(&Token::Semicolon(0))?;
                            if var_name == "GLOBALS" {
                                return Ok(Stmt::ExprStmt(self.globals_modification_error(line)));
                            }
                            return Ok(Stmt::ExprStmt(
                                self.finish_reference_assignment_precedence(
                                    Self::variable_expression(var_name, line),
                                    target,
                                ),
                            ));
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
                    self.skip(); // consume the compound operator
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
                    if *self.peek_ref() == Token::QuestionQuestionAssign {
                        return self.finish_coalesce_assign_statement(expr);
                    }
                    if Self::compound_assign_op(self.peek_ref()).is_some() {
                        return self.finish_compound_assign_statement(expr);
                    }
                    // Check for property/array-dim assignment: $obj->prop = expr or $obj->prop[$key] = expr
                    if *self.peek_ref() == Token::Assign {
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
                                self.skip(); // consume '='
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
                            self.skip(); // consume '='
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
                self.skip(); // consume 'while'
                self.expect_lparen()?;
                let condition = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                let body = self.parse_control_body(Token::EndWhile)?;
                Ok(Stmt::While { condition, body })
            }
            Token::Do => {
                self.skip(); // consume 'do'
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
                self.skip();
                let level = self.parse_break_continue_level("break", line)?;
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Break { level, line })
            }
            Token::Continue { line } => {
                self.skip();
                let level = self.parse_break_continue_level("continue", line)?;
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Continue { level, line })
            }
            Token::Switch => {
                self.skip(); // consume 'switch'
                self.expect_lparen()?;
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                let alternative = match self.peek() {
                    Token::LBrace(_) => {
                        self.skip();
                        false
                    }
                    Token::Colon => {
                        self.skip();
                        true
                    }
                    token => return Err(format!("Expected switch body, got {token:?}")),
                };
                let mut cases = Vec::new();
                let mut has_default = false;
                while !matches!(self.peek_ref(), Token::RBrace(_) | Token::EndSwitch) && !self.at_eof() {
                    match self.peek() {
                        Token::Case(_) => {
                            self.skip();
                            let value = self.parse_expr()?;
                            self.consume_switch_label_separator()?;
                            let mut body = Vec::new();
                            while !matches!(
                                self.peek_ref(),
                                Token::Case(_)
                                    | Token::Default(_)
                                    | Token::RBrace(_)
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
                        Token::Default(line) => {
                            if has_default {
                                let _ = self.compile_error(
                                    "Switch statements may only contain one default clause",
                                    line,
                                );
                            } else {
                                has_default = true;
                            }
                            self.skip();
                            self.consume_switch_label_separator()?;
                            let mut body = Vec::new();
                            while !matches!(
                                self.peek_ref(),
                                Token::Case(_)
                                    | Token::Default(_)
                                    | Token::RBrace(_)
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
                    self.expect(&Token::RBrace(0))?;
                }
                Ok(Stmt::Switch { expr, cases })
            }
            Token::For => {
                self.skip(); // consume 'for'
                self.expect_lparen()?;

                // Init: optional assignment or expression before first ;
                let mut init = Vec::new();
                while !matches!(self.peek_ref(), Token::Semicolon(_)) {
                    init.push(self.parse_for_init()?);
                    if !matches!(self.peek_ref(), Token::Comma(_)) {
                        break;
                    }
                    self.skip();
                }
                self.expect(&Token::Semicolon(0))?;

                // Every comma-separated condition is evaluated; the last one
                // determines whether the loop continues.
                let mut condition = Vec::new();
                while !matches!(self.peek_ref(), Token::Semicolon(_)) {
                    condition.push(self.parse_expr()?);
                    if !matches!(self.peek_ref(), Token::Comma(_)) {
                        break;
                    }
                    self.skip();
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
                while *self.peek_ref() != Token::RParen {
                    update.push(self.parse_expr()?);
                    if !matches!(self.peek_ref(), Token::Comma(_)) {
                        break;
                    }
                    self.skip();
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
                self.skip(); // consume 'foreach'
                self.expect_lparen()?;
                let array = self.parse_expr()?;
                if !self.consume_as_keyword() {
                    self.expect(&Token::As(0))?;
                }
                // foreach ($arr as $key => $val), foreach ($arr as $val),
                // and the corresponding destructuring value forms.
                let first_reference_line = if matches!(self.peek_ref(), Token::Ampersand(_)) {
                    // PHP locates the invalid key-reference diagnostic at the
                    // source/as boundary, including when the key starts on a
                    // following line.
                    let line = self.closest_token_source_line();
                    self.skip();
                    Some(line)
                } else {
                    None
                };
                let first_by_ref = first_reference_line.is_some();
                let first_target_position = self.pos;
                let deferred_before_first_target = self.deferred_compile_error.clone();
                let first = if let Some(targets) = self.parse_foreach_destructure()? {
                    if first_by_ref {
                        return Err("Foreach destructuring target cannot be a reference".into());
                    }
                    ForeachTarget::Destructure(targets)
                } else {
                    let first_expr = self.parse_foreach_target_expression()?;
                    self.into_foreach_target(first_expr)?
                };
                let (key, value, by_ref) = if *self.peek_ref() == Token::DoubleArrow {
                    let first = if matches!(&first, ForeachTarget::Destructure(_)) {
                        self.deferred_compile_error = deferred_before_first_target;
                        let first_target_line =
                            self.closest_token_source_line_before(first_target_position);
                        ForeachTarget::Target(
                            self.compile_error("Cannot use list as key element", first_target_line),
                        )
                    } else {
                        first
                    };
                    if let Some(line) = first_reference_line {
                        let _ = self.compile_error("Key element cannot be a reference", line);
                    }
                    self.skip(); // consume '=>'
                    let by_ref = if matches!(self.peek_ref(), Token::Ampersand(_)) {
                        self.skip();
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
                    self.peek_at_ref(1),
                    Token::Identifier(_, _)
                        | Token::Enum { .. }
                        | Token::From
                        | Token::Exit { .. }
                )
                    || (matches!(self.peek_at_ref(1), Token::Ampersand(_))
                        && matches!(
                            self.peek_at_ref(2),
                            Token::Identifier(_, _)
                                | Token::Enum { .. }
                                | Token::From
                                | Token::Exit { .. }
                        )) =>
            {
                self.skip(); // consume 'function'
                // Accept the PHP reference-return declaration marker. Return
                // aliasing itself remains outside the current execution
                // contract, matching the closure parser's bounded handling.
                let returns_by_ref = matches!(self.peek_ref(), Token::Ampersand(_));
                self.consume_reference_return_marker();
                let name = match self.advance() {
                    Token::Identifier(ref name, line) if Self::reserved_identifier(name).is_some() => {
                        return Err(self.unexpected_token_error(&self.tokens[self.pos - 1], "\"(\"", line));
                    }
                    Token::Identifier(n, _) | Token::Enum { name: n, .. } => n,
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
                let previous_reference_context = std::mem::replace(&mut self.reference_return_context, returns_by_ref);
                let body = self.parse_function_body()?;
                self.pop_generic_scope();
                self.class_scope_active = previous_class_scope;
                self.reference_return_context = previous_reference_context;
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
                self.skip(); // consume 'return'
                if matches!(self.peek_ref(), Token::Semicolon(_)) {
                    self.skip();
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
                self.skip();
                let list_line = self.expect_lparen()?;
                if matches!(self.peek_ref(), Token::Comma(_)) {
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
                self.skip();
                let expr = self.parse_expr()?;
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Throw { expr, line })
            }
            Token::AttributeStart(_) => {
                let statement_start = self.statement_start;
                let attributes = self.parse_attribute_groups()?;
                self.pending_statement_start = Some(statement_start);
                let declaration = self.parse_stmt()?;
                self.attach_attributes(declaration, attributes)
            }
            Token::Class | Token::Abstract(_) | Token::Final(_) => self.parse_class(),
            Token::Identifier(ref name, _)
                if name.eq_ignore_ascii_case("class")
                    || name.eq_ignore_ascii_case("abstract")
                    || name.eq_ignore_ascii_case("final") =>
            {
                self.parse_class()
            }
            Token::Identifier(ref name, _)
                if name.eq_ignore_ascii_case("readonly")
                    && matches!(
                        self.peek_at_ref(1),
                        Token::Enum { .. } | Token::Interface | Token::Trait
                    ) =>
            {
                let (unexpected, line) = match (self.peek_at(1), self.peek()) {
                    (Token::Enum { .. }, Token::Identifier(_, line)) => ("enum", line),
                    (Token::Interface, Token::Identifier(_, line)) => ("interface", line),
                    (Token::Trait, Token::Identifier(_, line)) => ("trait", line),
                    _ => unreachable!("guarded readonly class-like diagnostic"),
                };
                Err(self.source_error(
                    &format!(
                        "syntax error, unexpected token \"{unexpected}\", expecting \"abstract\" or \"final\" or \"readonly\" or \"class\""
                    ),
                    line,
                ))
            }
            Token::Identifier(ref name, _)
                if name.eq_ignore_ascii_case("readonly")
                    && (matches!(
                        self.peek_at_ref(1),
                        Token::Class | Token::Abstract(_) | Token::Final(_)
                    ) || matches!(
                        self.peek_at_ref(1),
                        Token::Identifier(keyword, _)
                            if keyword.eq_ignore_ascii_case("class")
                                || keyword.eq_ignore_ascii_case("readonly")
                    )) =>
            {
                self.parse_class()
            }
            Token::Enum { .. } if matches!(self.peek_at_ref(1), Token::LBrace(_)) => {
                let Token::LBrace(line) = self.peek_at(1) else {
                    unreachable!("guarded contextual enum block diagnostic")
                };
                Err(self.source_error("syntax error, unexpected token \"{\"", line))
            }
            Token::Enum { .. }
                if matches!(
                    self.peek_at_ref(1),
                    Token::Identifier(_, _) | Token::Enum { .. }
                ) => self.parse_enum(),
            Token::Enum { .. }
                if matches!(
                    self.peek_at_ref(1),
                    Token::Backslash
                        | Token::DoubleColon
                        | Token::LParen(_)
                        | Token::LBracket(_)
                        | Token::Arrow
                        | Token::NullSafe
                        | Token::Semicolon(_)
                        | Token::Assign
                        | Token::Plus
                        | Token::Minus
                        | Token::Star
                        | Token::Slash
                        | Token::Percent(_)
                        | Token::Dot
                        | Token::PlusPlus
                        | Token::MinusMinus
                        | Token::EqualEqual
                        | Token::IdenticalEqual
                        | Token::NotEqual
                        | Token::NotIdentical
                        | Token::Less
                        | Token::LessEqual
                        | Token::Greater
                        | Token::GreaterEqual
                        | Token::AmpAmp
                        | Token::PipePipe
                        | Token::LogicalAnd
                        | Token::LogicalOr
                        | Token::LogicalXor
                        | Token::PipeGreater(_)
                        | Token::Question
                        | Token::QuestionQuestion
                        | Token::Instanceof
                        | Token::Pipe
                        | Token::Ampersand(_)
                        | Token::Caret
                        | Token::StarStar
                        | Token::Spaceship
                        | Token::ShiftLeft(_)
                        | Token::ShiftRight(_)
                ) => {
                    let expr = self.parse_expr()?;
                    self.finish_static_property_statement(expr)
                }
            Token::Enum { .. } => self.parse_enum(),
            Token::Interface => self.parse_interface(),
            Token::Trait => self.parse_trait(),
            Token::Static(_) if *self.peek_at_ref(1) == Token::DoubleColon => {
                let expr = self.parse_expr()?;
                self.finish_static_property_statement(expr)
            }
            Token::Static(_)
                if matches!(self.peek_at_ref(1), Token::Function(_) | Token::Fn(_)) =>
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
            | Token::BacktickLiteral { .. }
            | Token::InterpolatedStringStart { .. }
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
            | Token::Tilde(_)
            | Token::PlusPlus
            | Token::MinusMinus
            | Token::ArrayKw => self.parse_expression_statement(),
            Token::Identifier(_, _) | Token::Backslash | Token::Namespace | Token::From => {
                // Check for list() destructuring: list($a, $b) = expr;
                if let Token::Identifier(ref name, _) = self.peek() {
                    if name.eq_ignore_ascii_case("list")
                        && matches!(self.peek_at_ref(1), Token::LParen(_))
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
                self.skip(); // consume 'global'
                let mut vars = Vec::new();
                loop {
                    match self.peek() {
                        Token::Variable(_, _) => match self.advance() {
                            Token::Variable(name, _) => {
                                vars.push(GlobalTarget::Variable(name))
                            }
                            _ => unreachable!(),
                        },
                        Token::This(line) => {
                            self.skip();
                            vars.push(GlobalTarget::Variable(
                                self.invalid_this_binding("global", line),
                            ));
                        }
                        Token::Dollar(_) => {
                            self.skip();
                            let name = if matches!(self.peek_ref(), Token::LBrace(_)) {
                                self.skip();
                                let name = self.parse_expr()?;
                                self.expect(&Token::RBrace(0))?;
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
                    if matches!(self.peek_ref(), Token::Comma(_)) {
                        self.skip();
                    } else {
                        break;
                    }
                }
                if !matches!(self.peek_ref(), Token::Semicolon(_)) {
                    return Err(self.unexpected_token_error(self.peek_ref(), "\",\" or \";\"", self.current_token_source_line()));
                }
                self.expect(&Token::Semicolon(0))?;
                Ok(Stmt::Global(vars))
            }
            Token::Static(_)
                if matches!(self.peek_at_ref(1), Token::Variable(_, _) | Token::This(_)) =>
            {
                // static $var = expr; (function-level static variable)
                self.skip(); // consume 'static'
                let line = match self.peek() {
                    Token::Variable(_, line) | Token::This(line) => line,
                    _ => unreachable!("static-variable lookahead was already validated"),
                };
                let mut vars = Vec::new();
                loop {
                    let var_name = match self.advance() {
                        Token::Variable(name, _) => name,
                        Token::This(this_line) => {
                            self.invalid_this_binding("static", this_line)
                        }
                        other => {
                            return Err(format!(
                                "Expected variable after 'static', got {:?}",
                                other
                            ));
                        }
                    };
                    let default = if *self.peek_ref() == Token::Assign {
                        self.skip();
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    vars.push((var_name, default));
                    if matches!(self.peek_ref(), Token::Comma(_)) {
                        self.skip();
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
        if *self.peek_ref() == Token::QuestionQuestionAssign {
            return self.finish_coalesce_assign_statement(expr);
        }
        if Self::compound_assign_op(self.peek_ref()).is_some() {
            return self.finish_compound_assign_statement(expr);
        }
        if *self.peek_ref() == Token::Assign && matches!(expr, Expr::ArrayAccess { .. }) {
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
            self.skip();
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
            if *self.peek_ref() == Token::Assign {
                self.skip();
                let value = self.parse_expr()?;
                self.expect(&Token::Semicolon(0))?;
                return Ok(Stmt::AssignStaticProp {
                    class_name,
                    property,
                    expr: value,
                    line,
                });
            }
            if let Some(op) = Self::compound_assign_op(self.peek_ref()) {
                self.skip();
                let right = self.parse_expr()?;
                self.expect(&Token::Semicolon(0))?;
                return Ok(Stmt::AssignStaticProp {
                    class_name,
                    property,
                    expr: Expr::BinaryOp {
                        op,
                        left: Box::new(expr),
                        right: Box::new(right),
                        line,
                    },
                    line,
                });
            }
        }
        self.finish_value_expression_statement(expr)
    }

    fn is_array_append_suffix(&self) -> bool {
        self.is_empty_array_dimension_suffix()
            && *self.peek_at_ref(2) == Token::Assign
    }

    fn is_empty_array_dimension_suffix(&self) -> bool {
        matches!(self.peek_ref(), Token::LBracket(_)) && *self.peek_at_ref(1) == Token::RBracket
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
        let elseif = *self.peek_ref() == Token::ElseIf;
        self.skip(); // consume 'if' or 'elseif'
        self.expect_lparen()?;
        let condition = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        if *self.peek_ref() == Token::Colon {
            return self.parse_alternative_if_after_condition(condition);
        }
        let then_body = self.parse_block_or_stmt()?;
        let else_body = if *self.peek_ref() == Token::ElseIf {
            // elseif desugars to else { if (...) { ... } }
            vec![self.parse_if()?]
        } else if *self.peek_ref() == Token::Else {
            self.skip();
            // Check for "else if" (two tokens) which is equivalent to "elseif"
            if *self.peek_ref() == Token::If {
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
            elseif,
        })
    }

    fn parse_alternative_if_after_condition(&mut self, condition: Expr) -> Result<Stmt, String> {
        self.expect(&Token::Colon)?;
        let then_body = self.parse_statements_until(|token| {
            matches!(token, Token::ElseIf | Token::Else | Token::EndIf)
        })?;
        let else_body = match self.peek() {
            Token::ElseIf => {
                self.skip();
                self.expect_lparen()?;
                let elseif_condition = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                vec![self.parse_alternative_if_after_condition(elseif_condition)?]
            }
            Token::Else => {
                self.skip();
                self.expect(&Token::Colon)?;
                let body = self.parse_statements_until(|token| *token == Token::EndIf)?;
                self.expect(&Token::EndIf)?;
                self.expect(&Token::Semicolon(0))?;
                body
            }
            Token::EndIf => {
                self.skip();
                self.expect(&Token::Semicolon(0))?;
                Vec::new()
            }
            token => return Err(format!("Expected elseif, else, or endif, got {token:?}")),
        };
        Ok(Stmt::If {
            condition,
            then_body,
            else_body,
            elseif: true,
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
                self.skip(); // consume compound operator
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
        if matches!(self.peek_ref(), Token::LBrace(_)) {
            self.skip(); // consume {
            let mut stmts = Vec::new();
            while !matches!(self.peek_ref(), Token::RBrace(_)) && !self.at_eof() {
                stmts.push(self.parse_stmt_in_scope(false)?);
            }
            self.expect(&Token::RBrace(0))?;
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
        if *self.peek_ref() != Token::Colon {
            return self.parse_block_or_stmt();
        }
        self.skip();
        let body = self.parse_statements_until(|token| *token == end_token)?;
        self.expect(&end_token)?;
        self.expect(&Token::Semicolon(0))?;
        Ok(body)
    }

    fn parse_stmt_in_scope(&mut self, outermost: bool) -> Result<Stmt, String> {
        let previous = self.outermost_scope;
        let previous_global_constant_scope = self.global_constant_scope;
        self.outermost_scope = outermost;
        self.global_constant_scope = outermost;
        let result = self.parse_stmt();
        self.outermost_scope = previous;
        self.global_constant_scope = previous_global_constant_scope;
        result
    }

    fn parse_stmt_in_namespace_scope(&mut self) -> Result<Stmt, String> {
        let previous_outermost = self.outermost_scope;
        let previous_global_constant_scope = self.global_constant_scope;
        self.outermost_scope = false;
        self.global_constant_scope = true;
        let result = self.parse_stmt();
        self.outermost_scope = previous_outermost;
        self.global_constant_scope = previous_global_constant_scope;
        result
    }

    fn consume_switch_label_separator(&mut self) -> Result<(), String> {
        match self.peek() {
            Token::Colon => {
                self.skip();
                Ok(())
            }
            Token::Semicolon(line) => {
                self.skip();
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
        if *self.peek_ref() == Token::QuestionQuestionAssign {
            return self.finish_coalesce_assign_statement(expr);
        }
        if Self::compound_assign_op(self.peek_ref()).is_some() {
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
