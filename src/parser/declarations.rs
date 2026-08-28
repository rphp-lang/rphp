#[derive(Debug, Clone, Copy)]
enum DuplicateMemberModifier {
    Access,
    Static,
    Final,
    Abstract,
    Readonly,
}

impl DuplicateMemberModifier {
    fn message(self) -> &'static str {
        match self {
            Self::Access => "Multiple access type modifiers are not allowed",
            Self::Static => "Multiple static modifiers are not allowed",
            Self::Final => "Multiple final modifiers are not allowed",
            Self::Abstract => "Multiple abstract modifiers are not allowed",
            Self::Readonly => "Multiple readonly modifiers are not allowed",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct MemberModifiers {
    visibility: Visibility,
    set_visibility: Option<Visibility>,
    has_visibility: bool,
    duplicate: Option<DuplicateMemberModifier>,
    is_static: bool,
    is_final: bool,
    is_readonly: bool,
    readonly_line: Option<usize>,
    is_abstract: bool,
}

impl Default for MemberModifiers {
    fn default() -> Self {
        Self {
            visibility: Visibility::Public,
            set_visibility: None,
            has_visibility: false,
            duplicate: None,
            is_static: false,
            is_final: false,
            is_readonly: false,
            readonly_line: None,
            is_abstract: false,
        }
    }
}

impl Parser {
    fn defer_duplicate_member_modifier(&mut self, modifiers: &MemberModifiers, line: usize) {
        if let Some(duplicate) = modifiers.duplicate {
            let _ = self.compile_error(duplicate.message(), line);
        }
    }

    fn defer_method_modifier_diagnostics(
        &mut self,
        modifiers: &MemberModifiers,
        method_line: usize,
    ) {
        self.defer_duplicate_member_modifier(modifiers, method_line);
        if modifiers.is_readonly {
            let line = modifiers.readonly_line.unwrap_or(method_line);
            let _ = self.compile_error("Cannot use the readonly modifier on a method", line);
        }
    }

    fn parse_trait_alias_adaptation(
        &mut self,
        trait_name: Option<String>,
        method: String,
    ) -> Result<TraitAlias, String> {
        self.expect(&Token::As(0))?;
        let mut is_final = false;
        let visibility = match self.peek() {
            Token::Public => Some(Visibility::Public),
            Token::Protected => Some(Visibility::Protected),
            Token::Private => Some(Visibility::Private),
            Token::Final(_) => {
                is_final = true;
                None
            }
            Token::Static(_) => {
                let line = self.closest_token_source_line();
                let _ = self.compile_error(
                    "Cannot use \"static\" as method modifier in trait alias",
                    line,
                );
                None
            }
            Token::Abstract(line) => {
                let _ = self.compile_error(
                    "Cannot use \"abstract\" as method modifier in trait alias",
                    line,
                );
                None
            }
            _ => None,
        };
        if visibility.is_some()
            || is_final
            || matches!(self.peek(), Token::Static(_) | Token::Abstract(_))
        {
            self.advance();
        }
        if let Token::Identifier(ref modifier, line) = self.peek()
            && modifier.eq_ignore_ascii_case("readonly")
        {
            self.advance();
            let _ = self.compile_error("Cannot use the readonly modifier on a method", line);
        }
        let alias = if matches!(self.peek(), Token::Semicolon(_)) {
            None
        } else {
            let token = self.advance();
            Some(
                Self::token_as_named_arg_label(&token)
                    .ok_or_else(|| format!("Expected trait method alias, got {token:?}"))?,
            )
        };
        self.expect(&Token::Semicolon(0))?;
        Ok(TraitAlias {
            trait_name,
            method,
            alias,
            visibility,
            is_final,
        })
    }

    pub(super) fn parse_promoted_property_hook_list(
        &mut self,
        property: &mut ClassProperty,
    ) -> Result<Vec<ClassMethod>, String> {
        self.expect(&Token::LBrace(0))?;
        let mut hook_methods = Vec::new();
        if self.peek() == Token::RBrace {
            self.compile_error("Property hook list must not be empty", property.line);
        }
        while self.peek() != Token::RBrace && !self.at_eof() {
            let hook_attributes = self.parse_attribute_groups()?;
            let hook_is_final = if matches!(self.peek(), Token::Final(_)) {
                self.advance();
                true
            } else {
                false
            };
            let invalid_modifier = match self.peek() {
                Token::Public => Some("public"),
                Token::Protected => Some("protected"),
                Token::Private => Some("private"),
                Token::Static(_) => Some("static"),
                _ => None,
            };
            if let Some(modifier) = invalid_modifier {
                self.advance();
                self.compile_error(
                    format!("Cannot use the {modifier} modifier on a property hook"),
                    property.line,
                );
            }
            let hook_returns_by_ref = if self.peek() == Token::Ampersand {
                self.advance();
                true
            } else {
                false
            };
            let (hook, hook_line) = match self.advance() {
                Token::Identifier(name, line) => (name, line),
                other => return Err(format!("Expected property hook, got {other:?}")),
            };
            let is_get = hook.eq_ignore_ascii_case("get");
            let is_set = hook.eq_ignore_ascii_case("set");
            if (is_get && property.has_get_hook) || (is_set && property.has_set_hook) {
                self.compile_error(
                    format!("Cannot redeclare property hook \"{}\"", hook.to_ascii_lowercase()),
                    hook_line,
                );
            }
            let params = if is_get && matches!(self.peek(), Token::LParen(_)) {
                self.expect_lparen()?;
                let mut params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                if params.is_empty() {
                    params.push(Param {
                        attributes: Vec::new(),
                        name: "\0property_get_parameter_list".to_string(),
                        line: hook_line,
                        default: None,
                        is_variadic: false,
                        is_ref: false,
                        type_hint: None,
                        promotion: None,
                        promoted_property: None,
                        promotion_hooks: Vec::new(),
                    });
                }
                params
            } else if is_get {
                Vec::new()
            } else if matches!(self.peek(), Token::LParen(_)) {
                self.expect_lparen()?;
                let mut params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                if params.is_empty() {
                    params.push(Param {
                        attributes: Vec::new(),
                        name: "\0property_set_parameter_list".to_string(),
                        line: hook_line,
                        default: None,
                        is_variadic: false,
                        is_ref: false,
                        type_hint: None,
                        promotion: None,
                        promoted_property: None,
                        promotion_hooks: Vec::new(),
                    });
                }
                params
            } else {
                vec![Param {
                    attributes: Vec::new(),
                    name: "value".to_string(),
                    line: hook_line,
                    default: None,
                    is_variadic: false,
                    is_ref: false,
                    type_hint: property.type_hint.clone(),
                    promotion: None,
                    promoted_property: None,
                    promotion_hooks: Vec::new(),
                }]
            };
            let (body, hook_is_abstract) = if matches!(self.peek(), Token::Semicolon(_)) {
                self.advance();
                (Vec::new(), true)
            } else if self.peek() == Token::DoubleArrow {
                self.advance();
                let expression = self.parse_expr()?;
                self.expect(&Token::Semicolon(0))?;
                let body = if is_get {
                    vec![Stmt::Return {
                        expr: Some(expression),
                        line: hook_line,
                    }]
                } else {
                    vec![Stmt::AssignProp {
                        object: Expr::Variable {
                            name: "this".to_string(),
                            line: hook_line,
                        },
                        property: property.name.clone(),
                        expr: expression,
                        line: hook_line,
                    }]
                };
                (body, false)
            } else {
                self.expect(&Token::LBrace(0))?;
                let mut body = Vec::new();
                while self.peek() != Token::RBrace && !self.at_eof() {
                    body.push(self.parse_stmt_in_scope(false)?);
                }
                self.expect(&Token::RBrace)?;
                (body, false)
            };
            property.has_get_hook |= is_get;
            property.has_set_hook |= is_set;
            property.has_abstract_get_hook |= is_get && hook_is_abstract;
            property.has_abstract_set_hook |= is_set && hook_is_abstract;
            hook_methods.push(ClassMethod {
                line: hook_line,
                attributes: hook_attributes,
                visibility: property.visibility,
                name: format!("${}::{}", property.name, hook.to_ascii_lowercase()),
                params,
                body,
                is_static: false,
                is_final: hook_is_final,
                is_abstract: hook_is_abstract,
                returns_by_ref: hook_returns_by_ref,
                return_type: is_get.then(|| property.type_hint.clone()).flatten(),
                generic_params: Vec::new(),
            });
        }
        self.expect(&Token::RBrace)?;
        Ok(hook_methods)
    }

    fn parse_property_declaration(
        &mut self,
        modifiers: &MemberModifiers,
        attributes: &[Attribute],
    ) -> Result<(Vec<ClassProperty>, Vec<ClassMethod>), String> {
        let type_hint = self.try_parse_type_hint(false)?;
        let mut properties = Vec::new();
        let mut hook_methods = Vec::new();
        loop {
            let (name, line) = match self.advance() {
                Token::Variable(name, line) => (name, line),
                other => return Err(format!("Expected property variable, got {other:?}")),
            };
            if properties.is_empty() {
                self.defer_duplicate_member_modifier(modifiers, line);
            }
            let default = if self.peek() == Token::Assign {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            properties.push(ClassProperty {
                attributes: attributes.to_vec(),
                line,
                visibility: modifiers.visibility,
                set_visibility: modifiers.set_visibility,
                name,
                type_hint: type_hint.clone(),
                default,
                is_static: modifiers.is_static,
                is_readonly: modifiers.is_readonly,
                is_final: modifiers.is_final,
                is_abstract: modifiers.is_abstract,
                has_get_hook: false,
                has_abstract_get_hook: false,
                has_set_hook: false,
                has_abstract_set_hook: false,
            });
            if !matches!(self.peek(), Token::Comma(_)) {
                break;
            }
            self.advance();
        }
        if matches!(self.peek(), Token::LBrace(_)) {
            if properties.len() != 1 {
                return Err("Hooked properties cannot declare multiple properties".into());
            }
            if modifiers.is_static {
                self.compile_error("Cannot declare hooks for static property", properties[0].line);
            }
            self.advance();
            let property = properties.last_mut().unwrap();
            if self.peek() == Token::RBrace {
                self.compile_error("Property hook list must not be empty", property.line);
            }
            while self.peek() != Token::RBrace && !self.at_eof() {
                let hook_attributes = self.parse_attribute_groups()?;
                let hook_is_final = if matches!(self.peek(), Token::Final(_)) {
                    self.advance();
                    true
                } else {
                    false
                };
                let invalid_modifier = match self.peek() {
                    Token::Public => Some("public"),
                    Token::Protected => Some("protected"),
                    Token::Private => Some("private"),
                    Token::Static(_) => Some("static"),
                    _ => None,
                };
                if let Some(modifier) = invalid_modifier {
                    self.advance();
                    self.compile_error(
                        format!("Cannot use the {modifier} modifier on a property hook"),
                        property.line,
                    );
                }
                let hook_returns_by_ref = if self.peek() == Token::Ampersand {
                    self.advance();
                    true
                } else {
                    false
                };
                let (hook, hook_line) = match self.advance() {
                    Token::Identifier(name, line) => (name, line),
                    other => return Err(format!("Expected property hook, got {other:?}")),
                };
                let is_get = hook.eq_ignore_ascii_case("get");
                let is_set = hook.eq_ignore_ascii_case("set");
                if (is_get && property.has_get_hook) || (is_set && property.has_set_hook) {
                    self.compile_error(
                        format!("Cannot redeclare property hook \"{}\"", hook.to_ascii_lowercase()),
                        hook_line,
                    );
                }
                let params = if is_get && matches!(self.peek(), Token::LParen(_)) {
                    self.expect_lparen()?;
                    let mut params = self.parse_param_list()?;
                    self.expect(&Token::RParen)?;
                    if params.is_empty() {
                        // Preserve the otherwise invisible presence of `()` so the class-aware
                        // compiler can emit PHP's property-qualified declaration diagnostic.
                        params.push(Param {
                            attributes: Vec::new(),
                            name: "\0property_get_parameter_list".to_string(),
                            line: hook_line,
                            default: None,
                            is_variadic: false,
                            is_ref: false,
                            type_hint: None,
                            promotion: None,
                            promoted_property: None,
                            promotion_hooks: Vec::new(),
                        });
                    }
                    params
                } else if is_get {
                    Vec::new()
                } else if matches!(self.peek(), Token::LParen(_)) {
                    self.expect_lparen()?;
                    let mut params = self.parse_param_list()?;
                    self.expect(&Token::RParen)?;
                    if params.is_empty() {
                        // As above, an internal sentinel carries the empty-list syntax only as
                        // far as the declaration validator; it can never enter executable code.
                        params.push(Param {
                            attributes: Vec::new(),
                            name: "\0property_set_parameter_list".to_string(),
                            line: hook_line,
                            default: None,
                            is_variadic: false,
                            is_ref: false,
                            type_hint: None,
                            promotion: None,
                            promoted_property: None,
                            promotion_hooks: Vec::new(),
                        });
                    }
                    params
                } else {
                    vec![Param {
                        attributes: Vec::new(),
                        name: "value".to_string(),
                        line: hook_line,
                        default: None,
                        is_variadic: false,
                        is_ref: false,
                        type_hint: property.type_hint.clone(),
                        promotion: None,
                        promoted_property: None,
                        promotion_hooks: Vec::new(),
                    }]
                };
                let (body, hook_is_abstract) = if matches!(self.peek(), Token::Semicolon(_)) {
                    self.advance();
                    (Vec::new(), true)
                } else if self.peek() == Token::DoubleArrow {
                    self.advance();
                    let expression = self.parse_expr()?;
                    self.expect(&Token::Semicolon(0))?;
                    let body = if is_get {
                        vec![Stmt::Return {
                            expr: Some(expression),
                            line: hook_line,
                        }]
                    } else {
                        vec![Stmt::AssignProp {
                            object: Expr::Variable {
                                name: "this".to_string(),
                                line: hook_line,
                            },
                            property: property.name.clone(),
                            expr: expression,
                            line: hook_line,
                        }]
                    };
                    (body, false)
                } else {
                    self.expect(&Token::LBrace(0))?;
                    let mut body = Vec::new();
                    while self.peek() != Token::RBrace && !self.at_eof() {
                        body.push(self.parse_stmt_in_scope(false)?);
                    }
                    self.expect(&Token::RBrace)?;
                    (body, false)
                };
                property.has_get_hook |= is_get;
                property.has_set_hook |= is_set;
                property.has_abstract_get_hook |= is_get && hook_is_abstract;
                property.has_abstract_set_hook |= is_set && hook_is_abstract;
                hook_methods.push(ClassMethod {
                    line: hook_line,
                    attributes: hook_attributes,
                    visibility: property.visibility,
                    name: format!("${}::{}", property.name, hook.to_ascii_lowercase()),
                    params,
                    body,
                    is_static: false,
                    is_final: hook_is_final,
                    is_abstract: hook_is_abstract,
                    returns_by_ref: hook_returns_by_ref,
                    return_type: is_get.then(|| property.type_hint.clone()).flatten(),
                    generic_params: Vec::new(),
                });
            }
            self.expect(&Token::RBrace)?;
            return Ok((properties, hook_methods));
        }
        self.expect(&Token::Semicolon(0))?;
        Ok((properties, hook_methods))
    }

    /// Consume the enum-case-shaped member grammar that PHP accepts in every
    /// classlike body before rejecting it at the compile-error boundary. Keep
    /// the member out of the AST: a non-enum case has no declaration semantics,
    /// but parsing its complete shape lets later syntax errors retain priority.
    fn parse_non_enum_case_declaration(&mut self) -> Result<usize, String> {
        debug_assert!(matches!(self.peek(), Token::Case(_)));
        self.advance();
        let case_line = match self.advance() {
            Token::Identifier(_, line) | Token::Enum { line, .. } | Token::Exit { line, .. } => {
                line
            }
            Token::Semicolon(line) => {
                return Err(self.source_error(
                    "syntax error, unexpected token \";\"",
                    line,
                ));
            }
            token => return Err(format!("Expected case name, got {token:?}")),
        };
        let _ = self.compile_error("Case can only be used in enums", case_line);
        if self.peek() == Token::Assign {
            self.advance();
            self.parse_expr()?;
        }
        self.expect(&Token::Semicolon(0))?;
        Ok(case_line)
    }

    fn parse_anonymous_class_body(
        &mut self,
    ) -> Result<
        (
            Vec<ClassProperty>,
            Vec<ClassConstant>,
            Vec<ClassMethod>,
            Vec<GenericAncestor>,
            Vec<TraitAlias>,
            Option<usize>,
        ),
        String,
    > {
        let mut properties = Vec::new();
        let mut constants = Vec::new();
        let mut methods = Vec::new();
        let mut uses = Vec::new();
        let mut trait_aliases = Vec::new();
        let mut invalid_case_line = None;
        let previous_class_body = self.in_class_body;
        let previous_class_scope = self.class_scope_active;
        self.in_class_body = true;
        self.class_scope_active = true;

        while self.peek() != Token::RBrace && !self.at_eof() {
            let member_start = self.pos;
            let attributes = self.parse_attribute_groups()?;
            if matches!(self.peek(), Token::Case(_)) {
                let line = self.parse_non_enum_case_declaration()?;
                invalid_case_line.get_or_insert(line);
                continue;
            }
            if matches!(self.peek(), Token::Use(_)) {
                let use_line = match self.advance() {
                    Token::Use(line) => line,
                    _ => unreachable!("trait use parser starts at use"),
                };
                let (trait_uses, adaptation_line) =
                    self.parse_trait_ancestor_list(use_line)?;
                uses.extend(trait_uses);
                if matches!(self.peek(), Token::LBrace(_)) {
                    self.advance();
                    while self.peek() != Token::RBrace && !self.at_eof() {
                        let (trait_name, method) =
                            self.parse_trait_method_reference(adaptation_line)?;
                        trait_aliases
                            .push(self.parse_trait_alias_adaptation(trait_name, method)?);
                    }
                    self.expect(&Token::RBrace)?;
                } else {
                    self.expect(&Token::Semicolon(0))?;
                }
                continue;
            }
            let modifiers = self.parse_member_modifiers();
            if matches!(self.peek(), Token::Function(_)) {
                let line = match self.advance() {
                    Token::Function(line) => line,
                    _ => unreachable!("method parser starts at function"),
                };
                self.defer_method_modifier_diagnostics(&modifiers, line);
                // PHP permits functions and methods to declare a reference
                // return with an ampersand before the name. The runtime's
                // return-reference contract is a separate compatibility
                // slice, but retaining the declaration as an ordinary method
                // keeps unexercised library helpers loadable.
                let returns_by_ref = self.peek() == Token::Ampersand;
                self.consume_reference_return_marker();
                let token = self.advance();
                let method_name = Self::token_as_named_arg_label(&token)
                    .ok_or_else(|| format!("Expected method name, got {token:?}"))?;
                let generic_params = self.parse_generic_parameters()?;
                self.push_generic_scope(&generic_params);
                self.expect_lparen()?;
                let params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                let promoted_hooks = params
                    .iter()
                    .flat_map(|parameter| parameter.promotion_hooks.iter().cloned())
                    .collect::<Vec<_>>();
                let return_type = self.parse_return_type(line, false)?;
                let body = self.parse_method_body(&modifiers, line, false, false)?;
                if modifiers.is_abstract {
                    self.compile_error(
                        format!("Anonymous class method {method_name}() must not be abstract"),
                        line,
                    );
                }
                self.pop_generic_scope();
                methods.push(ClassMethod {
                    line,
                    attributes: attributes.clone(),
                    visibility: modifiers.visibility,
                    name: method_name,
                    params,
                    body,
                    is_static: modifiers.is_static,
                    is_final: modifiers.is_final,
                    is_abstract: modifiers.is_abstract,
                    returns_by_ref,
                    return_type,
                    generic_params,
                });
                methods.extend(promoted_hooks);
            } else if self.peek() == Token::Const {
                constants.extend(self.parse_class_constant_declaration(
                    &modifiers,
                    false,
                    &attributes,
                    self.class_member_doc_comment(member_start, self.pos),
                )?);
            } else if let Token::Case(line) = self.peek()
                && modifiers.has_visibility
            {
                return Err(self.source_error(
                    "syntax error, unexpected token \"case\", expecting variable",
                    line,
                ));
            } else if matches!(self.peek(), Token::Variable(_, _)) || self.is_type_hint_start() {
                let (declared, hooks) =
                    self.parse_property_declaration(&modifiers, &attributes)?;
                properties.extend(declared);
                methods.extend(hooks);
            } else {
                return Err(format!(
                    "Unexpected token in anonymous class body: {:?}",
                    self.peek()
                ));
            }
        }
        self.expect(&Token::RBrace)?;
        self.in_class_body = previous_class_body;
        self.class_scope_active = previous_class_scope;
        Ok((
            properties,
            constants,
            methods,
            uses,
            trait_aliases,
            invalid_case_line,
        ))
    }

    /// Parse try { } catch (Type $e) { } finally { }
    fn parse_try_catch(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'try'
        self.expect(&Token::LBrace(0))?;
        let mut try_body = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            try_body.push(self.parse_stmt_in_scope(false)?);
        }
        self.expect(&Token::RBrace)?;
        if self.halted {
            return Ok(Stmt::TryCatch {
                try_body,
                catches: Vec::new(),
                finally_body: None,
            });
        }

        let mut catches = Vec::new();
        while self.peek() == Token::Catch {
            self.advance(); // consume 'catch'
            let catch_line = self.expect_lparen()?;
            // Parse exception type(s): ExA | ExB
            let mut types = Vec::new();
            let type_name = self.parse_qualified_name_with_reserved_static(
                ReservedStaticRole::Catch,
                None,
            )?;
            types.push(type_name);
            while self.peek() == Token::Pipe {
                self.advance();
                let t = self.parse_qualified_name_with_reserved_static(
                    ReservedStaticRole::Catch,
                    Some(catch_line),
                )?;
                types.push(t);
            }
            let var = match self.peek() {
                Token::Variable(_, _) => match self.advance() {
                    Token::Variable(n, _) => Some(n),
                    _ => unreachable!(),
                },
                Token::This(line) => {
                    self.advance();
                    let _ = self.compile_error("Cannot re-assign $this", line);
                    Some("this".to_string())
                }
                Token::RParen => None,
                ref other => {
                    return Err(format!(
                        "Expected variable or ')' in catch, got {:?}",
                        other
                    ));
                }
            };
            self.expect(&Token::RParen)?;
            self.expect(&Token::LBrace(0))?;
            let mut body = Vec::new();
            while self.peek() != Token::RBrace && !self.at_eof() {
                body.push(self.parse_stmt_in_scope(false)?);
            }
            self.expect(&Token::RBrace)?;
            catches.push(CatchClause { types, var, body });
        }

        let finally_body = if self.peek() == Token::Finally {
            self.advance();
            self.expect(&Token::LBrace(0))?;
            let mut body = Vec::new();
            while self.peek() != Token::RBrace && !self.at_eof() {
                body.push(self.parse_stmt_in_scope(false)?);
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
        let mut is_readonly = false;
        // Consume leading modifiers in any order before `class`.
        loop {
            match self.peek() {
                Token::Abstract(line) => {
                    self.advance();
                    if is_final {
                        let _ = self.compile_error(
                            "Cannot use the final modifier on an abstract class",
                            line,
                        );
                    }
                    is_abstract = true;
                }
                Token::Final(line) => {
                    self.advance();
                    if is_abstract {
                        let _ = self.compile_error(
                            "Cannot use the final modifier on an abstract class",
                            line,
                        );
                    }
                    is_final = true;
                }
                Token::Identifier(ref name, line) if name.eq_ignore_ascii_case("readonly") => {
                    self.advance();
                    if is_readonly {
                        let _ = self.compile_error(
                            "Multiple readonly modifiers are not allowed",
                            line,
                        );
                    }
                    is_readonly = true;
                }
                _ => break,
            }
        }
        self.advance(); // consume 'class'
        let (name, line) = self.parse_classlike_declaration_name("class")?;
        let generic_params = self.parse_generic_parameters()?;
        self.push_generic_scope(&generic_params);
        let parent = if self.peek() == Token::Extends {
            self.advance();
            Some(self.parse_generic_ancestor_with_reserved_static(
                ReservedStaticRole::Class,
                Some(line),
            )?)
        } else {
            None
        };
        let implements = if self.peek() == Token::Implements {
            self.advance();
            let mut ifaces = Vec::new();
            loop {
                ifaces.push(self.parse_generic_ancestor_with_reserved_static(
                    ReservedStaticRole::Interface,
                    Some(line),
                )?);
                if matches!(self.peek(), Token::Comma(_)) {
                    self.advance();
                } else {
                    break;
                }
            }
            ifaces
        } else {
            Vec::new()
        };
        self.expect(&Token::LBrace(0))?;

        let mut properties = Vec::new();
        let mut constants = Vec::new();
        let mut methods = Vec::new();
        let mut uses = Vec::new();
        let mut trait_aliases = Vec::new();
        let mut trait_precedences = Vec::new();
        let mut invalid_case_line = None;

        let prev_in_class = self.in_class_body;
        self.in_class_body = true;

        while self.peek() != Token::RBrace && !self.at_eof() {
            let member_start = self.pos;
            let attributes = self.parse_attribute_groups()?;
            if matches!(self.peek(), Token::Case(_)) {
                let line = self.parse_non_enum_case_declaration()?;
                invalid_case_line.get_or_insert(line);
                continue;
            }
            // Trait `use` statements: use Foo, Bar;
            if matches!(self.peek(), Token::Use(_)) {
                let use_line = match self.advance() {
                    Token::Use(line) => line,
                    _ => unreachable!("trait use parser starts at use"),
                };
                let (trait_uses, adaptation_line) =
                    self.parse_trait_ancestor_list(use_line)?;
                uses.extend(trait_uses);
                if matches!(self.peek(), Token::LBrace(_)) {
                    self.advance();
                    while self.peek() != Token::RBrace && !self.at_eof() {
                        let (trait_name, method) =
                            self.parse_trait_method_reference(adaptation_line)?;
                        if self.peek() == Token::Insteadof {
                            self.advance();
                            let Some(trait_name) = trait_name else {
                                return Err("Trait precedence requires an explicit trait name".into());
                            };
                            let mut instead_of = Vec::new();
                            loop {
                                instead_of.push(self.parse_qualified_name_with_reserved_static(
                                    ReservedStaticRole::Trait,
                                    Some(adaptation_line),
                                )?);
                                if matches!(self.peek(), Token::Comma(_)) {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                            self.expect(&Token::Semicolon(0))?;
                            trait_precedences.push(TraitPrecedence {
                                trait_name,
                                method,
                                instead_of,
                            });
                            continue;
                        }
                        trait_aliases
                            .push(self.parse_trait_alias_adaptation(trait_name, method)?);
                    }
                    self.expect(&Token::RBrace)?;
                } else {
                    self.expect(&Token::Semicolon(0))?;
                }
                continue;
            }

            let modifiers = self.parse_member_modifiers();

            if matches!(self.peek(), Token::Function(_)) {
                // Method
                let line = match self.advance() {
                    Token::Function(line) => line,
                    _ => unreachable!("method parser starts at function"),
                };
                self.defer_method_modifier_diagnostics(&modifiers, line);
                let returns_by_ref = self.peek() == Token::Ampersand;
                self.consume_reference_return_marker();
                let token = self.advance();
                let method_name = Self::token_as_named_arg_label(&token)
                    .ok_or_else(|| format!("Expected method name, got {:?}", token))?;
                let previous_class_scope = self.class_scope_active;
                self.class_scope_active = true;
                let generic_params = self.parse_generic_parameters()?;
                self.push_generic_scope(&generic_params);
                self.expect_lparen()?;
                let params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                let promoted_hooks = params
                    .iter()
                    .flat_map(|parameter| parameter.promotion_hooks.iter().cloned())
                    .collect::<Vec<_>>();
                let return_type = self.parse_return_type(line, false)?;
                let body = self.parse_method_body(&modifiers, line, false, false)?;
                self.pop_generic_scope();
                self.class_scope_active = previous_class_scope;
                methods.push(ClassMethod {
                    line,
                    attributes: attributes.clone(),
                    visibility: modifiers.visibility,
                    name: method_name,
                    params,
                    body,
                    is_static: modifiers.is_static,
                    is_final: modifiers.is_final,
                    is_abstract: modifiers.is_abstract,
                    returns_by_ref,
                    return_type,
                    generic_params,
                });
                methods.extend(promoted_hooks);
            } else if self.peek() == Token::Const {
                constants.extend(self.parse_class_constant_declaration(
                    &modifiers,
                    false,
                    &attributes,
                    self.class_member_doc_comment(member_start, self.pos),
                )?);
            } else if let Token::Case(line) = self.peek()
                && modifiers.has_visibility
            {
                return Err(self.source_error(
                    "syntax error, unexpected token \"case\", expecting variable",
                    line,
                ));
            } else if matches!(self.peek(), Token::Variable(_, _)) || self.is_type_hint_start() {
                // Property — possibly with type hint: `private int $x = 0;`
                let (declared, hooks) =
                    self.parse_property_declaration(&modifiers, &attributes)?;
                properties.extend(declared);
                methods.extend(hooks);
            } else {
                return Err(format!("Unexpected token in class body: {:?}", self.peek()));
            }
        }
        self.in_class_body = prev_in_class;
        self.expect(&Token::RBrace)?;
        self.pop_generic_scope();

        Ok(Stmt::Class {
            line,
            attributes: invalid_case_line
                .map(Attribute::non_enum_case_marker)
                .into_iter()
                .collect(),
            name,
            parent,
            implements,
            is_abstract,
            is_final,
            is_readonly,
            allow_dynamic_properties: false,
            properties,
            constants,
            methods,
            uses,
            trait_aliases,
            trait_precedences,
            generic_params,
        })
    }

    /// Parse trait declaration
    fn parse_trait(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'trait'
        let (name, line) = self.parse_classlike_declaration_name("trait")?;
        let generic_params = self.parse_generic_parameters()?;
        self.push_generic_scope(&generic_params);
        let invalid_relation = match self.peek() {
            Token::Extends => Some("extends"),
            Token::Implements => Some("implements"),
            _ => None,
        };
        if let Some(relation) = invalid_relation {
            return Err(self.source_error(
                &format!(
                    "syntax error, unexpected token \"{relation}\", expecting \"{{\""
                ),
                line,
            ));
        }
        self.expect(&Token::LBrace(0))?;

        let mut properties = Vec::new();
        let mut constants = Vec::new();
        let mut methods = Vec::new();
        let mut uses = Vec::new();
        let mut trait_aliases = Vec::new();
        let mut trait_precedences = Vec::new();
        let mut invalid_case_line = None;

        while self.peek() != Token::RBrace && !self.at_eof() {
            let member_start = self.pos;
            let attributes = self.parse_attribute_groups()?;
            if matches!(self.peek(), Token::Case(_)) {
                let line = self.parse_non_enum_case_declaration()?;
                invalid_case_line.get_or_insert(line);
                continue;
            }
            if matches!(self.peek(), Token::Use(_)) {
                let use_line = match self.advance() {
                    Token::Use(line) => line,
                    _ => unreachable!("trait use parser starts at use"),
                };
                let (trait_uses, adaptation_line) =
                    self.parse_trait_ancestor_list(use_line)?;
                uses.extend(trait_uses);
                if matches!(self.peek(), Token::LBrace(_)) {
                    self.advance();
                    while self.peek() != Token::RBrace && !self.at_eof() {
                        let (trait_name, method) =
                            self.parse_trait_method_reference(adaptation_line)?;
                        if self.peek() == Token::Insteadof {
                            self.advance();
                            let Some(trait_name) = trait_name else {
                                return Err("Trait precedence requires an explicit trait name".into());
                            };
                            let mut instead_of = Vec::new();
                            loop {
                                instead_of.push(self.parse_qualified_name_with_reserved_static(
                                    ReservedStaticRole::Trait,
                                    Some(adaptation_line),
                                )?);
                                if matches!(self.peek(), Token::Comma(_)) {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                            self.expect(&Token::Semicolon(0))?;
                            trait_precedences.push(TraitPrecedence {
                                trait_name,
                                method,
                                instead_of,
                            });
                            continue;
                        }
                        trait_aliases
                            .push(self.parse_trait_alias_adaptation(trait_name, method)?);
                    }
                    self.expect(&Token::RBrace)?;
                } else {
                    self.expect(&Token::Semicolon(0))?;
                }
                continue;
            }
            let modifiers = self.parse_member_modifiers();

            if matches!(self.peek(), Token::Function(_)) {
                let line = match self.advance() {
                    Token::Function(line) => line,
                    _ => unreachable!("method parser starts at function"),
                };
                self.defer_method_modifier_diagnostics(&modifiers, line);
                let returns_by_ref = self.peek() == Token::Ampersand;
                self.consume_reference_return_marker();
                let token = self.advance();
                let method_name = Self::token_as_named_arg_label(&token)
                    .ok_or_else(|| format!("Expected method name, got {:?}", token))?;
                let previous_class_scope = self.class_scope_active;
                self.class_scope_active = true;
                let method_generic_params = self.parse_generic_parameters()?;
                self.push_generic_scope(&method_generic_params);
                self.expect_lparen()?;
                let params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                let promoted_hooks = params
                    .iter()
                    .flat_map(|parameter| parameter.promotion_hooks.iter().cloned())
                    .collect::<Vec<_>>();
                let return_type = self.parse_return_type(line, false)?;
                let body = self.parse_method_body(&modifiers, line, true, false)?;
                self.pop_generic_scope();
                self.class_scope_active = previous_class_scope;
                methods.push(ClassMethod {
                    line,
                    attributes: attributes.clone(),
                    visibility: modifiers.visibility,
                    name: method_name,
                    params,
                    body,
                    is_static: modifiers.is_static,
                    is_final: modifiers.is_final,
                    is_abstract: modifiers.is_abstract,
                    returns_by_ref,
                    return_type,
                    generic_params: method_generic_params,
                });
                methods.extend(promoted_hooks);
            } else if self.peek() == Token::Const {
                constants.extend(self.parse_class_constant_declaration(
                    &modifiers,
                    false,
                    &attributes,
                    self.class_member_doc_comment(member_start, self.pos),
                )?);
            } else if let Token::Case(line) = self.peek()
                && modifiers.has_visibility
            {
                return Err(self.source_error(
                    "syntax error, unexpected token \"case\", expecting variable",
                    line,
                ));
            } else if matches!(self.peek(), Token::Variable(_, _)) || self.is_type_hint_start() {
                // Property — possibly with type hint
                let (declared, hooks) =
                    self.parse_property_declaration(&modifiers, &attributes)?;
                properties.extend(declared);
                methods.extend(hooks);
            } else {
                return Err(format!("Unexpected token in trait body: {:?}", self.peek()));
            }
        }
        self.expect(&Token::RBrace)?;
        self.pop_generic_scope();

        Ok(Stmt::Trait {
            line,
            attributes: invalid_case_line
                .map(Attribute::non_enum_case_marker)
                .into_iter()
                .collect(),
            name,
            properties,
            constants,
            methods,
            uses,
            trait_aliases,
            trait_precedences,
            generic_params,
        })
    }

    /// Parse interface declaration
    fn parse_interface(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'interface'
        let (name, line) = self.parse_classlike_declaration_name("interface")?;
        let generic_params = self.parse_generic_parameters()?;
        self.push_generic_scope(&generic_params);
        // interface Foo extends Bar, Baz { ... }
        let extends = if self.peek() == Token::Extends {
            self.advance();
            let mut parents = Vec::new();
            loop {
                parents.push(self.parse_generic_ancestor_with_reserved_static(
                    ReservedStaticRole::Interface,
                    Some(line),
                )?);
                if matches!(self.peek(), Token::Comma(_)) {
                    self.advance();
                } else {
                    break;
                }
            }
            parents
        } else {
            Vec::new()
        };
        self.expect(&Token::LBrace(0))?;

        let mut properties = Vec::new();
        let mut constants = Vec::new();
        let mut methods = Vec::new();
        let mut invalid_case_line = None;
        while self.peek() != Token::RBrace && !self.at_eof() {
            let member_start = self.pos;
            let attributes = self.parse_attribute_groups()?;
            if matches!(self.peek(), Token::Case(_)) {
                let line = self.parse_non_enum_case_declaration()?;
                invalid_case_line.get_or_insert(line);
                continue;
            }
            if matches!(self.peek(), Token::Use(_)) {
                let use_line = match self.advance() {
                    Token::Use(line) => line,
                    _ => unreachable!("trait use parser starts at use"),
                };
                let (trait_uses, adaptation_line) =
                    self.parse_trait_ancestor_list(use_line)?;
                let used_trait = trait_uses
                    .first()
                    .map(|ancestor| ancestor.name.rsplit('\\').next().unwrap_or(&ancestor.name))
                    .unwrap_or("")
                    .to_string();
                if matches!(self.peek(), Token::LBrace(_)) {
                    self.advance();
                    while self.peek() != Token::RBrace && !self.at_eof() {
                        let (trait_name, method) =
                            self.parse_trait_method_reference(adaptation_line)?;
                        if self.peek() == Token::Insteadof {
                            self.advance();
                            let Some(_) = trait_name else {
                                return Err(
                                    "Trait precedence requires an explicit trait name".into()
                                );
                            };
                            loop {
                                self.parse_qualified_name_with_reserved_static(
                                    ReservedStaticRole::Trait,
                                    Some(adaptation_line),
                                )?;
                                if matches!(self.peek(), Token::Comma(_)) {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                            self.expect(&Token::Semicolon(0))?;
                        } else {
                            self.parse_trait_alias_adaptation(trait_name, method)?;
                        }
                    }
                    self.expect(&Token::RBrace)?;
                } else {
                    self.expect(&Token::Semicolon(0))?;
                }
                let _ = self.compile_error(
                    format!(
                        "Cannot use traits inside of interfaces. {used_trait} is used in {name}"
                    ),
                    use_line,
                );
                continue;
            }
            let modifiers = self.parse_member_modifiers();
            if self.peek() == Token::Const {
                constants.extend(self.parse_class_constant_declaration(
                    &modifiers,
                    true,
                    &attributes,
                    self.class_member_doc_comment(member_start, self.pos),
                )?);
            } else if let Token::Case(line) = self.peek()
                && modifiers.has_visibility
            {
                return Err(self.source_error(
                    "syntax error, unexpected token \"case\", expecting variable",
                    line,
                ));
            } else if matches!(self.peek(), Token::Function(_)) {
                let line = match self.advance() {
                    Token::Function(line) => line,
                    _ => unreachable!("method parser starts at function"),
                };
                self.defer_method_modifier_diagnostics(&modifiers, line);
                let returns_by_ref = self.peek() == Token::Ampersand;
                self.consume_reference_return_marker();
                let token = self.advance();
                let method_name = Self::token_as_named_arg_label(&token)
                    .ok_or_else(|| format!("Expected method name, got {:?}", token))?;
                let previous_class_scope = self.class_scope_active;
                self.class_scope_active = true;
                let method_generic_params = self.parse_generic_parameters()?;
                self.push_generic_scope(&method_generic_params);
                // Interface methods must be public (PHP rule)
                if modifiers.visibility != Visibility::Public {
                    let vis_str = match modifiers.visibility {
                        Visibility::Protected => "protected",
                        Visibility::Private => "private",
                        _ => "public",
                    };
                    return Err(format!(
                        "Access type for interface method {}::{}() must be public (got {})",
                        name, method_name, vis_str
                    ));
                }
                self.expect_lparen()?;
                let params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                let promoted_hooks = params
                    .iter()
                    .flat_map(|parameter| parameter.promotion_hooks.iter().cloned())
                    .collect::<Vec<_>>();
                let return_type = self.parse_return_type(line, false)?;
                self.expect(&Token::Semicolon(0))?; // interface methods end with ;
                self.pop_generic_scope();
                self.class_scope_active = previous_class_scope;
                methods.push(ClassMethod {
                    line,
                    attributes: attributes.clone(),
                    visibility: modifiers.visibility,
                    name: method_name,
                    params,
                    body: vec![],
                    is_static: modifiers.is_static,
                    is_final: false,
                    is_abstract: true,
                    returns_by_ref,
                    return_type,
                    generic_params: method_generic_params,
                });
                methods.extend(promoted_hooks);
            } else if matches!(self.peek(), Token::Variable(_, _)) || self.is_type_hint_start() {
                let (declared, hooks) =
                    self.parse_property_declaration(&modifiers, &attributes)?;
                properties.extend(declared);
                methods.extend(hooks);
            } else {
                return Err(format!(
                    "Unexpected token in interface body: {:?}",
                    self.peek()
                ));
            }
        }
        self.expect(&Token::RBrace)?;
        self.pop_generic_scope();

        Ok(Stmt::Interface {
            line,
            attributes: invalid_case_line
                .map(Attribute::non_enum_case_marker)
                .into_iter()
                .collect(),
            name,
            extends,
            properties,
            constants,
            methods,
            generic_params,
        })
    }

    /// Parse enum declaration
    fn parse_enum(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'enum'
        let (name, line) = self.parse_classlike_declaration_name("enum")?;
        // Optional backing type: enum Foo: string { ... }
        let backing_type = if self.peek() == Token::Colon {
            self.advance(); // consume ':'
            let hint = self.parse_base_type_hint()?;
            Some(self.maybe_parse_compound_type(hint)?)
        } else {
            None
        };
        let implements = if self.peek() == Token::Implements {
            self.advance();
            let mut interfaces = Vec::new();
            loop {
                interfaces.push(self.parse_generic_ancestor_with_reserved_static(
                    ReservedStaticRole::Interface,
                    Some(line),
                )?);
                if !matches!(self.peek(), Token::Comma(_)) {
                    break;
                }
                self.advance();
            }
            interfaces
        } else {
            Vec::new()
        };
        self.expect(&Token::LBrace(0))?;

        let mut cases = Vec::new();
        let mut properties = Vec::new();
        let mut constants = Vec::new();
        let mut methods = Vec::new();
        let mut uses = Vec::new();
        let mut trait_aliases = Vec::new();

        let prev_in_class = self.in_class_body;
        self.in_class_body = true;

        while self.peek() != Token::RBrace && !self.at_eof() {
            let member_start = self.pos;
            let attributes = self.parse_attribute_groups()?;
            if matches!(self.peek(), Token::Use(_)) {
                let use_line = match self.advance() {
                    Token::Use(line) => line,
                    _ => unreachable!("trait use parser starts at use"),
                };
                let (trait_uses, adaptation_line) =
                    self.parse_trait_ancestor_list(use_line)?;
                uses.extend(trait_uses);
                if matches!(self.peek(), Token::LBrace(_)) {
                    self.advance();
                    while self.peek() != Token::RBrace && !self.at_eof() {
                        let (trait_name, method) =
                            self.parse_trait_method_reference(adaptation_line)?;
                        trait_aliases
                            .push(self.parse_trait_alias_adaptation(trait_name, method)?);
                    }
                    self.expect(&Token::RBrace)?;
                } else {
                    self.expect(&Token::Semicolon(0))?;
                }
            } else if matches!(self.peek(), Token::Case(_)) {
                self.advance(); // consume 'case'
                let (case_name, case_line) = match self.advance() {
                    Token::Identifier(n, line) | Token::Enum { name: n, line } => (n, line),
                    Token::Exit { name, line } => (name, line),
                    other => return Err(format!("Expected enum case name, got {:?}", other)),
                };
                let value = if self.peek() == Token::Assign {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(&Token::Semicolon(0))?;
                cases.push(EnumCase {
                    attributes,
                    line: case_line,
                    name: case_name,
                    value,
                });
            } else {
                // Method in enum
                let modifiers = self.parse_member_modifiers();
                if self.peek() == Token::Const {
                    constants.extend(self.parse_class_constant_declaration(
                        &modifiers,
                        false,
                        &attributes,
                        self.class_member_doc_comment(member_start, self.pos),
                    )?);
                } else if matches!(self.peek(), Token::Function(_)) {
                    let line = match self.advance() {
                        Token::Function(line) => line,
                        _ => unreachable!("method parser starts at function"),
                    };
                    self.defer_method_modifier_diagnostics(&modifiers, line);
                    let returns_by_ref = self.peek() == Token::Ampersand;
                    self.consume_reference_return_marker();
                    let token = self.advance();
                    let method_name = Self::token_as_named_arg_label(&token)
                        .ok_or_else(|| format!("Expected method name, got {:?}", token))?;
                    let previous_class_scope = self.class_scope_active;
                    self.class_scope_active = true;
                    let generic_params = self.parse_generic_parameters()?;
                    self.push_generic_scope(&generic_params);
                    self.expect_lparen()?;
                    let params = self.parse_param_list()?;
                    self.expect(&Token::RParen)?;
                    let return_type = self.parse_return_type(line, false)?;
                    let body = self.parse_method_body(&modifiers, line, true, true)?;
                    self.pop_generic_scope();
                    self.class_scope_active = previous_class_scope;
                    methods.push(ClassMethod {
                        line,
                        attributes: attributes.clone(),
                        visibility: modifiers.visibility,
                        name: method_name,
                        params,
                        body,
                        is_static: modifiers.is_static,
                        is_final: modifiers.is_final,
                        is_abstract: modifiers.is_abstract,
                        returns_by_ref,
                        return_type,
                        generic_params,
                    });
                } else if matches!(self.peek(), Token::Variable(_, _))
                    || self.is_type_hint_start()
                {
                    let (declared, hooks) =
                        self.parse_property_declaration(&modifiers, &attributes)?;
                    properties.extend(declared);
                    methods.extend(hooks);
                } else {
                    return Err(format!("Unexpected token in enum body: {:?}", self.peek()));
                }
            }
        }
        self.in_class_body = prev_in_class;
        self.expect(&Token::RBrace)?;

        Ok(Stmt::Enum {
            line,
            attributes: Vec::new(),
            name,
            backing_type,
            implements,
            uses,
            trait_aliases,
            cases,
            properties,
            constants,
            methods,
        })
    }

    fn parse_member_modifiers(&mut self) -> MemberModifiers {
        let mut modifiers = MemberModifiers::default();

        let record_duplicate =
            |modifiers: &mut MemberModifiers, duplicate: DuplicateMemberModifier| {
                if modifiers.duplicate.is_none() {
                    modifiers.duplicate = Some(duplicate);
                }
            };

        loop {
            match self.peek() {
                Token::Identifier(ref name, _) if name.eq_ignore_ascii_case("var") => {
                    self.advance();
                    if modifiers.has_visibility {
                        record_duplicate(&mut modifiers, DuplicateMemberModifier::Access);
                    }
                    modifiers.has_visibility = true;
                    modifiers.visibility = Visibility::Public;
                }
                Token::Public => {
                    self.advance();
                    if matches!(self.peek(), Token::LParen(_))
                        && matches!(self.peek_at(1), Token::Identifier(ref name, _) if name.eq_ignore_ascii_case("set"))
                        && self.peek_at(2) == Token::RParen
                    {
                        self.advance();
                        match self.advance() {
                            Token::Identifier(_, _) => {}
                            _ => unreachable!(),
                        }
                        self.advance();
                        if modifiers.set_visibility.is_some() {
                            record_duplicate(&mut modifiers, DuplicateMemberModifier::Access);
                        }
                        modifiers.set_visibility = Some(Visibility::Public);
                    } else {
                        if modifiers.has_visibility {
                            record_duplicate(&mut modifiers, DuplicateMemberModifier::Access);
                        }
                        modifiers.has_visibility = true;
                        modifiers.visibility = Visibility::Public;
                    }
                }
                Token::Protected => {
                    self.advance();
                    if matches!(self.peek(), Token::LParen(_))
                        && matches!(self.peek_at(1), Token::Identifier(ref name, _) if name.eq_ignore_ascii_case("set"))
                        && self.peek_at(2) == Token::RParen
                    {
                        self.advance();
                        match self.advance() {
                            Token::Identifier(_, _) => {}
                            _ => unreachable!(),
                        }
                        self.advance();
                        if modifiers.set_visibility.is_some() {
                            record_duplicate(&mut modifiers, DuplicateMemberModifier::Access);
                        }
                        modifiers.set_visibility = Some(Visibility::Protected);
                    } else {
                        if modifiers.has_visibility {
                            record_duplicate(&mut modifiers, DuplicateMemberModifier::Access);
                        }
                        modifiers.has_visibility = true;
                        modifiers.visibility = Visibility::Protected;
                    }
                }
                Token::Private => {
                    self.advance();
                    if matches!(self.peek(), Token::LParen(_))
                        && matches!(self.peek_at(1), Token::Identifier(ref name, _) if name.eq_ignore_ascii_case("set"))
                        && self.peek_at(2) == Token::RParen
                    {
                        self.advance();
                        match self.advance() {
                            Token::Identifier(_, _) => {}
                            _ => unreachable!(),
                        }
                        self.advance();
                        if modifiers.set_visibility.is_some() {
                            record_duplicate(&mut modifiers, DuplicateMemberModifier::Access);
                        }
                        modifiers.set_visibility = Some(Visibility::Private);
                    } else {
                        if modifiers.has_visibility {
                            record_duplicate(&mut modifiers, DuplicateMemberModifier::Access);
                        }
                        modifiers.has_visibility = true;
                        modifiers.visibility = Visibility::Private;
                    }
                }
                Token::Static(_) => {
                    self.advance();
                    if modifiers.is_static {
                        record_duplicate(&mut modifiers, DuplicateMemberModifier::Static);
                    }
                    modifiers.is_static = true;
                }
                Token::Final(_) => {
                    self.advance();
                    if modifiers.is_final {
                        record_duplicate(&mut modifiers, DuplicateMemberModifier::Final);
                    }
                    modifiers.is_final = true;
                }
                Token::Abstract(_) => {
                    self.advance();
                    if modifiers.is_abstract {
                        record_duplicate(&mut modifiers, DuplicateMemberModifier::Abstract);
                    }
                    modifiers.is_abstract = true;
                }
                Token::Identifier(ref s, line) if s.eq_ignore_ascii_case("readonly") => {
                    self.advance();
                    if modifiers.is_readonly {
                        record_duplicate(&mut modifiers, DuplicateMemberModifier::Readonly);
                    }
                    modifiers.is_readonly = true;
                    modifiers.readonly_line.get_or_insert(line);
                }
                _ => break,
            }
        }
        modifiers
    }

    fn parse_class_constant_declaration(
        &mut self,
        modifiers: &MemberModifiers,
        in_interface: bool,
        attributes: &[Attribute],
        doc_comment: Option<std::sync::Arc<str>>,
    ) -> Result<Vec<ClassConstant>, String> {
        if modifiers.is_static {
            return Err("Class constants cannot be declared static".into());
        }
        if modifiers.is_abstract {
            return Err("Class constants cannot be declared abstract".into());
        }
        if modifiers.is_readonly {
            let line = modifiers
                .readonly_line
                .unwrap_or_else(|| self.closest_token_source_line());
            let _ = self.compile_error(
                "Cannot use the readonly modifier on a class constant",
                line,
            );
        }
        if modifiers.is_final && modifiers.visibility == Visibility::Private {
            return Err("Private class constants cannot be final".into());
        }
        if in_interface && modifiers.visibility != Visibility::Public {
            return Err("Access type for interface constants must be public".into());
        }

        self.expect(&Token::Const)?;
        let type_hint = self.try_parse_class_constant_type()?;
        let mut constants = Vec::new();
        let mut doc_comment = doc_comment;
        loop {
            let (name, line) = match self.advance() {
                Token::Identifier(name, line)
                | Token::Enum { name, line }
                | Token::MagicConstant { name, line }
                | Token::Goto { name, line } => (name, line),
                Token::Exit { name, line } => (name, line),
                other => return Err(format!("Expected class constant name, got {:?}", other)),
            };
            self.expect(&Token::Assign)?;
            let value = self.parse_expr()?;
            constants.push(ClassConstant {
                attributes: attributes.to_vec(),
                doc_comment: doc_comment.take(),
                line,
                visibility: modifiers.visibility,
                name,
                value,
                type_hint: type_hint.clone(),
                is_final: modifiers.is_final,
            });
            if !matches!(self.peek(), Token::Comma(_)) {
                break;
            }
            self.advance();
        }
        self.expect(&Token::Semicolon(0))?;
        Ok(constants)
    }

    fn try_parse_class_constant_type(&mut self) -> Result<Option<TypeHint>, String> {
        if matches!(
            self.peek(),
            Token::Identifier(_, _) | Token::Enum { .. } | Token::Goto { .. }
        )
            && self.peek_at(1) == Token::Assign
        {
            return Ok(None);
        }

        let hint = if self.peek() == Token::Question {
            self.advance();
            TypeHint::Nullable(Box::new(self.parse_base_type_hint()?))
        } else if matches!(
            self.peek(),
            Token::Identifier(_, _)
                | Token::ArrayKw
                | Token::Null
                | Token::True
                | Token::False
                | Token::Static(_)
                | Token::LParen(_)
        ) {
            let first = self.parse_base_type_hint()?;
            self.maybe_parse_compound_type(first)?
        } else {
            return Ok(None);
        };

        Ok(Some(hint))
    }

    fn parse_method_body(
        &mut self,
        modifiers: &MemberModifiers,
        method_line: usize,
        allow_private_abstract: bool,
        defer_all_abstract_bodies: bool,
    ) -> Result<Vec<Stmt>, String> {
        if modifiers.duplicate.is_some() {
            if matches!(self.peek(), Token::Semicolon(_)) {
                self.advance();
                return Ok(Vec::new());
            }
            self.expect(&Token::LBrace(0))?;
            let mut body = Vec::new();
            while self.peek() != Token::RBrace && !self.at_eof() {
                body.push(self.parse_stmt_in_scope(false)?);
            }
            self.expect(&Token::RBrace)?;
            return Ok(body);
        }
        if modifiers.is_abstract {
            if modifiers.is_final {
                let _ = self.compile_error(
                    "Cannot use the final modifier on an abstract method",
                    method_line,
                );
            }
            if defer_all_abstract_bodies
                || modifiers.visibility == Visibility::Private && !allow_private_abstract
            {
                if matches!(self.peek(), Token::Semicolon(_)) {
                    self.advance();
                    return Ok(Vec::new());
                }
                self.expect(&Token::LBrace(0))?;
                let mut body = Vec::new();
                while self.peek() != Token::RBrace && !self.at_eof() {
                    body.push(self.parse_stmt_in_scope(false)?);
                }
                self.expect(&Token::RBrace)?;
                return Ok(body);
            }
            self.expect(&Token::Semicolon(0))?;
            return Ok(Vec::new());
        }
        self.expect(&Token::LBrace(0))?;
        let mut body = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            body.push(self.parse_stmt_in_scope(false)?);
        }
        self.expect(&Token::RBrace)?;
        Ok(body)
    }

    /// Parse match expression
    fn parse_match_expr(&mut self) -> Result<Expr, String> {
        let line = match self.advance() {
            Token::Match(line) => line,
            _ => unreachable!("parse_match_expr starts at match"),
        };
        self.expect_lparen()?;
        let expr = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::LBrace(0))?;

        let mut arms = Vec::new();
        let mut has_default = false;
        while self.peek() != Token::RBrace && !self.at_eof() {
            if let Token::Default(default_line) = self.peek() {
                if has_default {
                    let _ = self.compile_error(
                        "Match expressions may only contain one default arm",
                        default_line,
                    );
                } else {
                    has_default = true;
                }
                self.advance();
                if matches!(self.peek(), Token::Comma(_))
                    && self.peek_at(1) == Token::DoubleArrow
                {
                    self.advance();
                }
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
                while matches!(self.peek(), Token::Comma(_)) {
                    // A comma immediately before => terminates the condition list.
                    if self.peek_at(1) == Token::DoubleArrow {
                        self.advance();
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
            if matches!(self.peek(), Token::Comma(_)) {
                self.advance();
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::Match {
            line,
            expr: Box::new(expr),
            arms,
        })
    }

    /// Parse arrow function: fn($x, $y) => expr
    /// Desugars to Closure with auto-captured use vars and body = [Return(expr)]
    fn parse_arrow_function(&mut self, is_static: bool) -> Result<Expr, String> {
        let line = match self.advance() {
            Token::Fn(line) => line,
            token => return Err(format!("Expected fn, got {token:?}")),
        };
        let returns_by_ref = if self.peek() == Token::Ampersand {
            self.advance();
            true
        } else {
            false
        };
        let generic_params = self.parse_generic_parameters()?;
        self.push_generic_scope(&generic_params);
        self.expect_lparen()?;
        let params = self.parse_param_list()?;
        self.expect(&Token::RParen)?;
        let return_type = self.parse_return_type(line, true)?;
        self.expect(&Token::DoubleArrow)?;
        let expr = self.parse_expr()?;
        self.pop_generic_scope();

        // Auto-capture: collect free variables from expr that aren't params
        let param_names: std::collections::HashSet<&str> =
            params.iter().map(|p| p.name.as_str()).collect();
        let mut free_vars = Vec::new();
        Self::collect_free_vars(&expr, &param_names, &mut free_vars);

        let body = vec![Stmt::Return {
            expr: Some(expr),
            line: 0,
        }];
        Ok(Expr::Closure {
            line,
            attributes: Vec::new(),
            is_static,
            returns_by_ref,
            params,
            use_vars: free_vars
                .into_iter()
                .map(|name| (name, false, 0))
                .collect(),
            body,
            return_type,
            generic_params,
        })
    }

    /// Collect variable names referenced in an expression that are not in `bound`.
    fn collect_free_vars(
        expr: &Expr,
        bound: &std::collections::HashSet<&str>,
        out: &mut Vec<String>,
    ) {
        match expr {
            Expr::Variable { name, .. } => {
                if !bound.contains(name.as_str()) && !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Expr::BinaryOp { left, right, .. } => {
                Self::collect_free_vars(left, bound, out);
                Self::collect_free_vars(right, bound, out);
            }
            Expr::Pipe {
                input, callable, ..
            } => {
                Self::collect_free_vars(input, bound, out);
                Self::collect_free_vars(callable, bound, out);
            }
            Expr::UnaryPlus(inner)
            | Expr::UnaryMinus(inner)
            | Expr::ErrorSuppress(inner)
            | Expr::Not(inner)
            | Expr::Throw { expr: inner, .. }
            | Expr::Empty(inner)
            | Expr::Print(inner)
            | Expr::Include { path: inner, .. }
            | Expr::Eval { source: inner, .. }
            | Expr::BitwiseNot(inner)
            | Expr::DynamicVariable { name: inner, .. } => {
                Self::collect_free_vars(inner, bound, out);
            }
            Expr::Assign { var, expr: inner } => {
                if !bound.contains(var.as_str()) && !out.contains(var) {
                    out.push(var.clone());
                }
                Self::collect_free_vars(inner, bound, out);
            }
            Expr::AssignReference { var, target } => {
                if !bound.contains(var.as_str()) && !out.contains(var) {
                    out.push(var.clone());
                }
                Self::collect_free_vars(target, bound, out);
            }
            Expr::AssignTarget { target, expr }
            | Expr::AssignTargetReference {
                target,
                source: expr,
            }
            | Expr::ArrayAppendAssign { target, expr, .. } => {
                Self::collect_free_vars(target, bound, out);
                Self::collect_free_vars(expr, bound, out);
            }
            Expr::ListAssign { targets, expr, .. } => {
                for target in targets {
                    Self::collect_list_target_free_vars(target, bound, out);
                }
                Self::collect_free_vars(expr, bound, out);
            }
            Expr::CompoundAssignExpression { target, expr, .. } => {
                Self::collect_free_vars(target, bound, out);
                Self::collect_free_vars(expr, bound, out);
            }
            Expr::FunctionCall { args, .. } | Expr::StaticCall { args, .. } => {
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::DynamicCall { callable, args, .. } => {
                Self::collect_free_vars(callable, bound, out);
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::DynamicStaticCall {
                class,
                method,
                args,
                ..
            } => {
                Self::collect_free_vars(class, bound, out);
                Self::collect_free_vars(method, bound, out);
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::FirstClassCallable { callable, .. } => {
                Self::collect_free_vars(callable, bound, out);
            }
            Expr::Isset(exprs) => {
                for e in exprs {
                    Self::collect_free_vars(e, bound, out);
                }
            }
            Expr::PostInc { name, .. }
            | Expr::PostDec { name, .. }
            | Expr::PreInc { name, .. }
            | Expr::PreDec { name, .. } => {
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
            Expr::CoalesceAssign { target, expr } => {
                Self::collect_free_vars(target, bound, out);
                Self::collect_free_vars(expr, bound, out);
            }
            Expr::ArrayLiteral(elements) => {
                for elem in elements {
                    if let Some(k) = &elem.key {
                        Self::collect_free_vars(k, bound, out);
                    }
                    Self::collect_free_vars(&elem.value, bound, out);
                }
            }
            Expr::ArrayAccess { array, index, .. } => {
                Self::collect_free_vars(array, bound, out);
                Self::collect_free_vars(index, bound, out);
            }
            Expr::PropertyAccess { object, .. } => {
                Self::collect_free_vars(object, bound, out);
            }
            Expr::DynamicPropertyAccess {
                object, property, ..
            } => {
                Self::collect_free_vars(object, bound, out);
                Self::collect_free_vars(property, bound, out);
            }
            Expr::MethodCall { object, args, .. } => {
                Self::collect_free_vars(object, bound, out);
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::Closure { use_vars, .. } => {
                // Nested closure — only capture its explicit use vars
                for (v, _, _) in use_vars {
                    if !bound.contains(v.as_str()) && !out.contains(v) {
                        out.push(v.clone());
                    }
                }
            }
            Expr::Cast { expr: inner, .. } => {
                Self::collect_free_vars(inner, bound, out);
            }
            Expr::PostIncTarget(target)
            | Expr::PostDecTarget(target)
            | Expr::PreIncTarget(target)
            | Expr::PreDecTarget(target) => {
                Self::collect_free_vars(target, bound, out);
            }
            Expr::Instanceof { expr: inner, .. } => {
                Self::collect_free_vars(inner, bound, out);
            }
            Expr::DynamicInstanceof { expr, class } => {
                Self::collect_free_vars(expr, bound, out);
                Self::collect_free_vars(class, bound, out);
            }
            Expr::New { args, .. } => {
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::DynamicNew { class, args, .. } => {
                Self::collect_free_vars(class, bound, out);
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::AnonymousNew { args, .. } => {
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::Match {
                expr: inner, arms, ..
            } => {
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
            Expr::StaticProperty { .. }
            | Expr::ClassConstant { .. }
            | Expr::FirstClassFunctionCallable { .. } => {}
            Expr::DynamicNamedStaticProperty { property, .. } => {
                Self::collect_free_vars(property, bound, out);
            }
            Expr::DynamicStaticProperty {
                class, property, ..
            } => {
                Self::collect_free_vars(class, bound, out);
                Self::collect_free_vars(property, bound, out);
            }
            Expr::ArrayAppendArgument { target, .. } => {
                Self::collect_free_vars(target, bound, out);
            }
            Expr::DynamicClassConstant {
                class, constant, ..
            } => {
                Self::collect_free_vars(class, bound, out);
                Self::collect_free_vars(constant, bound, out);
            }
            Expr::DynamicNamedClassConstant { constant, .. } => {
                Self::collect_free_vars(constant, bound, out);
            }
            // Literals and constants — no variables
            Expr::Integer(_)
            | Expr::Float(_)
            | Expr::StringLiteral(_)
            | Expr::BinaryStringLiteral(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Globals { .. }
            | Expr::CompileError { .. }
            | Expr::CompileWarning { .. }
            | Expr::CompileDeprecation { .. }
            | Expr::Constant { .. }
            | Expr::CompilerHaltOffsetConstant { .. }
            | Expr::MagicConstant { .. } => {}
            // Yield — collect vars from value/key expressions
            Expr::Yield { value, key } => {
                if let Some(v) = value {
                    Self::collect_free_vars(v, bound, out);
                }
                if let Some(k) = key {
                    Self::collect_free_vars(k, bound, out);
                }
            }
            Expr::YieldFrom { expr, .. } => {
                Self::collect_free_vars(expr, bound, out);
            }
            Expr::Clone { expr: inner, .. } => {
                Self::collect_free_vars(inner, bound, out);
            }
        }
    }

    fn collect_list_target_free_vars(
        target: &ListTarget,
        bound: &std::collections::HashSet<&str>,
        out: &mut Vec<String>,
    ) {
        match target {
            ListTarget::Variable(name) => {
                if !bound.contains(name.as_str()) && !out.contains(name) {
                    out.push(name.clone());
                }
            }
            ListTarget::Reference(target)
            | ListTarget::Target(target)
            | ListTarget::AppendTarget(target) => Self::collect_free_vars(target, bound, out),
            ListTarget::Skip => {}
            ListTarget::Nested(targets) => {
                for target in targets {
                    Self::collect_list_target_free_vars(target, bound, out);
                }
            }
            ListTarget::KeyedVariable { key, var } => {
                Self::collect_free_vars(key, bound, out);
                if !bound.contains(var.as_str()) && !out.contains(var) {
                    out.push(var.clone());
                }
            }
            ListTarget::KeyedReference { key, target }
            | ListTarget::KeyedTarget { key, target }
            | ListTarget::KeyedAppendTarget { key, target } => {
                Self::collect_free_vars(key, bound, out);
                Self::collect_free_vars(target, bound, out);
            }
            ListTarget::KeyedNested { key, targets } => {
                Self::collect_free_vars(key, bound, out);
                for target in targets {
                    Self::collect_list_target_free_vars(target, bound, out);
                }
            }
        }
    }

    /// Parse closure: function($a, $b) use($c) { ... }
    fn parse_closure(&mut self, is_static: bool) -> Result<Expr, String> {
        let line = match self.advance() {
            Token::Function(line) => line,
            token => return Err(format!("Expected function, got {token:?}")),
        };
        let returns_by_ref = if self.peek() == Token::Ampersand {
            self.advance();
            true
        } else {
            false
        };
        let generic_params = self.parse_generic_parameters()?;
        self.push_generic_scope(&generic_params);
        self.expect_lparen()?;
        let params = self.parse_param_list()?;
        self.expect(&Token::RParen)?;

        let mut use_vars = Vec::new();
        if matches!(self.peek(), Token::Use(_)) {
            self.advance();
            self.expect_lparen()?;
            loop {
                let is_ref = if self.peek() == Token::Ampersand {
                    self.advance();
                    true
                } else {
                    false
                };
                let (v, line) = match self.advance() {
                    Token::Variable(n, line) => (n, line),
                    Token::This(line) => {
                        (self.invalid_this_binding("lexical", line), line)
                    }
                    other => {
                        let unexpected = match other {
                            Token::RParen => "token \")\"".to_string(),
                            Token::Comma(_) => "token \",\"".to_string(),
                            Token::Ampersand => "token \"&\"".to_string(),
                            Token::Integer(value) => format!("integer \"{value}\""),
                            Token::Float(value) => format!("floating-point number \"{value}\""),
                            Token::Identifier(name, _) => format!("identifier \"{name}\""),
                            token => format!("token \"{token:?}\""),
                        };
                        let expecting = if is_ref {
                            "variable"
                        } else {
                            "variable or \"&\" or token \"&\""
                        };
                        let line = self.closest_token_source_line();
                        return Err(self.source_error(
                            &format!(
                                "syntax error, unexpected {unexpected}, expecting {expecting}"
                            ),
                            line,
                        ));
                    }
                };
                if v == "GLOBALS" {
                    self.compile_error("Cannot use auto-global as lexical variable", line);
                }
                if let Some(parameter) = params.iter().find(|parameter| parameter.name == v) {
                    self.compile_error(
                        format!("Cannot use lexical variable ${v} as a parameter name"),
                        parameter.line,
                    );
                } else if let Some((_, _, first_line)) =
                    use_vars.iter().find(|(name, _, _)| name == &v)
                {
                    self.compile_error(format!("Cannot use variable ${v} twice"), *first_line);
                }
                use_vars.push((v, is_ref, line));

                let Token::Comma(_) = self.peek() else {
                    break;
                };
                self.advance();
                if self.peek() == Token::RParen {
                    break;
                }
                if let Token::Comma(second_comma_line) = self.peek() {
                    return Err(self.source_error(
                        "syntax error, unexpected token \",\", expecting \")\"",
                        second_comma_line,
                    ));
                }
            }
            self.expect(&Token::RParen)?;
        }

        let return_type = self.parse_return_type(line, true)?;

        self.expect(&Token::LBrace(0))?;
        let mut body = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            body.push(self.parse_stmt_in_scope(false)?);
        }
        self.expect(&Token::RBrace)?;
        self.pop_generic_scope();

        Ok(Expr::Closure {
            line,
            attributes: Vec::new(),
            is_static,
            returns_by_ref,
            params,
            use_vars,
            body,
            return_type,
            generic_params,
        })
    }
}
