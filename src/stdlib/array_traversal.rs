//! PHP 8.5 array traversal and predicate functions.
//!
//! The callback-bearing functions share one snapshot traversal contract:
//! structural keys and element handles are captured at call entry, referenced
//! elements remain live until visited, and callbacks receive dereferenced
//! value/key pairs in insertion order. Short-circuiting is observable through
//! callback side effects and exceptions, so it stays in this baseline path.

use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

#[derive(Clone, Copy)]
enum TraversalResult {
    Value,
    Key,
    Any,
    All,
}

fn key_value(key: &ArrayKey) -> Value {
    match key {
        ArrayKey::Int(value) => Value::long(*value),
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

fn ordinary_callback_invalid_reason(callback: &Value, eg: &ExecutorGlobals) -> String {
    if let Some(name) = callback.as_str() {
        let Some((class, method)) = name.rsplit_once("::") else {
            return format!("function \"{name}\" not found or invalid function name");
        };
        return class_method_invalid_reason(eg, class.trim_start_matches('\\'), method, false);
    }

    let Some(array) = callback.as_array() else {
        return "no array or string given".to_string();
    };
    if array.len() != 2 {
        return "array callback must have exactly two members".to_string();
    }
    let Some(first) = array.get_value_at(0) else {
        return "first array member is not a valid class name or object".to_string();
    };
    let Some(method) = array.get_value_at(1).and_then(Value::as_str) else {
        return "second array member is not a valid method".to_string();
    };
    if let Some(class) = first.as_str() {
        return class_method_invalid_reason(eg, class.trim_start_matches('\\'), method, false);
    }
    if let Some(object) = first.as_object() {
        return class_method_invalid_reason(eg, &object.class_name, method, true);
    }
    if first.value_type() == ValueType::Closure {
        return class_method_invalid_reason(eg, "Closure", method, true);
    }
    "first array member is not a valid class name or object".to_string()
}

fn class_method_invalid_reason(
    eg: &ExecutorGlobals,
    class: &str,
    method: &str,
    object_form: bool,
) -> String {
    let Some(class_definition) = super::find_class_case_insensitive(eg, class) else {
        return format!("class \"{class}\" not found");
    };
    let canonical = class_definition.name.clone();
    let Some((visibility, is_static, _, declaring)) =
        super::find_method_in_class_hierarchy(eg, &canonical, method)
    else {
        return format!("class {canonical} does not have a method \"{method}\"");
    };
    if visibility != Visibility::Public {
        let visibility = match visibility {
            Visibility::Private => "private",
            Visibility::Protected => "protected",
            Visibility::Public => unreachable!(),
        };
        return format!("cannot access {visibility} method {declaring}::{method}()");
    }
    if !object_form && !is_static {
        return format!("non-static method {declaring}::{method}() cannot be called statically");
    }
    "no array or string given".to_string()
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
            let reason = ordinary_callback_invalid_reason(&callback, eg);
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "{function}(): Argument #2 ($callback) must be a valid callback, {reason}"
                ),
            ));
        }
        return Ok(());
    };

    let entries = traversal_entries(array.as_array().expect("validated array argument"));
    for (key, value) in entries {
        let callback_value = value.dereferenced().clone();
        let accepted =
            super::call_resolved_with_values(eg, &resolved, &[callback_value, key_value(&key)])?;
        if eg.exception.is_some() {
            return Ok(());
        }
        if accepted.is_truthy() {
            let value = match result {
                TraversalResult::Value => value.dereferenced().clone(),
                TraversalResult::Key => key_value(&key),
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
