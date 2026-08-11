use super::lsp::effective_inheritance_arguments;
use super::{
    GenericDeclaration, GenericMetadata, GenericSymbol, GenericType, GenericUseSite, ReifiedBinding,
};
use crate::value::{Value, ValueType};

impl GenericMetadata {
    pub fn value_matches_binding_reified<F, G>(
        &self,
        value: &Value,
        expected: &GenericType,
        binding: ReifiedBinding,
        class_is_a: F,
        generic_arguments_match: G,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
        G: Fn(&Value, &str, &[GenericType], &GenericDeclaration, &GenericUseSite) -> bool,
    {
        let Some(declaration) = self.declaration(binding) else {
            return false;
        };
        let Some(site) = self.use_site(binding.use_site) else {
            return false;
        };
        let value = if value.is_reference() {
            unsafe { &*value.as_ref_ptr() }
        } else {
            value
        };
        self.value_matches_binding_type(
            value,
            expected,
            declaration,
            site,
            &class_is_a,
            &generic_arguments_match,
            0,
        )
    }

    pub fn value_matches_resolved_type_reified<F, G>(
        &self,
        value: &Value,
        expected: &GenericType,
        class_is_a: F,
        generic_arguments_match: G,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
        G: Fn(&Value, &str, &[GenericType]) -> bool,
    {
        let value = if value.is_reference() {
            unsafe { &*value.as_ref_ptr() }
        } else {
            value
        };
        self.value_matches_resolved_reified_type(
            value,
            expected,
            &class_is_a,
            &generic_arguments_match,
        )
    }

    pub(crate) fn reified_arguments_for_owner(
        &self,
        binding: ReifiedBinding,
        owner: GenericSymbol,
    ) -> Option<Box<[GenericType]>> {
        let declaration = self.declaration(binding)?;
        let site = self.use_site(binding.use_site)?;
        let effective = effective_inheritance_arguments(declaration, &site.arguments);
        if declaration.owner == owner {
            return Some(effective.into_boxed_slice());
        }
        self.ancestor_bindings_from(declaration, &effective)
            .into_iter()
            .find_map(|(ancestor, arguments)| {
                (ancestor.owner == owner).then(|| arguments.into_boxed_slice())
            })
    }

    /// A zero-parameter concrete descendant has no per-object reified
    /// binding, but its link-time ancestor tuple is still canonical. Generic
    /// declarations with unresolved own parameters deliberately fail closed.
    pub(crate) fn concrete_arguments_for_owner(
        &self,
        concrete: &str,
        owner: GenericSymbol,
    ) -> Option<Box<[GenericType]>> {
        let declaration = self.find_class_like(concrete)?;
        if !declaration.parameters.is_empty() {
            return None;
        }
        if declaration.owner == owner {
            return Some(Box::new([]));
        }
        self.ancestor_bindings(declaration)
            .into_iter()
            .find_map(|(ancestor, arguments)| {
                (ancestor.owner == owner).then(|| arguments.into_boxed_slice())
            })
    }

    pub(crate) fn reified_arguments_match_binding(
        &self,
        actual: &[GenericType],
        expected: &[GenericType],
        declaration: &GenericDeclaration,
        site: &GenericUseSite,
    ) -> bool {
        actual.len() == expected.len()
            && actual.iter().zip(expected).all(|(actual, expected)| {
                self.reified_type_matches_binding(actual, expected, declaration, site, 0)
            })
    }

    pub(crate) fn reified_arguments_match_resolved(
        &self,
        actual: &[GenericType],
        expected: &[GenericType],
    ) -> bool {
        actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected)
                .all(|(actual, expected)| self.reified_type_matches_resolved(actual, expected))
    }

    #[allow(clippy::too_many_arguments)]
    fn value_matches_binding_type<F, G>(
        &self,
        value: &Value,
        expected: &GenericType,
        declaration: &GenericDeclaration,
        site: &GenericUseSite,
        class_is_a: &F,
        generic_arguments_match: &G,
        depth: usize,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
        G: Fn(&Value, &str, &[GenericType], &GenericDeclaration, &GenericUseSite) -> bool,
    {
        if depth > declaration.parameters.len() + 1 {
            return false;
        }
        match expected {
            GenericType::Parameter(index) => declaration
                .parameters
                .get(*index as usize)
                .and_then(|parameter| {
                    site.arguments
                        .get(*index as usize)
                        .or(parameter.default.as_ref())
                })
                .is_some_and(|resolved| {
                    self.value_matches_binding_type(
                        value,
                        resolved,
                        declaration,
                        site,
                        class_is_a,
                        generic_arguments_match,
                        depth + 1,
                    )
                }),
            GenericType::Int => value.value_type() == ValueType::Long,
            GenericType::Float => value.value_type() == ValueType::Double,
            GenericType::String => value.value_type() == ValueType::String,
            GenericType::Bool => matches!(value.value_type(), ValueType::True | ValueType::False),
            GenericType::Array => value.value_type() == ValueType::Array,
            GenericType::Callable => matches!(
                value.value_type(),
                ValueType::String | ValueType::Array | ValueType::Closure
            ),
            GenericType::Null => value.value_type() == ValueType::Null,
            GenericType::Void | GenericType::Never => false,
            GenericType::Mixed => true,
            GenericType::Named { name, arguments } => {
                let Some(expected_name) = self.symbol(*name) else {
                    return false;
                };
                if !self.outer_named_type_matches(value, expected_name, class_is_a) {
                    return false;
                }
                arguments.is_empty()
                    || generic_arguments_match(value, expected_name, arguments, declaration, site)
            }
            GenericType::Nullable(inner) => {
                value.value_type() == ValueType::Null
                    || self.value_matches_binding_type(
                        value,
                        inner,
                        declaration,
                        site,
                        class_is_a,
                        generic_arguments_match,
                        depth,
                    )
            }
            GenericType::Union(parts) => parts.iter().any(|part| {
                self.value_matches_binding_type(
                    value,
                    part,
                    declaration,
                    site,
                    class_is_a,
                    generic_arguments_match,
                    depth,
                )
            }),
            GenericType::Intersection(parts) => parts.iter().all(|part| {
                self.value_matches_binding_type(
                    value,
                    part,
                    declaration,
                    site,
                    class_is_a,
                    generic_arguments_match,
                    depth,
                )
            }),
        }
    }

    fn value_matches_resolved_reified_type<F, G>(
        &self,
        value: &Value,
        expected: &GenericType,
        class_is_a: &F,
        generic_arguments_match: &G,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
        G: Fn(&Value, &str, &[GenericType]) -> bool,
    {
        match expected {
            GenericType::Parameter(_) => false,
            GenericType::Int => value.value_type() == ValueType::Long,
            GenericType::Float => value.value_type() == ValueType::Double,
            GenericType::String => value.value_type() == ValueType::String,
            GenericType::Bool => matches!(value.value_type(), ValueType::True | ValueType::False),
            GenericType::Array => value.value_type() == ValueType::Array,
            GenericType::Callable => matches!(
                value.value_type(),
                ValueType::String | ValueType::Array | ValueType::Closure
            ),
            GenericType::Null => value.value_type() == ValueType::Null,
            GenericType::Void | GenericType::Never => false,
            GenericType::Mixed => true,
            GenericType::Named { name, arguments } => {
                let Some(expected_name) = self.symbol(*name) else {
                    return false;
                };
                self.outer_named_type_matches(value, expected_name, class_is_a)
                    && (arguments.is_empty()
                        || generic_arguments_match(value, expected_name, arguments))
            }
            GenericType::Nullable(inner) => {
                value.value_type() == ValueType::Null
                    || self.value_matches_resolved_reified_type(
                        value,
                        inner,
                        class_is_a,
                        generic_arguments_match,
                    )
            }
            GenericType::Union(parts) => parts.iter().any(|part| {
                self.value_matches_resolved_reified_type(
                    value,
                    part,
                    class_is_a,
                    generic_arguments_match,
                )
            }),
            GenericType::Intersection(parts) => parts.iter().all(|part| {
                self.value_matches_resolved_reified_type(
                    value,
                    part,
                    class_is_a,
                    generic_arguments_match,
                )
            }),
        }
    }

    fn outer_named_type_matches<F>(
        &self,
        value: &Value,
        expected_name: &str,
        class_is_a: &F,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
    {
        if value.value_type() != ValueType::Object {
            return false;
        }
        let class_name = unsafe { value.object_class_name_unchecked() };
        expected_name.eq_ignore_ascii_case("object")
            || class_name.eq_ignore_ascii_case(expected_name)
            || class_is_a(class_name, expected_name)
    }

    fn reified_type_matches_binding(
        &self,
        actual: &GenericType,
        expected: &GenericType,
        declaration: &GenericDeclaration,
        site: &GenericUseSite,
        depth: usize,
    ) -> bool {
        if depth > declaration.parameters.len() + 1 {
            return false;
        }
        if let GenericType::Parameter(index) = expected {
            return declaration
                .parameters
                .get(*index as usize)
                .and_then(|parameter| {
                    site.arguments
                        .get(*index as usize)
                        .or(parameter.default.as_ref())
                })
                .is_some_and(|resolved| {
                    self.reified_type_matches_binding(
                        actual,
                        resolved,
                        declaration,
                        site,
                        depth + 1,
                    )
                });
        }
        match (actual, expected) {
            (
                GenericType::Named {
                    name: actual_name,
                    arguments: actual_arguments,
                },
                GenericType::Named {
                    name: expected_name,
                    arguments: expected_arguments,
                },
            ) => {
                self.symbol(*actual_name)
                    .zip(self.symbol(*expected_name))
                    .is_some_and(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
                    && actual_arguments.len() == expected_arguments.len()
                    && actual_arguments
                        .iter()
                        .zip(expected_arguments)
                        .all(|(actual, expected)| {
                            self.reified_type_matches_binding(
                                actual,
                                expected,
                                declaration,
                                site,
                                depth,
                            )
                        })
            }
            (GenericType::Nullable(actual), GenericType::Nullable(expected)) => {
                self.reified_type_matches_binding(actual, expected, declaration, site, depth)
            }
            (GenericType::Union(actual), GenericType::Union(expected))
            | (GenericType::Intersection(actual), GenericType::Intersection(expected)) => {
                actual.len() == expected.len()
                    && actual.iter().all(|actual| {
                        expected.iter().any(|expected| {
                            self.reified_type_matches_binding(
                                actual,
                                expected,
                                declaration,
                                site,
                                depth,
                            )
                        })
                    })
                    && expected.iter().all(|expected| {
                        actual.iter().any(|actual| {
                            self.reified_type_matches_binding(
                                actual,
                                expected,
                                declaration,
                                site,
                                depth,
                            )
                        })
                    })
            }
            _ => actual == expected,
        }
    }

    fn reified_type_matches_resolved(&self, actual: &GenericType, expected: &GenericType) -> bool {
        match (actual, expected) {
            (
                GenericType::Named {
                    name: actual_name,
                    arguments: actual_arguments,
                },
                GenericType::Named {
                    name: expected_name,
                    arguments: expected_arguments,
                },
            ) => {
                self.symbol(*actual_name)
                    .zip(self.symbol(*expected_name))
                    .is_some_and(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
                    && self.reified_arguments_match_resolved(actual_arguments, expected_arguments)
            }
            (GenericType::Nullable(actual), GenericType::Nullable(expected)) => {
                self.reified_type_matches_resolved(actual, expected)
            }
            (GenericType::Union(actual), GenericType::Union(expected))
            | (GenericType::Intersection(actual), GenericType::Intersection(expected)) => {
                actual.len() == expected.len()
                    && actual.iter().all(|actual| {
                        expected
                            .iter()
                            .any(|expected| self.reified_type_matches_resolved(actual, expected))
                    })
                    && expected.iter().all(|expected| {
                        actual
                            .iter()
                            .any(|actual| self.reified_type_matches_resolved(actual, expected))
                    })
            }
            _ => actual == expected,
        }
    }
}
