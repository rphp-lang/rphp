impl Parser {
    fn peek(&self) -> Token {
        self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof)
    }

    fn peek_at(&self, offset: usize) -> Token {
        self.tokens
            .get(self.pos + offset)
            .cloned()
            .unwrap_or(Token::Eof)
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
            Token::Identifier(n) => Some(n.clone()),
            // All keyword tokens — PHP accepts any reserved word as a named arg label
            Token::ArrayKw => Some("array".to_string()),
            Token::Null => Some("null".to_string()),
            Token::True => Some("true".to_string()),
            Token::False => Some("false".to_string()),
            Token::Match => Some("match".to_string()),
            Token::Static => Some("static".to_string()),
            Token::Function => Some("function".to_string()),
            Token::Class => Some("class".to_string()),
            Token::New => Some("new".to_string()),
            Token::Return => Some("return".to_string()),
            Token::Echo => Some("echo".to_string()),
            Token::If => Some("if".to_string()),
            Token::Else => Some("else".to_string()),
            Token::ElseIf => Some("elseif".to_string()),
            Token::While => Some("while".to_string()),
            Token::Do => Some("do".to_string()),
            Token::For => Some("for".to_string()),
            Token::Foreach => Some("foreach".to_string()),
            Token::As => Some("as".to_string()),
            Token::Switch => Some("switch".to_string()),
            Token::Case => Some("case".to_string()),
            Token::Default => Some("default".to_string()),
            Token::Break => Some("break".to_string()),
            Token::Continue => Some("continue".to_string()),
            Token::Try => Some("try".to_string()),
            Token::Catch => Some("catch".to_string()),
            Token::Finally => Some("finally".to_string()),
            Token::Throw => Some("throw".to_string()),
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
            Token::Fn => Some("fn".to_string()),
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
            Token::Clone => Some("clone".to_string()),
            _ => None,
        }
    }

    fn parse_call_args(&mut self) -> Result<Vec<CallArg>, String> {
        let mut args: Vec<CallArg> = Vec::new();
        let mut seen_named = false;
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
                            continue;
                        } else {
                            break;
                        }
                    }
                }
                // Positional argument
                if seen_named {
                    return Err("Cannot use positional argument after named argument".to_string());
                }
                if self.peek() == Token::DotDotDot {
                    self.advance();
                    let expr = self.parse_expr()?;
                    args.push(CallArg::Unpack(expr));
                    if self.peek() == Token::Comma {
                        self.advance();
                        continue;
                    }
                    break;
                }
                let expr = self.parse_expr()?;
                args.push(CallArg::Positional(expr));
                if self.peek() == Token::Comma {
                    self.advance();
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
            Token::Variable(_)
                | Token::DotDotDot
                | Token::Ampersand
                | Token::Question
                | Token::Backslash
                | Token::ArrayKw
                | Token::Null
                | Token::Static
                | Token::Identifier(_)
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
            Token::Identifier(n) => parts.push(n),
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
                Token::Identifier(n) => parts.push(n),
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
                Some(Token::Identifier(_))
                    | Some(Token::Backslash)
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
                    Some(Token::Identifier(_))
                        | Some(Token::Backslash)
                        | Some(Token::ArrayKw)
                        | Some(Token::Null)
                        | Some(Token::Static)
                )
            }
            Token::Identifier(_) => {
                if self.peek_at(1) == Token::Less {
                    return true;
                }
                matches!(
                    self.tokens.get(self.pos + 1),
                    Some(Token::Variable(_)) | Some(Token::Pipe) | Some(Token::Ampersand)
                )
            }
            Token::Backslash => true,
            Token::ArrayKw | Token::Null => {
                matches!(
                    self.tokens.get(self.pos + 1),
                    Some(Token::Variable(_)) | Some(Token::Pipe) | Some(Token::Ampersand)
                )
            }
            // `static` is a return-only PHP type. Parameter/property parsing
            // enters the parameter parser only so it can emit the precise
            // return-only diagnostic. Class-member modifiers are consumed
            // before this lookahead runs.
            Token::Static => matches!(
                self.tokens.get(self.pos + 1),
                Some(Token::Less)
                    | Some(Token::Variable(_))
                    | Some(Token::Pipe)
                    | Some(Token::Ampersand)
            ),
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
                Some(Token::Identifier(_))
                    | Some(Token::Backslash)
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
            Token::Identifier(_) => {
                let next = self.tokens.get(self.pos + 1);
                let is_type_context = matches!(
                    next,
                    Some(Token::Variable(_))
                        | Some(Token::Ampersand)
                        | Some(Token::DotDotDot)
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
            Token::Backslash => {
                let hint = self.parse_base_type_hint()?;
                let hint = self.maybe_parse_compound_type(hint)?;
                Ok(Some(hint))
            }
            Token::ArrayKw => {
                let next = self.tokens.get(self.pos + 1);
                let is_type_context = matches!(
                    next,
                    Some(Token::Variable(_))
                        | Some(Token::Ampersand)
                        | Some(Token::DotDotDot)
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
            _ => Ok(None),
        }
    }

    /// Parse a non-nullable type hint (int, string, float, bool, array, ClassName).
    fn parse_base_type_hint(&mut self) -> Result<TypeHint, String> {
        match self.advance() {
            Token::Identifier(name) => match name.as_str() {
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
            Token::Backslash => {
                let first = match self.advance() {
                    Token::Identifier(name) => name,
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
                Token::Identifier(name) => parts.push(name),
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
        let mut promotion: Option<(Visibility, bool)> = None;
        let mut promo_readonly = false;
        match self.peek() {
            Token::Public | Token::Protected | Token::Private => {
                let vis = match self.advance() {
                    Token::Public => Visibility::Public,
                    Token::Protected => Visibility::Protected,
                    Token::Private => Visibility::Private,
                    _ => unreachable!(),
                };
                // Check for 'readonly' after visibility
                if matches!(self.peek(), Token::Identifier(ref s) if s == "readonly") {
                    self.advance();
                    promo_readonly = true;
                }
                promotion = Some((vis, promo_readonly));
            }
            _ => {}
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
        let is_variadic = if self.peek() == Token::DotDotDot {
            self.advance(); // consume '...'
            true
        } else {
            false
        };
        let name = match self.advance() {
            Token::Variable(n) => n,
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
            default,
            is_variadic,
            is_ref,
            type_hint,
            promotion,
        })
    }

    /// Check if an expression is a variable-like target (valid for isset/empty/unset).
    fn is_variable_like(expr: &Expr) -> bool {
        matches!(
            expr,
            Expr::Variable(_) | Expr::ArrayAccess { .. } | Expr::PropertyAccess { .. }
        )
    }

    fn is_isset_target(expr: &Expr) -> bool {
        Self::is_variable_like(expr) || matches!(expr, Expr::PropertyAccess { .. })
    }

    /// Check if current `$var[...]...` chain ends in an assignment. Bracketed
    /// index expressions may themselves contain nested brackets or calls.
    fn is_array_assign(&self) -> bool {
        let mut i = self.pos + 1;
        let mut saw_dimension = false;
        while self.tokens.get(i) == Some(&Token::LBracket) {
            saw_dimension = true;
            let mut depth = 1usize;
            i += 1;
            // Leave an empty final dimension to the array-append parser.
            if self.tokens.get(i) == Some(&Token::RBracket) {
                return false;
            }
            while i < self.tokens.len() && depth != 0 {
                match &self.tokens[i] {
                    Token::LBracket | Token::LParen => depth += 1,
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
        saw_dimension && self.tokens.get(i) == Some(&Token::Assign)
    }

    fn split_array_access(mut expr: Expr) -> (Expr, Vec<Expr>) {
        let mut indices = Vec::new();
        while let Expr::ArrayAccess { array, index } = expr {
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
                Token::LBracket => depth += 1,
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
        self.advance(); // consume 'list' identifier
        self.expect(&Token::LParen)?;
        let targets = self.parse_list_targets(&Token::RParen)?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::Assign)?;
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon)?;
        Ok(Stmt::ListAssign { targets, expr })
    }

    /// Parse `[$a, $b, ...] = expr;`
    fn parse_short_list_assign(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume '['
        let targets = self.parse_list_targets(&Token::RBracket)?;
        self.expect(&Token::RBracket)?;
        self.expect(&Token::Assign)?;
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon)?;
        Ok(Stmt::ListAssign { targets, expr })
    }

    /// Parse comma-separated list targets (variables, skips, nested brackets).
    /// `end_token` is `)` for list() or `]` for short syntax.
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
            // Check for nested: list(...) or [...]
            if self.peek() == Token::LBracket {
                self.advance(); // consume '['
                let nested = self.parse_list_targets(&Token::RBracket)?;
                self.expect(&Token::RBracket)?;
                targets.push(ListTarget::Nested(nested));
            } else if let Token::Identifier(ref name) = self.peek() {
                if name == "list" && self.peek_at(1) == Token::LParen {
                    self.advance(); // consume 'list'
                    self.expect(&Token::LParen)?;
                    let nested = self.parse_list_targets(&Token::RParen)?;
                    self.expect(&Token::RParen)?;
                    targets.push(ListTarget::Nested(nested));
                } else {
                    return Err(format!(
                        "Expected variable in list/destructuring, got identifier '{}'",
                        name
                    ));
                }
            } else if let Token::Variable(_) = self.peek() {
                let target = self.parse_expr()?;
                match target {
                    Expr::Variable(var) => targets.push(ListTarget::Variable(var)),
                    Expr::ArrayAccess { .. }
                    | Expr::PropertyAccess {
                        nullsafe: false, ..
                    }
                    | Expr::StaticProperty { .. } => targets.push(ListTarget::Target(target)),
                    _ => return Err("Invalid destructuring assignment target".into()),
                }
            } else if matches!(self.peek(), Token::Integer(_) | Token::StringLiteral(_)) {
                // Explicit key: 0 => $var, 'key' => $var
                let key_expr = self.parse_expr()?;
                self.expect(&Token::DoubleArrow)?;
                let var_name = match self.advance() {
                    Token::Variable(n) => n,
                    other => {
                        return Err(format!(
                            "Expected variable after '=>' in list, got {:?}",
                            other
                        ));
                    }
                };
                targets.push(ListTarget::KeyedVariable {
                    key: key_expr,
                    var: var_name,
                });
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
