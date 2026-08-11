use super::{ExecutorGlobals, ReifiedNestedArgumentsBinding};
use crate::generics::{GenericDeclaration, GenericType, GenericUseSite};
use crate::value::Value;

impl ExecutorGlobals {
    pub(crate) fn reified_object_arguments_match_binding(
        &self,
        value: &Value,
        expected_name: &str,
        expected_arguments: &[GenericType],
        declaration: &GenericDeclaration,
        site: &GenericUseSite,
        scope: &str,
    ) -> bool {
        let Some(expected_owner) = self.generic_type_name_in_scope(expected_name, scope) else {
            return false;
        };
        if value.value_type() != crate::value::ValueType::Object {
            return false;
        }
        let identity = unsafe { value.object_identity_unchecked() };
        {
            let mut cache = self.reified_nested_arguments_cache.borrow_mut();
            if let Some(cached) = cache.as_mut()
                && cached.identity == identity
                && cached.object.strong_count() != 0
                && cached.owner_name_identity == expected_owner.as_ptr() as usize
                && cached.owner_name_len == expected_owner.len()
            {
                let expected_pointer = expected_arguments.as_ptr() as usize;
                let site_pointer = site as *const GenericUseSite as usize;
                if cached.binding_expected_arguments == expected_pointer
                    && cached.binding_expected_len == expected_arguments.len()
                    && cached.binding_site == site_pointer
                {
                    return cached.binding_matches;
                }
                let matches = self.generic_metadata.reified_arguments_match_binding(
                    &cached.arguments,
                    expected_arguments,
                    declaration,
                    site,
                );
                cached.binding_expected_arguments = expected_pointer;
                cached.binding_expected_len = expected_arguments.len();
                cached.binding_site = site_pointer;
                cached.binding_matches = matches;
                return matches;
            }
        }
        let Some(arguments) = self.reified_object_arguments_for_owner(value, expected_owner) else {
            return false;
        };
        let matches = self.generic_metadata.reified_arguments_match_binding(
            &arguments,
            expected_arguments,
            declaration,
            site,
        );
        self.reified_nested_arguments_cache
            .replace(Some(ReifiedNestedArgumentsBinding {
                identity,
                object: self.reified_object_weak(value),
                owner_name_identity: expected_owner.as_ptr() as usize,
                owner_name_len: expected_owner.len(),
                arguments,
                binding_expected_arguments: expected_arguments.as_ptr() as usize,
                binding_expected_len: expected_arguments.len(),
                binding_site: site as *const GenericUseSite as usize,
                binding_matches: matches,
            }));
        matches
    }

    pub(crate) fn reified_object_arguments_match_resolved(
        &self,
        value: &Value,
        expected_name: &str,
        expected_arguments: &[GenericType],
        scope: &str,
    ) -> bool {
        let Some(expected_owner) = self.generic_type_name_in_scope(expected_name, scope) else {
            return false;
        };
        if value.value_type() != crate::value::ValueType::Object {
            return false;
        }
        let identity = unsafe { value.object_identity_unchecked() };
        {
            let cache = self.reified_nested_arguments_cache.borrow();
            if let Some(cached) = cache.as_ref()
                && cached.identity == identity
                && cached.object.strong_count() != 0
                && cached.owner_name_identity == expected_owner.as_ptr() as usize
                && cached.owner_name_len == expected_owner.len()
            {
                return self
                    .generic_metadata
                    .reified_arguments_match_resolved(&cached.arguments, expected_arguments);
            }
        }
        let Some(arguments) = self.reified_object_arguments_for_owner(value, expected_owner) else {
            return false;
        };
        let matches = self
            .generic_metadata
            .reified_arguments_match_resolved(&arguments, expected_arguments);
        self.reified_nested_arguments_cache
            .replace(Some(ReifiedNestedArgumentsBinding {
                identity,
                object: self.reified_object_weak(value),
                owner_name_identity: expected_owner.as_ptr() as usize,
                owner_name_len: expected_owner.len(),
                arguments,
                binding_expected_arguments: 0,
                binding_expected_len: 0,
                binding_site: 0,
                binding_matches: false,
            }));
        matches
    }

    /// Includes rebuild the boxed metadata arrays. Drop the only cache key
    /// that intentionally uses an immutable metadata-slice address before an
    /// old allocation can be recycled.
    pub(crate) fn clear_reified_nested_arguments_cache(&self) {
        self.reified_nested_arguments_cache.replace(None);
    }

    fn reified_object_arguments_for_owner(
        &self,
        value: &Value,
        expected_owner: &str,
    ) -> Option<Box<[GenericType]>> {
        let declaration_index = self
            .generic_metadata
            .find_class_like_index(expected_owner)?;
        let owner = self
            .generic_metadata
            .declarations()
            .get(declaration_index as usize)?
            .owner;
        if let Some(binding) = self.reified_object_binding(value) {
            return self
                .generic_metadata
                .reified_arguments_for_owner(binding, owner);
        }
        let concrete = unsafe { value.object_class_name_unchecked() };
        self.generic_metadata
            .concrete_arguments_for_owner(concrete, owner)
    }

    fn reified_object_weak(
        &self,
        value: &Value,
    ) -> std::rc::Weak<std::cell::RefCell<crate::value::PhpObject>> {
        let object = value
            .as_object_rc()
            .expect("nested reified matcher already proved an object");
        std::rc::Rc::downgrade(&object)
    }
}
