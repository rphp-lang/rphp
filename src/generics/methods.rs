use super::link::substitute_generic_parameters;
use super::lsp::effective_inheritance_arguments;
use super::{
    GenericDeclaration, GenericMetadata, GenericMethodMetadata, GenericType, ReifiedBinding,
    ReifiedMethodContract,
};

impl GenericMetadata {
    /// Whether a monomorphic method cache needs to probe for a receiver
    /// binding. An override without class-parameter uses deliberately stops
    /// the search: its widened/concrete signature is the effective contract.
    pub fn has_reified_instance_method_contract(&self, owner: &str, method: &str) -> bool {
        let Some(child) = self.find_class_like(owner) else {
            return false;
        };
        if let Some(implementation) = self.find_method(child, method) {
            return method_depends_on_parameters(implementation);
        }
        self.ancestor_bindings(child)
            .into_iter()
            .any(|(ancestor, _)| {
                self.find_method(ancestor, method)
                    .is_some_and(method_depends_on_parameters)
            })
    }

    /// Resolve the method signature against one concrete reified object
    /// binding. The returned contract is self-contained and can safely live in
    /// an executor sidecar across nested calls and metadata-table borrows.
    pub fn reified_instance_method_contract(
        &self,
        binding: ReifiedBinding,
        method: &str,
    ) -> Option<ReifiedMethodContract> {
        let child = self.declaration(binding)?;
        let site = self.use_site(binding.use_site)?;
        let effective = effective_inheritance_arguments(child, &site.arguments);
        if effective.len() != child.parameters.len() {
            return None;
        }

        if let Some(implementation) = self.find_method(child, method) {
            return self.substituted_method_contract(child, implementation, &effective);
        }
        for (ancestor, arguments) in self.ancestor_bindings_from(child, &effective) {
            let Some(prototype) = self.find_method(ancestor, method) else {
                continue;
            };
            return self.substituted_method_contract(ancestor, prototype, &arguments);
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

    fn substituted_method_contract(
        &self,
        declaration: &GenericDeclaration,
        method: &GenericMethodMetadata,
        arguments: &[GenericType],
    ) -> Option<ReifiedMethodContract> {
        if !method_depends_on_parameters(method) {
            return None;
        }
        Some(ReifiedMethodContract {
            owner: self.symbol(declaration.owner).unwrap_or("?").into(),
            method: self.symbol(method.name).unwrap_or("?").into(),
            value_parameters: method
                .value_parameters
                .iter()
                .map(|value| {
                    value
                        .as_ref()
                        .map(|value| substitute_generic_parameters(value, arguments))
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            return_type: method
                .return_type
                .as_ref()
                .map(|value| substitute_generic_parameters(value, arguments)),
            is_variadic: method.is_variadic,
        })
    }
}

fn method_depends_on_parameters(method: &GenericMethodMetadata) -> bool {
    method
        .value_parameters
        .iter()
        .flatten()
        .any(type_contains_parameter)
        || method
            .return_type
            .as_ref()
            .is_some_and(type_contains_parameter)
}

fn type_contains_parameter(value: &GenericType) -> bool {
    match value {
        GenericType::Parameter(_) => true,
        GenericType::Named { arguments, .. } | GenericType::Union(arguments) => {
            arguments.iter().any(type_contains_parameter)
        }
        GenericType::Nullable(inner) => type_contains_parameter(inner),
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
