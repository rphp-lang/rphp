//! Generic ancestor Reflection handlers.
//!
//! These façades validate PHP Reflection targets and translate the canonical
//! interned inheritance graph into lists of `ReflectionType` values. Keeping
//! the target/error policy here leaves `generics/reflection.rs` responsible
//! only for graph traversal and effective bindings.

use super::generic_parameters::reflected_type;
use super::{argument_string, generic_target, reflection_exception, return_value};
use crate::generics::{
    GenericDeclaration, GenericDeclarationKind, GenericInheritanceKind, GenericMetadata,
    GenericReflectionBinding,
};
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

fn reflected_class_owner(ed: *mut ExecuteData) -> Option<String> {
    generic_target(ed)
        .and_then(|(kind, owner)| (kind == GenericDeclarationKind::Class).then_some(owner))
}

pub(super) fn reflected_arguments(
    metadata: &GenericMetadata,
    declaration: Option<&GenericDeclaration>,
    binding: &GenericReflectionBinding,
) -> PhpArray {
    let Some(declaration) = declaration else {
        return PhpArray::new();
    };
    let mut arguments = PhpArray::with_packed_capacity(binding.arguments.len());
    for argument in binding.arguments.iter() {
        arguments.push(reflected_type(metadata, declaration, argument));
    }
    arguments
}

pub(super) fn generic_arguments_for_parent_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(owner) = reflected_class_owner(ed) else {
        reflection_exception(eg, "Reflection target is not a class");
        return Ok(());
    };
    if eg.class_is_interface(&owner) {
        reflection_exception(eg, format!("Interface {owner} has no parent class"));
        return Ok(());
    }
    let Some(binding) = eg.generic_metadata.reflection_direct_binding(
        &owner,
        GenericInheritanceKind::Extends,
        None,
    ) else {
        reflection_exception(eg, format!("Class {owner} has no parent class"));
        return Ok(());
    };
    let context = eg
        .generic_metadata
        .find_class_like_index(&owner)
        .and_then(|index| eg.generic_metadata.declarations().get(index as usize));
    let arguments = reflected_arguments(&eg.generic_metadata, context, &binding);
    return_value(rv, Value::array(arguments))
}

pub(super) fn generic_arguments_for_parent_interface(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(owner) = reflected_class_owner(ed) else {
        reflection_exception(eg, "Reflection target is not a class or interface");
        return Ok(());
    };
    let ancestor = argument_string(ed, 1);
    if owner.eq_ignore_ascii_case(&ancestor)
        || !eg.class_is_interface(&ancestor)
        || !eg.class_is_a(&owner, &ancestor)
    {
        reflection_exception(
            eg,
            format!("Interface {ancestor} is not an ancestor interface of {owner}"),
        );
        return Ok(());
    }
    let bindings = eg
        .generic_metadata
        .reflection_interface_bindings(&owner, &ancestor);
    let context = eg
        .generic_metadata
        .find_class_like_index(&owner)
        .and_then(|index| eg.generic_metadata.declarations().get(index as usize));
    let mut result = PhpArray::with_packed_capacity(bindings.len());
    for binding in bindings.iter() {
        result.push(Value::array(reflected_arguments(
            &eg.generic_metadata,
            context,
            binding,
        )));
    }
    return_value(rv, Value::array(result))
}

pub(super) fn generic_arguments_for_used_trait(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(owner) = reflected_class_owner(ed) else {
        reflection_exception(eg, "Reflection target is not a class");
        return Ok(());
    };
    let trait_name = argument_string(ed, 1);
    let Some(binding) = eg.generic_metadata.reflection_direct_binding(
        &owner,
        GenericInheritanceKind::Uses,
        Some(&trait_name),
    ) else {
        reflection_exception(
            eg,
            format!("Trait {trait_name} is not directly used by {owner}"),
        );
        return Ok(());
    };
    let context = eg
        .generic_metadata
        .find_class_like_index(&owner)
        .and_then(|index| eg.generic_metadata.declarations().get(index as usize));
    let arguments = reflected_arguments(&eg.generic_metadata, context, &binding);
    return_value(rv, Value::array(arguments))
}
