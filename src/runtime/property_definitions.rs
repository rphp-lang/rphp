/// Merge inherited declarations while preserving PHP's private-slot rule.
/// The same rule applies to instance and static properties; keeping it here
/// prevents their registration paths from drifting.
#[cold]
#[inline(never)]
fn inherit_property_definitions(
    child: &mut Vec<PropertyDefinition>,
    parent: &[PropertyDefinition],
) {
    // Every child declaration inherits a concrete parent hook that it does not
    // replace. A plain child supplies backing storage for inherited concrete
    // hooks, while still satisfying abstract hook requirements directly.
    for child_property in child.iter_mut() {
        let child_declares_plain_storage =
            !child_property.has_get_hook && !child_property.has_set_hook;
        let Some(parent_property) = parent.iter().find(|parent_property| {
            parent_property.name == child_property.name
                && parent_property.visibility != Visibility::Private
        }) else {
            continue;
        };
        // Redeclaring an inherited backed property keeps storage even when
        // the child hook body itself does not access `$this->$property`.
        if !parent_property.is_virtual_hook_property() {
            if child_property.has_get_hook {
                child_property.get_hook_is_backed = true;
            }
            if child_property.has_set_hook {
                child_property.set_hook_is_backed = true;
            }
        }
        if !child_property.has_get_hook
            && parent_property.has_get_hook
            && !parent_property.abstract_get_hook()
        {
            child_property.has_get_hook = true;
            child_property.get_hook_is_backed =
                child_declares_plain_storage || parent_property.get_hook_is_backed;
        }
        if !child_property.has_set_hook
            && parent_property.has_set_hook
            && !parent_property.abstract_set_hook()
        {
            child_property.has_set_hook = true;
            child_property.set_hook_is_backed =
                child_declares_plain_storage || parent_property.set_hook_is_backed;
        }
    }
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

fn inherited_property_types_are_compatible(
    eg: &ExecutorGlobals,
    child: &PropertyDefinition,
    parent: &PropertyDefinition,
    child_class: &str,
    linking_class: &ClassDef,
) -> bool {
    if property_type_hints_are_equivalent(&child.type_hint, &parent.type_hint) {
        return true;
    }

    // Backed storage can be both read and written independently of the hooks
    // declared on the surface, so PHP keeps its property type invariant. Only
    // a virtual get-only or set-only contract can expose directional variance.
    if !parent.is_virtual_hook_property() {
        return property_type_hints_are_invariant(
            eg,
            child,
            parent,
            child_class,
            linking_class,
        );
    }

    let child_is_subtype = || {
        eg.is_return_type_compatible(
            &child.type_hint,
            &parent.type_hint,
            child_class,
            &parent.declaring_class,
            Some(linking_class),
        )
    };
    let child_is_supertype = || {
        eg.is_return_type_compatible(
            &parent.type_hint,
            &child.type_hint,
            &parent.declaring_class,
            child_class,
            Some(linking_class),
        )
    };

    match (parent.has_get_hook, parent.has_set_hook) {
        (true, false) => child_is_subtype(),
        (false, true) => child_is_supertype(),
        _ => property_type_hints_are_invariant(eg, child, parent, child_class, linking_class),
    }
}

#[inline]
fn property_has_set_capability(property: &PropertyDefinition) -> bool {
    property.has_set_hook || !property.is_virtual_hook_property()
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
                || name.eq_ignore_ascii_case("iterable")
                || name.eq_ignore_ascii_case("false")
                || name.eq_ignore_ascii_case("true") =>
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

fn property_setter_method<'a>(
    class_def: &'a ClassDef,
    property: &PropertyDefinition,
) -> Option<&'a crate::vm::function::UserFunction> {
    let method_name = format!("${}::set", property.name);
    class_def
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name.eq_ignore_ascii_case(&method_name))
        .map(|(_, _, _, _, function)| function)
}

fn resolved_property_setter_hints(
    eg: &ExecutorGlobals,
    class_def: &ClassDef,
    property: &PropertyDefinition,
) -> Option<(
    crate::vm::function::ParamTypeHint,
    crate::vm::function::ParamTypeHint,
)> {
    let setter = property_setter_method(class_def, property)?;
    let setter_hint = setter.common.sig.param_type_hints.first()?;
    Some((
        eg.resolve_variance_type_hint(setter_hint, &class_def.name, Some(class_def)),
        eg.resolve_variance_type_hint(&property.type_hint, &property.type_scope, Some(class_def)),
    ))
}

/// A named declaration may precede the class-like types used to establish a
/// setter's contravariant relation. Keep it out of the class table only while
/// a later declaration in the same source unit can make that relation known.
fn property_hook_setter_variance_requires_delayed_linking(
    eg: &ExecutorGlobals,
    class_def: &ClassDef,
) -> bool {
    use crate::vm::function::ParamTypeHint;

    class_def.properties.iter().any(|property| {
        if !property.has_set_hook {
            return false;
        }
        let Some((setter_hint, property_hint)) =
            resolved_property_setter_hints(eg, class_def, property)
        else {
            return false;
        };
        if matches!(setter_hint, ParamTypeHint::None)
            || matches!(property_hint, ParamTypeHint::None)
        {
            return false;
        }
        let proven_compatible = eg.is_param_type_compatible_strict(
            &setter_hint,
            &property_hint,
            &class_def.name,
            &property.type_scope,
            Some(class_def),
        );
        if proven_compatible
            || !eg.is_param_type_potentially_compatible(
                &setter_hint,
                &property_hint,
                &class_def.name,
                &property.type_scope,
                Some(class_def),
            )
        {
            return false;
        }
        property_type_has_unknown_class(eg, &setter_hint, class_def)
            || property_type_has_unknown_class(eg, &property_hint, class_def)
    })
}

fn validate_property_hook_setter_variance(
    eg: &ExecutorGlobals,
    class_def: &ClassDef,
) -> Result<(), String> {
    use crate::vm::function::ParamTypeHint;

    for property in class_def
        .properties
        .iter()
        .filter(|property| property.has_set_hook)
    {
        let Some(setter) = property_setter_method(class_def, property) else {
            continue;
        };
        let Some(setter_hint) = setter.common.sig.param_type_hints.first() else {
            continue;
        };
        let compatible = match (setter_hint, &property.type_hint) {
            (ParamTypeHint::None, ParamTypeHint::None) => true,
            (ParamTypeHint::None, _) | (_, ParamTypeHint::None) => false,
            _ => {
                let setter_hint = eg.resolve_variance_type_hint(
                    setter_hint,
                    &class_def.name,
                    Some(class_def),
                );
                let property_hint = eg.resolve_variance_type_hint(
                    &property.type_hint,
                    &property.type_scope,
                    Some(class_def),
                );
                eg.is_param_type_compatible_strict(
                    &setter_hint,
                    &property_hint,
                    &class_def.name,
                    &property.type_scope,
                    Some(class_def),
                )
            }
        };
        if compatible {
            continue;
        }

        let parameter = setter
            .common
            .sig
            .param_names
            .first()
            .map(String::as_str)
            .unwrap_or("value");
        let location = if setter.op_array.source_file.is_empty() {
            String::new()
        } else {
            setter.op_array.declaration_line().map_or_else(String::new, |line| {
                format!(" in {} on line {line}", setter.op_array.source_file)
            })
        };
        return Err(format!(
            "Type of parameter ${parameter} of hook {}::${}::set must be compatible with property type{location}",
            class_def.name, property.name
        ));
    }
    Ok(())
}

fn collect_variance_class_names(
    hint: &crate::vm::function::ParamTypeHint,
    dependencies: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    use crate::vm::function::ParamTypeHint;

    match hint {
        ParamTypeHint::ClassName(name)
            if ![
                "self", "parent", "static", "object", "iterable", "false", "true", "null",
            ]
            .iter()
            .any(|builtin| name.eq_ignore_ascii_case(builtin)) =>
        {
            let key = name.to_ascii_lowercase();
            if seen.insert(key) {
                dependencies.push(name.clone());
            }
        }
        ParamTypeHint::Nullable(inner) => {
            collect_variance_class_names(inner, dependencies, seen);
        }
        ParamTypeHint::Union(parts) | ParamTypeHint::Intersection(parts) => {
            for part in parts {
                collect_variance_class_names(part, dependencies, seen);
            }
        }
        _ => {}
    }
}

fn variance_type_hint_mentions_class(
    hint: &crate::vm::function::ParamTypeHint,
    class_name: &str,
) -> bool {
    use crate::vm::function::ParamTypeHint;

    match hint {
        ParamTypeHint::ClassName(name) => name.eq_ignore_ascii_case(class_name),
        ParamTypeHint::Nullable(inner) => variance_type_hint_mentions_class(inner, class_name),
        ParamTypeHint::Union(parts) | ParamTypeHint::Intersection(parts) => parts
            .iter()
            .any(|part| variance_type_hint_mentions_class(part, class_name)),
        _ => false,
    }
}

/// Runtime includes execute after user autoloaders may have been installed.
/// Return setter/property type dependencies in PHP's property-then-parameter
/// order so the include path can request them before link validation.
pub(crate) fn property_hook_setter_variance_dependencies(
    eg: &ExecutorGlobals,
    class_def: &ClassDef,
) -> Vec<String> {
    use crate::vm::function::ParamTypeHint;

    let mut dependencies = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for property in class_def
        .properties
        .iter()
        .filter(|property| property.has_set_hook)
    {
        let Some((setter_hint, property_hint)) =
            resolved_property_setter_hints(eg, class_def, property)
        else {
            continue;
        };
        if matches!(setter_hint, ParamTypeHint::None)
            || matches!(property_hint, ParamTypeHint::None)
            || eg.is_param_type_compatible_strict(
                &setter_hint,
                &property_hint,
                &class_def.name,
                &property.type_scope,
                Some(class_def),
            )
            || !eg.is_param_type_potentially_compatible(
                &setter_hint,
                &property_hint,
                &class_def.name,
                &property.type_scope,
                Some(class_def),
            )
        {
            continue;
        }
        collect_variance_class_names(&property_hint, &mut dependencies, &mut seen);
        collect_variance_class_names(&setter_hint, &mut dependencies, &mut seen);
    }
    dependencies.retain(|dependency| !dependency.eq_ignore_ascii_case(&class_def.name));
    dependencies
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
                    !inherited_property_types_are_compatible(
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
        && left.is_final() == right.is_final()
        && left.abstract_get_hook() == right.abstract_get_hook()
        && left.abstract_set_hook() == right.abstract_set_hook()
        && left.has_get_hook == right.has_get_hook
        && left.get_hook_is_backed == right.get_hook_is_backed
        && left.has_set_hook == right.has_set_hook
        && left.set_hook_is_backed == right.set_hook_is_backed
        && match (&left.default, &right.default) {
            (None, None) => true,
            (Some(left), Some(right)) => left.structurally_equal(right),
            _ => false,
        }
}

#[derive(Clone)]
struct ComposedTraitPropertyOrigin {
    trait_name: String,
    is_static: bool,
}

#[cold]
#[inline(never)]
fn incompatible_trait_property_error(
    first_owner: &str,
    second_owner: &str,
    property: &str,
    class_name: &str,
    source_file: Option<&str>,
    declaration_line: usize,
) -> String {
    let location = source_file.map_or_else(String::new, |file| {
        format!(" in {file} on line {declaration_line}")
    });
    format!(
        "{first_owner} and {second_owner} define the same property (${property}) in the composition of {class_name}. \
         However, the definition differs and is considered incompatible. Class was composed{location}"
    )
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
            format!("{message} in {file} on line {}", child.declaration_line())
        } else {
            message
        }
    };
    let getter_returns_reference = linking_class.methods.iter().any(
        |(method, _, _, _, function)| {
            method.eq_ignore_ascii_case(&format!("${}::get", child.name))
                && function.common.sig.returns_reference
        },
    );
    if getter_returns_reference
        && child.has_set_hook
        && (child.get_hook_is_backed || (!parent.has_get_hook && !parent.has_set_hook))
    {
        return Err(error(format!(
            "Get hook of backed property {}::{} with set hook may not return by reference",
            child_class, child.name
        )));
    }
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
    if parent.is_final() || parent.set_visibility == Some(Visibility::Private) {
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
        (None, Some(_)) if property_has_set_capability(parent) => {
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
    let types_are_compatible =
        inherited_property_types_are_compatible(eg, child, parent, child_class, linking_class);
    if !types_are_compatible {
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
        if child.is_virtual_hook_property() && child.default.is_some() {
            let inherits_backing_storage = parent_instance.iter().any(|parent| {
                parent.name == child.name
                    && parent.visibility != Visibility::Private
                    && !parent.is_virtual_hook_property()
            });
            if !inherits_backing_storage {
                let location = child.source_file.as_ref().map_or_else(String::new, |file| {
                    format!(" in {file} on line {}", child.declaration_line())
                });
                return Err(format!(
                    "Cannot specify default value for virtual hooked property {child_class}::${}{location}",
                    child.name
                ));
            }
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
    own_instance_names: &std::collections::HashSet<String>,
    composed_names: &mut std::collections::HashMap<String, ComposedTraitPropertyOrigin>,
    source_file: Option<&str>,
    declaration_line: usize,
) -> Result<(), String> {
    debug_assert_eq!(target.len(), target_slots.len());
    for property in source {
        let existing = target
            .iter()
            .position(|candidate| candidate.name == property.name);
        if own_instance_names.contains(&property.name) {
            return Err(incompatible_trait_property_error(
                class_name,
                trait_name,
                &property.name,
                class_name,
                source_file,
                declaration_line,
            ));
        }
        if own_names.contains(&property.name) {
            let index = existing.expect("own/composed static property definition");
            let existing_property = &target[index];
            if !property_definitions_are_compatible(existing_property, property) {
                return Err(incompatible_trait_property_error(
                    class_name,
                    trait_name,
                    &property.name,
                    class_name,
                    source_file,
                    declaration_line,
                ));
            }
            continue;
        }
        if let Some(first) = composed_names.get(&property.name) {
            if !first.is_static {
                return Err(incompatible_trait_property_error(
                    &first.trait_name,
                    trait_name,
                    &property.name,
                    class_name,
                    source_file,
                    declaration_line,
                ));
            }
            let index = existing.expect("composed static property definition");
            if !property_definitions_are_compatible(&target[index], property) {
                return Err(incompatible_trait_property_error(
                    &first.trait_name,
                    trait_name,
                    &property.name,
                    class_name,
                    source_file,
                    declaration_line,
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
            // metadata and receives a fresh storage slot. An inherited private
            // declaration is a distinct slot, so retain it behind the new
            // child-scoped declaration just like an explicit child property.
            if target[index].visibility == Visibility::Private {
                target.insert(index, definition);
                target_slots.insert(index, None);
            } else {
                target[index] = definition;
                target_slots[index] = None;
            }
        } else {
            target.push(definition);
            target_slots.push(None);
        }
        composed_names.insert(
            property.name.clone(),
            ComposedTraitPropertyOrigin {
                trait_name: trait_name.to_string(),
                is_static: true,
            },
        );
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
    own_static_names: &std::collections::HashSet<String>,
    composed_names: &mut std::collections::HashMap<String, ComposedTraitPropertyOrigin>,
    source_file: Option<&str>,
    declaration_line: usize,
) -> Result<(), String> {
    for property in source {
        let existing = target
            .iter()
            .position(|candidate| candidate.name == property.name);
        if own_static_names.contains(&property.name) {
            return Err(incompatible_trait_property_error(
                class_name,
                trait_name,
                &property.name,
                class_name,
                source_file,
                declaration_line,
            ));
        }
        if own_names.contains(&property.name) {
            let existing_property = &target[existing.expect("own property definition")];
            if existing_property.has_get_hook
                || existing_property.has_set_hook
                || property.has_get_hook
                || property.has_set_hook
            {
                let location = source_file.map_or_else(String::new, |file| {
                    format!(" in {file} on line {declaration_line}")
                });
                return Err(format!(
                    "{class_name} and {trait_name} define the same hooked property (${}) in the composition of {class_name}. \
                     Conflict resolution between hooked properties is currently not supported. Class was composed{location}",
                    property.name
                ));
            }
            if !property_definitions_are_compatible(existing_property, property) {
                return Err(incompatible_trait_property_error(
                    class_name,
                    trait_name,
                    &property.name,
                    class_name,
                    source_file,
                    declaration_line,
                ));
            }
            // Compatible class declarations retain their source metadata and
            // storage while satisfying the trait declaration.
            continue;
        }
        if let Some(first) = composed_names.get(&property.name) {
            if first.is_static {
                return Err(incompatible_trait_property_error(
                    &first.trait_name,
                    trait_name,
                    &property.name,
                    class_name,
                    source_file,
                    declaration_line,
                ));
            }
            let existing_property = &target[existing.expect("composed trait property")];
            if existing_property.has_get_hook
                || existing_property.has_set_hook
                || property.has_get_hook
                || property.has_set_hook
            {
                let location = source_file.map_or_else(String::new, |file| {
                    format!(" in {file} on line {declaration_line}")
                });
                return Err(format!(
                    "{} and {trait_name} define the same hooked property (${}) in the composition of {class_name}. \
                     Conflict resolution between hooked properties is currently not supported. Class was composed{location}",
                    first.trait_name,
                    property.name
                ));
            }
            let compatible = property_definitions_are_compatible(existing_property, property);
            if !compatible {
                return Err(incompatible_trait_property_error(
                    &first.trait_name,
                    trait_name,
                    &property.name,
                    class_name,
                    source_file,
                    declaration_line,
                ));
            }
            continue;
        }

        let mut addition = property.clone();
        addition.declaring_class = class_name.to_string();
        addition.type_scope = class_name.to_string();
        if let Some(index) = existing {
            // The first trait declaration in this class replaces inherited
            // metadata. Inherited private storage remains an independent
            // parent slot and stays behind the new child-scoped declaration.
            if target[index].visibility == Visibility::Private {
                target.insert(index, addition);
            } else {
                target[index] = addition;
            }
        } else {
            target.push(addition);
        }
        composed_names.insert(
            property.name.clone(),
            ComposedTraitPropertyOrigin {
                trait_name: trait_name.to_string(),
                is_static: false,
            },
        );
    }
    Ok(())
}
