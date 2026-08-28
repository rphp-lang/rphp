impl Parser {
    /// Return the closest doc comment within one class-member declaration.
    /// PHP associates a docblock with only the first constant in a grouped
    /// declaration, so the caller deliberately consumes this value once.
    fn class_member_doc_comment(
        &self,
        start: usize,
        end: usize,
    ) -> Option<std::sync::Arc<str>> {
        let boundary = self
            .doc_comments
            .partition_point(|(position, _)| *position <= end);
        let (position, comment) = self.doc_comments[..boundary].last()?;
        (*position >= start).then(|| comment.clone())
    }

    fn closest_token_source_line(&self) -> usize {
        self.closest_token_source_line_before(self.pos)
    }

    fn closest_token_source_line_before(&self, position: usize) -> usize {
        self.tokens[..position]
            .iter()
            .rev()
            .find_map(|token| match token {
                Token::This(line)
                | Token::Variable(_, line)
                | Token::Identifier(_, line)
                | Token::Dollar(line)
                | Token::LParen(line)
                | Token::LBracket(line)
                | Token::LBrace(line)
                | Token::Fn(line)
                | Token::Use(line)
                | Token::Static(line)
                | Token::Abstract(line)
                | Token::Final(line)
                | Token::Enum { line, .. }
                | Token::Comma(line)
                | Token::Case(line)
                | Token::Default(line)
                | Token::DotDotDot(line)
                | Token::PipeGreater(line)
                | Token::Function(line)
                | Token::Exit { line, .. }
                | Token::Match(line)
                | Token::Yield(line)
                | Token::Clone(line)
                | Token::AttributeStart(line)
                | Token::ParseError(_, line)
                | Token::CompileError(_, line)
                | Token::CompileWarning(_, line)
                | Token::CompileDeprecation(_, line)
                | Token::MagicConstant { line, .. }
                | Token::Goto { line, .. }
                | Token::Echo { line }
                | Token::Return { line }
                | Token::Foreach { line } => Some(*line),
                Token::As(line) => Some(*line),
                Token::New(line) | Token::Throw(line) => Some(*line as usize),
                _ => None,
            })
            .unwrap_or(1)
    }

    fn following_semicolon_source_line(&self) -> Option<usize> {
        self.tokens[self.pos..].iter().find_map(|token| match token {
            Token::Semicolon(line) => Some(*line),
            _ => None,
        })
    }

    /// Consume PHP's marker for a function or method returning by reference.
    pub(super) fn consume_reference_return_marker(&mut self) {
        if self.peek() == Token::Ampersand {
            self.advance();
        }
    }

    fn peek(&self) -> Token {
        self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof)
    }

    fn peek_at(&self, offset: usize) -> Token {
        self.tokens
            .get(self.pos + offset)
            .cloned()
            .unwrap_or(Token::Eof)
    }

    fn expect_lbracket(&mut self) -> Result<usize, String> {
        match self.advance() {
            Token::LBracket(line) => Ok(line),
            token => Err(format!("Expected LBracket, got {token:?}")),
        }
    }

    fn expect_lparen(&mut self) -> Result<usize, String> {
        match self.advance() {
            Token::LParen(line) => Ok(line),
            token => Err(format!("Expected LParen, got {token:?}")),
        }
    }

    fn parse_unset_target(&mut self) -> Result<Expr, String> {
        let previous = self.empty_dimension_unset_context;
        self.empty_dimension_unset_context = true;
        let result = self.parse_expr();
        self.empty_dimension_unset_context = previous;
        result
    }

    fn with_new_postfix_error_suffix<T>(
        &mut self,
        suffix: Option<&'static str>,
        parse: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let previous = std::mem::replace(&mut self.new_postfix_error_suffix, suffix);
        let result = parse(self);
        self.new_postfix_error_suffix = previous;
        result
    }

    fn parse_new_arguments(&mut self, line: usize) -> Result<(Vec<CallArg>, bool), String> {
        if !matches!(self.peek(), Token::LParen(_)) {
            return Ok((Vec::new(), false));
        }
        self.expect_lparen()?;
        if self.consume_first_class_callable_placeholder() {
            self.compile_error("Cannot create Closure for new expression", line);
            Ok((Vec::new(), true))
        } else {
            Ok((self.parse_call_args()?, true))
        }
    }

    fn parse_empty_dimension_target_prefix(&mut self) -> Result<Expr, String> {
        let previous = self.preserve_empty_dimension_suffix;
        self.preserve_empty_dimension_suffix = true;
        let result = self.parse_expr();
        self.preserve_empty_dimension_suffix = previous;
        result
    }

    fn parse_positional_call_argument(&mut self) -> Result<Expr, String> {
        let target = self.parse_empty_dimension_target_prefix()?;
        if !self.is_empty_array_dimension_suffix() {
            return Ok(target);
        }

        let bracket_line = self.expect_lbracket()?;
        self.expect(&Token::RBracket)?;
        let line = self.last_primary_line.unwrap_or(bracket_line);
        self.parse_postfix_chain(Expr::ArrayAppendArgument {
            target: Box::new(target),
            line,
        })
    }

    /// Parse a foreach key/value target while preserving one terminal empty
    /// dimension. In this write context `$target[]` is an append destination,
    /// not the ordinary read error produced by expression parsing.
    fn parse_foreach_target_expression(&mut self) -> Result<Expr, String> {
        let target = self.parse_empty_dimension_target_prefix()?;
        if !self.is_empty_array_dimension_suffix() {
            return Ok(target);
        }

        let bracket_line = self.expect_lbracket()?;
        self.expect(&Token::RBracket)?;
        let line = self.last_primary_line.unwrap_or(bracket_line);
        Ok(Expr::ArrayAppendArgument {
            target: Box::new(target),
            line,
        })
    }

    fn comma_list_error(&self, line: usize, expecting_closing_paren: bool) -> String {
        let expectation = if expecting_closing_paren {
            ", expecting \")\""
        } else {
            ""
        };
        self.source_error(
            &format!("syntax error, unexpected token \",\"{expectation}"),
            line,
        )
    }

    /// Consume the separator after one completed call-like list item.
    /// `false` means either no separator or one legal trailing comma; `true`
    /// means another item follows. A second comma is a distinct parser state
    /// that expects the closing parenthesis.
    fn comma_list_has_next(&mut self, line: usize) -> Result<bool, String> {
        if !matches!(self.peek(), Token::Comma(_)) {
            return Ok(false);
        }
        self.advance();
        if self.peek() == Token::RParen {
            return Ok(false);
        }
        if matches!(self.peek(), Token::Comma(_)) {
            return Err(self.comma_list_error(line, true));
        }
        Ok(true)
    }

    /// Parse comma-separated call arguments supporting both positional and
    /// named (PHP 8 `name: expr`) arguments.  The opening `(` must already
    /// be consumed; this method consumes everything up to and including the
    /// closing `)`.
    /// Try to extract a string name from the current token if it can serve as
    /// a named argument label. Returns Some(name) for Identifier and keyword
    /// tokens that PHP accepts as named arg labels (array, string, int, etc.).
    /// Any token that can serve as a named argument label.
    /// PHP allows all reserved words as named arg labels.
    fn token_as_named_arg_label(tok: &Token) -> Option<String> {
        match tok {
            Token::Identifier(n, _) => Some(n.clone()),
            // All keyword tokens — PHP accepts any reserved word as a named arg label
            Token::ArrayKw => Some("array".to_string()),
            Token::Null => Some("null".to_string()),
            Token::True => Some("true".to_string()),
            Token::False => Some("false".to_string()),
            Token::Match(_) => Some("match".to_string()),
            Token::Static(_) => Some("static".to_string()),
            Token::Function(_) => Some("function".to_string()),
            Token::Class => Some("class".to_string()),
            Token::New(_) => Some("new".to_string()),
            Token::Return { .. } => Some("return".to_string()),
            Token::Echo { .. } => Some("echo".to_string()),
            Token::If => Some("if".to_string()),
            Token::Else => Some("else".to_string()),
            Token::ElseIf => Some("elseif".to_string()),
            Token::While => Some("while".to_string()),
            Token::Do => Some("do".to_string()),
            Token::For => Some("for".to_string()),
            Token::Foreach { .. } => Some("foreach".to_string()),
            Token::As(_) => Some("as".to_string()),
            Token::Switch => Some("switch".to_string()),
            Token::Case(_) => Some("case".to_string()),
            Token::Default(_) => Some("default".to_string()),
            Token::EndIf => Some("endif".to_string()),
            Token::EndWhile => Some("endwhile".to_string()),
            Token::EndFor => Some("endfor".to_string()),
            Token::EndForeach => Some("endforeach".to_string()),
            Token::EndSwitch => Some("endswitch".to_string()),
            Token::Break { .. } => Some("break".to_string()),
            Token::Continue { .. } => Some("continue".to_string()),
            Token::Try => Some("try".to_string()),
            Token::Catch => Some("catch".to_string()),
            Token::Finally => Some("finally".to_string()),
            Token::Throw(_) => Some("throw".to_string()),
            Token::Instanceof => Some("instanceof".to_string()),
            Token::Abstract(_) => Some("abstract".to_string()),
            Token::Interface => Some("interface".to_string()),
            Token::Implements => Some("implements".to_string()),
            Token::Extends => Some("extends".to_string()),
            Token::Public => Some("public".to_string()),
            Token::Protected => Some("protected".to_string()),
            Token::Private => Some("private".to_string()),
            Token::Const => Some("const".to_string()),
            Token::Isset => Some("isset".to_string()),
            Token::Empty => Some("empty".to_string()),
            Token::Unset => Some("unset".to_string()),
            Token::Fn(_) => Some("fn".to_string()),
            Token::Use(_) => Some("use".to_string()),
            Token::Declare => Some("declare".to_string()),
            Token::Trait => Some("trait".to_string()),
            Token::Final(_) => Some("final".to_string()),
            Token::Enum { name, .. } => Some(name.clone()),
            Token::Namespace => Some("namespace".to_string()),
            Token::Yield(_) => Some("yield".to_string()),
            Token::From => Some("from".to_string()),
            Token::Global => Some("global".to_string()),
            Token::Print => Some("print".to_string()),
            Token::Clone(_) => Some("clone".to_string()),
            Token::Include => Some("include".to_string()),
            Token::IncludeOnce => Some("include_once".to_string()),
            Token::Require => Some("require".to_string()),
            Token::RequireOnce => Some("require_once".to_string()),
            Token::Exit { name, .. } => Some(name.clone()),
            Token::Goto { name, .. } => Some(name.clone()),
            Token::MagicConstant { name, .. } => Some(name.clone()),
            Token::LogicalAnd => Some("and".to_string()),
            Token::LogicalOr => Some("or".to_string()),
            Token::LogicalXor => Some("xor".to_string()),
            Token::Insteadof => Some("insteadof".to_string()),
            _ => None,
        }
    }

    fn parse_call_args(&mut self) -> Result<Vec<CallArg>, String> {
        let mut args: Vec<CallArg> = Vec::new();
        let mut seen_named = false;
        let mut seen_unpack = false;
        let call_line = self.last_primary_line.unwrap_or(1);
        if matches!(self.peek(), Token::Comma(_)) {
            return Err(self.comma_list_error(call_line, false));
        }
        if self.peek() != Token::RParen {
            loop {
                // Check for named argument: identifier-like token followed by Colon
                if let Some(_label) = Self::token_as_named_arg_label(&self.peek()) {
                    if self.peek_at(1) == Token::Colon {
                        let name = Self::token_as_named_arg_label(&self.advance()).unwrap();
                        self.advance(); // consume ':'
                        let value = self.with_new_postfix_error_suffix(
                            Some(", expecting \")\""),
                            |parser| parser.parse_expr(),
                        )?;
                        if let Expr::FunctionCall { name, line, .. } = &value
                            && name.eq_ignore_ascii_case("list")
                        {
                            return Err(self.source_error(
                                "syntax error, unexpected token \")\", expecting \"=\"",
                                *line,
                            ));
                        }
                        args.push(CallArg::Named { name, value });
                        seen_named = true;
                        if self.comma_list_has_next(call_line)? {
                            continue;
                        }
                        break;
                    }
                }
                // Argument unpacking has its own ordering rule. It must be
                // checked before the generic positional-after-named branch.
                if matches!(self.peek(), Token::DotDotDot(_)) {
                    if seen_named {
                        return Err(
                            "Cannot use argument unpacking after named arguments".to_string()
                        );
                    }
                    self.advance();
                    let expr = self.with_new_postfix_error_suffix(
                        Some(", expecting \")\""),
                        |parser| parser.parse_expr(),
                    )?;
                    if let Expr::FunctionCall { name, line, .. } = &expr
                        && name.eq_ignore_ascii_case("list")
                    {
                        return Err(self.source_error(
                            "syntax error, unexpected token \")\", expecting \"=\"",
                            *line,
                        ));
                    }
                    args.push(CallArg::Unpack(expr));
                    seen_unpack = true;
                    if self.comma_list_has_next(call_line)? {
                        continue;
                    }
                    break;
                }
                // Positional argument
                if seen_named {
                    self.compile_error(
                        "Cannot use positional argument after named argument",
                        call_line,
                    );
                }
                if seen_unpack {
                    return Err(
                        "Cannot use positional argument after argument unpacking".to_string()
                    );
                }
                let expr = self.with_new_postfix_error_suffix(
                    Some(", expecting \")\""),
                    |parser| parser.parse_positional_call_argument(),
                )?;
                if let Expr::FunctionCall { name, line, .. } = &expr
                    && name.eq_ignore_ascii_case("list")
                {
                    return Err(self.source_error(
                        "syntax error, unexpected token \")\", expecting \"=\"",
                        *line,
                    ));
                }
                args.push(CallArg::Positional(expr));
                if self.comma_list_has_next(call_line)? {
                    continue;
                }
                break;
            }
        }
        self.expect(&Token::RParen)?;
        Ok(args)
    }

    fn parse_attribute_groups(&mut self) -> Result<Vec<Attribute>, String> {
        let mut attributes = Vec::new();
        while let Token::AttributeStart(line) = self.peek() {
            self.advance();
            loop {
                let name = self.parse_qualified_name()?;
                let args = if matches!(self.peek(), Token::LParen(_)) {
                    self.advance();
                    if matches!(self.peek(), Token::DotDotDot(_))
                        && self.peek_at(1) == Token::RParen
                    {
                        self.advance();
                        self.expect(&Token::RParen)?;
                        self.compile_error("Cannot create Closure as attribute argument", line);
                        Vec::new()
                    } else {
                        self.parse_call_args()?
                    }
                } else {
                    Vec::new()
                };
                if args.iter().any(|arg| matches!(arg, CallArg::Unpack(_))) {
                    self.compile_error(
                        "Cannot use unpacking in attribute argument list",
                        line,
                    );
                }
                attributes.push(Attribute { name, args, line });
                if !matches!(self.peek(), Token::Comma(_)) {
                    break;
                }
                self.advance();
                if self.peek() == Token::RBracket {
                    break;
                }
            }
            self.expect(&Token::RBracket)?;
        }
        Ok(attributes)
    }

    fn attach_attributes(
        &mut self,
        mut statement: Stmt,
        attributes: Vec<Attribute>,
    ) -> Result<Stmt, String> {
        let attribute_line = attributes.first().map_or(1, |attribute| attribute.line);
        let allow_dynamic_properties = attributes.iter().any(|attribute| {
            attribute
                .name
                .strip_prefix('\\')
                .unwrap_or(&attribute.name)
                .eq_ignore_ascii_case("AllowDynamicProperties")
        });
        match &mut statement {
            Stmt::Function {
                attributes: target,
                ..
            }
            | Stmt::Class {
                attributes: target,
                ..
            }
            | Stmt::Interface {
                attributes: target,
                ..
            }
            | Stmt::Trait {
                attributes: target,
                ..
            }
            | Stmt::Enum {
                attributes: target,
                ..
            }
            | Stmt::Const {
                attributes: target,
                ..
            } => *target = attributes,
            Stmt::ExprStmt(Expr::Closure {
                attributes: target,
                ..
            })
            | Stmt::ExprStmt(Expr::AnonymousNew {
                attributes: target,
                ..
            }) => *target = attributes,
            _ => {
                return Err(self.source_error(
                    "syntax error, unexpected token \"#[\"",
                    attribute_line,
                ));
            }
        }

        if allow_dynamic_properties {
            let invalid_target = match &mut statement {
                Stmt::Class {
                    name,
                    is_readonly: true,
                    ..
                } => Some(format!("readonly class {name}")),
                Stmt::Class {
                    allow_dynamic_properties,
                    ..
                } => {
                    *allow_dynamic_properties = true;
                    None
                }
                Stmt::Interface { name, .. } => Some(format!("interface {name}")),
                Stmt::Trait { name, .. } => Some(format!("trait {name}")),
                Stmt::Enum { name, .. } => Some(format!("enum {name}")),
                _ => None,
            };
            if let Some(target) = invalid_target {
                return Ok(Stmt::ExprStmt(Expr::CompileError {
                    message: format!("Cannot apply #[\\AllowDynamicProperties] to {target}"),
                    line: attribute_line,
                }));
            }
        }
        Ok(statement)
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek();
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), String> {
        let tok = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            Ok(())
        } else if self.halted && tok == Token::Eof {
            Ok(())
        } else if let Token::ParseError(message, line) = &tok {
            Err(self.source_error(message, *line))
        } else if let Token::LBrace(line) = tok
            && matches!(expected, Token::RParen)
        {
            Err(self.source_error(
                "syntax error, unexpected token \"{\", expecting \")\"",
                line,
            ))
        } else if let Token::Identifier(name, line) = &tok
            && self.source_name.is_some()
        {
            Err(self.source_error(
                &format!("syntax error, unexpected identifier \"{name}\""),
                *line,
            ))
        } else {
            let expected = match expected {
                Token::Semicolon(_) => "Semicolon".to_string(),
                Token::LBrace(_) => "LBrace".to_string(),
                token => format!("{token:?}"),
            };
            let actual = match tok {
                Token::LBrace(_) => "LBrace".to_string(),
                token => format!("{token:?}"),
            };
            Err(format!("Expected {expected}, got {actual}"))
        }
    }

    fn at_eof(&self) -> bool {
        self.peek() == Token::Eof
    }

    /// Parse a comma-separated list of function parameters with optional defaults.
    /// Expects the opening '(' to already be consumed; stops before ')'.
    fn parse_param_list(&mut self) -> Result<Vec<Param>, String> {
        let mut params = Vec::new();
        if self.is_param_start() {
            params.push(self.parse_one_param()?);
            while matches!(self.peek(), Token::Comma(_)) {
                self.advance();
                // Allow trailing comma before closing paren
                if !self.is_param_start() {
                    break;
                }
                params.push(self.parse_one_param()?);
            }
        }
        Ok(params)
    }

    /// Check if the current token can start a parameter declaration.
    /// Matches: type hints (identifiers, ?, array, null), &, ..., $var
    fn is_param_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::Variable(_, _)
                | Token::This(_)
                | Token::DotDotDot(_)
                | Token::Ampersand
                | Token::Question
                | Token::Backslash
                | Token::Namespace
                | Token::ArrayKw
                | Token::Null
                | Token::True
                | Token::False
                | Token::Static(_)
                | Token::LParen(_)
                | Token::Identifier(_, _)
                | Token::Enum { .. }
                | Token::Public
                | Token::Protected
                | Token::Private
                | Token::Final(_)
                | Token::AttributeStart(_)
        )
    }

    /// Parse an optional return type hint after `)`.
    /// If the next token is `:`, consume it and parse a type hint.
    /// Parse a qualified name like `App\Models\User` or `\App\Models\User`.
    /// Consumes Identifier (Backslash Identifier)* tokens.
    /// May start with a leading backslash for fully qualified names.
    fn parse_qualified_name(&mut self) -> Result<String, String> {
        let mut parts = Vec::new();
        // Optional leading backslash (fully qualified)
        let leading_bs = if self.peek() == Token::Backslash {
            self.advance();
            true
        } else {
            false
        };
        match self.advance() {
            Token::Identifier(n, line) | Token::Enum { name: n, line } => {
                self.last_primary_line = Some(line);
                parts.push(n);
            }
            Token::True => parts.push("true".to_string()),
            Token::False => parts.push("false".to_string()),
            Token::Null => parts.push("null".to_string()),
            other => {
                return Err(format!(
                    "Expected identifier in qualified name, got {:?}",
                    other
                ));
            }
        }
        while self.peek() == Token::Backslash {
            self.advance(); // consume '\'
            match self.advance() {
                Token::Identifier(n, _) | Token::Enum { name: n, .. } => parts.push(n),
                Token::True => parts.push("true".to_string()),
                Token::False => parts.push("false".to_string()),
                Token::Null => parts.push("null".to_string()),
                other => {
                    return Err(format!(
                        "Expected identifier after '\\' in qualified name, got {:?}",
                        other
                    ));
                }
            }
        }
        let name = parts.join("\\");
        if leading_bs {
            Ok(format!("\\{}", name))
        } else {
            Ok(name)
        }
    }

    /// Parse PHP's explicit namespace-relative form, `namespace\\Name`.
    /// The marker is retained so resolution can bypass ordinary imports.
    fn parse_namespace_relative_name(&mut self) -> Result<String, String> {
        self.expect(&Token::Namespace)?;
        self.expect(&Token::Backslash)?;
        let first = match self.advance() {
            Token::Identifier(name, line) | Token::Enum { name, line } => {
                self.last_primary_line = Some(line);
                name
            }
            Token::True => "true".to_string(),
            Token::False => "false".to_string(),
            Token::Null => "null".to_string(),
            other => {
                return Err(format!(
                    "Expected identifier after 'namespace\\\\', got {:?}",
                    other
                ));
            }
        };
        Ok(format!(
            "namespace\\{}",
            self.parse_type_name_tail(first, false)?
        ))
    }

    /// Parse a qualified name where PHP also admits the explicit
    /// `namespace\Name` form.
    fn parse_qualified_or_namespace_relative_name(&mut self) -> Result<String, String> {
        if self.peek() == Token::Namespace && self.peek_at(1) == Token::Backslash {
            self.parse_namespace_relative_name()
        } else {
            self.parse_qualified_name()
        }
    }

    /// Parse the left-hand reference of a trait adaptation. The unqualified
    /// form is a method name and therefore admits PHP's semi-reserved words;
    /// qualified and namespace-relative forms identify a trait before `::`.
    fn parse_trait_method_reference(
        &mut self,
        adaptation_line: usize,
    ) -> Result<(Option<String>, String), String> {
        let first = match self.peek() {
            Token::Namespace if self.peek_at(1) == Token::Backslash => {
                self.parse_qualified_or_namespace_relative_name()?
            }
            Token::Backslash | Token::Identifier(_, _) | Token::Enum { .. } => {
                self.parse_qualified_name()?
            }
            Token::Static(line) if self.peek_at(1) == Token::DoubleColon => {
                self.advance();
                self.last_primary_line = Some(line);
                self.compile_error(ReservedStaticRole::Trait.diagnostic(), adaptation_line);
                "static".to_string()
            }
            _ => {
                let token = self.advance();
                let method = Self::token_as_named_arg_label(&token)
                    .ok_or_else(|| format!("Expected trait method name, got {token:?}"))?;
                return Ok((None, method));
            }
        };

        if self.peek() != Token::DoubleColon {
            return Ok((None, first));
        }
        self.advance();
        let token = self.advance();
        let method = Self::token_as_named_arg_label(&token)
            .ok_or_else(|| format!("Expected trait method name, got {token:?}"))?;
        Ok((Some(first), method))
    }

    /// Parse the leading name of a use declaration, retaining the `\{`
    /// boundary that distinguishes a group import from an ordinary qualified
    /// name. Whitespace is already absent from the token stream.
    fn parse_use_name(&mut self) -> Result<(String, bool, usize), String> {
        let leading_backslash = if self.peek() == Token::Backslash {
            self.advance();
            true
        } else {
            false
        };
        let (mut parts, name_line) = match self.advance() {
            Token::Identifier(name, line) | Token::Enum { name, line } => (vec![name], line),
            Token::Exit { line, .. } => {
                return Err(self.source_error(
                    "syntax error, unexpected token \"exit\", expecting identifier or fully qualified name or namespaced name",
                    line,
                ));
            }
            other => {
                return Err(format!(
                    "Expected identifier in use declaration, got {:?}",
                    other
                ));
            }
        };
        while self.peek() == Token::Backslash {
            self.advance();
            if matches!(self.peek(), Token::LBrace(_)) {
                self.advance();
                let mut prefix = parts.join("\\");
                if leading_backslash {
                    prefix.insert(0, '\\');
                }
                return Ok((prefix, true, name_line));
            }
            match self.advance() {
                Token::Identifier(name, _) | Token::Enum { name, .. } => parts.push(name),
                other => {
                    return Err(format!(
                        "Expected identifier after '\\' in use declaration, got {:?}",
                        other
                    ));
                }
            }
        }
        let mut name = parts.join("\\");
        if leading_backslash {
            name.insert(0, '\\');
        }
        Ok((name, false, name_line))
    }

    fn consume_as_keyword(&mut self) -> bool {
        let is_alias = matches!(self.peek(), Token::As(_))
            || matches!(self.peek(), Token::Identifier(name, _) if name.eq_ignore_ascii_case("as"));
        if is_alias {
            self.advance();
        }
        is_alias
    }

    fn parse_return_type(
        &mut self,
        declaration_line: usize,
        bindable_closure: bool,
    ) -> Result<Option<TypeHint>, String> {
        if self.peek() == Token::Colon {
            self.advance(); // consume ':'
            // Handle nullable return types: ?: type
            let hint = if self.peek() == Token::Question {
                self.advance(); // consume '?'
                let inner = self.parse_base_type_hint()?;
                TypeHint::Nullable(Box::new(inner))
            } else {
                let hint = self.parse_base_type_hint()?;
                self.maybe_parse_compound_type(hint)?
            };
            if Self::type_hint_uses_static(&hint)
                && !self.class_scope_active
                && !bindable_closure
            {
                self.compile_error(
                    "Cannot use \"static\" when no class scope is active",
                    declaration_line,
                );
            }
            Ok(Some(hint))
        } else {
            Ok(None)
        }
    }

    /// Parse intersections before unions so `A&B|C` follows PHP's type
    /// precedence. An ampersand immediately followed by a variable remains a
    /// by-reference marker and is not consumed here.
    fn maybe_parse_compound_type(&mut self, first: TypeHint) -> Result<TypeHint, String> {
        let first = self.maybe_parse_intersection_type(first)?;
        if self.peek() != Token::Pipe {
            return Ok(first);
        }
        let mut types = vec![first];
        while self.peek() == Token::Pipe {
            self.advance(); // consume '|'
            let t = self.parse_base_type_hint()?;
            types.push(self.maybe_parse_intersection_type(t)?);
        }
        Ok(TypeHint::Union(types))
    }

    fn maybe_parse_intersection_type(&mut self, first: TypeHint) -> Result<TypeHint, String> {
        let mut types = vec![first];
        while self.peek() == Token::Ampersand
            && matches!(
                self.tokens.get(self.pos + 1),
                Some(Token::Identifier(_, _))
                    | Some(Token::Enum { .. })
                    | Some(Token::Backslash)
                    | Some(Token::Namespace)
                    | Some(Token::ArrayKw)
                    | Some(Token::Null)
                    | Some(Token::Static(_))
            )
        {
            self.advance();
            types.push(self.parse_base_type_hint()?);
        }
        if types.len() == 1 {
            Ok(types.pop().expect("one parsed type"))
        } else {
            Ok(TypeHint::Intersection(types))
        }
    }

    /// Check if the current token could be the start of a type hint (without consuming tokens).
    fn is_type_hint_start(&self) -> bool {
        match self.peek() {
            Token::Question => {
                matches!(
                    self.tokens.get(self.pos + 1),
                    Some(Token::Identifier(_, _))
                        | Some(Token::Enum { .. })
                        | Some(Token::Backslash)
                        | Some(Token::Namespace)
                        | Some(Token::ArrayKw)
                        | Some(Token::Null)
                        | Some(Token::True)
                        | Some(Token::False)
                        | Some(Token::Static(_))
                )
            }
            Token::Identifier(_, _) => {
                if self.peek_at(1) == Token::Less {
                    return true;
                }
                matches!(
                    self.tokens.get(self.pos + 1),
                    Some(Token::Variable(_, _)) | Some(Token::Pipe) | Some(Token::Ampersand)
                )
            }
            Token::Enum { .. } => {
                matches!(
                    self.tokens.get(self.pos + 1),
                    Some(Token::Variable(_, _))
                        | Some(Token::Pipe)
                        | Some(Token::Ampersand)
                        | Some(Token::Backslash)
                        | Some(Token::Less)
                )
            }
            Token::Backslash | Token::Namespace => true,
            Token::ArrayKw | Token::Null | Token::True | Token::False => {
                matches!(
                    self.tokens.get(self.pos + 1),
                    Some(Token::Variable(_, _)) | Some(Token::Pipe) | Some(Token::Ampersand)
                )
            }
            // `static` is a return-only PHP type. Parameter/property parsing
            // enters the parameter parser only so it can emit the precise
            // return-only diagnostic. Class-member modifiers are consumed
            // before this lookahead runs.
            Token::Static(_) => matches!(
                self.tokens.get(self.pos + 1),
                Some(Token::Less)
                    | Some(Token::Variable(_, _))
                    | Some(Token::Pipe)
                    | Some(Token::Ampersand)
            ),
            // PHP DNF types parenthesize intersection arms, for example
            // `(Countable&Iterator)|null`.
            Token::LParen(_) => true,
            _ => false,
        }
    }

    /// Try to parse a type hint at the start of a parameter.
    /// Returns None if no type hint is present (next token is $var, &, or ...).
    fn finish_non_return_type_hint(
        &mut self,
        hint: TypeHint,
        parameter: bool,
        line: usize,
    ) -> Result<Option<TypeHint>, String> {
        if !Self::type_hint_uses_static(&hint) {
            return Ok(Some(hint));
        }
        if Self::type_hint_uses_static_generic_application(&hint) {
            return Err("static is only allowed as a return type".to_string());
        }
        if parameter {
            self.compile_error("Cannot use the static modifier on a parameter", line);
            Ok(Some(hint))
        } else {
            Err(self.source_error("syntax error, unexpected token \"static\"", line))
        }
    }

    fn try_parse_type_hint(&mut self, parameter: bool) -> Result<Option<TypeHint>, String> {
        let declaration_line = self.tokens[self.pos..]
            .iter()
            .find_map(|token| match token {
                Token::Variable(_, line) | Token::This(line) => Some(*line),
                _ => None,
            })
            .unwrap_or_else(|| self.closest_token_source_line());
        // Nullable: ?type
        if self.peek() == Token::Question {
            // Peek ahead: ?$var or ?... means ternary/other, not type hint
            // In param context, ?Identifier or ?ArrayKw means nullable type
            let next = self.tokens.get(self.pos + 1);
            if matches!(next, Some(Token::Static(_))) {
                return Err(self.source_error(
                    "syntax error, unexpected token \"static\"",
                    declaration_line,
                ));
            }
            let is_type = matches!(
                next,
                Some(Token::Identifier(_, _))
                    | Some(Token::Enum { .. })
                    | Some(Token::Backslash)
                    | Some(Token::Namespace)
                    | Some(Token::ArrayKw)
                    | Some(Token::Null)
                    | Some(Token::True)
                    | Some(Token::False)
            );
            if is_type {
                self.advance(); // consume '?'
                let inner = self.parse_base_type_hint()?;
                return Ok(Some(TypeHint::Nullable(Box::new(inner))));
            }
            return Ok(None);
        }
        // Check if current token looks like a type hint
        // Disambiguate: Identifier followed by $var, &, or ... means it's a type hint
        // Identifier NOT followed by those means it's not a type hint (shouldn't happen in param context)
        match self.peek() {
            Token::Identifier(_, _) | Token::Enum { .. } => {
                let next = self.tokens.get(self.pos + 1);
                let is_type_context = matches!(
                    next,
                    Some(Token::Variable(_, _))
                        | Some(Token::Ampersand)
                        | Some(Token::DotDotDot(_))
                        | Some(Token::Pipe)
                        | Some(Token::Backslash)
                );
                let is_type_context = is_type_context || matches!(next, Some(Token::Less));
                if is_type_context {
                    let hint = self.parse_base_type_hint()?;
                    let hint = self.maybe_parse_compound_type(hint)?;
                    return self.finish_non_return_type_hint(
                        hint,
                        parameter,
                        declaration_line,
                    );
                }
                Ok(None)
            }
            Token::Namespace => {
                let hint = self.parse_base_type_hint()?;
                let hint = self.maybe_parse_compound_type(hint)?;
                self.finish_non_return_type_hint(hint, parameter, declaration_line)
            }
            Token::Backslash => {
                let hint = self.parse_base_type_hint()?;
                let hint = self.maybe_parse_compound_type(hint)?;
                self.finish_non_return_type_hint(hint, parameter, declaration_line)
            }
            Token::ArrayKw | Token::Null | Token::True | Token::False => {
                let next = self.tokens.get(self.pos + 1);
                let is_type_context = matches!(
                    next,
                    Some(Token::Variable(_, _))
                        | Some(Token::Ampersand)
                        | Some(Token::DotDotDot(_))
                        | Some(Token::Pipe)
                );
                if is_type_context {
                    let hint = self.parse_base_type_hint()?;
                    let hint = self.maybe_parse_compound_type(hint)?;
                    return self.finish_non_return_type_hint(
                        hint,
                        parameter,
                        declaration_line,
                    );
                }
                Ok(None)
            }
            Token::Static(_) => {
                let hint = self.parse_base_type_hint()?;
                let hint = self.maybe_parse_compound_type(hint)?;
                self.finish_non_return_type_hint(hint, parameter, declaration_line)
            }
            Token::LParen(_) => {
                let hint = self.parse_base_type_hint()?;
                let hint = self.maybe_parse_compound_type(hint)?;
                self.finish_non_return_type_hint(hint, parameter, declaration_line)
            }
            _ => Ok(None),
        }
    }

    /// Parse a non-nullable type hint (int, string, float, bool, array, ClassName).
    fn parse_base_type_hint(&mut self) -> Result<TypeHint, String> {
        match self.advance() {
            Token::Identifier(name, _) => match name.as_str() {
                "int" | "integer" => Ok(TypeHint::Int),
                "float" | "double" => Ok(TypeHint::Float),
                "string" => Ok(TypeHint::String),
                "bool" | "boolean" => Ok(TypeHint::Bool),
                "callable" => Ok(TypeHint::Callable),
                "null" => Ok(TypeHint::Null),
                "void" => Ok(TypeHint::Void),
                "mixed" => Ok(TypeHint::Mixed),
                "never" => Ok(TypeHint::Never),
                _ => {
                    let name = self.parse_type_name_tail(name, false)?;
                    if self.peek() == Token::Less {
                        if !cfg!(any(
                            feature = "php-generics-erased",
                            feature = "php-generics-reified"
                        )) {
                            return Err(
                                "Generic syntax requires php-generics-erased or php-generics-reified"
                                    .to_string(),
                            );
                        }
                        let arguments = self.parse_generic_type_arguments()?;
                        return Ok(TypeHint::GenericApplication {
                            base: name,
                            arguments,
                        });
                    }
                    if let Some(hint) = self.generic_parameter_type_hint(&name) {
                        return Ok(hint);
                    }
                    Ok(TypeHint::ClassName(name))
                }
            },
            Token::Enum { name, .. } => {
                let name = self.parse_type_name_tail(name, false)?;
                Ok(TypeHint::ClassName(name))
            }
            Token::Namespace => {
                self.expect(&Token::Backslash)?;
                let first = match self.advance() {
                    Token::Identifier(name, _) | Token::Enum { name, .. } => name,
                    other => {
                        return Err(format!(
                            "Expected identifier after 'namespace\\\\' in type hint, got {:?}",
                            other
                        ));
                    }
                };
                let name = self.parse_type_name_tail(first, false)?;
                Ok(TypeHint::ClassName(format!("namespace\\{name}")))
            }
            Token::Backslash => {
                let first = match self.advance() {
                    Token::Identifier(name, _) | Token::Enum { name, .. } => name,
                    other => {
                        return Err(format!(
                            "Expected identifier after leading '\\' in type hint, got {:?}",
                            other
                        ));
                    }
                };
                let name = self.parse_type_name_tail(first, true)?;
                if self.peek() == Token::Less {
                    if !cfg!(any(
                        feature = "php-generics-erased",
                        feature = "php-generics-reified"
                    )) {
                        return Err(
                            "Generic syntax requires php-generics-erased or php-generics-reified"
                                .to_string(),
                        );
                    }
                    let arguments = self.parse_generic_type_arguments()?;
                    Ok(TypeHint::GenericApplication {
                        base: name,
                        arguments,
                    })
                } else {
                    Ok(TypeHint::ClassName(name))
                }
            }
            Token::ArrayKw => Ok(TypeHint::Array),
            Token::Null => Ok(TypeHint::Null),
            Token::True => Ok(TypeHint::ClassName("true".to_string())),
            Token::False => Ok(TypeHint::ClassName("false".to_string())),
            Token::Static(_) => {
                if self.peek() == Token::Less {
                    if !cfg!(any(
                        feature = "php-generics-erased",
                        feature = "php-generics-reified"
                    )) {
                        return Err(
                            "Generic syntax requires php-generics-erased or php-generics-reified"
                                .to_string(),
                        );
                    }
                    let arguments = self.parse_generic_type_arguments()?;
                    Ok(TypeHint::GenericApplication {
                        base: "static".to_string(),
                        arguments,
                    })
                } else {
                    Ok(TypeHint::ClassName("static".to_string()))
                }
            }
            Token::LParen(_) => {
                let first = self.parse_base_type_hint()?;
                let intersection = self.maybe_parse_intersection_type(first)?;
                self.expect(&Token::RParen)?;
                Ok(intersection)
            }
            other => Err(format!("Expected type hint, got {:?}", other)),
        }
    }

    fn parse_type_name_tail(
        &mut self,
        first: String,
        fully_qualified: bool,
    ) -> Result<String, String> {
        let mut parts = vec![first];
        while self.peek() == Token::Backslash {
            self.advance();
            match self.advance() {
                Token::Identifier(name, _) | Token::Enum { name, .. } => parts.push(name),
                other => {
                    return Err(format!(
                        "Expected identifier after '\\' in type hint, got {:?}",
                        other
                    ));
                }
            }
        }
        let name = parts.join("\\");
        Ok(if fully_qualified {
            format!("\\{name}")
        } else {
            name
        })
    }

    fn type_hint_uses_static(hint: &TypeHint) -> bool {
        match hint {
            TypeHint::ClassName(name) => name.eq_ignore_ascii_case("static"),
            TypeHint::GenericApplication { base, arguments } => {
                base.eq_ignore_ascii_case("static")
                    || arguments.iter().any(Self::type_hint_uses_static)
            }
            TypeHint::Nullable(inner) => Self::type_hint_uses_static(inner),
            TypeHint::Union(parts) | TypeHint::Intersection(parts) => {
                parts.iter().any(Self::type_hint_uses_static)
            }
            TypeHint::GenericParameter { erased, .. } => Self::type_hint_uses_static(erased),
            _ => false,
        }
    }

    fn type_hint_uses_static_generic_application(hint: &TypeHint) -> bool {
        match hint {
            TypeHint::GenericApplication { base, arguments } => {
                base.eq_ignore_ascii_case("static")
                    || arguments
                        .iter()
                        .any(Self::type_hint_uses_static_generic_application)
            }
            TypeHint::Nullable(inner) | TypeHint::GenericParameter { erased: inner, .. } => {
                Self::type_hint_uses_static_generic_application(inner)
            }
            TypeHint::Union(parts) | TypeHint::Intersection(parts) => parts
                .iter()
                .any(Self::type_hint_uses_static_generic_application),
            _ => false,
        }
    }

    fn parse_one_param(&mut self) -> Result<Param, String> {
        let attributes = self.parse_attribute_groups()?;
        // Check for constructor property promotion: visibility keyword before type hint
        let mut promotion: Option<(Visibility, Option<Visibility>, bool)> = None;
        let mut promo_readonly = false;
        let mut promo_final = false;
        let mut promo_visibility = None;
        let mut promo_set_visibility = None;
        loop {
            match self.peek() {
                Token::Public | Token::Protected | Token::Private => {
                    let vis = match self.advance() {
                        Token::Public => Visibility::Public,
                        Token::Protected => Visibility::Protected,
                        Token::Private => Visibility::Private,
                        _ => unreachable!(),
                    };
                    if matches!(self.peek(), Token::LParen(_)) {
                        self.advance();
                        match self.advance() {
                            Token::Identifier(name, _) if name.eq_ignore_ascii_case("set") => {}
                            other => {
                                return Err(format!(
                                    "Expected set in asymmetric visibility, got {other:?}"
                                ));
                            }
                        }
                        self.expect(&Token::RParen)?;
                        promo_set_visibility = Some(vis);
                        continue;
                    }
                    promo_visibility = Some(vis);
                    if matches!(self.peek(), Token::Identifier(ref s, _) if s == "readonly") {
                        self.advance();
                        promo_readonly = true;
                    }
                    continue;
                }
                Token::Identifier(ref s, _) if s == "readonly" && promo_visibility.is_some() => {
                    self.advance();
                    promo_readonly = true;
                    continue;
                }
                Token::Final(_) => {
                    self.advance();
                    promo_final = true;
                    continue;
                }
                _ => break,
            }
        }
        if promo_visibility.is_some() || promo_set_visibility.is_some() || promo_final {
            promotion = Some((
                promo_visibility.unwrap_or(Visibility::Public),
                promo_set_visibility,
                promo_readonly,
            ));
        }
        // Optional type hint before &, ..., $var
        let type_hint = self.try_parse_type_hint(true)?;
        // Optional & prefix for pass-by-reference
        let is_ref = if self.peek() == Token::Ampersand {
            self.advance(); // consume '&'
            true
        } else {
            false
        };
        let is_variadic = if matches!(self.peek(), Token::DotDotDot(_)) {
            self.advance(); // consume '...'
            true
        } else {
            false
        };
        let (name, line) = match self.advance() {
            Token::Variable(n, line) => (n, line),
            Token::This(line) => ("this".to_string(), line),
            other => return Err(format!("Expected parameter variable, got {:?}", other)),
        };
        let default = if self.peek() == Token::Assign {
            self.advance(); // consume '='
            Some(self.parse_expr()?)
        } else {
            None
        };
        let has_hooks = matches!(self.peek(), Token::LBrace(_));
        if has_hooks && promotion.is_none() {
            promotion = Some((Visibility::Public, None, false));
        }
        let mut promoted_property = promotion.map(|(visibility, set_visibility, is_readonly)| {
            ClassProperty {
                attributes: attributes.clone(),
                line,
                visibility,
                set_visibility,
                name: name.clone(),
                type_hint: type_hint.clone(),
                default: None,
                is_static: false,
                is_readonly,
                is_final: promo_final,
                is_abstract: false,
                has_get_hook: false,
                has_abstract_get_hook: false,
                has_set_hook: false,
                has_abstract_set_hook: false,
            }
        });
        let promotion_hooks = if has_hooks {
            self.parse_promoted_property_hook_list(
                promoted_property
                    .as_mut()
                    .expect("hook syntax creates a promoted property"),
            )?
        } else {
            Vec::new()
        };
        Ok(Param {
            attributes,
            name,
            line,
            default,
            is_variadic,
            is_ref,
            type_hint,
            promotion,
            promoted_property,
            promotion_hooks,
        })
    }

    fn variable_expression(name: String, line: usize) -> Expr {
        if name == "GLOBALS" {
            Expr::Globals { line }
        } else {
            Expr::Variable { name, line }
        }
    }

    fn compile_error(&mut self, message: impl Into<String>, line: usize) -> Expr {
        let message = message.into();
        if self.deferred_compile_error.is_none() {
            self.deferred_compile_error = Some((message.clone(), line));
        }
        Expr::CompileError { message, line }
    }

    #[cold]
    #[inline(never)]
    fn invalid_this_binding(&mut self, role: &str, line: usize) -> String {
        // Reuse the established static-binding diagnostic storage and replace
        // only its role on this invalid path. Keeping message construction
        // outlined avoids enlarging ordinary statement/closure parser blocks.
        let mut message = "Cannot use $this as static variable".to_string();
        message.replace_range(20..26, role);
        self.compile_error(message, line);
        "this".to_string()
    }

    fn globals_modification_error(&mut self, line: usize) -> Expr {
        self.compile_error(
            "$GLOBALS can only be modified using the $GLOBALS[$name] = $value syntax",
            line,
        )
    }

    fn nullsafe_chain_line(expr: &Expr) -> Option<usize> {
        match expr {
            Expr::PropertyAccess {
                object,
                nullsafe,
                line,
                ..
            }
            | Expr::DynamicPropertyAccess {
                object,
                nullsafe,
                line,
                ..
            }
            | Expr::MethodCall {
                object,
                nullsafe,
                line,
                ..
            } => {
                if *nullsafe {
                    Some(*line)
                } else {
                    Self::nullsafe_chain_line(object)
                }
            }
            Expr::ArrayAccess { array, .. } => Self::nullsafe_chain_line(array),
            Expr::DynamicStaticProperty { class, .. }
            | Expr::DynamicStaticCall { class, .. } => Self::nullsafe_chain_line(class),
            _ => None,
        }
    }

    fn nullsafe_write_error(&mut self, line: usize) -> Expr {
        self.compile_error("Can't use nullsafe operator in write context", line)
    }

    fn call_write_error(&mut self, target: &Expr) -> Option<Expr> {
        let (kind, line) = match target {
            // `list(...)` can still reach expression parsing in nested list
            // assignment positions. It is a contextual language construct,
            // not an ordinary function call result.
            Expr::FunctionCall { name, .. } if name.eq_ignore_ascii_case("list") => return None,
            Expr::FunctionCall { line, .. } => ("function", *line),
            Expr::DynamicCall {
                method_syntax,
                line,
                ..
            } => (
                if *method_syntax {
                    "method"
                } else {
                    "function"
                },
                *line,
            ),
            Expr::MethodCall { line, .. }
            | Expr::StaticCall { line, .. }
            | Expr::DynamicStaticCall { line, .. } => ("method", *line),
            _ => return None,
        };
        Some(self.compile_error(
            format!("Can't use {kind} return value in write context"),
            line,
        ))
    }

    /// PHP permits an ordinary call result to be used as the temporary array
    /// container in `call()[] = value`. Whether the result is detached or
    /// aliases caller-visible storage is decided by the callee's return mode
    /// at runtime, not by the parser.
    fn is_call_result(target: &Expr) -> bool {
        match target {
            // `list(...)` is a contextual language construct that can still
            // reach expression parsing in malformed assignment positions.
            Expr::FunctionCall { name, .. } => !name.eq_ignore_ascii_case("list"),
            Expr::DynamicCall { .. }
            | Expr::MethodCall { .. }
            | Expr::StaticCall { .. }
            | Expr::DynamicStaticCall { .. } => true,
            _ => false,
        }
    }

    /// PHP permits mutable variables/properties and ordinary call results as
    /// array-write roots. Other expression results are non-writeable
    /// temporaries; `clone` is the one non-call shape that uses PHP's
    /// built-in-result diagnostic instead of the generic temporary wording.
    fn array_write_root_error(&self, target: &Expr) -> Option<(&'static str, usize)> {
        let fallback_line = match target {
            Expr::ArrayAccess { line, .. } | Expr::ArrayAppendArgument { line, .. } => *line,
            _ => 0,
        };
        let mut root = target;
        while let Expr::ArrayAccess { array, .. } = root {
            root = array;
        }

        if matches!(
            root,
            Expr::Variable { .. }
                | Expr::DynamicVariable { .. }
                | Expr::Globals { .. }
                | Expr::CompileError { .. }
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
        ) || Self::is_call_result(root)
        {
            return None;
        }

        let line = match root {
            Expr::Clone { line, .. }
            | Expr::New { line, .. }
            | Expr::DynamicNew { line, .. }
            | Expr::ClassConstant { line, .. }
            | Expr::MagicConstant { line, .. }
            | Expr::Match { line, .. } => *line,
            _ => self.last_primary_line.unwrap_or(fallback_line),
        };
        let message = if matches!(root, Expr::Clone { .. }) {
            "Cannot use result of built-in function in write context"
        } else {
            "Cannot use temporary expression in write context"
        };
        Some((message, line))
    }

    fn nullsafe_reference_error(&mut self, line: usize) -> Expr {
        self.compile_error("Cannot take reference of a nullsafe chain", line)
    }

    /// Check if an expression is a variable-like target (valid for isset/empty/unset).
    fn is_variable_like(expr: &Expr) -> bool {
        matches!(
            expr,
            Expr::Variable { .. }
                | Expr::DynamicVariable { .. }
                | Expr::Globals { .. }
                | Expr::CompileError { .. }
                | Expr::ArrayAccess { .. }
                | Expr::PropertyAccess { .. }
                | Expr::DynamicPropertyAccess { .. }
                | Expr::StaticProperty { .. }
                | Expr::DynamicNamedStaticProperty { .. }
                | Expr::DynamicStaticProperty { .. }
        )
    }

    fn normalize_unset_target(&mut self, expr: Expr) -> Result<Expr, String> {
        if let Expr::Globals { line } = &expr {
            return Ok(self.globals_modification_error(*line));
        }
        if let Some(line) = Self::nullsafe_chain_line(&expr) {
            return Ok(self.nullsafe_write_error(line));
        }
        if let Some(error) = self.call_write_error(&expr) {
            return Ok(error);
        }
        if matches!(expr, Expr::ArrayAccess { .. })
            && let Some((message, line)) = self.array_write_root_error(&expr)
        {
            return Ok(self.compile_error(message, line));
        }
        if !Self::is_variable_like(&expr) {
            return Err("Cannot use unset() on the result of an expression".into());
        }
        Ok(expr)
    }

    fn into_foreach_target(&mut self, expr: Expr) -> Result<ForeachTarget, String> {
        if let Some(line) = Self::nullsafe_chain_line(&expr) {
            return Ok(ForeachTarget::Target(self.nullsafe_write_error(line)));
        }
        if let Some(error) = self.call_write_error(&expr) {
            return Ok(ForeachTarget::Target(error));
        }
        if matches!(expr, Expr::ArrayAccess { .. } | Expr::ArrayAppendArgument { .. })
            && let Some((message, line)) = self.array_write_root_error(&expr)
        {
            return Ok(ForeachTarget::Target(self.compile_error(message, line)));
        }
        match expr {
            Expr::Variable { ref name, line } if name == "this" => Ok(
                ForeachTarget::Target(self.compile_error("Cannot re-assign $this", line)),
            ),
            Expr::Variable { name, .. } => Ok(ForeachTarget::Variable(name)),
            target @ Expr::DynamicVariable { .. } => Ok(ForeachTarget::Target(target)),
            Expr::Globals { line } => Ok(ForeachTarget::Target(
                self.globals_modification_error(line),
            )),
            target @ (Expr::ArrayAccess { .. }
            | Expr::ArrayAppendArgument { .. }
            | Expr::PropertyAccess {
                nullsafe: false, ..
            }
            | Expr::DynamicPropertyAccess {
                nullsafe: false, ..
            }
            | Expr::StaticProperty { .. }
            | Expr::DynamicNamedStaticProperty { .. }
            | Expr::DynamicStaticProperty { .. }) => Ok(ForeachTarget::Target(target)),
            _ => Err("Invalid foreach assignment target".into()),
        }
    }

    fn is_isset_target(expr: &Expr) -> bool {
        Self::is_variable_like(expr)
    }

    /// Check if current `$var[...]...` chain ends in an assignment. Bracketed
    /// index expressions may themselves contain nested brackets or calls.
    fn is_array_assign(&self) -> bool {
        let mut i = self.pos + 1;
        let mut saw_dimension = false;
        while matches!(self.tokens.get(i), Some(Token::LBracket(_))) {
            saw_dimension = true;
            let mut depth = 1usize;
            i += 1;
            // Leave an empty final dimension to the array-append parser.
            if self.tokens.get(i) == Some(&Token::RBracket) {
                return false;
            }
            while i < self.tokens.len() && depth != 0 {
                match &self.tokens[i] {
                    Token::LBracket(_) | Token::LParen(_) => depth += 1,
                    Token::RBracket | Token::RParen => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            if depth != 0 {
                return false;
            }
            if !matches!(self.tokens.get(i - 1), Some(Token::RBracket)) {
                return false;
            }
        }
        saw_dimension
            && self.tokens.get(i) == Some(&Token::Assign)
            && self.tokens.get(i + 1) != Some(&Token::Ampersand)
    }

    fn split_array_access(mut expr: Expr) -> (Expr, Vec<Expr>) {
        let mut indices = Vec::new();
        while let Expr::ArrayAccess { array, index, .. } = expr {
            indices.push(*index);
            expr = *array;
        }
        indices.reverse();
        (expr, indices)
    }

    /// Check if `[` at current position starts a short list destructuring pattern.
    /// Scans ahead for pattern: `[` (vars/commas) `]` `=`
    fn is_short_list_assign(&self) -> bool {
        let mut i = self.pos + 1; // skip '['
        let mut depth = 1;
        while i < self.tokens.len() && depth > 0 {
            match &self.tokens[i] {
                Token::LBracket(_) => depth += 1,
                Token::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        return self.tokens.get(i + 1) == Some(&Token::Assign);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// Check whether the `(` at the current position closes immediately
    /// before the assignment in a value-producing `list(...) = expression`.
    fn is_legacy_list_assign(&self) -> bool {
        if !matches!(self.peek(), Token::LParen(_)) {
            return false;
        }
        let mut i = self.pos + 1;
        let mut paren_depth = 1usize;
        let mut bracket_depth = 0usize;
        while i < self.tokens.len() {
            match &self.tokens[i] {
                Token::LParen(_) => paren_depth += 1,
                Token::RParen => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        return bracket_depth == 0
                            && self.tokens.get(i + 1) == Some(&Token::Assign);
                    }
                }
                Token::LBracket(_) => bracket_depth += 1,
                Token::RBracket if bracket_depth != 0 => bracket_depth -= 1,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// Parse `list($a, $b, ...) = expr;`
    fn parse_list_assign(&mut self) -> Result<Stmt, String> {
        let line = match self.peek() {
            Token::Identifier(_, line) => line,
            _ => 0,
        };
        self.advance(); // consume 'list' identifier
        self.expect_lparen()?;
        let targets = self.parse_list_targets(&Token::RParen)?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::Assign)?;
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon(0))?;
        Ok(Stmt::ListAssign {
            targets,
            expr,
            line,
        })
    }

    /// Parse `[$a, $b, ...] = expr;`
    fn parse_short_list_assign(&mut self) -> Result<Stmt, String> {
        let line = match self.peek() {
            Token::LBracket(line) => line,
            _ => 0,
        };
        self.advance(); // consume '['
        let targets = self.parse_list_targets(&Token::RBracket)?;
        self.expect(&Token::RBracket)?;
        self.expect(&Token::Assign)?;
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon(0))?;
        Ok(Stmt::ListAssign {
            targets,
            expr,
            line,
        })
    }

    /// Parse comma-separated list targets (variables, skips, nested brackets).
    /// `end_token` is `)` for list() or `]` for short syntax.
    fn parse_list_reference_target(&mut self) -> Result<Expr, String> {
        if !matches!(
            self.peek(),
            Token::Variable(_, _) | Token::This(_) | Token::Dollar(_)
        ) {
            return Err("Expected writable variable after '&' in destructuring".into());
        }
        let target = self.parse_empty_dimension_target_prefix()?;
        if let Some(line) = Self::nullsafe_chain_line(&target) {
            return Ok(self.compile_error(
                "Cannot assign reference to non referenceable value",
                line,
            ));
        }
        match target {
            Expr::Variable { ref name, line } if name == "this" => Ok(self
                .compile_error("Cannot re-assign $this", line)),
            Expr::Globals { line } => Ok(self.compile_error(
                "Cannot assign reference to non referenceable value",
                line,
            )),
            target @ (Expr::Variable { .. }
            | Expr::DynamicVariable { .. }
            | Expr::ArrayAccess { .. }
            | Expr::PropertyAccess {
                nullsafe: false, ..
            }
            | Expr::DynamicPropertyAccess {
                nullsafe: false, ..
            }
            | Expr::StaticProperty { .. }
            | Expr::DynamicNamedStaticProperty { .. }
            | Expr::DynamicStaticProperty { .. }) => Ok(target),
            _ => Err("Invalid reference destructuring assignment target".into()),
        }
    }

    fn normalize_list_target(
        &mut self,
        target: Expr,
        fallback_line: usize,
    ) -> Result<ListTarget, String> {
        let nullsafe_line = Self::nullsafe_chain_line(&target);
        if self.is_empty_array_dimension_suffix() {
            self.expect_lbracket()?;
            self.expect(&Token::RBracket)?;
            if let Some(line) = nullsafe_line {
                return Ok(ListTarget::Target(self.compile_error(
                    "Assignments can only happen to writable values",
                    line,
                )));
            }
            if !matches!(
                &target,
                Expr::Variable { .. }
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
                return Ok(ListTarget::Target(self.compile_error(
                    "Assignments can only happen to writable values",
                    fallback_line,
                )));
            }
            return Ok(ListTarget::AppendTarget(target));
        }

        if let Some(line) = nullsafe_line {
            return Ok(ListTarget::Target(self.compile_error(
                "Assignments can only happen to writable values",
                line,
            )));
        }
        if let Expr::Pipe { line, .. } = &target {
            return Ok(ListTarget::Target(self.compile_error(
                "Can't use function return value in write context",
                *line,
            )));
        }
        if let Some(error) = self.call_write_error(&target) {
            return Ok(ListTarget::Target(error));
        }
        Ok(match target {
            Expr::Variable { name, line } if name == "this" => ListTarget::Target(
                self.compile_error("Cannot re-assign $this", line),
            ),
            Expr::Variable { name, .. } => ListTarget::Variable(name),
            target @ Expr::DynamicVariable { .. } => ListTarget::Target(target),
            Expr::Globals { line } => {
                ListTarget::Target(self.globals_modification_error(line))
            }
            target @ (Expr::ArrayAccess { .. }
            | Expr::PropertyAccess {
                nullsafe: false, ..
            }
            | Expr::DynamicPropertyAccess {
                nullsafe: false, ..
            }
            | Expr::StaticProperty { .. }
            | Expr::DynamicNamedStaticProperty { .. }
            | Expr::DynamicStaticProperty { .. }
            | Expr::CompileError { .. }) => ListTarget::Target(target),
            Expr::ArrayLiteral(_) => ListTarget::Target(
                self.compile_error("Cannot assign to array(), use [] instead", fallback_line),
            ),
            _ => ListTarget::Target(self.compile_error(
                "Assignments can only happen to writable values",
                fallback_line,
            )),
        })
    }

    fn key_list_target(&mut self, key: Expr, target: ListTarget) -> ListTarget {
        match target {
            ListTarget::Variable(var) => ListTarget::KeyedVariable { key, var },
            ListTarget::Reference(target) => ListTarget::KeyedReference { key, target },
            ListTarget::Target(target) => ListTarget::KeyedTarget { key, target },
            ListTarget::AppendTarget(target) => {
                ListTarget::KeyedAppendTarget { key, target }
            }
            ListTarget::Nested(targets) => ListTarget::KeyedNested { key, targets },
            ListTarget::Skip
            | ListTarget::KeyedVariable { .. }
            | ListTarget::KeyedReference { .. }
            | ListTarget::KeyedTarget { .. }
            | ListTarget::KeyedAppendTarget { .. }
            | ListTarget::KeyedNested { .. } => unreachable!("one list entry has one key"),
        }
    }

    fn list_target_is_keyed(target: &ListTarget) -> bool {
        matches!(
            target,
            ListTarget::KeyedVariable { .. }
                | ListTarget::KeyedReference { .. }
                | ListTarget::KeyedTarget { .. }
                | ListTarget::KeyedAppendTarget { .. }
                | ListTarget::KeyedNested { .. }
        )
    }

    fn parse_one_list_target(&mut self, end_token: &Token) -> Result<ListTarget, String> {
        if let Token::DotDotDot(line) = self.peek() {
            self.advance();
            if !matches!(self.peek(), Token::Variable(_, _) | Token::This(_)) {
                return Err(
                    "Expected assignment target after spread operator in destructuring".into(),
                );
            }
            let _ = self.parse_empty_dimension_target_prefix()?;
            return Ok(ListTarget::Target(self.compile_error(
                "Spread operator is not supported in assignments",
                line,
            )));
        }
        if self.peek() == Token::Ampersand {
            self.advance();
            return Ok(ListTarget::Reference(self.parse_list_reference_target()?));
        }
        if let Token::LBracket(line) = self.peek() {
            if *end_token == Token::RParen {
                self.compile_error("Cannot mix [] and list()", line);
            }
            self.advance();
            let nested = self.parse_list_targets(&Token::RBracket)?;
            self.expect(&Token::RBracket)?;
            return Ok(ListTarget::Nested(nested));
        }
        if let Token::Identifier(ref name, line) = self.peek()
            && name.eq_ignore_ascii_case("list")
            && matches!(self.peek_at(1), Token::LParen(_))
        {
            if *end_token == Token::RBracket {
                self.compile_error("Cannot mix [] and list()", line);
            }
            self.advance();
            self.expect_lparen()?;
            let nested = self.parse_list_targets(&Token::RParen)?;
            self.expect(&Token::RParen)?;
            return Ok(ListTarget::Nested(nested));
        }

        let fallback_line = match self.peek() {
            Token::Variable(_, line)
            | Token::This(line)
            | Token::Dollar(line)
            | Token::Identifier(_, line)
            | Token::MagicConstant { line, .. }
            | Token::LParen(line) => line,
            _ => self.closest_token_source_line(),
        };
        let expression = self.parse_empty_dimension_target_prefix()?;
        if self.peek() == Token::DoubleArrow {
            self.advance();
            let target = self.parse_one_list_target(end_token)?;
            return Ok(self.key_list_target(expression, target));
        }
        self.normalize_list_target(expression, fallback_line)
    }

    pub(super) fn parse_list_targets(
        &mut self,
        end_token: &Token,
    ) -> Result<Vec<ListTarget>, String> {
        let mut targets = Vec::new();
        let list_line = self.closest_token_source_line();
        let mut keyed = None;
        let mut saw_skip = false;
        while self.peek() != *end_token && !self.at_eof() {
            if matches!(self.peek(), Token::Comma(_)) {
                // Skip element (empty slot before comma or between commas)
                targets.push(ListTarget::Skip);
                saw_skip = true;
                self.advance(); // consume ','
                continue;
            }
            let target = self.parse_one_list_target(end_token)?;
            let target_keyed = Self::list_target_is_keyed(&target);
            if let Some(previous_keyed) = keyed {
                if previous_keyed != target_keyed {
                    self.compile_error(
                        "Cannot mix keyed and unkeyed array entries in assignments",
                        list_line,
                    );
                }
            } else {
                keyed = Some(target_keyed);
            }
            targets.push(target);
            // Consume comma if present
            if matches!(self.peek(), Token::Comma(_)) {
                self.advance();
            } else {
                break;
            }
        }
        if keyed.is_none() {
            self.compile_error("Cannot use empty list", list_line);
        } else if keyed == Some(true) && saw_skip {
            self.compile_error(
                "Cannot use empty array entries in keyed array assignment",
                list_line,
            );
        }
        Ok(targets)
    }

    /// Parse the optional level operand after `break` or `continue`.
    /// PHP still accepts the historical expression grammar, but compilation
    /// permits only a positive integer literal (optionally parenthesized).
    fn parse_break_continue_level(
        &mut self,
        operator: &str,
        line: usize,
    ) -> Result<Option<u32>, String> {
        if matches!(self.peek(), Token::Semicolon(_)) {
            return Ok(None);
        }

        let operand = self.parse_expr()?;
        let literal_requires_positive_integer = match operand {
            Expr::Integer(level) if level > 0 => return Ok(Some(level as u32)),
            Expr::Integer(0) | Expr::StringLiteral(_) | Expr::BinaryStringLiteral(_) => true,
            Expr::Float(level) if !level.is_sign_negative() => true,
            _ => false,
        };
        let message = if literal_requires_positive_integer {
            format!("'{operator}' operator accepts only positive integers")
        } else {
            format!("'{operator}' operator with non-integer operand is no longer supported")
        };
        self.compile_error(message, line);
        Ok(Some(1))
    }
}
