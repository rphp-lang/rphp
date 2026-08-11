use super::ExecutorGlobals;
use crate::generics::{GenericMethodContract, GenericRuntimeMode, GenericType};
use crate::value::Value;

impl ExecutorGlobals {
    pub(crate) fn value_matches_generic_method_contract(
        &self,
        value: &Value,
        expected: &GenericType,
        contract: &GenericMethodContract,
    ) -> bool {
        match contract.runtime_mode {
            GenericRuntimeMode::BoundErased => self.generic_metadata.value_matches_resolved_type(
                value,
                expected,
                |actual, bound| {
                    self.class_is_a_in_generic_scopes(
                        actual,
                        bound,
                        &contract.scope,
                        Some(&contract.called_scope),
                    )
                },
            ),
            GenericRuntimeMode::Reified => {
                #[cfg(feature = "php-generics-reified")]
                {
                    self.generic_metadata.value_matches_resolved_type_reified(
                        value,
                        expected,
                        |actual, bound| {
                            self.class_is_a_in_generic_scopes(
                                actual,
                                bound,
                                &contract.scope,
                                Some(&contract.called_scope),
                            )
                        },
                        |value, name, arguments| {
                            self.reified_object_arguments_match_resolved(
                                value,
                                name,
                                arguments,
                                &contract.scope,
                                Some(&contract.called_scope),
                            )
                        },
                    )
                }
                #[cfg(not(feature = "php-generics-reified"))]
                {
                    false
                }
            }
        }
    }
}
