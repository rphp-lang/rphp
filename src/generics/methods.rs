use super::link::{erase_method_signature, substitute_generic_parameters};
use super::lsp::effective_inheritance_arguments;
use super::{
    GenericDeclaration, GenericMetadata, GenericMethodContract, GenericMethodMetadata,
    GenericRuntimeMode, GenericSymbol, GenericType, ReifiedBinding,
};

impl GenericMetadata {
    /// Raw PHP signatures expose only erased bounds. Once a method carries a
    /// type-parameter relationship, Parametric LSP is the authoritative link
    /// check and the legacy interface validator must not re-check that erased
    /// approximation against the substituted implementation.
    pub fn method_has_parametric_signature(&self, owner: &str, method: &str) -> bool {
        self.find_class_like(owner)
            .and_then(|declaration| self.find_method(declaration, method))
            .is_some_and(|method| {
                method.parameters.iter().any(|parameter| {
                    parameter
                        .bound
                        .as_ref()
                        .is_some_and(type_contains_any_parameter)
                        || parameter
                            .default
                            .as_ref()
                            .is_some_and(type_contains_any_parameter)
                }) || method
                    .value_parameters
                    .iter()
                    .flatten()
                    .any(type_contains_any_parameter)
                    || method
                        .return_type
                        .as_ref()
                        .is_some_and(type_contains_any_parameter)
            })
    }

    /// Whether a monomorphic method cache needs a receiver-specific generic
    /// contract. Own methods need one only for an explicitly reified receiver;
    /// inherited methods may also need a linked bound-erased child view.
    pub fn has_instance_method_contract(&self, owner: &str, method: &str) -> bool {
        let Some(child) = self.find_class_like(owner) else {
            return false;
        };
        if cfg!(feature = "php-generics-reified") && !child.parameters.is_empty() {
            if let Some(implementation) = self.find_method(child, method) {
                return method_depends_on_parameters(implementation);
            }
            if self
                .ancestor_bindings(child)
                .into_iter()
                .any(|(ancestor, _)| {
                    self.find_method(ancestor, method)
                        .is_some_and(method_depends_on_parameters)
                })
            {
                return true;
            }
        }
        self.linked_instance_method_contract_for(child, method)
            .is_some()
    }

    /// Resolve a method signature against one concrete reified object
    /// binding. The returned contract is self-contained and can safely live in
    /// an executor sidecar across nested calls and metadata-table borrows.
    pub fn reified_instance_method_contract(
        &self,
        binding: ReifiedBinding,
        method: &str,
    ) -> Option<GenericMethodContract> {
        let child = self.declaration(binding)?;
        let site = self.use_site(binding.use_site)?;
        let effective = effective_inheritance_arguments(child, &site.arguments);
        if effective.len() != child.parameters.len() {
            return None;
        }

        if let Some(implementation) = self.find_method(child, method) {
            return self.substituted_reified_method_contract(child, implementation, &effective);
        }
        let candidates = self
            .ancestor_bindings_scoped_from(child, &effective)
            .into_iter()
            .filter_map(|(ancestor, arguments, scope)| {
                self.find_method(ancestor, method)
                    .map(|prototype| (ancestor, prototype, arguments, scope))
            })
            .collect::<Vec<_>>();
        if candidates.is_empty()
            || !candidates
                .iter()
                .any(|(_, prototype, _, _)| method_depends_on_parameters(prototype))
        {
            return None;
        }
        let parameter_count = candidates
            .iter()
            .map(|(_, prototype, _, _)| prototype.value_parameters.len())
            .max()
            .unwrap_or(0);
        let value_parameters = (0..parameter_count)
            .map(|index| {
                let merged = self.merge_generic_union(candidates.iter().map(
                    |(ancestor, prototype, arguments, scope)| {
                        prototype
                            .value_parameters
                            .get(index)
                            .and_then(Option::as_ref)
                            .map(|value| substitute_generic_parameters(value, arguments))
                            .map(|value| {
                                erase_method_signature(&value, ancestor, prototype, arguments)
                            })
                            .map(|value| self.resolve_contract_type(value, *scope))
                            .unwrap_or(GenericType::Mixed)
                    },
                ));
                (!matches!(merged, GenericType::Mixed)).then_some(merged)
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let return_type = self.merge_generic_intersection(candidates.iter().map(
            |(ancestor, prototype, arguments, scope)| {
                prototype
                    .return_type
                    .as_ref()
                    .map(|value| substitute_generic_parameters(value, arguments))
                    .map(|value| erase_method_signature(&value, ancestor, prototype, arguments))
                    .map(|value| self.resolve_contract_type(value, *scope))
                    .unwrap_or(GenericType::Mixed)
            },
        ));
        let return_type = (!matches!(return_type, GenericType::Mixed)).then_some(return_type);
        if value_parameters.iter().all(Option::is_none) && return_type.is_none() {
            return None;
        }
        Some(GenericMethodContract {
            owner: self.symbol(candidates[0].0.owner).unwrap_or("?").into(),
            scope: self.symbol(candidates[0].3).unwrap_or("?").into(),
            method: self.symbol(candidates[0].1.name).unwrap_or(method).into(),
            value_parameters,
            return_type,
            is_variadic: candidates
                .iter()
                .any(|(_, prototype, _, _)| prototype.is_variadic),
            runtime_mode: GenericRuntimeMode::Reified,
        })
    }

    /// Materialize only the inherited boundaries whose child-substituted
    /// bound-erased runtime type differs from the executable parent ABI.
    pub fn linked_instance_method_contract(
        &self,
        declaration: u32,
        method: &str,
    ) -> Option<GenericMethodContract> {
        let child = self.declarations.get(declaration as usize)?;
        self.linked_instance_method_contract_for(child, method)
    }

    /// Whether one non-reifiable receiver has a stable linked Long-to-Long
    /// boundary. A method IC may use this proof before materializing a runtime
    /// contract; a scalar-plan side exit still falls back to canonical checks.
    pub fn linked_instance_method_contract_admits_exact_long(
        &self,
        owner: &str,
        method: &str,
        arguments: u32,
    ) -> bool {
        let Some(child) = self.find_class_like(owner) else {
            return false;
        };
        if cfg!(feature = "php-generics-reified") && !child.parameters.is_empty() {
            return false;
        }
        self.linked_instance_method_contract_for(child, method)
            .is_some_and(|contract| contract.admits_exact_long_call(arguments))
    }

    fn linked_instance_method_contract_for(
        &self,
        child: &GenericDeclaration,
        method: &str,
    ) -> Option<GenericMethodContract> {
        // An override is compiled against the child's own erased signature and
        // deliberately stops inherited contract lookup.
        if self.find_method(child, method).is_some() {
            return None;
        }
        let candidates = self
            .ancestor_bindings_scoped(child)
            .into_iter()
            .filter_map(|(ancestor, arguments, scope)| {
                self.find_method(ancestor, method)
                    .map(|prototype| (ancestor, prototype, arguments, scope))
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return None;
        }
        let parameter_count = candidates
            .iter()
            .map(|(_, prototype, _, _)| prototype.value_parameters.len())
            .max()
            .unwrap_or(0);
        let value_parameters = (0..parameter_count)
            .map(|index| {
                let child_abi = self.merge_generic_union(candidates.iter().map(
                    |(_ancestor, prototype, arguments, scope)| {
                        prototype
                            .value_parameters
                            .get(index)
                            .and_then(Option::as_ref)
                            .map(|value| substitute_generic_parameters(value, arguments))
                            .map(|value| {
                                erase_method_signature(&value, child, prototype, arguments)
                            })
                            .map(|value| self.resolve_contract_type(value, *scope))
                            .unwrap_or(GenericType::Mixed)
                    },
                ));
                let parent_abi = self.merge_generic_union(candidates.iter().map(
                    |(ancestor, prototype, _, scope)| {
                        let identity = (0..ancestor.parameters.len())
                            .map(|parameter| GenericType::Parameter(parameter as u8))
                            .collect::<Vec<_>>();
                        prototype
                            .value_parameters
                            .get(index)
                            .and_then(Option::as_ref)
                            .map(|value| {
                                erase_method_signature(value, ancestor, prototype, &identity)
                            })
                            .map(|value| self.resolve_contract_type(value, *scope))
                            .unwrap_or(GenericType::Mixed)
                    },
                ));
                let child_abi = erase_named_type_arguments(child_abi);
                let parent_abi = erase_named_type_arguments(parent_abi);
                (child_abi != parent_abi).then_some(child_abi)
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let child_return = erase_named_type_arguments(self.merge_generic_intersection(
            candidates.iter().map(|(_, prototype, arguments, scope)| {
                prototype
                    .return_type
                    .as_ref()
                    .map(|value| substitute_generic_parameters(value, arguments))
                    .map(|value| erase_method_signature(&value, child, prototype, arguments))
                    .map(|value| self.resolve_contract_type(value, *scope))
                    .unwrap_or(GenericType::Mixed)
            }),
        ));
        let parent_return = erase_named_type_arguments(self.merge_generic_intersection(
            candidates.iter().map(|(ancestor, prototype, _, scope)| {
                let identity = (0..ancestor.parameters.len())
                    .map(|parameter| GenericType::Parameter(parameter as u8))
                    .collect::<Vec<_>>();
                prototype
                    .return_type
                    .as_ref()
                    .map(|value| erase_method_signature(value, ancestor, prototype, &identity))
                    .map(|value| self.resolve_contract_type(value, *scope))
                    .unwrap_or(GenericType::Mixed)
            }),
        ));
        let return_type = (child_return != parent_return).then_some(child_return);
        if value_parameters.iter().all(Option::is_none) && return_type.is_none() {
            return None;
        }
        Some(GenericMethodContract {
            owner: self.symbol(child.owner).unwrap_or("?").into(),
            scope: self.symbol(candidates[0].3).unwrap_or("?").into(),
            method: method.into(),
            value_parameters,
            return_type,
            is_variadic: candidates
                .iter()
                .any(|(_, prototype, _, _)| prototype.is_variadic),
            runtime_mode: GenericRuntimeMode::BoundErased,
        })
    }

    fn find_method<'a>(
        &'a self,
        declaration: &'a GenericDeclaration,
        method: &str,
    ) -> Option<&'a GenericMethodMetadata> {
        declaration.methods.iter().find(|candidate| {
            self.symbol(candidate.name)
                .is_some_and(|name| name.eq_ignore_ascii_case(method))
        })
    }

    fn substituted_reified_method_contract(
        &self,
        declaration: &GenericDeclaration,
        method: &GenericMethodMetadata,
        arguments: &[GenericType],
    ) -> Option<GenericMethodContract> {
        if !method_depends_on_parameters(method) {
            return None;
        }
        Some(GenericMethodContract {
            owner: self.symbol(declaration.owner).unwrap_or("?").into(),
            scope: self.symbol(declaration.owner).unwrap_or("?").into(),
            method: self.symbol(method.name).unwrap_or("?").into(),
            value_parameters: method
                .value_parameters
                .iter()
                .map(|value| {
                    value
                        .as_ref()
                        .map(|value| substitute_generic_parameters(value, arguments))
                        .map(|value| erase_method_signature(&value, declaration, method, arguments))
                        .map(|value| self.resolve_contract_type(value, declaration.owner))
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            return_type: method
                .return_type
                .as_ref()
                .map(|value| substitute_generic_parameters(value, arguments))
                .map(|value| erase_method_signature(&value, declaration, method, arguments))
                .map(|value| self.resolve_contract_type(value, declaration.owner)),
            is_variadic: method.is_variadic,
            runtime_mode: GenericRuntimeMode::Reified,
        })
    }

    /// Resolve `self`/`parent` while each diamond candidate still owns its
    /// lexical scope. An invalid or currently unsupported pseudo-type becomes
    /// `never`, which keeps argument unions neutral and makes return
    /// intersections fail closed. Supported contracts therefore enter the hot
    /// runtime matcher with no candidate-specific scope left to recover.
    fn resolve_contract_type(&self, value: GenericType, scope: GenericSymbol) -> GenericType {
        self.resolve_lexical_class_pseudo_types(&value, scope)
            .unwrap_or(GenericType::Never)
    }
}

/// Bound erasure drops arguments on a named type application before deciding
/// whether a linked child boundary differs from its executable PHP ABI. The
/// outer named type remains enforced by the ordinary signature; reified
/// contracts deliberately retain their nested arguments.
fn erase_named_type_arguments(value: GenericType) -> GenericType {
    match value {
        GenericType::Named { name, .. } => GenericType::Named {
            name,
            arguments: Box::new([]),
        },
        GenericType::Nullable(inner) => {
            GenericType::Nullable(Box::new(erase_named_type_arguments(*inner)))
        }
        GenericType::Union(parts) => GenericType::Union(
            parts
                .into_vec()
                .into_iter()
                .map(erase_named_type_arguments)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        GenericType::Intersection(parts) => GenericType::Intersection(
            parts
                .into_vec()
                .into_iter()
                .map(erase_named_type_arguments)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        value => value,
    }
}

fn type_contains_any_parameter(value: &GenericType) -> bool {
    match value {
        GenericType::Parameter(_) => true,
        GenericType::Named { arguments, .. }
        | GenericType::Union(arguments)
        | GenericType::Intersection(arguments) => arguments.iter().any(type_contains_any_parameter),
        GenericType::Nullable(inner) => type_contains_any_parameter(inner),
        GenericType::Int
        | GenericType::Float
        | GenericType::String
        | GenericType::Bool
        | GenericType::Array
        | GenericType::Callable
        | GenericType::Null
        | GenericType::Void
        | GenericType::Mixed
        | GenericType::Never => false,
    }
}

fn method_depends_on_parameters(method: &GenericMethodMetadata) -> bool {
    method
        .value_parameters
        .iter()
        .flatten()
        .any(|value| type_depends_on_class_parameter(value, method, method.parameters.len() + 1))
        || method.return_type.as_ref().is_some_and(|value| {
            type_depends_on_class_parameter(value, method, method.parameters.len() + 1)
        })
}

fn type_depends_on_class_parameter(
    value: &GenericType,
    method: &GenericMethodMetadata,
    remaining: usize,
) -> bool {
    if remaining == 0 {
        return false;
    }
    match value {
        GenericType::Parameter(index) => {
            super::method_parameter_index(*index).map_or(true, |index| {
                method
                    .parameters
                    .get(index)
                    .and_then(|parameter| parameter.bound.as_ref())
                    .is_some_and(|bound| {
                        type_depends_on_class_parameter(bound, method, remaining - 1)
                    })
            })
        }
        GenericType::Named { arguments, .. }
        | GenericType::Union(arguments)
        | GenericType::Intersection(arguments) => arguments
            .iter()
            .any(|value| type_depends_on_class_parameter(value, method, remaining)),
        GenericType::Nullable(inner) => type_depends_on_class_parameter(inner, method, remaining),
        GenericType::Int
        | GenericType::Float
        | GenericType::String
        | GenericType::Bool
        | GenericType::Array
        | GenericType::Callable
        | GenericType::Null
        | GenericType::Void
        | GenericType::Mixed
        | GenericType::Never => false,
    }
}
