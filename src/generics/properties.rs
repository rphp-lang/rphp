use super::link::substitute_generic_parameters;
use super::lsp::effective_inheritance_arguments;
use super::{
    GenericDeclaration, GenericMetadata, GenericPropertyMetadata, GenericType, ReifiedBinding,
};
use crate::value::Value;

impl GenericMetadata {
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
            return Some(substitute_generic_parameters(
                &property.value_type,
                effective,
            ));
        }
        let inherited = self
            .ancestor_bindings_from(child, effective)
            .into_iter()
            .filter_map(|(ancestor, arguments)| {
                let property = self.find_instance_property(ancestor, name)?;
                Some(substitute_generic_parameters(
                    &property.value_type,
                    &arguments,
                ))
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
}
