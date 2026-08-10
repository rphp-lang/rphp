use super::ExecutorGlobals;
#[cfg(feature = "php-generics-reified")]
use super::ReifiedPropertyContractBinding;
#[cfg(feature = "php-generics-reified")]
use crate::generics::ReifiedBinding;

impl ExecutorGlobals {
    #[cfg(feature = "php-generics-reified")]
    fn value_matches_reified_property(
        &self,
        value: &crate::value::Value,
        name: &str,
        binding: ReifiedBinding,
    ) -> Option<bool> {
        {
            let cache = self.reified_property_contract_cache.borrow();
            if let Some(cached) = cache.as_ref() {
                if cached.binding == binding && cached.property.as_ref() == name {
                    return Some(self.generic_metadata.value_matches_resolved_type(
                        value,
                        &cached.expected,
                        |actual, bound| self.class_is_a(actual, bound),
                    ));
                }
            }
        }
        let expected = self
            .generic_metadata
            .reified_instance_property_type(binding, name)?;
        let matches =
            self.generic_metadata
                .value_matches_resolved_type(value, &expected, |actual, bound| {
                    self.class_is_a(actual, bound)
                });
        self.reified_property_contract_cache
            .replace(Some(ReifiedPropertyContractBinding {
                binding,
                property: name.into(),
                expected,
            }));
        Some(matches)
    }

    /// Returns the declaration index when this is a generic property and its
    /// erased or reified contract was checked, or `None` for ordinary
    /// properties. The caller stores that index in the existing property IC.
    pub(crate) fn check_generic_property_value(
        &self,
        object: &crate::value::Value,
        owner: &str,
        name: &str,
        value: &crate::value::Value,
    ) -> Result<Option<u32>, String> {
        #[cfg(not(feature = "php-generics-reified"))]
        let _ = object;
        #[cfg(feature = "php-generics-reified")]
        if let Some(binding) = self.reified_object_binding(object) {
            if let Some(matches) = self.value_matches_reified_property(value, name, binding) {
                if matches {
                    return Ok(Some(binding.declaration));
                }
                let declaration_owner = self
                    .generic_metadata
                    .declaration(binding)
                    .and_then(|declaration| self.generic_metadata.symbol(declaration.owner))
                    .unwrap_or("?");
                return Err(format!(
                    "Value does not match reified property {}::${}",
                    declaration_owner, name
                ));
            }
        }
        let Some(declaration) = self
            .generic_metadata
            .find_index(crate::generics::GenericDeclarationKind::Class, owner)
        else {
            return Ok(None);
        };
        let Some(matches) = self
            .generic_metadata
            .value_matches_erased_property_declaration(
                declaration,
                name,
                value,
                |actual, bound| self.class_is_a(actual, bound),
            )
        else {
            return Ok(None);
        };
        if matches {
            return Ok(Some(declaration));
        }
        Err(format!(
            "Value does not match bound-erased property {}::${}",
            owner, name
        ))
    }

    pub(crate) fn check_cached_generic_property_value(
        &self,
        object: &crate::value::Value,
        name: &str,
        value: &crate::value::Value,
        declaration: u32,
    ) -> Result<(), String> {
        #[cfg(not(feature = "php-generics-reified"))]
        let _ = object;
        #[cfg(feature = "php-generics-reified")]
        if let Some(binding) = self.reified_object_binding(object) {
            if binding.declaration == declaration {
                let matches = self
                    .value_matches_reified_property(value, name, binding)
                    .ok_or_else(|| "Invalid cached reified property metadata".to_string())?;
                if matches {
                    return Ok(());
                }
                let owner = self
                    .generic_metadata
                    .declaration(binding)
                    .and_then(|declaration| self.generic_metadata.symbol(declaration.owner))
                    .unwrap_or("?");
                return Err(format!(
                    "Value does not match reified property {}::${}",
                    owner, name
                ));
            }
        }
        let matches = self
            .generic_metadata
            .value_matches_erased_property_declaration(declaration, name, value, |actual, bound| {
                self.class_is_a(actual, bound)
            })
            .ok_or_else(|| "Invalid cached bound-erased property metadata".to_string())?;
        if matches {
            return Ok(());
        }
        let owner = self
            .generic_metadata
            .declarations()
            .get(declaration as usize)
            .and_then(|declaration| self.generic_metadata.symbol(declaration.owner))
            .unwrap_or("?");
        Err(format!(
            "Value does not match bound-erased property {}::${}",
            owner, name
        ))
    }
}
