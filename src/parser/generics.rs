const MAX_GENERIC_ARITY: usize = 127;

#[derive(Clone, Copy)]
enum ReservedStaticRole {
    Class,
    Interface,
    Trait,
    Catch,
}

impl ReservedStaticRole {
    fn diagnostic(self) -> &'static str {
        match self {
            Self::Class => "Cannot use \"static\" as class name, as it is reserved",
            Self::Interface => "Cannot use \"static\" as interface name, as it is reserved",
            Self::Trait => "Cannot use \"static\" as trait name, as it is reserved",
            Self::Catch => "Bad class name in the catch statement",
        }
    }
}

impl Parser {
    fn parse_classlike_declaration_name(
        &mut self,
        expected_kind: &str,
    ) -> Result<(String, usize), String> {
        match self.advance() {
            Token::Identifier(name, line) | Token::Enum { name, line } => Ok((name, line)),
            Token::Static(line) => Err(self.source_error(
                "syntax error, unexpected token \"static\", expecting identifier",
                line,
            )),
            Token::Exit { line, .. } => Err(self.source_error(
                "syntax error, unexpected token \"exit\", expecting identifier",
                line,
            )),
            token => Err(format!("Expected {expected_kind} name, got {token:?}")),
        }
    }

    fn parse_generic_ancestor(&mut self) -> Result<GenericAncestor, String> {
        let name = self.parse_qualified_name()?;
        self.finish_generic_ancestor(name)
    }

    fn parse_generic_ancestor_with_reserved_static(
        &mut self,
        role: ReservedStaticRole,
        diagnostic_line: Option<usize>,
    ) -> Result<GenericAncestor, String> {
        let Token::Static(static_line) = self.peek() else {
            return self.parse_generic_ancestor();
        };
        self.advance();
        self.last_primary_line = Some(static_line);
        self.compile_error(role.diagnostic(), diagnostic_line.unwrap_or(static_line));
        self.finish_generic_ancestor("static".to_string())
    }

    fn finish_generic_ancestor(&mut self, name: String) -> Result<GenericAncestor, String> {
        let arguments = if self.peek() == Token::Less {
            if !GenericRuntimeCapabilities::CONFIGURED.syntax_enabled() {
                return Err(
                    "Generic syntax requires php-generics-erased or php-generics-reified"
                        .to_string(),
                );
            }
            self.parse_generic_type_arguments()?
        } else {
            Vec::new()
        };
        Ok(GenericAncestor { name, arguments })
    }

    fn parse_qualified_name_with_reserved_static(
        &mut self,
        role: ReservedStaticRole,
        diagnostic_line: Option<usize>,
    ) -> Result<String, String> {
        let Token::Static(static_line) = self.peek() else {
            return self.parse_qualified_or_namespace_relative_name();
        };
        self.advance();
        self.last_primary_line = Some(static_line);
        self.compile_error(role.diagnostic(), diagnostic_line.unwrap_or(static_line));
        Ok("static".to_string())
    }

    fn parse_trait_ancestor_list(
        &mut self,
        use_line: usize,
    ) -> Result<(Vec<GenericAncestor>, usize), String> {
        let mut ancestors = Vec::new();
        let mut first_ancestor_line = None;
        loop {
            ancestors.push(self.parse_generic_ancestor_with_reserved_static(
                ReservedStaticRole::Trait,
                first_ancestor_line,
            )?);
            if first_ancestor_line.is_none() {
                first_ancestor_line = Some(self.last_primary_line.unwrap_or(use_line));
            }
            if !matches!(self.peek(), Token::Comma(_)) {
                break;
            }
            self.advance();
        }
        Ok((ancestors, first_ancestor_line.unwrap_or(use_line)))
    }

    /// Parse an optional RFC v0.22 type-parameter list immediately following
    /// a declaration name. The two Cargo features select the runtime model;
    /// without either one the shared engine remains compiled but syntax is rejected.
    fn parse_generic_parameters(&mut self) -> Result<Vec<GenericParameter>, String> {
        if self.peek() != Token::Less {
            return Ok(Vec::new());
        }
        if !cfg!(any(
            feature = "php-generics-erased",
            feature = "php-generics-reified"
        )) {
            return Err(
                "Generic syntax requires php-generics-erased or php-generics-reified"
                    .to_string(),
            );
        }
        self.advance();

        if matches!(self.peek(), Token::Greater | Token::ShiftRight(_)) {
            return Err("A generic parameter list cannot be empty".to_string());
        }

        let mut parameters = Vec::new();
        let mut seen_default = false;
        loop {
            if parameters.len() == MAX_GENERIC_ARITY {
                return Err(format!(
                    "A generic declaration may contain at most {} parameters",
                    MAX_GENERIC_ARITY
                ));
            }

            let variance = match self.peek() {
                Token::Plus => {
                    self.advance();
                    GenericVariance::Covariant
                }
                Token::Minus => {
                    self.advance();
                    GenericVariance::Contravariant
                }
                _ => GenericVariance::Invariant,
            };
            let name = match self.advance() {
                Token::Identifier(name, _) => name,
                other => {
                    return Err(format!(
                        "Expected generic parameter name, got {:?}",
                        other
                    ));
                }
            };

            if parameters
                .iter()
                .any(|parameter: &GenericParameter| parameter.name == name)
            {
                return Err(format!("Duplicate generic parameter {}", name));
            }
            if self.generic_scopes.iter().rev().any(|scope| {
                scope
                    .iter()
                    .any(|parameter| parameter.name == name)
            }) {
                return Err(format!(
                    "Generic parameter {} shadows an outer generic parameter",
                    name
                ));
            }

            let bound = if self.peek() == Token::Colon {
                self.advance();
                Some(self.parse_generic_type_expression()?)
            } else {
                None
            };
            let default = if self.peek() == Token::Assign {
                self.advance();
                seen_default = true;
                Some(self.parse_generic_type_expression()?)
            } else {
                if seen_default {
                    return Err(format!(
                        "Required generic parameter {} follows an optional parameter",
                        name
                    ));
                }
                None
            };

            if bound
                .as_ref()
                .is_some_and(|hint| Self::is_direct_generic_name(hint, &name))
            {
                return Err(format!(
                    "Generic parameter {} cannot use itself as a top-level bound",
                    name
                ));
            }
            if default
                .as_ref()
                .is_some_and(|hint| Self::is_direct_generic_name(hint, &name))
            {
                return Err(format!(
                    "Generic parameter {} cannot use itself as a default",
                    name
                ));
            }
            if let (Some(bound), Some(default)) = (bound.as_ref(), default.as_ref()) {
                if !Self::generic_default_satisfies_bound(default, bound) {
                    return Err(format!(
                        "Generic default for {} does not satisfy its bound",
                        name
                    ));
                }
            }

            parameters.push(GenericParameter {
                name,
                variance,
                bound,
                default,
            });

            match self.peek() {
                Token::Comma(_) => {
                    self.advance();
                    if matches!(self.peek(), Token::Greater | Token::ShiftRight(_)) {
                        return Err("A generic parameter list cannot end with a comma".into());
                    }
                }
                Token::Greater | Token::ShiftRight(_) => {
                    self.consume_generic_close()?;
                    break;
                }
                other => {
                    return Err(format!(
                        "Expected ',' or '>' in generic parameter list, got {:?}",
                        other
                    ));
                }
            }
        }

        for (index, parameter) in parameters.iter().enumerate() {
            let Some(default) = parameter.default.as_ref() else {
                continue;
            };
            for used in Self::generic_names(default) {
                if let Some(used_index) = parameters
                    .iter()
                    .position(|candidate| candidate.name == used)
                {
                    if used_index >= index {
                        return Err(format!(
                            "Generic default for {} references {} before it is declared",
                            parameter.name, used
                        ));
                    }
                }
            }
        }
        Ok(parameters)
    }

    fn parse_generic_type_expression(&mut self) -> Result<TypeHint, String> {
        let first = self.parse_base_type_hint()?;
        self.maybe_parse_compound_type(first)
    }

    fn parse_generic_type_arguments(&mut self) -> Result<Vec<TypeHint>, String> {
        self.expect(&Token::Less)?;
        if matches!(self.peek(), Token::Greater | Token::ShiftRight(_)) {
            return Err("A generic type-argument list cannot be empty".to_string());
        }

        let mut arguments = Vec::new();
        loop {
            if arguments.len() == MAX_GENERIC_ARITY {
                return Err(format!(
                    "A generic type use may contain at most {} arguments",
                    MAX_GENERIC_ARITY
                ));
            }
            arguments.push(self.parse_generic_type_expression()?);
            match self.peek() {
                Token::Comma(_) => {
                    self.advance();
                    if matches!(self.peek(), Token::Greater | Token::ShiftRight(_)) {
                        return Err("A generic type-argument list cannot end with a comma".into());
                    }
                }
                Token::Greater | Token::ShiftRight(_) => {
                    self.consume_generic_close()?;
                    break;
                }
                other => {
                    return Err(format!(
                        "Expected ',' or '>' in generic type-argument list, got {:?}",
                        other
                    ));
                }
            }
        }
        Ok(arguments)
    }

    fn parse_optional_turbofish(&mut self) -> Result<Vec<TypeHint>, String> {
        if self.peek() != Token::DoubleColon || self.peek_at(1) != Token::Less {
            return Ok(Vec::new());
        }
        if !GenericRuntimeCapabilities::CONFIGURED.syntax_enabled() {
            return Err(
                "Generic syntax requires php-generics-erased or php-generics-reified"
                    .to_string(),
            );
        }
        self.advance(); // ::
        self.parse_generic_type_arguments()
    }

    /// The lexer correctly treats `>>` as a shift in expressions. Inside a
    /// generic list it instead closes two nested lists, so split it lazily at
    /// the grammar boundary and leave expression tokenization untouched.
    fn consume_generic_close(&mut self) -> Result<(), String> {
        match self.peek() {
            Token::Greater => {
                self.advance();
                Ok(())
            }
            Token::ShiftRight(_) => {
                self.tokens[self.pos] = Token::Greater;
                self.tokens.insert(self.pos + 1, Token::Greater);
                self.advance();
                Ok(())
            }
            other => Err(format!("Expected '>' in generic type, got {:?}", other)),
        }
    }

    fn push_generic_scope(&mut self, parameters: &[GenericParameter]) {
        self.generic_scopes.push(parameters.to_vec());
    }

    fn pop_generic_scope(&mut self) {
        if !self.generic_scopes.is_empty() {
            self.generic_scopes.pop();
        }
    }

    fn active_generic_parameter(&self, name: &str) -> Option<&GenericParameter> {
        self.generic_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.iter().find(|parameter| parameter.name == name))
    }

    fn generic_parameter_type_hint(&self, name: &str) -> Option<TypeHint> {
        self.active_generic_parameter(name).map(|parameter| {
            let mut visiting = Vec::new();
            let erased = self.erase_generic_parameter(parameter, &mut visiting);
            TypeHint::GenericParameter {
                name: name.to_string(),
                erased: Box::new(erased),
            }
        })
    }

    fn erase_generic_parameter(
        &self,
        parameter: &GenericParameter,
        visiting: &mut Vec<String>,
    ) -> TypeHint {
        if visiting.iter().any(|name| name == &parameter.name) {
            return TypeHint::Mixed;
        }
        visiting.push(parameter.name.clone());
        let erased = parameter
            .bound
            .as_ref()
            .map(|bound| self.erase_generic_type(bound, visiting))
            .unwrap_or(TypeHint::Mixed);
        visiting.pop();
        erased
    }

    fn erase_generic_type(&self, hint: &TypeHint, visiting: &mut Vec<String>) -> TypeHint {
        match hint {
            TypeHint::ClassName(name) => self
                .active_generic_parameter(name)
                .map(|parameter| self.erase_generic_parameter(parameter, visiting))
                .unwrap_or_else(|| TypeHint::ClassName(name.clone())),
            TypeHint::Nullable(inner) => {
                TypeHint::Nullable(Box::new(self.erase_generic_type(inner, visiting)))
            }
            TypeHint::Union(types) => TypeHint::Union(
                types
                    .iter()
                    .map(|part| self.erase_generic_type(part, visiting))
                    .collect(),
            ),
            TypeHint::Intersection(types) => TypeHint::Intersection(
                types
                    .iter()
                    .map(|part| self.erase_generic_type(part, visiting))
                    .collect(),
            ),
            TypeHint::GenericParameter { erased, .. } => self.erase_generic_type(erased, visiting),
            TypeHint::GenericApplication { base, .. } => TypeHint::ClassName(base.clone()),
            other => other.clone(),
        }
    }

    fn is_direct_generic_name(hint: &TypeHint, name: &str) -> bool {
        matches!(hint, TypeHint::ClassName(candidate) if candidate == name)
    }

    fn generic_names(hint: &TypeHint) -> Vec<String> {
        let mut names = Vec::new();
        Self::collect_generic_names(hint, &mut names);
        names
    }

    fn collect_generic_names(hint: &TypeHint, names: &mut Vec<String>) {
        match hint {
            TypeHint::ClassName(name) => names.push(name.clone()),
            TypeHint::Nullable(inner) => Self::collect_generic_names(inner, names),
            TypeHint::Union(parts) | TypeHint::Intersection(parts) => {
                for part in parts {
                    Self::collect_generic_names(part, names);
                }
            }
            TypeHint::GenericApplication { arguments, .. } => {
                for argument in arguments {
                    Self::collect_generic_names(argument, names);
                }
            }
            TypeHint::GenericParameter { name, .. } => names.push(name.clone()),
            _ => {}
        }
    }

    fn generic_default_satisfies_bound(default: &TypeHint, bound: &TypeHint) -> bool {
        if matches!(bound, TypeHint::Mixed) || default == bound {
            return true;
        }
        match bound {
            TypeHint::Union(parts) => parts
                .iter()
                .any(|part| Self::generic_default_satisfies_bound(default, part)),
            TypeHint::Intersection(parts) => parts
                .iter()
                .all(|part| Self::generic_default_satisfies_bound(default, part)),
            // Class hierarchy conformance belongs to the link phase. Do not
            // reject unresolved named types while parsing the source unit.
            TypeHint::ClassName(_) | TypeHint::GenericApplication { .. } => true,
            _ => false,
        }
    }
}
