// Class-like constant composition and inheritance contracts.
//
// Kept outside the executor registry so registration orchestration remains
// readable while these cold PHP compatibility rules can evolve independently.

fn constant_definitions_compatible(
    left: &ClassConstantDefinition,
    right: &ClassConstantDefinition,
    eg: &mut ExecutorGlobals,
) -> Result<bool, String> {
    if left.visibility != right.visibility
        || left.type_hint != right.type_hint
        || left.is_final != right.is_final
        || left.evaluation_error != right.evaluation_error
    {
        return Ok(false);
    }
    // A deferred initializer's placeholder is not its PHP value. Compare
    // only colliding declarations, after metadata, and do not apply the
    // eventual typed-constant coercion before this strict value comparison.
    // Resolve the incoming trait declaration first, including autoload effects.
    let Some(right) = crate::stdlib::reflection::evaluate_class_constant_comparison_value(right, eg)
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };
    let Some(left) = crate::stdlib::reflection::evaluate_class_constant_comparison_value(left, eg)
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };
    Ok(left.structurally_equal(&right))
}

fn rebind_trait_constant(
    source: &ClassConstantDefinition,
    owner: &str,
    parent: Option<&str>,
) -> ClassConstantDefinition {
    let mut composed = source.clone();
    composed.declaring_class = owner.to_string();
    if let Some(scope) = composed.evaluation_scope.as_mut() {
        let scope = std::rc::Rc::make_mut(scope);
        scope.lexical_class = Some(owner.to_string());
        scope.lexical_parent = parent.map(str::to_owned);
    }
    composed
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
    parent: Option<&str>,
    trait_name: &str,
    target: &mut Vec<ClassConstantDefinition>,
    trait_constants: &[ClassConstantDefinition],
    origins: &mut std::collections::HashMap<String, String>,
    source_file: Option<&str>,
    declaration_line: usize,
    eg: &mut ExecutorGlobals,
) -> Result<(), String> {
    let location = class_constant_declaration_location(source_file, declaration_line);
    for source in trait_constants {
        let composed = rebind_trait_constant(source, owner, parent);
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
            } else {
                // Only the already composed prefix is visible. Later constants
                // cannot satisfy dependencies in the current comparison.
                eg.refresh_linking_class_constants(owner, target);
                if !constant_definitions_compatible(existing, &composed, eg)? {
                    let existing_owner = origins
                        .get(&composed.name)
                        .map_or(owner, String::as_str);
                    return Err(format!(
                        "{} and {} define the same constant ({}) in the composition of {}. However, the definition differs and is considered incompatible. Class was composed{}",
                        existing_owner, trait_name, composed.name, owner, location
                    ));
                }
            }
        } else {
            origins.insert(composed.name.clone(), trait_name.to_string());
            target.push(composed);
        }
    }
    Ok(())
}

/// An expression-only view of a composing class. This is never inserted into
/// the public class table: autoload callbacks must still see the class as
/// unavailable until its complete link succeeds.
struct LinkingClassConstants {
    definitions: Vec<ClassConstantDefinition>,
    evaluating: Vec<String>,
}

impl ExecutorGlobals {
    fn install_linking_class_constants(
        &mut self,
        class: &ClassDef,
    ) -> (bool, Option<Box<LinkingClassConstants>>) {
        let key = class.name.to_ascii_lowercase();
        let inserted = !self.active_runtime_class_relations.contains_key(&key);
        let relation = self.active_runtime_class_relations.entry(key)
            .or_insert_with(|| ActiveRuntimeClassRelation::from_class(class));
        let previous = relation.constant_scope.replace(Box::new(LinkingClassConstants {
            definitions: Vec::new(),
            evaluating: Vec::new(),
        }));
        (inserted, previous)
    }

    fn refresh_linking_class_constants(&mut self, owner: &str, definitions: &[ClassConstantDefinition]) {
        if let Some(scope) = self.active_runtime_class_relations.get_mut(&owner.to_ascii_lowercase())
            .and_then(|relation| relation.constant_scope.as_mut()) {
            scope.definitions.clear();
            scope.definitions.extend_from_slice(definitions);
        }
    }

    fn restore_linking_class_constants(
        &mut self,
        owner: &str,
        saved: (bool, Option<Box<LinkingClassConstants>>),
    ) {
        let key = owner.to_ascii_lowercase();
        if saved.0 {
            self.active_runtime_class_relations.remove(&key);
        } else if let Some(relation) = self.active_runtime_class_relations.get_mut(&key) {
            relation.constant_scope = saved.1;
        }
    }

    pub(crate) fn linking_class_constant(
        &self,
        owner: &str,
        name: &str,
    ) -> Option<Option<ClassConstantDefinition>> {
        let scope = self.active_runtime_class_relations.get(&owner.to_ascii_lowercase())?
            .constant_scope.as_ref()?;
        Some(scope.definitions.iter().find(|definition| definition.name == name).cloned())
    }

    pub(crate) fn enter_linking_constant(&mut self, owner: &str, name: &str) -> bool {
        let Some(scope) = self.active_runtime_class_relations.get_mut(&owner.to_ascii_lowercase())
            .and_then(|relation| relation.constant_scope.as_mut()) else { return false };
        if scope.evaluating.iter().any(|active| active == name) { return false; }
        scope.evaluating.push(name.to_string());
        true
    }

    pub(crate) fn leave_linking_constant(&mut self, owner: &str) {
        if let Some(scope) = self.active_runtime_class_relations.get_mut(&owner.to_ascii_lowercase())
            .and_then(|relation| relation.constant_scope.as_mut()) {
            scope.evaluating.pop();
        }
    }
}

fn merge_interface_constant_definitions(
    owner_kind: &str,
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
                        "Access level to {}::{} must be public (as in interface {}){}",
                        owner, existing.name, inherited.declaring_class, location
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
                    "{} {} inherits both {}::{} and {}::{}, which is ambiguous{}",
                    owner_kind,
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
