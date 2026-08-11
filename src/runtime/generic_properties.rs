use super::ExecutorGlobals;
use super::GenericPropertyContractBinding;
#[cfg(feature = "php-generics-reified")]
use crate::generics::ReifiedBinding;

impl ExecutorGlobals {
    #[cfg(feature = "php-generics-reified")]
    fn value_matches_reified_property(
        &self,
        value: &crate::value::Value,
        receiver_scope: &str,
        name: &str,
        binding: ReifiedBinding,
    ) -> Option<bool> {
        {
            let cache = self.generic_property_contract_cache.borrow();
            if let Some(cached) = cache.as_ref() {
                if cached.declaration == binding.declaration
                    && cached.use_site == Some(binding.use_site)
                    && cached.property.as_ref() == name
                {
                    return Some(self.generic_metadata.value_matches_resolved_type_reified(
                        value,
                        &cached.expected,
                        |actual, bound| {
                            self.class_is_a_in_generic_scope(actual, bound, &cached.scope)
                        },
                        |value, expected, arguments| {
                            self.reified_object_arguments_match_resolved(
                                value,
                                expected,
                                arguments,
                                &cached.scope,
                            )
                        },
                    ));
                }
            }
        }
        let expected = self
            .generic_metadata
            .reified_instance_property_type(binding, name)?;
        let scope: Box<str> = self.generic_property_scope(receiver_scope, name).into();
        let matches = self.generic_metadata.value_matches_resolved_type_reified(
            value,
            &expected,
            |actual, bound| self.class_is_a_in_generic_scope(actual, bound, &scope),
            |value, expected, arguments| {
                self.reified_object_arguments_match_resolved(value, expected, arguments, &scope)
            },
        );
        self.generic_property_contract_cache
            .replace(Some(GenericPropertyContractBinding {
                declaration: binding.declaration,
                use_site: Some(binding.use_site),
                property: name.into(),
                scope,
                expected,
            }));
        Some(matches)
    }

    fn value_matches_linked_erased_property(
        &self,
        value: &crate::value::Value,
        receiver_scope: &str,
        name: &str,
        declaration: u32,
    ) -> Option<bool> {
        {
            let cache = self.generic_property_contract_cache.borrow();
            if let Some(cached) = cache.as_ref() {
                if cached.declaration == declaration
                    && cached.use_site.is_none()
                    && cached.property.as_ref() == name
                {
                    return self
                        .generic_metadata
                        .value_matches_erased_instance_property_type(
                            value,
                            &cached.expected,
                            declaration,
                            |actual, bound| {
                                self.class_is_a_in_generic_scope(actual, bound, &cached.scope)
                            },
                        );
                }
            }
        }
        let expected = self
            .generic_metadata
            .linked_instance_property_type(declaration, name)?;
        let scope: Box<str> = self.generic_property_scope(receiver_scope, name).into();
        let matches = self
            .generic_metadata
            .value_matches_erased_instance_property_type(
                value,
                &expected,
                declaration,
                |actual, bound| self.class_is_a_in_generic_scope(actual, bound, &scope),
            )?;
        self.generic_property_contract_cache
            .replace(Some(GenericPropertyContractBinding {
                declaration,
                use_site: None,
                property: name.into(),
                scope,
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
        let declaration = self.generic_metadata.find_class_like_index(owner);
        #[cfg(feature = "php-generics-reified")]
        if declaration.is_some_and(|declaration| {
            self.generic_metadata
                .declarations()
                .get(declaration as usize)
                .is_some_and(|declaration| !declaration.parameters.is_empty())
        }) {
            if let Some(binding) = self.reified_object_binding(object) {
                if let Some(matches) =
                    self.value_matches_reified_property(value, owner, name, binding)
                {
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
        }
        let Some(declaration) = declaration else {
            return Ok(None);
        };
        let Some(matches) =
            self.value_matches_linked_erased_property(value, owner, name, declaration)
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
        let receiver_scope = if object.value_type() == crate::value::ValueType::Object {
            unsafe { object.object_class_name_unchecked() }
        } else {
            "?"
        };
        #[cfg(feature = "php-generics-reified")]
        if self
            .generic_metadata
            .declarations()
            .get(declaration as usize)
            .is_some_and(|declaration| !declaration.parameters.is_empty())
        {
            if let Some(binding) = self.reified_object_binding(object) {
                if binding.declaration == declaration {
                    let owner = self
                        .generic_metadata
                        .declaration(binding)
                        .and_then(|declaration| self.generic_metadata.symbol(declaration.owner))
                        .unwrap_or("?");
                    let matches = self
                        .value_matches_reified_property(value, receiver_scope, name, binding)
                        .ok_or_else(|| "Invalid cached reified property metadata".to_string())?;
                    if matches {
                        return Ok(());
                    }
                    return Err(format!(
                        "Value does not match reified property {}::${}",
                        owner, name
                    ));
                }
            }
        }
        let owner = self
            .generic_metadata
            .declarations()
            .get(declaration as usize)
            .and_then(|declaration| self.generic_metadata.symbol(declaration.owner))
            .unwrap_or("?");
        let matches = self
            .value_matches_linked_erased_property(value, receiver_scope, name, declaration)
            .ok_or_else(|| "Invalid cached bound-erased property metadata".to_string())?;
        if matches {
            return Ok(());
        }
        Err(format!(
            "Value does not match bound-erased property {}::${}",
            owner, name
        ))
    }
}
