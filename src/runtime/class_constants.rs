// Class-like constant composition and inheritance contracts.
//
// Kept outside the executor registry so registration orchestration remains
// readable while these cold PHP compatibility rules can evolve independently.

fn constant_definitions_compatible(
    left: &ClassConstantDefinition,
    right: &ClassConstantDefinition,
) -> bool {
    left.visibility == right.visibility
        && left.type_hint == right.type_hint
        && left.is_final == right.is_final
        && left.evaluation_error == right.evaluation_error
        && left.value.structurally_equal(&right.value)
}

fn class_constant_type_is_covariant(
    implementation: &crate::vm::function::ParamTypeHint,
    inherited: &crate::vm::function::ParamTypeHint,
    class_is_a: &dyn Fn(&str, &str) -> bool,
) -> bool {
    use crate::vm::function::ParamTypeHint;

    if matches!(inherited, ParamTypeHint::None | ParamTypeHint::Mixed)
        || implementation == inherited
    {
        return true;
    }
    if matches!(implementation, ParamTypeHint::None | ParamTypeHint::Mixed) {
        return false;
    }
    if let ParamTypeHint::Union(parts) = implementation {
        return parts
            .iter()
            .all(|part| class_constant_type_is_covariant(part, inherited, class_is_a));
    }
    if let ParamTypeHint::Union(parts) = inherited {
        return parts
            .iter()
            .any(|part| class_constant_type_is_covariant(implementation, part, class_is_a));
    }
    if let ParamTypeHint::Intersection(parts) = inherited {
        return parts
            .iter()
            .all(|part| class_constant_type_is_covariant(implementation, part, class_is_a));
    }
    if let ParamTypeHint::Intersection(parts) = implementation {
        return parts
            .iter()
            .any(|part| class_constant_type_is_covariant(part, inherited, class_is_a));
    }
    if let ParamTypeHint::Nullable(inner) = inherited {
        return class_constant_type_is_covariant(implementation, inner, class_is_a)
            || matches!(
                implementation,
                ParamTypeHint::Nullable(implementation_inner)
                    if matches!(implementation_inner.as_ref(), ParamTypeHint::None)
            )
            || matches!(
                implementation,
                ParamTypeHint::Nullable(implementation_inner)
                    if class_constant_type_is_covariant(implementation_inner, inner, class_is_a)
            );
    }
    match (implementation, inherited) {
        (ParamTypeHint::Array, ParamTypeHint::ClassName(name))
            if name.eq_ignore_ascii_case("iterable") =>
        {
            true
        }
        (ParamTypeHint::ClassName(_), ParamTypeHint::ClassName(name))
            if name.eq_ignore_ascii_case("object") =>
        {
            true
        }
        (ParamTypeHint::ClassName(left), ParamTypeHint::ClassName(right)) => {
            left.eq_ignore_ascii_case(right) || class_is_a(left, right)
        }
        _ => false,
    }
}

fn visibility_rank(visibility: Visibility) -> u8 {
    match visibility {
        Visibility::Private => 0,
        Visibility::Protected => 1,
        Visibility::Public => 2,
    }
}

fn class_constant_declaration_location(source_file: Option<&str>, line: usize) -> String {
    source_file.map_or_else(String::new, |file| format!(" in {file} on line {line}"))
}

fn merge_parent_constant_definitions(
    owner: &str,
    target: &mut Vec<ClassConstantDefinition>,
    parent: &[ClassConstantDefinition],
    source_file: Option<&str>,
    declaration_line: usize,
    class_is_a: &dyn Fn(&str, &str) -> bool,
) -> Result<(), String> {
    let location = class_constant_declaration_location(source_file, declaration_line);
    for inherited in parent {
        if inherited.visibility == Visibility::Private {
            continue;
        }
        if let Some(existing) = target
            .iter()
            .find(|constant| constant.name == inherited.name)
        {
            if inherited.is_final {
                return Err(format!(
                    "{}::{} cannot override final constant {}::{}{}",
                    owner, existing.name, inherited.declaring_class, inherited.name, location
                ));
            }
            if visibility_rank(existing.visibility) < visibility_rank(inherited.visibility) {
                return Err(format!(
                    "Access level to constant {}::{} must be {:?} or weaker{}",
                    owner, existing.name, inherited.visibility, location
                ));
            }
            if !class_constant_type_is_covariant(
                &existing.type_hint,
                &inherited.type_hint,
                class_is_a,
            ) {
                return Err(format!(
                    "Type of {}::{} must be compatible with {}::{} of type {}{}",
                    owner,
                    existing.name,
                    inherited.declaring_class,
                    inherited.name,
                    inherited.type_hint.display_name(),
                    location
                ));
            }
            continue;
        }
        target.push(inherited.clone());
    }
    Ok(())
}

fn merge_trait_constant_definitions(
    owner: &str,
    trait_name: &str,
    target: &mut Vec<ClassConstantDefinition>,
    trait_constants: &[ClassConstantDefinition],
    origins: &mut std::collections::HashMap<String, String>,
    source_file: Option<&str>,
    declaration_line: usize,
) -> Result<(), String> {
    let location = class_constant_declaration_location(source_file, declaration_line);
    for source in trait_constants {
        let mut composed = source.clone();
        composed.declaring_class = owner.to_string();
        if let Some(position) = target
            .iter()
            .position(|constant| constant.name == composed.name)
        {
            let existing = &target[position];
            if existing.declaring_class != owner {
                if existing.is_final {
                    return Err(format!(
                        "{}::{} cannot override final constant {}::{}{}",
                        owner,
                        composed.name,
                        existing.declaring_class,
                        existing.name,
                        location
                    ));
                }
                origins.insert(composed.name.clone(), trait_name.to_string());
                target[position] = composed;
            } else if !constant_definitions_compatible(existing, &composed) {
                let existing_owner = origins
                    .get(&composed.name)
                    .map_or(owner, String::as_str);
                return Err(format!(
                    "{} and {} define the same constant ({}) in the composition of {}. However, the definition differs and is considered incompatible. Class was composed{}",
                    existing_owner, trait_name, composed.name, owner, location
                ));
            }
        } else {
            origins.insert(composed.name.clone(), trait_name.to_string());
            target.push(composed);
        }
    }
    Ok(())
}

fn merge_interface_constant_definitions(
    owner: &str,
    target: &mut Vec<ClassConstantDefinition>,
    interface_constants: &[ClassConstantDefinition],
    source_file: Option<&str>,
    declaration_line: usize,
    class_is_a: &dyn Fn(&str, &str) -> bool,
) -> Result<(), String> {
    let location = class_constant_declaration_location(source_file, declaration_line);
    for inherited in interface_constants {
        if let Some(existing) = target
            .iter()
            .find(|constant| constant.name == inherited.name)
        {
            if existing.declaring_class == owner {
                if inherited.is_final {
                    return Err(format!(
                        "{}::{} cannot override final constant {}::{}{}",
                        owner,
                        existing.name,
                        inherited.declaring_class,
                        inherited.name,
                        location
                    ));
                }
                if visibility_rank(existing.visibility) < visibility_rank(inherited.visibility) {
                    return Err(format!(
                        "Access level to constant {}::{} must be {:?} or weaker{}",
                        owner, existing.name, inherited.visibility, location
                    ));
                }
                if !class_constant_type_is_covariant(
                    &existing.type_hint,
                    &inherited.type_hint,
                    class_is_a,
                ) {
                    return Err(format!(
                        "Type of {}::{} must be compatible with {}::{} of type {}{}",
                        owner,
                        existing.name,
                        inherited.declaring_class,
                        inherited.name,
                        inherited.type_hint.display_name(),
                        location
                    ));
                }
            } else if existing.declaring_class != inherited.declaring_class {
                return Err(format!(
                    "Class {} inherits both {}::{} and {}::{}, which is ambiguous{}",
                    owner,
                    existing.declaring_class,
                    existing.name,
                    inherited.declaring_class,
                    inherited.name,
                    location
                ));
            }
            continue;
        }
        target.push(inherited.clone());
    }
    Ok(())
}
