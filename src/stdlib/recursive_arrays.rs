//! Recursive PHP array combiners.
//!
//! These functions deliberately stay off the ordinary `array_merge()` path.
//! PHP recursive combiners have distinct numeric-key, reference-unwrapping,
//! object-projection, recursion and next-index-overflow contracts.

use std::collections::HashSet;

use crate::runtime::ExecutorGlobals;
use crate::value::{
    ArrayKey, PhpArray, Value, ValueType, normalize_array_key_for_external_storage,
};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CombineError {
    Recursion,
    NextIndexOccupied,
}

#[derive(Default)]
struct ActiveBranches {
    left: HashSet<usize>,
    right: HashSet<usize>,
}

fn array_value<'a>(array: &'a PhpArray, key: &ArrayKey) -> Option<&'a Value> {
    match key {
        ArrayKey::Int(key) => array.get_int(*key),
        ArrayKey::String(key) => array.get_str(key),
    }
}

fn set_array_value(array: &mut PhpArray, key: ArrayKey, value: Value) {
    match key {
        ArrayKey::Int(key) => array.set_int(key, value),
        ArrayKey::String(key) => array.set_str(&key, value),
    }
}

fn copy_array_key_provenance(source: &PhpArray, result: &PhpArray) {
    if source.has_external_byte_keys() {
        result.mark_external_byte_keys();
    } else if source.has_utf8_text_keys() {
        result.mark_utf8_text_keys();
    }
}

fn absorb_array_key_provenance(
    destination: &mut PhpArray,
    source_external_byte_keys: bool,
    source_utf8_text_keys: bool,
) {
    if source_external_byte_keys {
        destination.promote_keys_to_external_storage();
    } else if source_utf8_text_keys && !destination.has_external_byte_keys() {
        destination.mark_utf8_text_keys();
    }
}

fn normalize_combined_key(
    destination: &PhpArray,
    key: ArrayKey,
    source_external_byte_keys: bool,
) -> ArrayKey {
    if destination.has_external_byte_keys() {
        normalize_array_key_for_external_storage(key, source_external_byte_keys)
    } else {
        debug_assert!(!source_external_byte_keys);
        key
    }
}

/// Clone a value into an array-function result. A reference with another PHP-
/// visible owner remains a reference; a lone wrapper is observably unwrapped.
fn clone_entry_value(value: &Value) -> Value {
    if value.is_owned_reference() && value.owned_reference_is_aliased() {
        value.clone_owned_reference_alias()
    } else {
        value.clone()
    }
}

/// Detach every nested array in the completed result and apply the same
/// reference rule at every depth. Aliased references are leaves: following
/// one could enter a source cycle, and PHP keeps that alias pointed at the
/// source cell rather than rebuilding the cycle in the returned array.
fn snapshot_value(value: &Value) -> Value {
    if value.is_owned_reference() && value.owned_reference_is_aliased() {
        return value.clone_owned_reference_alias();
    }
    let value = value.dereferenced();
    let Some(array) = value.as_array() else {
        return value.clone();
    };

    let mut result = PhpArray::new();
    for (key, value) in array.iter() {
        set_array_value(&mut result, key, snapshot_value(value));
    }
    copy_array_key_provenance(array, &result);
    Value::array(result)
}

fn shallow_array_copy(array: &PhpArray) -> Value {
    let mut result = PhpArray::new();
    for (key, value) in array.iter() {
        set_array_value(&mut result, key, clone_entry_value(value));
    }
    copy_array_key_provenance(array, &result);
    Value::array(result)
}

/// PHP merge collisions treat arrays normally, project objects to their
/// property arrays and wrap every other value (including null) as element 0.
fn merge_projection(value: &Value, eg: &ExecutorGlobals) -> (Value, Option<usize>) {
    let value = value.dereferenced();
    match value.value_type() {
        ValueType::Array => (value.clone(), value.array_identity()),
        ValueType::Object => (
            crate::vm::execute::cast_object_to_array(value, eg),
            value.object_identity(),
        ),
        ValueType::Closure => (
            Value::array(PhpArray::new()),
            value.cycle_node().map(|node| node.0),
        ),
        _ => {
            let mut array = PhpArray::new();
            array.push(value.clone());
            (Value::array(array), None)
        }
    }
}

fn enter_branch(active: &mut HashSet<usize>, identity: Option<usize>) -> Result<(), CombineError> {
    if let Some(identity) = identity
        && !active.insert(identity)
    {
        return Err(CombineError::Recursion);
    }
    Ok(())
}

fn leave_branch(active: &mut HashSet<usize>, identity: Option<usize>) {
    if let Some(identity) = identity {
        active.remove(&identity);
    }
}

fn merge_collision(
    left: &Value,
    right: &Value,
    eg: &ExecutorGlobals,
    active: &mut ActiveBranches,
) -> Result<Value, CombineError> {
    let (mut left, left_identity) = merge_projection(left, eg);
    let (right, right_identity) = merge_projection(right, eg);

    enter_branch(&mut active.left, left_identity)?;
    if let Err(error) = enter_branch(&mut active.right, right_identity) {
        leave_branch(&mut active.left, left_identity);
        return Err(error);
    }

    let result = merge_array_into(&mut left, &right, eg, active);
    leave_branch(&mut active.right, right_identity);
    leave_branch(&mut active.left, left_identity);
    result?;
    Ok(left)
}

fn merge_array_into(
    destination: &mut Value,
    source: &Value,
    eg: &ExecutorGlobals,
    active: &mut ActiveBranches,
) -> Result<(), CombineError> {
    let source_array = source
        .as_array()
        .expect("merge projection is always an array");
    let source_external_byte_keys = source_array.has_external_byte_keys();
    let source_utf8_text_keys = source_array.has_utf8_text_keys();
    let source_entries: Vec<(ArrayKey, Value)> = source_array
        .iter()
        .map(|(key, value)| (key, clone_entry_value(value)))
        .collect();
    let destination = destination
        .as_array_mut()
        .expect("merge destination is always an array");
    absorb_array_key_provenance(
        destination,
        source_external_byte_keys,
        source_utf8_text_keys,
    );

    for (key, incoming) in source_entries {
        let key = normalize_combined_key(destination, key, source_external_byte_keys);
        match key {
            ArrayKey::Int(_) => {
                if !destination.try_push(incoming) {
                    return Err(CombineError::NextIndexOccupied);
                }
            }
            ArrayKey::String(key) => {
                let existing = destination.get_str(&key).map(clone_entry_value);
                let value = if let Some(existing) = existing {
                    merge_collision(&existing, &incoming, eg, active)?
                } else {
                    incoming
                };
                destination.set_str(&key, value);
            }
        }
    }
    Ok(())
}

fn replace_array_into(
    destination: &mut Value,
    source: &Value,
    recursive: bool,
    active: &mut ActiveBranches,
) -> Result<(), CombineError> {
    let source_array = source
        .dereferenced()
        .as_array()
        .expect("validated replacement is an array");
    let source_external_byte_keys = source_array.has_external_byte_keys();
    let source_utf8_text_keys = source_array.has_utf8_text_keys();
    let source_entries: Vec<(ArrayKey, Value)> = source_array
        .iter()
        .map(|(key, value)| (key, clone_entry_value(value)))
        .collect();
    let destination_array = destination
        .as_array_mut()
        .expect("validated destination is an array");
    absorb_array_key_provenance(
        destination_array,
        source_external_byte_keys,
        source_utf8_text_keys,
    );

    for (key, incoming) in source_entries {
        let key = normalize_combined_key(destination_array, key, source_external_byte_keys);
        let existing = recursive
            .then(|| array_value(destination_array, &key))
            .flatten()
            .filter(|value| value.dereferenced().as_array().is_some())
            .map(clone_entry_value);
        let incoming_array = incoming.dereferenced().as_array().is_some();

        let value = if let Some(existing) = existing.filter(|_| incoming_array) {
            let left_identity = existing.dereferenced().array_identity();
            let right_identity = incoming.dereferenced().array_identity();
            let mut existing = existing.dereferenced().clone();
            enter_branch(&mut active.left, left_identity)?;
            if let Err(error) = enter_branch(&mut active.right, right_identity) {
                leave_branch(&mut active.left, left_identity);
                return Err(error);
            }
            let result = replace_array_into(&mut existing, incoming.dereferenced(), true, active);
            leave_branch(&mut active.right, right_identity);
            leave_branch(&mut active.left, left_identity);
            result?;
            existing
        } else {
            incoming
        };
        set_array_value(destination_array, key, value);
    }
    Ok(())
}

fn type_error(eg: &mut ExecutorGlobals, function: &str, position: usize, value: &Value) {
    let parameter = if position == 1 && function.starts_with("array_replace") {
        " ($array)"
    } else {
        ""
    };
    let value = value.dereferenced();
    let actual = match value.value_type() {
        ValueType::True => "true".into(),
        ValueType::False => "false".into(),
        _ => value.diagnostic_type_name(),
    };
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "{function}(): Argument #{position}{parameter} must be of type array, {} given",
            actual
        ),
    ));
}

fn combine_error(eg: &mut ExecutorGlobals, error: CombineError) {
    let message = match error {
        CombineError::Recursion => "Recursion detected",
        CombineError::NextIndexOccupied => {
            "Cannot add element to the array as the next element is already occupied"
        }
    };
    eg.exception = Some(crate::value::make_error_value("Error", message));
}

fn variadic_values(value: &Value) -> impl Iterator<Item = &Value> {
    value
        .as_array()
        .into_iter()
        .flat_map(|array| array.values())
}

#[cold]
pub(super) fn fn_array_merge_recursive(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arguments = super::owned_argument(execute_data, 0);
    let values: Vec<Value> = variadic_values(&arguments).map(clone_entry_value).collect();
    for (index, value) in values.iter().enumerate() {
        if value.dereferenced().as_array().is_none() {
            type_error(eg, "array_merge_recursive", index + 1, value);
            return Ok(());
        }
    }

    let mut result = Value::array(PhpArray::new());
    let mut active = ActiveBranches::default();
    for value in &values {
        if let Err(error) = merge_array_into(&mut result, value.dereferenced(), eg, &mut active) {
            combine_error(eg, error);
            return Ok(());
        }
    }
    super::write_return_value(return_pointer, snapshot_value(&result));
    Ok(())
}

fn array_replace_recursive_impl(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let first = super::owned_argument(execute_data, 0);
    if first.dereferenced().as_array().is_none() {
        type_error(eg, "array_replace_recursive", 1, &first);
        return Ok(());
    }

    let rest = super::owned_argument(execute_data, 1);
    let replacements: Vec<Value> = variadic_values(&rest).map(clone_entry_value).collect();
    for (index, value) in replacements.iter().enumerate() {
        if value.dereferenced().as_array().is_none() {
            type_error(eg, "array_replace_recursive", index + 2, value);
            return Ok(());
        }
    }

    let mut result = shallow_array_copy(first.dereferenced().as_array().unwrap());
    let mut active = ActiveBranches::default();
    for replacement in &replacements {
        if let Err(error) =
            replace_array_into(&mut result, replacement.dereferenced(), true, &mut active)
        {
            combine_error(eg, error);
            return Ok(());
        }
    }
    super::write_return_value(return_pointer, snapshot_value(&result));
    Ok(())
}

#[cold]
pub(super) fn fn_array_replace_recursive(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    array_replace_recursive_impl(execute_data, return_pointer, eg)
}
