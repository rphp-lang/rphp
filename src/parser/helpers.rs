impl Parser {
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
            Token::Static => Some("static".to_string()),
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
            Token::As => Some("as".to_string()),
            Token::Switch => Some("switch".to_string()),
            Token::Case => Some("case".to_string()),
            Token::Default => Some("default".to_string()),
            Token::Break => Some("break".to_string()),
            Token::Continue => Some("continue".to_string()),
            Token::Try => Some("try".to_string()),
            Token::Catch => Some("catch".to_string()),
            Token::Finally => Some("finally".to_string()),
            Token::Throw(_) => Some("throw".to_string()),
            Token::Instanceof => Some("instanceof".to_string()),
            Token::Abstract => Some("abstract".to_string()),
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
            Token::Use => Some("use".to_string()),
            Token::Declare => Some("declare".to_string()),
            Token::Trait => Some("trait".to_string()),
            Token::Final => Some("final".to_string()),
            Token::Enum => Some("enum".to_string()),
            Token::Namespace => Some("namespace".to_string()),
            Token::Yield => Some("yield".to_string()),
            Token::From => Some("from".to_string()),
            Token::Global => Some("global".to_string()),
            Token::Print => Some("print".to_string()),
            Token::Clone(_) => Some("clone".to_string()),
            Token::Include => Some("include".to_string()),
            Token::IncludeOnce => Some("include_once".to_string()),
            Token::Require => Some("require".to_string()),
            Token::RequireOnce => Some("require_once".to_string()),
            Token::Goto { name, .. } => Some(name.clone()),
            Token::LogicalXor => Some("xor".to_string()),
            _ => None,
        }
    }

    fn parse_call_args(&mut self) -> Result<Vec<CallArg>, String> {
        let mut args: Vec<CallArg> = Vec::new();
        let mut seen_named = false;
        let mut seen_unpack = false;
        if self.peek() != Token::RParen {
            loop {
                // Check for named argument: identifier-like token followed by Colon
                if let Some(_label) = Self::token_as_named_arg_label(&self.peek()) {
                    if self.peek_at(1) == Token::Colon {
                        let name = Self::token_as_named_arg_label(&self.advance()).unwrap();
                        self.advance(); // consume ':'
                        let value = self.parse_expr()?;
                        args.push(CallArg::Named { name, value });
                        seen_named = true;
                        if self.peek() == Token::Comma {
                            self.advance();
                            if self.peek() == Token::RParen {
                                break;
                            }
                            continue;
                        } else {
                            break;
                        }
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
                    let expr = self.parse_expr()?;
                    args.push(CallArg::Unpack(expr));
                    seen_unpack = true;
                    if self.peek() == Token::Comma {
                        self.advance();
                        if self.peek() == Token::RParen {
                            break;
                        }
                        continue;
                    }
                    break;
                }
                // Positional argument
                if seen_named {
                    return Err("Cannot use positional argument after named argument".to_string());
                }
                if seen_unpack {
                    return Err(
                        "Cannot use positional argument after argument unpacking".to_string()
                    );
                }
                let expr = self.parse_positional_call_argument()?;
                args.push(CallArg::Positional(expr));
                if self.peek() == Token::Comma {
                    self.advance();
                    if self.peek() == Token::RParen {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::RParen)?;
        Ok(args)
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
        } else {
            Err(format!("Expected {:?}, got {:?}", expected, tok))
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
            while self.peek() == Token::Comma {
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
                | Token::Static
                | Token::LParen(_)
                | Token::Identifier(_, _)
                | Token::Public
                | Token::Protected
                | Token::Private
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
            Token::Identifier(n, line) => {
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
                Token::Identifier(n, _) => parts.push(n),
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
            Token::Identifier(name, line) => {
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

    /// Parse the leading name of a use declaration, retaining the `\{`
    /// boundary that distinguishes a group import from an ordinary qualified
    /// name. Whitespace is already absent from the token stream.
    fn parse_use_name(&mut self) -> Result<(String, bool), String> {
        let leading_backslash = if self.peek() == Token::Backslash {
            self.advance();
            true
        } else {
            false
        };
        let mut parts = match self.advance() {
            Token::Identifier(name, _) => vec![name],
            other => {
                return Err(format!(
                    "Expected identifier in use declaration, got {:?}",
                    other
                ));
            }
        };
        while self.peek() == Token::Backslash {
            self.advance();
            if self.peek() == Token::LBrace {
                self.advance();
                let mut prefix = parts.join("\\");
                if leading_backslash {
                    prefix.insert(0, '\\');
                }
                return Ok((prefix, true));
            }
            match self.advance() {
                Token::Identifier(name, _) => parts.push(name),
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
        Ok((name, false))
    }

    fn consume_use_alias_keyword(&mut self) -> bool {
        let is_alias = self.peek() == Token::As
            || matches!(self.peek(), Token::Identifier(name, _) if name.eq_ignore_ascii_case("as"));
        if is_alias {
            self.advance();
        }
        is_alias
    }

    fn parse_return_type(&mut self) -> Result<Option<TypeHint>, String> {
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
            if Self::type_hint_uses_static(&hint) && !self.class_scope_active {
                return Err("Cannot use \"static\" when no class scope is active".to_string());
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
                    | Some(Token::Backslash)
                    | Some(Token::Namespace)
                    | Some(Token::ArrayKw)
                    | Some(Token::Null)
                    | Some(Token::Static)
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
                        | Some(Token::Backslash)
                        | Some(Token::Namespace)
                        | Some(Token::ArrayKw)
                        | Some(Token::Null)
                        | Some(Token::Static)
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
            Token::Backslash | Token::Namespace => true,
            Token::ArrayKw | Token::Null => {
                matches!(
                    self.tokens.get(self.pos + 1),
                    Some(Token::Variable(_, _)) | Some(Token::Pipe) | Some(Token::Ampersand)
                )
            }
            // `static` is a return-only PHP type. Parameter/property parsing
            // enters the parameter parser only so it can emit the precise
            // return-only diagnostic. Class-member modifiers are consumed
            // before this lookahead runs.
            Token::Static => matches!(
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
    fn try_parse_type_hint(&mut self) -> Result<Option<TypeHint>, String> {
        // Nullable: ?type
        if self.peek() == Token::Question {
            // Peek ahead: ?$var or ?... means ternary/other, not type hint
            // In param context, ?Identifier or ?ArrayKw means nullable type
            let next = self.tokens.get(self.pos + 1);
            if matches!(next, Some(Token::Static)) {
                return Err("static is only allowed as a return type".to_string());
            }
            let is_type = matches!(
                next,
                Some(Token::Identifier(_, _))
                    | Some(Token::Backslash)
                    | Some(Token::Namespace)
                    | Some(Token::ArrayKw)
                    | Some(Token::Null)
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
            Token::Identifier(_, _) => {
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
                    if Self::type_hint_uses_static(&hint) {
                        return Err("static is only allowed as a return type".to_string());
                    }
                    return Ok(Some(hint));
                }
                Ok(None)
            }
            Token::Namespace => {
                let hint = self.parse_base_type_hint()?;
                let hint = self.maybe_parse_compound_type(hint)?;
                Ok(Some(hint))
            }
            Token::Backslash => {
                let hint = self.parse_base_type_hint()?;
                let hint = self.maybe_parse_compound_type(hint)?;
                Ok(Some(hint))
            }
            Token::ArrayKw => {
                let next = self.tokens.get(self.pos + 1);
                let is_type_context = matches!(
                    next,
                    Some(Token::Variable(_, _))
                        | Some(Token::Ampersand)
                        | Some(Token::DotDotDot(_))
                        | Some(Token::Pipe)
                );
                if is_type_context {
                    self.advance(); // consume 'array'
                    let hint = self.maybe_parse_compound_type(TypeHint::Array)?;
                    if Self::type_hint_uses_static(&hint) {
                        return Err("static is only allowed as a return type".to_string());
                    }
                    return Ok(Some(hint));
                }
                Ok(None)
            }
            Token::Static => Err("static is only allowed as a return type".to_string()),
            Token::LParen(_) => {
                let hint = self.parse_base_type_hint()?;
                let hint = self.maybe_parse_compound_type(hint)?;
                if Self::type_hint_uses_static(&hint) {
                    return Err("static is only allowed as a return type".to_string());
                }
                Ok(Some(hint))
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
            Token::Namespace => {
                self.expect(&Token::Backslash)?;
                let first = match self.advance() {
                    Token::Identifier(name, _) => name,
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
                    Token::Identifier(name, _) => name,
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
            Token::Static => {
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
                Token::Identifier(name, _) => parts.push(name),
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

    fn parse_one_param(&mut self) -> Result<Param, String> {
        // Check for constructor property promotion: visibility keyword before type hint
        let mut promotion: Option<(Visibility, Option<Visibility>, bool)> = None;
        let mut promo_readonly = false;
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
                _ => break,
            }
        }
        if promo_visibility.is_some() || promo_set_visibility.is_some() {
            promotion = Some((
                promo_visibility.unwrap_or(Visibility::Public),
                promo_set_visibility,
                promo_readonly,
            ));
        }
        // Optional type hint before &, ..., $var
        let type_hint = self.try_parse_type_hint()?;
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
            if is_variadic {
                return Err(format!(
                    "Variadic parameter ${} cannot have a default value",
                    name
                ));
            }
            self.advance(); // consume '='
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Param {
            name,
            line,
            default,
            is_variadic,
            is_ref,
            type_hint,
            promotion,
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

    fn into_foreach_target(&mut self, expr: Expr) -> Result<ForeachTarget, String> {
        if let Some(line) = Self::nullsafe_chain_line(&expr) {
            return Ok(ForeachTarget::Target(self.nullsafe_write_error(line)));
        }
        match expr {
            Expr::Variable { name, .. } => Ok(ForeachTarget::Variable(name)),
            target @ Expr::DynamicVariable { .. } => Ok(ForeachTarget::Target(target)),
            Expr::Globals { line } => Ok(ForeachTarget::Target(
                self.globals_modification_error(line),
            )),
            target @ (Expr::ArrayAccess { .. }
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
        self.expect(&Token::Semicolon)?;
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
        self.expect(&Token::Semicolon)?;
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
            Expr::Variable { ref name, .. } if name == "this" => {
                Err("Cannot re-assign $this".into())
            }
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

    pub(super) fn parse_list_targets(
        &mut self,
        end_token: &Token,
    ) -> Result<Vec<ListTarget>, String> {
        let mut targets = Vec::new();
        while self.peek() != *end_token && !self.at_eof() {
            if self.peek() == Token::Comma {
                // Skip element (empty slot before comma or between commas)
                targets.push(ListTarget::Skip);
                self.advance(); // consume ','
                continue;
            }
            if let Token::DotDotDot(line) = self.peek() {
                self.advance(); // consume the unsupported spread marker
                if !matches!(self.peek(), Token::Variable(_, _) | Token::This(_)) {
                    return Err(
                        "Expected assignment target after spread operator in destructuring".into(),
                    );
                }
                // Consume the complete l-value so the parser can finish the
                // source unit. PHP accepts this grammar shape and rejects it
                // during compilation, before any right-hand side can run.
                let _ = self.parse_empty_dimension_target_prefix()?;
                let error = self.compile_error(
                    "Spread operator is not supported in assignments",
                    line,
                );
                targets.push(ListTarget::Target(error));
            }
            else if self.peek() == Token::Ampersand {
                self.advance();
                let target = self.parse_list_reference_target()?;
                targets.push(ListTarget::Reference(target));
            }
            // Check for nested: list(...) or [...]
            else if matches!(self.peek(), Token::LBracket(_)) {
                self.advance(); // consume '['
                let nested = self.parse_list_targets(&Token::RBracket)?;
                self.expect(&Token::RBracket)?;
                targets.push(ListTarget::Nested(nested));
            } else if let Token::Identifier(ref name, line) = self.peek() {
                if name == "list" && matches!(self.peek_at(1), Token::LParen(_)) {
                    self.advance(); // consume 'list'
                    self.expect_lparen()?;
                    let nested = self.parse_list_targets(&Token::RParen)?;
                    self.expect(&Token::RParen)?;
                    targets.push(ListTarget::Nested(nested));
                } else if matches!(self.peek_at(1), Token::PipeGreater(_) | Token::LParen(_)) {
                    // Calls and pipes are valid expression grammar here, but
                    // their results cannot become destructuring write targets.
                    // Consume the complete expression so PHP's compile-time
                    // diagnostic survives dead-code elimination.
                    let line = line;
                    let _ = self.parse_expr()?;
                    targets.push(ListTarget::Target(self.compile_error(
                        "Can't use function return value in write context",
                        line,
                    )));
                } else {
                    return Err(format!(
                        "Expected variable in list/destructuring, got identifier '{}'",
                        name
                    ));
                }
            } else if matches!(
                self.peek(),
                Token::Variable(_, _) | Token::This(_) | Token::Dollar(_)
            ) {
                let target = self.parse_empty_dimension_target_prefix()?;
                let nullsafe_line = Self::nullsafe_chain_line(&target);
                if matches!(self.peek(), Token::LBracket(_))
                    && self.peek_at(1) == Token::RBracket
                {
                    if let Some(line) = nullsafe_line {
                        self.advance();
                        self.advance();
                        targets.push(ListTarget::Target(self.compile_error(
                            "Assignments can only happen to writable values",
                            line,
                        )));
                    } else {
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
                            return Err("Invalid array append destructuring target".into());
                        }
                        self.advance();
                        self.advance();
                        targets.push(ListTarget::AppendTarget(target));
                    }
                } else if let Some(line) = nullsafe_line {
                    targets.push(ListTarget::Target(self.compile_error(
                        "Assignments can only happen to writable values",
                        line,
                    )));
                } else {
                    match target {
                        Expr::Variable { name: var, .. } if var == "this" => {
                            return Err("Cannot re-assign $this".into());
                        }
                        Expr::Variable { name: var, .. } => {
                            targets.push(ListTarget::Variable(var))
                        }
                        target @ Expr::DynamicVariable { .. } => {
                            targets.push(ListTarget::Target(target))
                        }
                        Expr::Globals { line } => targets.push(ListTarget::Target(
                            self.globals_modification_error(line),
                        )),
                        target @ (Expr::ArrayAccess { .. }
                        | Expr::PropertyAccess {
                            nullsafe: false, ..
                        }
                        | Expr::DynamicPropertyAccess {
                            nullsafe: false, ..
                        }
                        | Expr::StaticProperty { .. }
                        | Expr::DynamicNamedStaticProperty { .. }
                        | Expr::DynamicStaticProperty { .. }) => {
                            targets.push(ListTarget::Target(target))
                        }
                        _ => return Err("Invalid destructuring assignment target".into()),
                    }
                }
            } else if matches!(
                self.peek(),
                Token::Integer(_) | Token::StringLiteral(_) | Token::LParen(_)
            ) {
                // Explicit key: constants and parenthesized expressions such
                // as `($array['marker'] = 1) => &$target` are evaluated in
                // source order before the referenced dimension is selected.
                let key_expr = self.parse_expr()?;
                self.expect(&Token::DoubleArrow)?;
                if self.peek() == Token::Ampersand {
                    self.advance();
                    let target = self.parse_list_reference_target()?;
                    targets.push(ListTarget::KeyedReference {
                        key: key_expr,
                        target,
                    });
                } else if matches!(self.peek(), Token::LBracket(_)) {
                    self.advance();
                    let nested = self.parse_list_targets(&Token::RBracket)?;
                    self.expect(&Token::RBracket)?;
                    targets.push(ListTarget::KeyedNested {
                        key: key_expr,
                        targets: nested,
                    });
                } else if matches!(self.peek(), Token::Identifier(name, _) if name == "list")
                    && matches!(self.peek_at(1), Token::LParen(_))
                {
                    self.advance();
                    self.expect_lparen()?;
                    let nested = self.parse_list_targets(&Token::RParen)?;
                    self.expect(&Token::RParen)?;
                    targets.push(ListTarget::KeyedNested {
                        key: key_expr,
                        targets: nested,
                    });
                } else {
                    let (var_name, line) = match self.advance() {
                        Token::Variable(n, line) => (n, line),
                        other => {
                            return Err(format!(
                                "Expected variable after '=>' in list, got {:?}",
                                other
                            ));
                        }
                    };
                    if var_name == "GLOBALS" {
                        let error = self.globals_modification_error(line);
                        targets.push(ListTarget::Target(error));
                    } else {
                        targets.push(ListTarget::KeyedVariable {
                            key: key_expr,
                            var: var_name,
                        });
                    }
                }
            } else {
                return Err(format!(
                    "Unexpected token in list/destructuring: {:?}",
                    self.peek()
                ));
            }
            // Consume comma if present
            if self.peek() == Token::Comma {
                self.advance();
            } else {
                break;
            }
        }
        Ok(targets)
    }

    /// Parse optional integer level after break/continue (e.g. `break 2;`)
    fn parse_break_continue_level(&mut self) -> Result<Option<u32>, String> {
        if let Token::Integer(n) = self.peek() {
            self.advance();
            if n < 1 {
                return Err(format!(
                    "break/continue level must be at least 1, got {}",
                    n
                ));
            }
            Ok(Some(n as u32))
        } else {
            Ok(None)
        }
    }
}
