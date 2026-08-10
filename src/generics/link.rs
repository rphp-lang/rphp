use super::{GenericDeclaration, GenericDeclarationKind, GenericMetadata, GenericType};

impl GenericMetadata {
    /// Validate the direct generic bindings declared by one class-like. This
    /// runs when the class is linked, after metadata from its compilation unit
    /// has joined the executor-wide table.
    pub fn validate_inheritance<F>(&self, owner: &str, class_is_a: F) -> Result<(), String>
    where
        F: Fn(&str, &str) -> bool,
    {
        let owner_declaration = self.find_class_like(owner);
        for inheritance in self.inheritances.iter().filter(|inheritance| {
            self.symbol(inheritance.owner)
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(owner))
        }) {
            let ancestor = self.symbol(inheritance.ancestor).unwrap_or("?");
            let Some(ancestor_declaration) = self.find_class_like(ancestor) else {
                if inheritance.arguments.is_empty() {
                    continue;
                }
                return Err(format!(
                    "Cannot supply generic arguments to non-generic ancestor {}",
                    ancestor
                ));
            };

            let required = ancestor_declaration
                .parameters
                .iter()
                .take_while(|parameter| parameter.default.is_none())
                .count();
            let supplied = inheritance.arguments.len();
            if supplied < required || supplied > ancestor_declaration.parameters.len() {
                return Err(format!(
                    "Generic ancestor {} expects {} to {} type arguments, {} given",
                    ancestor,
                    required,
                    ancestor_declaration.parameters.len(),
                    supplied
                ));
            }

            let mut effective = inheritance.arguments.to_vec();
            for parameter in ancestor_declaration.parameters.iter().skip(effective.len()) {
                let default = parameter
                    .default
                    .as_ref()
                    .expect("optional generic ancestor parameter must have a default");
                effective.push(substitute_generic_parameters(default, &effective));
            }
            for (index, parameter) in ancestor_declaration.parameters.iter().enumerate() {
                let Some(bound) = parameter.bound.as_ref() else {
                    continue;
                };
                // Ancestor bounds use ancestor parameter indices. Substitute
                // those first, then erase any forwarded owner parameter to its
                // own bound (the RFC's bound-on-bound rule).
                let bound = substitute_generic_parameters(bound, &effective);
                let actual = erase_forwarded_parameters(
                    &effective[index],
                    owner_declaration,
                    owner_declaration.map_or(1, |declaration| declaration.parameters.len() + 1),
                );
                let bound = erase_forwarded_parameters(
                    &bound,
                    owner_declaration,
                    owner_declaration.map_or(1, |declaration| declaration.parameters.len() + 1),
                );
                if !self.type_satisfies(&actual, &bound, &[], &class_is_a) {
                    let parameter_name = self.symbol(parameter.name).unwrap_or("?");
                    return Err(format!(
                        "Type argument {} for generic ancestor {} does not satisfy bound of {}",
                        index + 1,
                        ancestor,
                        parameter_name
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn find_class_like(&self, owner: &str) -> Option<&GenericDeclaration> {
        self.declarations.iter().find(|declaration| {
            matches!(
                declaration.kind,
                GenericDeclarationKind::Class
                    | GenericDeclarationKind::Interface
                    | GenericDeclarationKind::Trait
            ) && self
                .symbol(declaration.owner)
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(owner))
        })
    }
}

fn substitute_generic_parameters(value: &GenericType, arguments: &[GenericType]) -> GenericType {
    match value {
        GenericType::Parameter(index) => arguments
            .get(*index as usize)
            .cloned()
            .unwrap_or(GenericType::Mixed),
        GenericType::Named {
            name,
            arguments: inner,
        } => GenericType::Named {
            name: *name,
            arguments: inner
                .iter()
                .map(|argument| substitute_generic_parameters(argument, arguments))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        },
        GenericType::Nullable(inner) => {
            GenericType::Nullable(Box::new(substitute_generic_parameters(inner, arguments)))
        }
        GenericType::Union(parts) => GenericType::Union(
            parts
                .iter()
                .map(|part| substitute_generic_parameters(part, arguments))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        concrete => concrete.clone(),
    }
}

fn erase_forwarded_parameters(
    value: &GenericType,
    owner: Option<&GenericDeclaration>,
    remaining: usize,
) -> GenericType {
    if remaining == 0 {
        return GenericType::Mixed;
    }
    match value {
        GenericType::Parameter(index) => owner
            .and_then(|declaration| declaration.parameters.get(*index as usize))
            .and_then(|parameter| parameter.bound.as_ref())
            .map(|bound| erase_forwarded_parameters(bound, owner, remaining - 1))
            .unwrap_or(GenericType::Mixed),
        GenericType::Named { name, arguments } => GenericType::Named {
            name: *name,
            arguments: arguments
                .iter()
                .map(|argument| erase_forwarded_parameters(argument, owner, remaining))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        },
        GenericType::Nullable(inner) => GenericType::Nullable(Box::new(
            erase_forwarded_parameters(inner, owner, remaining),
        )),
        GenericType::Union(parts) => GenericType::Union(
            parts
                .iter()
                .map(|part| erase_forwarded_parameters(part, owner, remaining))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        concrete => concrete.clone(),
    }
}
