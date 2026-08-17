/// Merge inherited declarations while preserving PHP's private-slot rule.
/// The same rule applies to instance and static properties; keeping it here
/// prevents their registration paths from drifting.
fn inherit_property_definitions(
    child: &mut Vec<PropertyDefinition>,
    parent: &[PropertyDefinition],
) {
    let child_names: std::collections::HashSet<&str> = child
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    let mut inherited = Vec::new();
    for property in parent {
        if child_names.contains(property.name.as_str()) {
            if property.visibility == Visibility::Private {
                inherited.push(property.clone());
            }
        } else {
            inherited.push(property.clone());
        }
    }
    child.extend(inherited);
}

/// Static declarations additionally carry a storage identity. An inherited
/// declaration reuses its parent's slot; a redeclaration keeps the child's
/// independently allocated slot.
fn inherit_static_property_definitions(
    child: &mut Vec<PropertyDefinition>,
    child_slots: &mut Vec<Option<u32>>,
    parent: &[PropertyDefinition],
    parent_slots: &[u32],
) {
    debug_assert_eq!(child.len(), child_slots.len());
    debug_assert_eq!(parent.len(), parent_slots.len());
    let child_names: std::collections::HashSet<&str> = child
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    let mut inherited = Vec::new();
    for (index, property) in parent.iter().enumerate() {
        let keep = if child_names.contains(property.name.as_str()) {
            property.visibility == Visibility::Private
        } else {
            true
        };
        if keep {
            inherited.push((property.clone(), parent_slots[index]));
        }
    }
    for (definition, slot) in inherited {
        child.push(definition);
        child_slots.push(Some(slot));
    }
}

#[inline]
fn property_type_hint_key(hint: &crate::vm::function::ParamTypeHint) -> String {
    use crate::vm::function::ParamTypeHint;

    fn union_parts(hint: &ParamTypeHint, parts: &mut Vec<String>) {
        match hint {
            ParamTypeHint::Union(nested) => {
                for part in nested {
                    union_parts(part, parts);
                }
            }
            ParamTypeHint::Nullable(inner) => {
                if !matches!(inner.as_ref(), ParamTypeHint::None) {
                    union_parts(inner, parts);
                }
                parts.push("02:null".to_string());
            }
            ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("iterable") => {
                parts.push("01:array".to_string());
                parts.push("03:traversable".to_string());
            }
            part => parts.push(property_type_hint_key(part)),
        }
    }

    match hint {
        ParamTypeHint::None => "00:untyped".to_string(),
        ParamTypeHint::Int => "01:int".to_string(),
        ParamTypeHint::Float => "01:float".to_string(),
        ParamTypeHint::String => "01:string".to_string(),
        ParamTypeHint::Bool => "01:bool".to_string(),
        ParamTypeHint::Array => "01:array".to_string(),
        ParamTypeHint::Callable => "01:callable".to_string(),
        ParamTypeHint::Void => "01:void".to_string(),
        ParamTypeHint::Mixed => "01:mixed".to_string(),
        ParamTypeHint::Never => "01:never".to_string(),
        ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("iterable") => {
            "04:01:array|03:traversable".to_string()
        }
        ParamTypeHint::ClassName(name) => format!("03:{}", name.to_ascii_lowercase()),
        ParamTypeHint::Nullable(_) | ParamTypeHint::Union(_) => {
            let mut parts = Vec::new();
            union_parts(hint, &mut parts);
            parts.sort_unstable();
            parts.dedup();
            format!("04:{}", parts.join("|"))
        }
        ParamTypeHint::Intersection(parts) => {
            let mut parts = parts.iter().map(property_type_hint_key).collect::<Vec<_>>();
            parts.sort_unstable();
            parts.dedup();
            format!("05:{}", parts.join("&"))
        }
    }
}

#[inline]
fn property_type_hints_are_equivalent(
    left: &crate::vm::function::ParamTypeHint,
    right: &crate::vm::function::ParamTypeHint,
) -> bool {
    left == right || property_type_hint_key(left) == property_type_hint_key(right)
}

fn property_type_hints_are_invariant(
    eg: &ExecutorGlobals,
    child: &PropertyDefinition,
    parent: &PropertyDefinition,
    child_class: &str,
    linking_class: &ClassDef,
) -> bool {
    property_type_hints_are_equivalent(&child.type_hint, &parent.type_hint)
        || (eg.is_return_type_compatible(
            &child.type_hint,
            &parent.type_hint,
            child_class,
            &parent.declaring_class,
            Some(linking_class),
        ) && eg.is_return_type_compatible(
            &parent.type_hint,
            &child.type_hint,
            &parent.declaring_class,
            child_class,
            Some(linking_class),
        ))
}

fn property_type_has_unknown_class(
    eg: &ExecutorGlobals,
    hint: &crate::vm::function::ParamTypeHint,
    linking_class: &ClassDef,
) -> bool {
    use crate::vm::function::ParamTypeHint;

    match hint {
        ParamTypeHint::ClassName(name)
            if name.eq_ignore_ascii_case("object")
                || name.eq_ignore_ascii_case("iterable") =>
        {
            false
        }
        ParamTypeHint::ClassName(name) => {
            !eg.variance_class_is_known(name, Some(linking_class))
        }
        ParamTypeHint::Nullable(inner) => {
            property_type_has_unknown_class(eg, inner, linking_class)
        }
        ParamTypeHint::Union(parts) | ParamTypeHint::Intersection(parts) => parts
            .iter()
            .any(|part| property_type_has_unknown_class(eg, part, linking_class)),
        _ => false,
    }
}

fn property_inheritance_requires_delayed_linking(
    eg: &ExecutorGlobals,
    class_def: &ClassDef,
    parent: &ClassDef,
) -> bool {
    let declarations_require_delayed_linking =
        |children: &[PropertyDefinition], parents: &[PropertyDefinition]| {
            children.iter().any(|child| {
                let parent_property = parents.iter().find(|candidate| {
                    candidate.name == child.name && candidate.visibility != Visibility::Private
                });
                parent_property.is_some_and(|parent_property| {
                    !property_type_hints_are_invariant(
                        eg,
                        child,
                        parent_property,
                        &class_def.name,
                        class_def,
                    ) && (property_type_has_unknown_class(eg, &child.type_hint, class_def)
                        || property_type_has_unknown_class(
                            eg,
                            &parent_property.type_hint,
                            class_def,
                        ))
                })
            })
        };

    declarations_require_delayed_linking(&class_def.properties, &parent.properties)
        || declarations_require_delayed_linking(
            &class_def.static_properties,
            &parent.static_properties,
        )
}

#[inline]
fn property_definitions_are_compatible(
    left: &PropertyDefinition,
    right: &PropertyDefinition,
) -> bool {
    left.visibility == right.visibility
        && left.set_visibility == right.set_visibility
        && property_type_hints_are_equivalent(&left.type_hint, &right.type_hint)
        && left.is_readonly == right.is_readonly
        && match (&left.default, &right.default) {
            (None, None) => true,
            (Some(left), Some(right)) => left.structurally_equal(right),
            _ => false,
        }
}

fn validate_inherited_property_definition(
    eg: &ExecutorGlobals,
    child: &PropertyDefinition,
    parent: &PropertyDefinition,
    child_class: &str,
    linking_class: &ClassDef,
) -> Result<(), String> {
    let error = |message: String| {
        if let Some(file) = &child.source_file {
            format!("{message} in {file} on line {}", child.source_line)
        } else {
            message
        }
    };
    debug_assert_ne!(parent.visibility, Visibility::Private);
    let visibility_is_compatible = match parent.visibility {
        Visibility::Public => child.visibility == Visibility::Public,
        Visibility::Protected => child.visibility != Visibility::Private,
        Visibility::Private => true,
    };
    if !visibility_is_compatible {
        let required = match parent.visibility {
            Visibility::Public => "public",
            Visibility::Protected => "protected",
            Visibility::Private => unreachable!(),
        };
        return Err(error(format!(
            "Access level to {}::${} must be {} (as in class {}) or weaker",
            child_class, child.name, required, parent.declaring_class
        )));
    }
    if parent.set_visibility == Some(Visibility::Private) {
        return Err(error(format!(
            "Cannot override final property {}::${}",
            parent.declaring_class, parent.name
        )));
    }
    let visibility_rank = |visibility| match visibility {
        Visibility::Private => 0,
        Visibility::Protected => 1,
        Visibility::Public => 2,
    };
    match (parent.set_visibility, child.set_visibility) {
        (None, Some(_)) => {
            return Err(error(format!(
                "Set access level of {}::${} must be omitted (as in class {})",
                child_class, child.name, parent.declaring_class
            )));
        }
        (Some(parent_set), Some(child_set))
            if visibility_rank(child_set) < visibility_rank(parent_set) =>
        {
            let required = match parent_set {
                Visibility::Public => "public(set)",
                Visibility::Protected => "protected(set)",
                Visibility::Private => unreachable!(),
            };
            return Err(error(format!(
                "Set access level of {}::${} must be {} (as in class {}) or weaker",
                child_class, child.name, required, parent.declaring_class
            )));
        }
        _ => {}
    }
    if child.is_readonly != parent.is_readonly {
        return Err(error(format!(
            "Cannot redeclare {} property {}::${} as {} {}::${}",
            if parent.is_readonly {
                "readonly"
            } else {
                "non-readonly"
            },
            parent.declaring_class,
            parent.name,
            if child.is_readonly {
                "readonly"
            } else {
                "non-readonly"
            },
            child_class,
            child.name
        )));
    }
    let types_are_invariant =
        property_type_hints_are_invariant(eg, child, parent, child_class, linking_class);
    if !types_are_invariant {
        if matches!(parent.type_hint, crate::vm::function::ParamTypeHint::None) {
            return Err(error(format!(
                "Type of {}::${} must be omitted to match the parent definition in class {}",
                child_class, child.name, parent.declaring_class
            )));
        }
        return Err(error(format!(
            "Type of {}::${} must be {} (as in class {})",
            child_class,
            child.name,
            parent.type_hint.property_declaration_display_name(),
            parent.declaring_class
        )));
    }
    Ok(())
}

fn validate_property_inheritance(
    eg: &ExecutorGlobals,
    linking_class: &ClassDef,
    child_class: &str,
    child_instance: &[PropertyDefinition],
    child_static: &[PropertyDefinition],
    parent_instance: &[PropertyDefinition],
    parent_static: &[PropertyDefinition],
) -> Result<(), String> {
    for child in child_instance {
        if let Some(parent) = parent_static
            .iter()
            .find(|parent| parent.name == child.name && parent.visibility != Visibility::Private)
        {
            return Err(format!(
                "Cannot redeclare static {}::${} as non static {}::${}",
                parent.declaring_class, parent.name, child_class, child.name
            ));
        }
        if let Some(parent) = parent_instance
            .iter()
            .find(|parent| parent.name == child.name && parent.visibility != Visibility::Private)
        {
            validate_inherited_property_definition(eg, child, parent, child_class, linking_class)?;
        }
    }
    for child in child_static {
        if let Some(parent) = parent_instance
            .iter()
            .find(|parent| parent.name == child.name && parent.visibility != Visibility::Private)
        {
            return Err(format!(
                "Cannot redeclare non static {}::${} as static {}::${}",
                parent.declaring_class, parent.name, child_class, child.name
            ));
        }
        if let Some(parent) = parent_static
            .iter()
            .find(|parent| parent.name == child.name && parent.visibility != Visibility::Private)
        {
            validate_inherited_property_definition(eg, child, parent, child_class, linking_class)?;
        }
    }
    Ok(())
}

/// A trait static property is composed into the consuming class, not shared
/// with the trait or unrelated consumers. Since PHP 8.3, using the same trait
/// again in a child also creates storage distinct from the parent's inherited
/// property. Class/trait and trait/trait declarations still have to be
/// compatible; a trait declaration simply replaces an inherited declaration.
fn merge_trait_static_property_definitions(
    target: &mut Vec<PropertyDefinition>,
    target_slots: &mut Vec<Option<u32>>,
    source: &[PropertyDefinition],
    class_name: &str,
    trait_name: &str,
    own_names: &std::collections::HashSet<String>,
    composed_names: &mut std::collections::HashSet<String>,
) -> Result<(), String> {
    debug_assert_eq!(target.len(), target_slots.len());
    for property in source {
        let existing = target
            .iter()
            .position(|candidate| candidate.name == property.name);
        if own_names.contains(&property.name) || composed_names.contains(&property.name) {
            let index = existing.expect("own/composed static property definition");
            let existing_property = &target[index];
            if !property_definitions_are_compatible(existing_property, property) {
                return Err(format!(
                    "{} and {} define the same property (${}) in the composition of {}. \
                     However, the definition differs and is considered incompatible",
                    existing_property.declaring_class, trait_name, property.name, class_name
                ));
            }
            continue;
        }

        let mut definition = property.clone();
        // PHP composes a trait property into each consuming class. The class
        // is therefore the declaring scope for visibility and the private
        // storage key; the trait name is only relevant while validating the
        // composition above.
        definition.declaring_class = class_name.to_string();
        definition.type_scope = class_name.to_string();
        if let Some(index) = existing {
            // A first trait declaration in this class overrides inherited
            // metadata and receives a fresh storage slot.
            target[index] = definition;
            target_slots[index] = None;
        } else {
            target.push(definition);
            target_slots.push(None);
        }
        composed_names.insert(property.name.clone());
    }
    Ok(())
}

/// Merge one trait's declarations into a consuming class. Instance and static
/// tables both use this exact collision contract, but remain separate storage.
fn merge_trait_property_definitions(
    target: &mut Vec<PropertyDefinition>,
    source: &[PropertyDefinition],
    class_name: &str,
    trait_name: &str,
    own_names: &std::collections::HashSet<String>,
    composed_names: &mut std::collections::HashSet<String>,
) -> Result<(), String> {
    for property in source {
        let existing = target
            .iter()
            .position(|candidate| candidate.name == property.name);
        if own_names.contains(&property.name) {
            // Preserve the existing class-over-trait behavior. Compatibility
            // of an explicit class declaration is handled by the declaration
            // validation path.
            continue;
        }
        if composed_names.contains(&property.name) {
            let existing_property = &target[existing.expect("composed trait property")];
            let compatible = property_definitions_are_compatible(existing_property, property);
            if !compatible {
                return Err(format!(
                    "{} and {} define the same property (${}) in the composition of {}. \
                     However, the definition differs and is considered incompatible",
                    existing_property.declaring_class, trait_name, property.name, class_name
                ));
            }
            continue;
        }

        let mut addition = property.clone();
        addition.declaring_class = class_name.to_string();
        addition.type_scope = class_name.to_string();
        if let Some(index) = existing {
            // The first trait declaration in this class replaces inherited
            // metadata, just as the static-property composition path does.
            target[index] = addition;
        } else {
            target.push(addition);
        }
        composed_names.insert(property.name.clone());
    }
    Ok(())
}
