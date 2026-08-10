use super::link::{erase_method_signature, substitute_generic_parameters};
use super::lsp::effective_inheritance_arguments;
use super::{
    GenericDeclaration, GenericMetadata, GenericMethodContract, GenericMethodMetadata,
    GenericRuntimeMode, GenericType, ReifiedBinding,
};

impl GenericMetadata {
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
        for (ancestor, arguments) in self.ancestor_bindings_from(child, &effective) {
            let Some(prototype) = self.find_method(ancestor, method) else {
                continue;
            };
            return self.substituted_reified_method_contract(ancestor, prototype, &arguments);
        }
        None
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
        for (ancestor, arguments) in self.ancestor_bindings(child) {
            let Some(prototype) = self.find_method(ancestor, method) else {
                continue;
            };
            let parent_identity = (0..ancestor.parameters.len())
                .map(|index| GenericType::Parameter(index as u8))
                .collect::<Vec<_>>();
            let value_parameters = prototype
                .value_parameters
                .iter()
                .map(|value| {
                    let value = value.as_ref()?;
                    let parent_abi =
                        erase_method_signature(value, ancestor, prototype, &parent_identity);
                    let substituted = substitute_generic_parameters(value, &arguments);
                    let child_abi =
                        erase_method_signature(&substituted, child, prototype, &arguments);
                    (child_abi != parent_abi).then_some(child_abi)
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let return_type = prototype.return_type.as_ref().and_then(|value| {
                let parent_abi =
                    erase_method_signature(value, ancestor, prototype, &parent_identity);
                let substituted = substitute_generic_parameters(value, &arguments);
                let child_abi = erase_method_signature(&substituted, child, prototype, &arguments);
                (child_abi != parent_abi).then_some(child_abi)
            });
            if value_parameters.iter().all(Option::is_none) && return_type.is_none() {
                return None;
            }
            return Some(GenericMethodContract {
                owner: self.symbol(child.owner).unwrap_or("?").into(),
                method: self.symbol(prototype.name).unwrap_or("?").into(),
                value_parameters,
                return_type,
                is_variadic: prototype.is_variadic,
                runtime_mode: GenericRuntimeMode::BoundErased,
            });
        }
        None
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
            method: self.symbol(method.name).unwrap_or("?").into(),
            value_parameters: method
                .value_parameters
                .iter()
                .map(|value| {
                    value
                        .as_ref()
                        .map(|value| substitute_generic_parameters(value, arguments))
                        .map(|value| erase_method_signature(&value, declaration, method, arguments))
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            return_type: method
                .return_type
                .as_ref()
                .map(|value| substitute_generic_parameters(value, arguments))
                .map(|value| erase_method_signature(&value, declaration, method, arguments)),
            is_variadic: method.is_variadic,
            runtime_mode: GenericRuntimeMode::Reified,
        })
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
        GenericType::Named { arguments, .. } | GenericType::Union(arguments) => arguments
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
