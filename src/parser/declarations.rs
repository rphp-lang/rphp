#[derive(Debug, Clone, Copy)]
struct MemberModifiers {
    visibility: Visibility,
    is_static: bool,
    is_final: bool,
    is_readonly: bool,
    is_abstract: bool,
}

impl Default for MemberModifiers {
    fn default() -> Self {
        Self {
            visibility: Visibility::Public,
            is_static: false,
            is_final: false,
            is_readonly: false,
            is_abstract: false,
        }
    }
}

impl Parser {
    fn parse_property_declaration(
        &mut self,
        modifiers: &MemberModifiers,
    ) -> Result<Vec<ClassProperty>, String> {
        if modifiers.is_abstract {
            return Err("Properties cannot be declared abstract".into());
        }
        let type_hint = self.try_parse_type_hint()?;
        let mut properties = Vec::new();
        loop {
            let name = match self.advance() {
                Token::Variable(name, _) => name,
                other => return Err(format!("Expected property variable, got {other:?}")),
            };
            let default = if self.peek() == Token::Assign {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            properties.push(ClassProperty {
                visibility: modifiers.visibility,
                name,
                type_hint: type_hint.clone(),
                default,
                is_static: modifiers.is_static,
                is_readonly: modifiers.is_readonly,
            });
            if self.peek() != Token::Comma {
                break;
            }
            self.advance();
        }
        self.expect(&Token::Semicolon)?;
        Ok(properties)
    }

    fn parse_anonymous_class_body(
        &mut self,
    ) -> Result<(Vec<ClassProperty>, Vec<ClassConstant>, Vec<ClassMethod>), String> {
        let mut properties = Vec::new();
        let mut constants = Vec::new();
        let mut methods = Vec::new();
        let previous_class_body = self.in_class_body;
        let previous_class_scope = self.class_scope_active;
        self.in_class_body = true;
        self.class_scope_active = true;

        while self.peek() != Token::RBrace && !self.at_eof() {
            let modifiers = self.parse_member_modifiers();
            if matches!(self.peek(), Token::Function(_)) {
                self.advance();
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
                let return_type = self.parse_return_type()?;
                let body = self.parse_method_body(&modifiers, &method_name)?;
                self.pop_generic_scope();
                methods.push(ClassMethod {
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
            } else if self.peek() == Token::Const {
                constants.extend(self.parse_class_constant_declaration(&modifiers, false)?);
            } else if matches!(self.peek(), Token::Variable(_, _)) || self.is_type_hint_start() {
                properties.extend(self.parse_property_declaration(&modifiers)?);
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
        Ok((properties, constants, methods))
    }

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
            self.expect_lparen()?;
            // Parse exception type(s): ExA | ExB
            let mut types = Vec::new();
            let type_name = self.parse_qualified_name()?;
            types.push(type_name);
            while self.peek() == Token::Pipe {
                self.advance();
                let t = self.parse_qualified_name()?;
                types.push(t);
            }
            let var = match self.peek() {
                Token::Variable(_, _) => match self.advance() {
                    Token::Variable(n, _) => Some(n),
                    _ => unreachable!(),
                },
                Token::RParen => None,
                ref other => {
                    return Err(format!(
                        "Expected variable or ')' in catch, got {:?}",
                        other
                    ));
                }
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
        let mut is_readonly = false;
        // Consume leading modifiers in any order before `class`.
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
                Token::Identifier(ref name, _) if name.eq_ignore_ascii_case("readonly") => {
                    self.advance();
                    is_readonly = true;
                }
                _ => break,
            }
        }
        if is_abstract && is_final {
            return Err("Cannot use the final modifier on an abstract class".into());
        }
        self.advance(); // consume 'class'
        let name = match self.advance() {
            Token::Identifier(n, _) => n,
            other => return Err(format!("Expected class name, got {:?}", other)),
        };
        let generic_params = self.parse_generic_parameters()?;
        self.push_generic_scope(&generic_params);
        let parent = if self.peek() == Token::Extends {
            self.advance();
            Some(self.parse_generic_ancestor()?)
        } else {
            None
        };
        let implements = if self.peek() == Token::Implements {
            self.advance();
            let mut ifaces = Vec::new();
            loop {
                ifaces.push(self.parse_generic_ancestor()?);
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
        let mut constants = Vec::new();
        let mut methods = Vec::new();
        let mut uses = Vec::new();
        let mut trait_aliases = Vec::new();

        let prev_in_class = self.in_class_body;
        self.in_class_body = true;

        while self.peek() != Token::RBrace && !self.at_eof() {
            // Trait `use` statements: use Foo, Bar;
            if self.peek() == Token::Use {
                self.advance(); // consume 'use'
                loop {
                    uses.push(self.parse_generic_ancestor()?);
                    if self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                if self.peek() == Token::LBrace {
                    self.advance();
                    while self.peek() != Token::RBrace && !self.at_eof() {
                        let first = self.parse_qualified_name()?;
                        let (trait_name, method) = if self.peek() == Token::DoubleColon {
                            self.advance();
                            let token = self.advance();
                            let method = Self::token_as_named_arg_label(&token).ok_or_else(|| {
                                format!("Expected trait method name, got {token:?}")
                            })?;
                            (Some(first), method)
                        } else {
                            (None, first)
                        };
                        self.expect(&Token::As)?;
                        let visibility = match self.peek() {
                            Token::Public => Some(Visibility::Public),
                            Token::Protected => Some(Visibility::Protected),
                            Token::Private => Some(Visibility::Private),
                            _ => None,
                        };
                        if visibility.is_some() {
                            self.advance();
                        }
                        let alias = if self.peek() == Token::Semicolon {
                            None
                        } else {
                            let token = self.advance();
                            Some(Self::token_as_named_arg_label(&token).ok_or_else(|| {
                                format!("Expected trait method alias, got {token:?}")
                            })?)
                        };
                        self.expect(&Token::Semicolon)?;
                        trait_aliases.push(TraitAlias {
                            trait_name,
                            method,
                            alias,
                            visibility,
                        });
                    }
                    self.expect(&Token::RBrace)?;
                } else {
                    self.expect(&Token::Semicolon)?;
                }
                continue;
            }

            let modifiers = self.parse_member_modifiers();

            if matches!(self.peek(), Token::Function(_)) {
                // Method
                self.advance(); // consume 'function'
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
                let return_type = self.parse_return_type()?;
                let body = self.parse_method_body(&modifiers, &method_name)?;
                self.pop_generic_scope();
                self.class_scope_active = previous_class_scope;
                methods.push(ClassMethod {
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
            } else if self.peek() == Token::Const {
                constants.extend(self.parse_class_constant_declaration(&modifiers, false)?);
            } else if matches!(self.peek(), Token::Variable(_, _)) || self.is_type_hint_start() {
                // Property — possibly with type hint: `private int $x = 0;`
                properties.extend(self.parse_property_declaration(&modifiers)?);
            } else {
                return Err(format!("Unexpected token in class body: {:?}", self.peek()));
            }
        }
        self.in_class_body = prev_in_class;
        self.expect(&Token::RBrace)?;
        self.pop_generic_scope();

        if !is_abstract
            && let Some(method) = methods.iter().find(|method| method.is_abstract)
        {
            return Err(format!(
                "Class {} declares abstract method {}() and must therefore be declared abstract",
                name, method.name
            ));
        }

        Ok(Stmt::Class {
            name,
            parent,
            implements,
            is_abstract,
            is_final,
            is_readonly,
            properties,
            constants,
            methods,
            uses,
            trait_aliases,
            generic_params,
        })
    }

    /// Parse trait declaration
    fn parse_trait(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'trait'
        let name = match self.advance() {
            Token::Identifier(n, _) => n,
            other => return Err(format!("Expected trait name, got {:?}", other)),
        };
        let generic_params = self.parse_generic_parameters()?;
        self.push_generic_scope(&generic_params);
        self.expect(&Token::LBrace)?;

        let mut properties = Vec::new();
        let mut constants = Vec::new();
        let mut methods = Vec::new();
        let mut uses = Vec::new();
        let mut trait_aliases = Vec::new();

        while self.peek() != Token::RBrace && !self.at_eof() {
            if self.peek() == Token::Use {
                self.advance();
                loop {
                    uses.push(self.parse_generic_ancestor()?);
                    if self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                if self.peek() == Token::LBrace {
                    self.advance();
                    while self.peek() != Token::RBrace && !self.at_eof() {
                        let first = self.parse_qualified_name()?;
                        let (trait_name, method) = if self.peek() == Token::DoubleColon {
                            self.advance();
                            let token = self.advance();
                            let method = Self::token_as_named_arg_label(&token).ok_or_else(|| {
                                format!("Expected trait method name, got {token:?}")
                            })?;
                            (Some(first), method)
                        } else {
                            (None, first)
                        };
                        self.expect(&Token::As)?;
                        let visibility = match self.peek() {
                            Token::Public => Some(Visibility::Public),
                            Token::Protected => Some(Visibility::Protected),
                            Token::Private => Some(Visibility::Private),
                            _ => None,
                        };
                        if visibility.is_some() {
                            self.advance();
                        }
                        let alias = if self.peek() == Token::Semicolon {
                            None
                        } else {
                            let token = self.advance();
                            Some(Self::token_as_named_arg_label(&token).ok_or_else(|| {
                                format!("Expected trait method alias, got {token:?}")
                            })?)
                        };
                        self.expect(&Token::Semicolon)?;
                        trait_aliases.push(TraitAlias {
                            trait_name,
                            method,
                            alias,
                            visibility,
                        });
                    }
                    self.expect(&Token::RBrace)?;
                } else {
                    self.expect(&Token::Semicolon)?;
                }
                continue;
            }
            let modifiers = self.parse_member_modifiers();

            if matches!(self.peek(), Token::Function(_)) {
                self.advance();
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
                let return_type = self.parse_return_type()?;
                let body = self.parse_method_body(&modifiers, &method_name)?;
                self.pop_generic_scope();
                self.class_scope_active = previous_class_scope;
                methods.push(ClassMethod {
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
            } else if self.peek() == Token::Const {
                constants.extend(self.parse_class_constant_declaration(&modifiers, false)?);
            } else if matches!(self.peek(), Token::Variable(_, _)) || self.is_type_hint_start() {
                // Property — possibly with type hint
                properties.extend(self.parse_property_declaration(&modifiers)?);
            } else {
                return Err(format!("Unexpected token in trait body: {:?}", self.peek()));
            }
        }
        self.expect(&Token::RBrace)?;
        self.pop_generic_scope();

        Ok(Stmt::Trait {
            name,
            properties,
            constants,
            methods,
            uses,
            trait_aliases,
            generic_params,
        })
    }

    /// Parse interface declaration
    fn parse_interface(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'interface'
        let name = match self.advance() {
            Token::Identifier(n, _) => n,
            other => return Err(format!("Expected interface name, got {:?}", other)),
        };
        let generic_params = self.parse_generic_parameters()?;
        self.push_generic_scope(&generic_params);
        // interface Foo extends Bar, Baz { ... }
        let extends = if self.peek() == Token::Extends {
            self.advance();
            let mut parents = Vec::new();
            loop {
                parents.push(self.parse_generic_ancestor()?);
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

        let mut constants = Vec::new();
        let mut methods = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            let modifiers = self.parse_member_modifiers();
            if self.peek() == Token::Const {
                constants.extend(self.parse_class_constant_declaration(&modifiers, true)?);
            } else if matches!(self.peek(), Token::Function(_)) {
                self.advance(); // consume 'function'
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
                let return_type = self.parse_return_type()?;
                self.expect(&Token::Semicolon)?; // interface methods end with ;
                self.pop_generic_scope();
                self.class_scope_active = previous_class_scope;
                methods.push(ClassMethod {
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
            name,
            extends,
            constants,
            methods,
            generic_params,
        })
    }

    /// Parse enum declaration
    fn parse_enum(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'enum'
        let name = match self.advance() {
            Token::Identifier(n, _) => n,
            other => return Err(format!("Expected enum name, got {:?}", other)),
        };
        // Optional backing type: enum Foo: string { ... }
        let backing_type = if self.peek() == Token::Colon {
            self.advance(); // consume ':'
            Some(self.parse_base_type_hint()?)
        } else {
            None
        };
        let implements = if self.peek() == Token::Implements {
            self.advance();
            let mut interfaces = Vec::new();
            loop {
                interfaces.push(self.parse_generic_ancestor()?);
                if self.peek() != Token::Comma {
                    break;
                }
                self.advance();
            }
            interfaces
        } else {
            Vec::new()
        };
        self.expect(&Token::LBrace)?;

        let mut cases = Vec::new();
        let mut constants = Vec::new();
        let mut methods = Vec::new();
        let mut uses = Vec::new();
        let mut trait_aliases = Vec::new();

        let prev_in_class = self.in_class_body;
        self.in_class_body = true;

        while self.peek() != Token::RBrace && !self.at_eof() {
            if self.peek() == Token::Use {
                self.advance();
                loop {
                    uses.push(self.parse_generic_ancestor()?);
                    if self.peek() != Token::Comma {
                        break;
                    }
                    self.advance();
                }
                if self.peek() == Token::LBrace {
                    self.advance();
                    while self.peek() != Token::RBrace && !self.at_eof() {
                        let first = self.parse_qualified_name()?;
                        let (trait_name, method) = if self.peek() == Token::DoubleColon {
                            self.advance();
                            let token = self.advance();
                            let method = Self::token_as_named_arg_label(&token).ok_or_else(|| {
                                format!("Expected trait method name, got {token:?}")
                            })?;
                            (Some(first), method)
                        } else {
                            (None, first)
                        };
                        self.expect(&Token::As)?;
                        let visibility = match self.peek() {
                            Token::Public => Some(Visibility::Public),
                            Token::Protected => Some(Visibility::Protected),
                            Token::Private => Some(Visibility::Private),
                            _ => None,
                        };
                        if visibility.is_some() {
                            self.advance();
                        }
                        let alias = if self.peek() == Token::Semicolon {
                            None
                        } else {
                            let token = self.advance();
                            Some(Self::token_as_named_arg_label(&token).ok_or_else(|| {
                                format!("Expected trait method alias, got {token:?}")
                            })?)
                        };
                        self.expect(&Token::Semicolon)?;
                        trait_aliases.push(TraitAlias {
                            trait_name,
                            method,
                            alias,
                            visibility,
                        });
                    }
                    self.expect(&Token::RBrace)?;
                } else {
                    self.expect(&Token::Semicolon)?;
                }
            } else if self.peek() == Token::Case {
                self.advance(); // consume 'case'
                let case_name = match self.advance() {
                    Token::Identifier(n, _) => n,
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
                let modifiers = self.parse_member_modifiers();
                if self.peek() == Token::Const {
                    constants.extend(self.parse_class_constant_declaration(&modifiers, false)?);
                } else if matches!(self.peek(), Token::Function(_)) {
                    self.advance();
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
                    let return_type = self.parse_return_type()?;
                    self.expect(&Token::LBrace)?;
                    let mut body = Vec::new();
                    while self.peek() != Token::RBrace && !self.at_eof() {
                        body.push(self.parse_stmt()?);
                    }
                    self.expect(&Token::RBrace)?;
                    self.pop_generic_scope();
                    self.class_scope_active = previous_class_scope;
                    methods.push(ClassMethod {
                        visibility: modifiers.visibility,
                        name: method_name,
                        params,
                        body,
                        is_static: modifiers.is_static,
                        is_final: modifiers.is_final,
                        is_abstract: false,
                        returns_by_ref,
                        return_type,
                        generic_params,
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
            implements,
            uses,
            trait_aliases,
            cases,
            constants,
            methods,
        })
    }

    fn parse_member_modifiers(&mut self) -> MemberModifiers {
        let mut modifiers = MemberModifiers::default();

        loop {
            match self.peek() {
                Token::Public => {
                    self.advance();
                    modifiers.visibility = Visibility::Public;
                }
                Token::Protected => {
                    self.advance();
                    modifiers.visibility = Visibility::Protected;
                }
                Token::Private => {
                    self.advance();
                    modifiers.visibility = Visibility::Private;
                }
                Token::Static => {
                    self.advance();
                    modifiers.is_static = true;
                }
                Token::Final => {
                    self.advance();
                    modifiers.is_final = true;
                }
                Token::Abstract => {
                    self.advance();
                    modifiers.is_abstract = true;
                }
                Token::Identifier(ref s, _) if s == "readonly" => {
                    self.advance();
                    modifiers.is_readonly = true;
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
    ) -> Result<Vec<ClassConstant>, String> {
        if modifiers.is_static {
            return Err("Class constants cannot be declared static".into());
        }
        if modifiers.is_abstract {
            return Err("Class constants cannot be declared abstract".into());
        }
        if modifiers.is_readonly {
            return Err("Class constants cannot be declared readonly".into());
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
        loop {
            let name = match self.advance() {
                Token::Identifier(name, _) | Token::MagicConstant { name, .. } => name,
                Token::Goto { name, .. } => name,
                other => return Err(format!("Expected class constant name, got {:?}", other)),
            };
            self.expect(&Token::Assign)?;
            let value = self.parse_expr()?;
            constants.push(ClassConstant {
                visibility: modifiers.visibility,
                name,
                value,
                type_hint: type_hint.clone(),
                is_final: modifiers.is_final,
            });
            if self.peek() != Token::Comma {
                break;
            }
            self.advance();
        }
        self.expect(&Token::Semicolon)?;
        Ok(constants)
    }

    fn try_parse_class_constant_type(&mut self) -> Result<Option<TypeHint>, String> {
        if matches!(self.peek(), Token::Identifier(_, _) | Token::Goto { .. })
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
                | Token::Static
        ) {
            let first = self.parse_base_type_hint()?;
            self.maybe_parse_compound_type(first)?
        } else {
            return Ok(None);
        };

        if Self::class_constant_type_is_forbidden(&hint) {
            return Err(format!(
                "Class constant type {} is not permitted",
                match hint {
                    TypeHint::Void => "void",
                    TypeHint::Never => "never",
                    TypeHint::Callable => "callable",
                    _ => "declaration",
                }
            ));
        }
        Ok(Some(hint))
    }

    fn class_constant_type_is_forbidden(hint: &TypeHint) -> bool {
        match hint {
            TypeHint::Void | TypeHint::Never | TypeHint::Callable => true,
            TypeHint::Nullable(inner) | TypeHint::GenericParameter { erased: inner, .. } => {
                Self::class_constant_type_is_forbidden(inner)
            }
            TypeHint::Union(parts) | TypeHint::Intersection(parts) => {
                parts.iter().any(Self::class_constant_type_is_forbidden)
            }
            TypeHint::GenericApplication { arguments, .. } => arguments
                .iter()
                .any(Self::class_constant_type_is_forbidden),
            _ => false,
        }
    }

    fn parse_method_body(
        &mut self,
        modifiers: &MemberModifiers,
        method_name: &str,
    ) -> Result<Vec<Stmt>, String> {
        if modifiers.is_abstract {
            if modifiers.is_final {
                return Err(format!(
                    "Cannot use the final modifier on an abstract method {}()",
                    method_name
                ));
            }
            if modifiers.visibility == Visibility::Private {
                return Err(format!(
                    "Abstract function {}() cannot be declared private",
                    method_name
                ));
            }
            self.expect(&Token::Semicolon)?;
            return Ok(Vec::new());
        }
        self.expect(&Token::LBrace)?;
        let mut body = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            body.push(self.parse_stmt()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(body)
    }

    /// Parse match expression
    fn parse_match_expr(&mut self) -> Result<Expr, String> {
        self.advance(); // consume 'match'
        self.expect_lparen()?;
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
        let return_type = self.parse_return_type()?;
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
            Expr::ListAssign { expr, .. } => Self::collect_free_vars(expr, bound, out),
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
            Expr::FirstClassCallable(callable) => {
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
            Expr::StaticProperty { .. }
            | Expr::ClassConstant { .. }
            | Expr::FirstClassFunctionCallable(_) => {}
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
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Globals { .. }
            | Expr::CompileError { .. }
            | Expr::Constant(_)
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
            Expr::YieldFrom(sub) => {
                Self::collect_free_vars(sub, bound, out);
            }
            Expr::Clone { expr: inner, .. } => {
                Self::collect_free_vars(inner, bound, out);
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
        if self.peek() == Token::Use {
            self.advance();
            self.expect_lparen()?;
            let is_ref = if self.peek() == Token::Ampersand {
                self.advance();
                true
            } else {
                false
            };
            let (v, line) = match self.advance() {
                Token::Variable(n, line) => (n, line),
                other => return Err(format!("Expected variable in use, got {:?}", other)),
            };
            if v == "GLOBALS" {
                self.compile_error("Cannot use auto-global as lexical variable", line);
            }
            use_vars.push((v, is_ref, line));
            while self.peek() == Token::Comma {
                self.advance();
                let is_ref = if self.peek() == Token::Ampersand {
                    self.advance();
                    true
                } else {
                    false
                };
                let (v, line) = match self.advance() {
                    Token::Variable(n, line) => (n, line),
                    other => return Err(format!("Expected variable in use, got {:?}", other)),
                };
                if v == "GLOBALS" {
                    self.compile_error("Cannot use auto-global as lexical variable", line);
                }
                use_vars.push((v, is_ref, line));
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
        self.pop_generic_scope();

        Ok(Expr::Closure {
            line,
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
