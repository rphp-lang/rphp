//! Request-local default Stream Context handlers.

use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::super::checked_args::{argument_error, given_type_name};
use super::super::{argument, optional_argument, return_value};
use super::{StreamContext, context_snapshot, empty_context, invalid_options_error};

const DEFAULT_CONTEXT_STATE: &str = "\0rphp-stream-context-default";
const DEFAULT_CONTEXT_VALUE: &str = "resource";

#[cold]
pub(in crate::stdlib::streams) fn fn_stream_context_get_default(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let options = match nullable_options_argument(execute_data, eg) {
        Ok(options) => options,
        Err(()) => return Ok(()),
    };
    default_context_with_options(return_pointer, eg, options)
}

#[cold]
pub(in crate::stdlib::streams) fn fn_stream_context_set_default(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = argument(execute_data, 0);
    let Some(options) = value.as_array() else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "stream_context_set_default(): Argument #1 ($options) must be of type array, {} given",
                given_type_name(value)
            ),
        );
        return Ok(());
    };
    default_context_with_options(return_pointer, eg, Some(options))
}

fn nullable_options_argument<'a>(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<Option<&'a PhpArray>, ()> {
    let Some(value) = optional_argument(execute_data, 0) else {
        return Ok(None);
    };
    if value.value_type() == ValueType::Null {
        return Ok(None);
    }
    let Some(options) = value.as_array() else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "stream_context_get_default(): Argument #1 ($options) must be of type ?array, {} given",
                given_type_name(value)
            ),
        );
        return Err(());
    };
    Ok(Some(options))
}

fn default_context_with_options(
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    options: Option<&PhpArray>,
) -> Result<(), VmError> {
    let (value, resource) = default_context(eg);
    if let Some(options) = options {
        let mut context = context_snapshot(eg, resource).unwrap_or_else(empty_context);
        let valid = apply_options_in_order(&mut context.options, options, eg);
        replace_context(eg, resource, context);
        if !valid {
            return Ok(());
        }
    }
    return_value(return_pointer, value)
}

/// PHP applies each valid outer entry before validating the next one. Keep
/// that observable partial-update policy local to the default-context API;
/// regular context constructors and mutators validate their full input first.
fn apply_options_in_order(
    stored: &mut PhpArray,
    updates: &PhpArray,
    eg: &mut ExecutorGlobals,
) -> bool {
    for (wrapper, value) in updates.iter() {
        let ArrayKey::String(wrapper) = wrapper else {
            invalid_options_error(eg);
            return false;
        };
        let Some(updates) = value.as_array() else {
            invalid_options_error(eg);
            return false;
        };
        let mut wrapper_options = stored
            .get_str(&wrapper)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(PhpArray::new);
        for (name, value) in updates.iter() {
            if let ArrayKey::String(name) = name {
                wrapper_options.set_str(&name, value.clone());
            }
        }
        if !wrapper_options.is_empty() {
            stored.set_str(&wrapper, Value::array(wrapper_options));
        }
    }
    true
}

fn default_context(eg: &mut ExecutorGlobals) -> (Value, i64) {
    let stored = eg
        .static_vars
        .get(DEFAULT_CONTEXT_STATE)
        .and_then(|state| state.get(DEFAULT_CONTEXT_VALUE))
        .cloned();
    if let Some(value) = stored
        && let Some(resource) = value.as_resource_id()
        && context_snapshot(eg, resource).is_some()
    {
        return (value, resource);
    }

    #[cfg(feature = "resource-lifetime")]
    let value = super::super::super::resource::insert_value_for_request(
        eg,
        "stream-context",
        empty_context(),
    );
    #[cfg(not(feature = "resource-lifetime"))]
    let value = Value::resource(super::super::super::resource::insert_for_request(
        eg,
        "stream-context",
        empty_context(),
    ));
    let resource = value
        .as_resource_id()
        .expect("new default stream context must be a resource");
    eg.static_vars
        .entry(DEFAULT_CONTEXT_STATE.to_string())
        .or_default()
        .insert(DEFAULT_CONTEXT_VALUE.to_string(), value.clone());
    (value, resource)
}

fn replace_context(eg: &mut ExecutorGlobals, resource: i64, context: StreamContext) {
    let _ = super::super::super::resource::with_request_payload_mut::<StreamContext, _>(
        eg,
        resource,
        |stored| *stored = context,
    );
}
