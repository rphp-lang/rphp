use super::{
    GenericDeclaration, GenericDeclarationKind, GenericMetadata, GenericMethodMetadata,
    GenericType, method_parameter_index,
};

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

    pub fn find_class_like_index(&self, owner: &str) -> Option<u32> {
        self.declarations
            .iter()
            .position(|declaration| {
                matches!(
                    declaration.kind,
                    GenericDeclarationKind::Class
                        | GenericDeclarationKind::Interface
                        | GenericDeclarationKind::Trait
                ) && self
                    .symbol(declaration.owner)
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(owner))
            })
            .map(|index| index as u32)
    }

    pub(super) fn find_class_like(&self, owner: &str) -> Option<&GenericDeclaration> {
        self.find_class_like_index(owner)
            .and_then(|index| self.declarations.get(index as usize))
    }

    /// Build a deterministic least-upper-bound node for contravariant merge
    /// positions. This is cold link/runtime-sidecar work; flattening and
    /// sorting here keeps the resulting contract independent of graph order.
    pub(super) fn merge_generic_union<I>(&self, values: I) -> GenericType
    where
        I: IntoIterator<Item = GenericType>,
    {
        let mut parts = Vec::new();
        for value in values {
            match value {
                GenericType::Mixed => return GenericType::Mixed,
                GenericType::Never => {}
                GenericType::Union(nested) => parts.extend(nested),
                value => parts.push(value),
            }
        }
        self.finish_generic_merge(parts, false)
    }

    /// Build the matching greatest-lower-bound node for covariant merge
    /// positions. `mixed` is the identity and `never` is absorbing.
    pub(super) fn merge_generic_intersection<I>(&self, values: I) -> GenericType
    where
        I: IntoIterator<Item = GenericType>,
    {
        let mut parts = Vec::new();
        for value in values {
            match value {
                GenericType::Never => return GenericType::Never,
                GenericType::Mixed => {}
                GenericType::Intersection(nested) => parts.extend(nested),
                value => parts.push(value),
            }
        }
        self.finish_generic_merge(parts, true)
    }

    fn finish_generic_merge(&self, mut parts: Vec<GenericType>, intersection: bool) -> GenericType {
        let mut keyed = parts
            .drain(..)
            .map(|part| (self.generic_type_sort_key(&part), part))
            .collect::<Vec<_>>();
        keyed.sort_by(|left, right| left.0.cmp(&right.0));
        keyed.dedup_by(|left, right| left.0 == right.0);
        parts.extend(keyed.into_iter().map(|(_, part)| part));
        match parts.len() {
            0 if intersection => GenericType::Mixed,
            0 => GenericType::Never,
            1 => parts.pop().expect("one merged generic type"),
            _ if intersection => GenericType::Intersection(parts.into_boxed_slice()),
            _ => GenericType::Union(parts.into_boxed_slice()),
        }
    }

    fn generic_type_sort_key(&self, value: &GenericType) -> String {
        match value {
            GenericType::Int => "01:int".into(),
            GenericType::Float => "02:float".into(),
            GenericType::String => "03:string".into(),
            GenericType::Bool => "04:bool".into(),
            GenericType::Array => "05:array".into(),
            GenericType::Callable => "06:callable".into(),
            GenericType::Null => "07:null".into(),
            GenericType::Void => "08:void".into(),
            GenericType::Mixed => "09:mixed".into(),
            GenericType::Never => "10:never".into(),
            GenericType::Named { name, arguments } => format!(
                "11:{}<{}>",
                self.symbol(*name).unwrap_or("?").to_ascii_lowercase(),
                arguments
                    .iter()
                    .map(|argument| self.generic_type_sort_key(argument))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            GenericType::Parameter(index) => format!("12:{index:03}"),
            GenericType::Nullable(inner) => {
                format!("13:?{}", self.generic_type_sort_key(inner))
            }
            GenericType::Union(parts) => {
                let mut keys = parts
                    .iter()
                    .map(|part| self.generic_type_sort_key(part))
                    .collect::<Vec<_>>();
                keys.sort_unstable();
                format!("14:{}", keys.join("|"))
            }
            GenericType::Intersection(parts) => {
                let mut keys = parts
                    .iter()
                    .map(|part| self.generic_type_sort_key(part))
                    .collect::<Vec<_>>();
                keys.sort_unstable();
                format!("15:{}", keys.join("&"))
            }
        }
    }
}

pub(super) fn substitute_generic_parameters(
    value: &GenericType,
    arguments: &[GenericType],
) -> GenericType {
    match value {
        GenericType::Parameter(index) if method_parameter_index(*index).is_some() => value.clone(),
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
        GenericType::Intersection(parts) => GenericType::Intersection(
            parts
                .iter()
                .map(|part| substitute_generic_parameters(part, arguments))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        concrete => concrete.clone(),
    }
}

/// Erase both class-like and method-local parameters after the class binding
/// has been selected. Method parameter indices use the spare high bit of the
/// RFC's bounded `u8` representation, so both scopes remain positional and
/// alpha-renaming is free at runtime.
pub(super) fn erase_method_signature(
    value: &GenericType,
    erasure_declaration: &GenericDeclaration,
    method: &GenericMethodMetadata,
    owner_arguments: &[GenericType],
) -> GenericType {
    erase_method_parameters(
        value,
        erasure_declaration,
        method,
        owner_arguments,
        erasure_declaration.parameters.len() + method.parameters.len() + 1,
    )
}

fn erase_method_parameters(
    value: &GenericType,
    erasure_declaration: &GenericDeclaration,
    method: &GenericMethodMetadata,
    owner_arguments: &[GenericType],
    remaining: usize,
) -> GenericType {
    if remaining == 0 {
        return GenericType::Mixed;
    }
    match value {
        GenericType::Parameter(index) => {
            if let Some(index) = method_parameter_index(*index) {
                return method
                    .parameters
                    .get(index)
                    .and_then(|parameter| parameter.bound.as_ref())
                    .map(|bound| substitute_generic_parameters(bound, owner_arguments))
                    .map(|bound| {
                        erase_method_parameters(
                            &bound,
                            erasure_declaration,
                            method,
                            owner_arguments,
                            remaining - 1,
                        )
                    })
                    .unwrap_or(GenericType::Mixed);
            }
            erasure_declaration
                .parameters
                .get(*index as usize)
                .and_then(|parameter| parameter.bound.as_ref())
                .map(|bound| {
                    erase_method_parameters(
                        bound,
                        erasure_declaration,
                        method,
                        owner_arguments,
                        remaining - 1,
                    )
                })
                .unwrap_or(GenericType::Mixed)
        }
        GenericType::Named { name, arguments } => GenericType::Named {
            name: *name,
            arguments: arguments
                .iter()
                .map(|argument| {
                    erase_method_parameters(
                        argument,
                        erasure_declaration,
                        method,
                        owner_arguments,
                        remaining,
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        },
        GenericType::Nullable(inner) => GenericType::Nullable(Box::new(erase_method_parameters(
            inner,
            erasure_declaration,
            method,
            owner_arguments,
            remaining,
        ))),
        GenericType::Union(parts) => GenericType::Union(
            parts
                .iter()
                .map(|part| {
                    erase_method_parameters(
                        part,
                        erasure_declaration,
                        method,
                        owner_arguments,
                        remaining,
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        GenericType::Intersection(parts) => GenericType::Intersection(
            parts
                .iter()
                .map(|part| {
                    erase_method_parameters(
                        part,
                        erasure_declaration,
                        method,
                        owner_arguments,
                        remaining,
                    )
                })
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
        GenericType::Intersection(parts) => GenericType::Intersection(
            parts
                .iter()
                .map(|part| erase_forwarded_parameters(part, owner, remaining))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        concrete => concrete.clone(),
    }
}
