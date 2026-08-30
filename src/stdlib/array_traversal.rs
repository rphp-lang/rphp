//! PHP 8.5 array traversal and predicate functions.
//!
//! The callback-bearing functions share one snapshot traversal contract:
//! structural keys and element handles are captured at call entry, referenced
//! elements remain live until visited, and callbacks receive dereferenced
//! value/key pairs in insertion order. Short-circuiting is observable through
//! callback side effects and exceptions, so it stays in this baseline path.

use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

#[derive(Clone, Copy)]
enum TraversalResult {
    Value,
    Key,
    Any,
    All,
}

fn key_value(key: &ArrayKey, external_byte_keys: bool) -> Value {
    match key {
        ArrayKey::Int(value) => Value::long(*value),
        ArrayKey::String(value) if external_byte_keys => {
            Value::binary_string_from_storage(value.clone())
        }
        ArrayKey::String(value) => Value::string(value.clone()),
    }
}

fn array_argument(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Option<Value> {
    let value = super::owned_argument(execute_data, 0);
    if value.as_array().is_some() {
        return Some(value);
    }
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "{function}(): Argument #1 ($array) must be of type array, {} given",
            value.diagnostic_type_name()
        ),
    ));
    None
}

fn traversal_entries(array: &PhpArray) -> Vec<(ArrayKey, Value)> {
    array
        .iter()
        .map(|(key, value)| (key, value.clone()))
        .collect()
}

fn traverse(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    result: TraversalResult,
) -> Result<(), VmError> {
    let Some(array) = array_argument(execute_data, eg, function) else {
        return Ok(());
    };
    let callback = super::owned_argument(execute_data, 1);
    let Some(resolved) = super::resolve_callback_at_callsite_checked(&callback, eg, execute_data)?
    else {
        if eg.exception.is_none() {
            let reason = super::ordinary_callback_invalid_reason(&callback, eg);
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "{function}(): Argument #2 ($callback) must be a valid callback, {reason}"
                ),
            ));
        }
        return Ok(());
    };

    let array = array.as_array().expect("validated array argument");
    let external_byte_keys = array.has_external_byte_keys();
    let entries = traversal_entries(array);
    for (key, value) in entries {
        let callback_value = value.dereferenced().clone();
        let accepted = super::call_resolved_with_values(
            eg,
            &resolved,
            &[callback_value, key_value(&key, external_byte_keys)],
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        if accepted.is_truthy() {
            let value = match result {
                TraversalResult::Value => value.dereferenced().clone(),
                TraversalResult::Key => key_value(&key, external_byte_keys),
                TraversalResult::Any => Value::bool(true),
                TraversalResult::All => continue,
            };
            super::write_return_value(return_pointer, value);
            return Ok(());
        }
        if matches!(result, TraversalResult::All) {
            super::write_return_value(return_pointer, Value::bool(false));
            return Ok(());
        }
    }

    let value = match result {
        TraversalResult::Any => Value::bool(false),
        TraversalResult::All => Value::bool(true),
        TraversalResult::Value | TraversalResult::Key => Value::null(),
    };
    super::write_return_value(return_pointer, value);
    Ok(())
}

#[cold]
pub(super) fn fn_array_find(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    traverse(
        execute_data,
        return_pointer,
        eg,
        "array_find",
        TraversalResult::Value,
    )
}

#[cold]
pub(super) fn fn_array_find_key(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    traverse(
        execute_data,
        return_pointer,
        eg,
        "array_find_key",
        TraversalResult::Key,
    )
}

#[cold]
pub(super) fn fn_array_any(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    traverse(
        execute_data,
        return_pointer,
        eg,
        "array_any",
        TraversalResult::Any,
    )
}

#[cold]
pub(super) fn fn_array_all(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    traverse(
        execute_data,
        return_pointer,
        eg,
        "array_all",
        TraversalResult::All,
    )
}

fn first_or_last(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    last: bool,
) -> Result<(), VmError> {
    let Some(array) = array_argument(execute_data, eg, function) else {
        return Ok(());
    };
    let array = array.as_array().expect("validated array argument");
    let value = if last {
        array.values().next_back()
    } else {
        array.values().next()
    }
    .map(|value| value.dereferenced().clone())
    .unwrap_or_else(Value::null);
    super::write_return_value(return_pointer, value);
    Ok(())
}

#[cold]
pub(super) fn fn_array_first(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    first_or_last(execute_data, return_pointer, eg, "array_first", false)
}

#[cold]
pub(super) fn fn_array_last(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    first_or_last(execute_data, return_pointer, eg, "array_last", true)
}
