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
            if property.visibility == Visibility::Private
                && child.iter().any(|child_property| {
                    child_property.name == property.name
                        && child_property.visibility == Visibility::Private
                })
            {
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
                && child.iter().any(|child_property| {
                    child_property.name == property.name
                        && child_property.visibility == Visibility::Private
                })
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

#[inline]
fn property_definitions_are_compatible(
    left: &PropertyDefinition,
    right: &PropertyDefinition,
) -> bool {
    left.visibility == right.visibility
        && property_type_hints_are_equivalent(&left.type_hint, &right.type_hint)
        && left.is_readonly == right.is_readonly
        && match (&left.default, &right.default) {
            (None, None) => true,
            (Some(left), Some(right)) => left.structurally_equal(right),
            _ => false,
        }
}

fn validate_inherited_property_definition(
    child: &PropertyDefinition,
    parent: &PropertyDefinition,
    child_class: &str,
) -> Result<(), String> {
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
        return Err(format!(
            "Access level to {}::${} must be {} (as in class {}) or weaker",
            child_class, child.name, required, parent.declaring_class
        ));
    }
    if child.is_readonly != parent.is_readonly {
        return Err(format!(
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
        ));
    }
    if !property_type_hints_are_equivalent(&child.type_hint, &parent.type_hint) {
        return Err(format!(
            "Type of {}::${} must be {} (as in class {})",
            child_class,
            child.name,
            parent.type_hint.display_name(),
            parent.declaring_class
        ));
    }
    Ok(())
}

fn validate_property_inheritance(
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
            validate_inherited_property_definition(child, parent, child_class)?;
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
            validate_inherited_property_definition(child, parent, child_class)?;
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
        definition.declaring_class = trait_name.to_string();
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
) -> Result<(), String> {
    let mut additions = Vec::new();
    for property in source {
        if let Some(existing_property) = target
            .iter()
            .find(|candidate| candidate.name == property.name)
        {
            if existing_property.declaring_class == class_name {
                continue;
            }
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
        addition.declaring_class = trait_name.to_string();
        addition.type_scope = class_name.to_string();
        additions.push(addition);
    }
    target.extend(additions);
    Ok(())
}
