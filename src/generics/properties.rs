use super::link::substitute_generic_parameters;
use super::lsp::effective_inheritance_arguments;
use super::{
    GenericDeclaration, GenericDeclarationKind, GenericMetadata, GenericPropertyMetadata,
    GenericType, ReifiedBinding,
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
        if let Some(property) = self.find_instance_property(child, name) {
            return Some(substitute_generic_parameters(
                &property.value_type,
                &effective,
            ));
        }
        self.resolved_inherited_property_type(child, &effective, name)
    }

    pub fn value_matches_erased_property<F>(
        &self,
        kind: GenericDeclarationKind,
        owner: &str,
        name: &str,
        value: &Value,
        class_is_a: F,
    ) -> Option<bool>
    where
        F: Fn(&str, &str) -> bool,
    {
        let declaration = self.find_index(kind, owner)?;
        self.value_matches_erased_property_declaration(declaration, name, value, class_is_a)
    }

    pub fn value_matches_erased_property_declaration<F>(
        &self,
        declaration: u32,
        name: &str,
        value: &Value,
        class_is_a: F,
    ) -> Option<bool>
    where
        F: Fn(&str, &str) -> bool,
    {
        let (declaration, property) = self.instance_property_declaration(declaration, name)?;
        let value = if value.is_reference() {
            unsafe { &*value.as_ref_ptr() }
        } else {
            value
        };
        Some(self.value_matches_erased_type(
            value,
            &property.value_type,
            declaration,
            &class_is_a,
            0,
        ))
    }

    pub fn property_erases_to_mixed(&self, declaration: u32, name: &str) -> bool {
        self.instance_property_declaration(declaration, name)
            .is_some_and(|(declaration, property)| {
                Self::type_erases_to_mixed(&property.value_type, declaration, 0)
            })
    }

    fn resolved_inherited_property_type(
        &self,
        child: &GenericDeclaration,
        effective: &[GenericType],
        name: &str,
    ) -> Option<GenericType> {
        for (ancestor, arguments) in self.ancestor_bindings_from(child, effective) {
            let Some(property) = self.find_instance_property(ancestor, name) else {
                continue;
            };
            return Some(substitute_generic_parameters(
                &property.value_type,
                &arguments,
            ));
        }
        None
    }

    fn instance_property_declaration(
        &self,
        declaration: u32,
        name: &str,
    ) -> Option<(&GenericDeclaration, &GenericPropertyMetadata)> {
        let child = self.declarations.get(declaration as usize)?;
        if let Some(property) = self.find_instance_property(child, name) {
            return Some((child, property));
        }
        for (ancestor, _) in self.ancestor_bindings(child) {
            if let Some(property) = self.find_instance_property(ancestor, name) {
                return Some((ancestor, property));
            }
        }
        None
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
