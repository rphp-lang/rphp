use super::link::substitute_generic_parameters;
use super::lsp::effective_inheritance_arguments;
use super::{
    GenericDeclaration, GenericMetadata, GenericPropertyMetadata, GenericType, ReifiedBinding,
};
use crate::value::Value;

impl GenericMetadata {
    fn type_contains_runtime_generic_contract(expected: &GenericType) -> bool {
        match expected {
            GenericType::Parameter(_) => true,
            GenericType::Named { arguments, .. } => {
                !arguments.is_empty()
                    || arguments
                        .iter()
                        .any(Self::type_contains_runtime_generic_contract)
            }
            GenericType::Nullable(inner) => Self::type_contains_runtime_generic_contract(inner),
            GenericType::Union(parts) | GenericType::Intersection(parts) => parts
                .iter()
                .any(Self::type_contains_runtime_generic_contract),
            _ => false,
        }
    }

    /// Whether an instance property originated from a parameterized contract,
    /// before inheritance substitution potentially turns it into an ordinary
    /// scalar. This distinguishes `T` forwarded as `int` from a plain `int`
    /// declaration: only the former needs the generic runtime/error boundary.
    pub fn instance_property_requires_generic_guard(&self, declaration: u32, name: &str) -> bool {
        let Some(child) = self.declarations.get(declaration as usize) else {
            return false;
        };
        if let Some(property) = self.find_instance_property(child, name) {
            return Self::type_contains_runtime_generic_contract(&property.value_type);
        }
        self.ancestor_bindings_scoped_from(child, &[])
            .into_iter()
            .filter_map(|(ancestor, _, _)| self.find_instance_property(ancestor, name))
            .any(|property| Self::type_contains_runtime_generic_contract(&property.value_type))
    }

    /// Produce one fully substituted instance-property type. Executors may
    /// cache the owned result because it contains no metadata-table borrows.
    pub fn reified_instance_property_type(
        &self,
        binding: ReifiedBinding,
        name: &str,
    ) -> Option<GenericType> {
        let child = self.declaration(binding)?;
        let site = self.use_site(binding.use_site)?;
        let effective = effective_inheritance_arguments(child, &site.arguments);
        if effective.len() != child.parameters.len() {
            return None;
        }
        self.resolved_instance_property_type(child, &effective, name)
    }

    /// Materialize the link-time view of an instance property relative to the
    /// concrete child declaration. Parameters that remain in the returned
    /// type belong to that child and therefore erase to the child's bounds.
    pub fn linked_instance_property_type(
        &self,
        declaration: u32,
        name: &str,
    ) -> Option<GenericType> {
        let child = self.declarations.get(declaration as usize)?;
        let identity = (0..child.parameters.len())
            .map(|index| GenericType::Parameter(index as u8))
            .collect::<Vec<_>>();
        self.resolved_instance_property_type(child, &identity, name)
    }

    /// Link one static property against the concrete called declaration.
    /// Class-level parameters are rejected in static contexts by variance
    /// validation, but concrete nested applications still need reified checks.
    pub fn linked_static_property_type(&self, declaration: u32, name: &str) -> Option<GenericType> {
        let child = self.declarations.get(declaration as usize)?;
        let identity = (0..child.parameters.len())
            .map(|index| GenericType::Parameter(index as u8))
            .collect::<Vec<_>>();
        self.resolved_static_property_type(child, &identity, name)
    }

    pub fn type_requires_reified_check(expected: &GenericType) -> bool {
        match expected {
            GenericType::Named { arguments, .. } => {
                !arguments.is_empty() || arguments.iter().any(Self::type_requires_reified_check)
            }
            GenericType::Parameter(_) => true,
            GenericType::Nullable(inner) => Self::type_requires_reified_check(inner),
            GenericType::Union(parts) | GenericType::Intersection(parts) => {
                parts.iter().any(Self::type_requires_reified_check)
            }
            _ => false,
        }
    }

    pub fn value_matches_erased_instance_property_type<F>(
        &self,
        value: &Value,
        expected: &GenericType,
        declaration: u32,
        class_is_a: F,
    ) -> Option<bool>
    where
        F: Fn(&str, &str) -> bool,
    {
        let declaration = self.declarations.get(declaration as usize)?;
        let value = if value.is_reference() {
            unsafe { &*value.as_ref_ptr() }
        } else {
            value
        };
        Some(self.value_matches_erased_type(value, expected, declaration, &class_is_a, 0))
    }

    pub fn property_erases_to_mixed(&self, declaration: u32, name: &str) -> bool {
        let Some(child) = self.declarations.get(declaration as usize) else {
            return false;
        };
        self.linked_instance_property_type(declaration, name)
            .is_some_and(|property| Self::type_erases_to_mixed(&property, child, 0))
    }

    fn resolved_instance_property_type(
        &self,
        child: &GenericDeclaration,
        effective: &[GenericType],
        name: &str,
    ) -> Option<GenericType> {
        if let Some(property) = self.find_instance_property(child, name) {
            let substituted = substitute_generic_parameters(&property.value_type, effective);
            return Some(
                self.resolve_lexical_class_pseudo_types(&substituted, child.owner)
                    .unwrap_or(GenericType::Never),
            );
        }
        let inherited = self
            .ancestor_bindings_scoped_from(child, effective)
            .into_iter()
            .filter_map(|(ancestor, arguments, scope)| {
                let property = self.find_instance_property(ancestor, name)?;
                let substituted = substitute_generic_parameters(&property.value_type, &arguments);
                Some(
                    self.resolve_lexical_class_pseudo_types(&substituted, scope)
                        .unwrap_or(GenericType::Never),
                )
            })
            .collect::<Vec<_>>();
        (!inherited.is_empty()).then(|| self.merge_generic_union(inherited))
    }

    fn resolved_static_property_type(
        &self,
        child: &GenericDeclaration,
        effective: &[GenericType],
        name: &str,
    ) -> Option<GenericType> {
        if let Some(property) = self.find_static_property(child, name) {
            let substituted = substitute_generic_parameters(&property.value_type, effective);
            return Some(
                self.resolve_lexical_class_pseudo_types(&substituted, child.owner)
                    .unwrap_or(GenericType::Never),
            );
        }
        let inherited = self
            .ancestor_bindings_scoped_from(child, effective)
            .into_iter()
            .filter_map(|(ancestor, arguments, scope)| {
                let property = self.find_static_property(ancestor, name)?;
                let substituted = substitute_generic_parameters(&property.value_type, &arguments);
                Some(
                    self.resolve_lexical_class_pseudo_types(&substituted, scope)
                        .unwrap_or(GenericType::Never),
                )
            })
            .collect::<Vec<_>>();
        (!inherited.is_empty()).then(|| self.merge_generic_union(inherited))
    }

    fn find_instance_property<'a>(
        &'a self,
        declaration: &'a GenericDeclaration,
        name: &str,
    ) -> Option<&'a GenericPropertyMetadata> {
        declaration.properties.iter().find(|property| {
            !property.is_static
                && self
                    .symbol(property.name)
                    .is_some_and(|candidate| candidate == name)
        })
    }

    fn find_static_property<'a>(
        &'a self,
        declaration: &'a GenericDeclaration,
        name: &str,
    ) -> Option<&'a GenericPropertyMetadata> {
        declaration.properties.iter().find(|property| {
            property.is_static
                && self
                    .symbol(property.name)
                    .is_some_and(|candidate| candidate == name)
        })
    }
}
