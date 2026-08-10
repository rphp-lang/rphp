use super::link::substitute_generic_parameters;
use super::{
    GenericDeclaration, GenericMetadata, GenericMethodMetadata, GenericSymbol, GenericType,
    GenericVariance,
};

impl GenericMetadata {
    /// Validate direct generic ancestor prototypes after substituting the
    /// binding supplied by the child. This is link-only metadata work; the
    /// executable body and ordinary method dispatch remain untouched.
    pub fn validate_parametric_lsp<F>(&self, owner: &str, class_is_a: F) -> Result<(), String>
    where
        F: Fn(&str, &str) -> bool,
    {
        let Some(child) = self.find_class_like(owner) else {
            return Ok(());
        };
        for (ancestor, effective) in self.ancestor_bindings(child) {
            for prototype in &ancestor.methods {
                let method_name = self.symbol(prototype.name).unwrap_or("?");
                if method_name.eq_ignore_ascii_case("__construct") {
                    continue;
                }
                let Some(implementation) = child.methods.iter().find(|method| {
                    self.symbol(method.name)
                        .is_some_and(|name| name.eq_ignore_ascii_case(method_name))
                }) else {
                    continue;
                };
                self.validate_method_lsp(
                    child,
                    implementation,
                    ancestor,
                    prototype,
                    &effective,
                    &class_is_a,
                )?;
            }
        }
        Ok(())
    }

    fn ancestor_bindings<'a>(
        &'a self,
        child: &'a GenericDeclaration,
    ) -> Vec<(&'a GenericDeclaration, Vec<GenericType>)> {
        let identity = (0..child.parameters.len())
            .map(|index| GenericType::Parameter(index as u8))
            .collect::<Vec<_>>();
        self.ancestor_bindings_from(child, &identity)
    }

    fn ancestor_bindings_from<'a>(
        &'a self,
        child: &'a GenericDeclaration,
        binding: &[GenericType],
    ) -> Vec<(&'a GenericDeclaration, Vec<GenericType>)> {
        let mut result = Vec::new();
        let mut seen = Vec::new();
        self.collect_ancestor_bindings(child, binding, &mut seen, &mut result);
        result
    }

    fn collect_ancestor_bindings<'a>(
        &'a self,
        current: &'a GenericDeclaration,
        current_binding: &[GenericType],
        seen: &mut Vec<(GenericSymbol, Vec<GenericType>)>,
        result: &mut Vec<(&'a GenericDeclaration, Vec<GenericType>)>,
    ) {
        for inheritance in self
            .inheritances
            .iter()
            .filter(|inheritance| inheritance.owner == current.owner)
        {
            let Some(ancestor_name) = self.symbol(inheritance.ancestor) else {
                continue;
            };
            let Some(ancestor) = self.find_class_like(ancestor_name) else {
                continue;
            };
            let direct = effective_inheritance_arguments(ancestor, &inheritance.arguments);
            let binding = direct
                .iter()
                .map(|argument| substitute_generic_parameters(argument, current_binding))
                .collect::<Vec<_>>();
            if seen
                .iter()
                .any(|(owner, arguments)| *owner == ancestor.owner && *arguments == binding)
            {
                continue;
            }
            seen.push((ancestor.owner, binding.clone()));
            result.push((ancestor, binding.clone()));
            self.collect_ancestor_bindings(ancestor, &binding, seen, result);
        }
    }

    fn validate_method_lsp<F>(
        &self,
        child: &GenericDeclaration,
        implementation: &GenericMethodMetadata,
        ancestor: &GenericDeclaration,
        prototype: &GenericMethodMetadata,
        arguments: &[GenericType],
        class_is_a: &F,
    ) -> Result<(), String>
    where
        F: Fn(&str, &str) -> bool,
    {
        let child_name = self.symbol(child.owner).unwrap_or("?");
        let ancestor_name = self.symbol(ancestor.owner).unwrap_or("?");
        let method_name = self.symbol(prototype.name).unwrap_or("?");
        if implementation.is_static != prototype.is_static {
            return Err(format!(
                "Parametric LSP violation: {}::{}() staticness is incompatible with {}::{}()",
                child_name, method_name, ancestor_name, method_name
            ));
        }

        if prototype.is_variadic && !implementation.is_variadic {
            return Err(format!(
                "Parametric LSP violation: {}::{}() must remain variadic as declared by substituted {}::{}()",
                child_name, method_name, ancestor_name, method_name
            ));
        }
        if implementation.required_parameters > prototype.required_parameters {
            return Err(format!(
                "Parametric LSP violation: {}::{}() requires {} parameters, substituted {}::{}() requires only {}",
                child_name,
                method_name,
                implementation.required_parameters,
                ancestor_name,
                method_name,
                prototype.required_parameters
            ));
        }
        if !implementation.is_variadic
            && implementation.value_parameters.len() < prototype.value_parameters.len()
        {
            return Err(format!(
                "Parametric LSP violation: {}::{}() accepts {} parameters, substituted {}::{}() accepts {}",
                child_name,
                method_name,
                implementation.value_parameters.len(),
                ancestor_name,
                method_name,
                prototype.value_parameters.len()
            ));
        }

        for (index, prototype_parameter) in prototype.value_parameters.iter().enumerate() {
            let substituted = prototype_parameter
                .as_ref()
                .map(|value| substitute_generic_parameters(value, arguments));
            let implementation_parameter = implementation_parameter(implementation, index);
            if !self.parameter_type_is_compatible(
                implementation_parameter,
                substituted.as_ref(),
                class_is_a,
            ) {
                return Err(format!(
                    "Parametric LSP violation: parameter {} of {}::{}() is incompatible with substituted {}::{}()",
                    index + 1,
                    child_name,
                    method_name,
                    ancestor_name,
                    method_name
                ));
            }
        }

        // A variadic prototype describes every argument from its final slot
        // onward. Optional fixed parameters added by the implementation and
        // its own variadic tail must therefore accept that same contract.
        if prototype.is_variadic {
            let variadic_index = prototype.value_parameters.len().saturating_sub(1);
            let substituted = prototype
                .value_parameters
                .get(variadic_index)
                .and_then(Option::as_ref)
                .map(|value| substitute_generic_parameters(value, arguments));
            for index in (variadic_index + 1)..implementation.value_parameters.len() {
                if !self.parameter_type_is_compatible(
                    implementation_parameter(implementation, index),
                    substituted.as_ref(),
                    class_is_a,
                ) {
                    return Err(format!(
                        "Parametric LSP violation: parameter {} of {}::{}() is incompatible with the variadic tail of substituted {}::{}()",
                        index + 1,
                        child_name,
                        method_name,
                        ancestor_name,
                        method_name
                    ));
                }
            }
        }

        let substituted_return = prototype
            .return_type
            .as_ref()
            .map(|value| substitute_generic_parameters(value, arguments));
        if !self.return_type_is_compatible(
            implementation.return_type.as_ref(),
            substituted_return.as_ref(),
            class_is_a,
        ) {
            return Err(format!(
                "Parametric LSP violation: return type of {}::{}() is incompatible with substituted {}::{}()",
                child_name, method_name, ancestor_name, method_name
            ));
        }
        Ok(())
    }

    fn parameter_type_is_compatible<F>(
        &self,
        implementation: Option<&GenericType>,
        prototype: Option<&GenericType>,
        class_is_a: &F,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
    {
        match (implementation, prototype) {
            (None | Some(GenericType::Mixed), _) => true,
            (Some(_), None) => false,
            (Some(implementation), Some(prototype)) => {
                self.generic_type_is_subtype(prototype, implementation, class_is_a)
            }
        }
    }

    fn return_type_is_compatible<F>(
        &self,
        implementation: Option<&GenericType>,
        prototype: Option<&GenericType>,
        class_is_a: &F,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
    {
        match (implementation, prototype) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(GenericType::Void), Some(GenericType::Mixed)) => false,
            (Some(implementation), Some(prototype)) => {
                self.generic_type_is_subtype(implementation, prototype, class_is_a)
            }
        }
    }

    fn generic_type_is_subtype<F>(
        &self,
        subtype: &GenericType,
        supertype: &GenericType,
        class_is_a: &F,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
    {
        if subtype == supertype
            || matches!(subtype, GenericType::Never)
            || matches!(supertype, GenericType::Mixed)
        {
            return true;
        }
        match (subtype, supertype) {
            (GenericType::Union(parts), supertype) => parts
                .iter()
                .all(|part| self.generic_type_is_subtype(part, supertype, class_is_a)),
            (subtype, GenericType::Union(parts)) => parts
                .iter()
                .any(|part| self.generic_type_is_subtype(subtype, part, class_is_a)),
            (GenericType::Null, GenericType::Nullable(_)) => true,
            (GenericType::Nullable(subtype), GenericType::Nullable(supertype)) => {
                self.generic_type_is_subtype(subtype, supertype, class_is_a)
            }
            (subtype, GenericType::Nullable(supertype)) => {
                self.generic_type_is_subtype(subtype, supertype, class_is_a)
            }
            (
                GenericType::Named {
                    name: subtype_name,
                    arguments: subtype_arguments,
                },
                GenericType::Named {
                    name: supertype_name,
                    arguments: supertype_arguments,
                },
            ) => {
                let Some(subtype_name_text) = self.symbol(*subtype_name) else {
                    return false;
                };
                let Some(supertype_name_text) = self.symbol(*supertype_name) else {
                    return false;
                };
                if !subtype_name_text.eq_ignore_ascii_case(supertype_name_text) {
                    if supertype_name_text.eq_ignore_ascii_case("object") {
                        return true;
                    }
                    if !supertype_arguments.is_empty() {
                        let Some(subtype_declaration) = self.find_class_like(subtype_name_text)
                        else {
                            return false;
                        };
                        let subtype_binding =
                            effective_inheritance_arguments(subtype_declaration, subtype_arguments);
                        if subtype_binding.len() != subtype_declaration.parameters.len() {
                            return false;
                        }
                        return self
                            .ancestor_bindings_from(subtype_declaration, &subtype_binding)
                            .into_iter()
                            .filter(|(ancestor, _)| {
                                self.symbol(ancestor.owner).is_some_and(|name| {
                                    name.eq_ignore_ascii_case(supertype_name_text)
                                })
                            })
                            .any(|(_, binding)| {
                                self.generic_type_is_subtype(
                                    &GenericType::Named {
                                        name: *supertype_name,
                                        arguments: binding.into_boxed_slice(),
                                    },
                                    supertype,
                                    class_is_a,
                                )
                            });
                    }
                    return class_is_a(subtype_name_text, supertype_name_text);
                }
                let target = self.find_class_like(subtype_name_text);
                subtype_arguments
                    .iter()
                    .zip(supertype_arguments)
                    .enumerate()
                    .all(|(index, (subtype_argument, supertype_argument))| {
                        match target
                            .and_then(|target| target.parameters.get(index))
                            .map(|parameter| parameter.variance)
                            .unwrap_or(GenericVariance::Invariant)
                        {
                            GenericVariance::Covariant => self.generic_type_is_subtype(
                                subtype_argument,
                                supertype_argument,
                                class_is_a,
                            ),
                            GenericVariance::Contravariant => self.generic_type_is_subtype(
                                supertype_argument,
                                subtype_argument,
                                class_is_a,
                            ),
                            GenericVariance::Invariant => subtype_argument == supertype_argument,
                        }
                    })
                    && subtype_arguments.len() == supertype_arguments.len()
            }
            _ => false,
        }
    }
}

fn implementation_parameter(method: &GenericMethodMetadata, index: usize) -> Option<&GenericType> {
    method
        .value_parameters
        .get(index)
        .or_else(|| {
            method
                .is_variadic
                .then(|| method.value_parameters.last())
                .flatten()
        })
        .and_then(Option::as_ref)
}

fn effective_inheritance_arguments(
    ancestor: &GenericDeclaration,
    supplied: &[GenericType],
) -> Vec<GenericType> {
    let mut effective = supplied.to_vec();
    for parameter in ancestor.parameters.iter().skip(effective.len()) {
        let Some(default) = parameter.default.as_ref() else {
            break;
        };
        effective.push(substitute_generic_parameters(default, &effective));
    }
    effective
}
