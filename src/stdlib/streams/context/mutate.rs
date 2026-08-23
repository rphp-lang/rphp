//! Mutable Stream Context handlers and their PHP-compatible argument policy.

use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::super::checked_args::{argument_error, given_type_name};
use super::super::{argument, optional_argument, return_value, with_stream};
use super::{
    StreamContext, apply_params, context_snapshot, empty_context, invalid_options_error,
    merge_options, normalize_options, set_option,
};

#[cold]
pub(in crate::stdlib::streams) fn fn_stream_context_set_option(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(owner) =
        stream_or_context_owner(execute_data, eg, "stream_context_set_option", "context")
    else {
        return Ok(());
    };
    if optional_argument(execute_data, 2).is_none() && optional_argument(execute_data, 3).is_none()
    {
        super::super::super::report_internal_deprecation(
            eg,
            execute_data,
            "Calling stream_context_set_option() with 2 arguments is deprecated, use stream_context_set_options() instead",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }

    let wrapper_or_options = argument(execute_data, 1);
    if let Some(options) = wrapper_or_options.as_array() {
        let option_name = match optional_weak_string_argument(
            execute_data,
            2,
            eg,
            "stream_context_set_option",
            3,
            "option_name",
        ) {
            Ok(option_name) => option_name,
            Err(()) => return Ok(()),
        };
        if option_name.is_some() {
            argument_error(
                eg,
                "ValueError",
                "stream_context_set_option(): Argument #3 ($option_name) must be null when argument #2 ($wrapper_or_options) is an array".to_string(),
            );
            return Ok(());
        }
        if optional_argument(execute_data, 3).is_some() {
            argument_error(
                eg,
                "ValueError",
                "stream_context_set_option(): Argument #4 ($value) cannot be provided when argument #2 ($wrapper_or_options) is an array".to_string(),
            );
            return Ok(());
        }
        let Some(options) = normalize_options(options) else {
            invalid_options_error(eg);
            return Ok(());
        };
        update_context(eg, owner, |context| {
            merge_options(&mut context.options, &options)
        });
        return return_value(return_pointer, Value::bool(true));
    }

    let Some(wrapper) = weak_string_value(wrapper_or_options) else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "stream_context_set_option(): Argument #2 ($wrapper_or_options) must be of type array|string, {} given",
                given_type_name(wrapper_or_options)
            ),
        );
        return Ok(());
    };
    let option_name = match optional_weak_string_argument(
        execute_data,
        2,
        eg,
        "stream_context_set_option",
        3,
        "option_name",
    ) {
        Ok(Some(option_name)) => option_name,
        Ok(None) => {
            argument_error(
                eg,
                "ValueError",
                "stream_context_set_option(): Argument #3 ($option_name) cannot be null when argument #2 ($wrapper_or_options) is a string".to_string(),
            );
            return Ok(());
        }
        Err(()) => return Ok(()),
    };
    let Some(value) = optional_argument(execute_data, 3) else {
        argument_error(
            eg,
            "ValueError",
            "stream_context_set_option(): Argument #4 ($value) must be provided when argument #2 ($wrapper_or_options) is a string".to_string(),
        );
        return Ok(());
    };
    update_context(eg, owner, |context| {
        set_option(&mut context.options, &wrapper, &option_name, value.clone())
    });
    return_value(return_pointer, Value::bool(true))
}

#[cold]
pub(in crate::stdlib::streams) fn fn_stream_context_set_options(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(owner) =
        stream_or_context_owner(execute_data, eg, "stream_context_set_options", "context")
    else {
        return Ok(());
    };
    let Some(options) =
        required_array_argument(execute_data, 1, eg, "stream_context_set_options", "options")
    else {
        return Ok(());
    };
    let Some(options) = normalize_options(options) else {
        invalid_options_error(eg);
        return Ok(());
    };
    update_context(eg, owner, |context| {
        merge_options(&mut context.options, &options)
    });
    return_value(return_pointer, Value::bool(true))
}

#[cold]
pub(in crate::stdlib::streams) fn fn_stream_context_set_params(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(owner) =
        stream_or_context_owner(execute_data, eg, "stream_context_set_params", "context")
    else {
        return Ok(());
    };
    let Some(params) =
        required_array_argument(execute_data, 1, eg, "stream_context_set_params", "params")
    else {
        return Ok(());
    };

    // Callback resolution may inspect executor state, so do not hold the
    // registry borrow across it. Publish even a failing clone because PHP
    // retains a valid notification update that precedes invalid options.
    let mut context = context_for_owner(eg, owner);
    if !apply_params(
        &mut context,
        params,
        eg,
        execute_data,
        "stream_context_set_params",
        "context",
    ) {
        update_context(eg, owner, |stored| *stored = context);
        return Ok(());
    }
    update_context(eg, owner, |stored| *stored = context);
    return_value(return_pointer, Value::bool(true))
}

fn required_array_argument<'a>(
    execute_data: *mut ExecuteData,
    index: u32,
    eg: &mut ExecutorGlobals,
    function: &str,
    name: &str,
) -> Option<&'a PhpArray> {
    let value = argument(execute_data, index);
    let Some(array) = value.as_array() else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "{function}(): Argument #{} (${name}) must be of type array, {} given",
                index + 1,
                given_type_name(value)
            ),
        );
        return None;
    };
    Some(array)
}

fn weak_string_value(value: &Value) -> Option<String> {
    match value.value_type() {
        ValueType::Null
        | ValueType::False
        | ValueType::True
        | ValueType::Long
        | ValueType::Double
        | ValueType::String => Some(value.echo_to_string()),
        _ => None,
    }
}

fn optional_weak_string_argument(
    execute_data: *mut ExecuteData,
    index: u32,
    eg: &mut ExecutorGlobals,
    function: &str,
    argument_number: u32,
    name: &str,
) -> Result<Option<String>, ()> {
    let Some(value) = optional_argument(execute_data, index) else {
        return Ok(None);
    };
    if value.value_type() == ValueType::Null {
        return Ok(None);
    }
    let Some(value_string) = weak_string_value(value) else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "{function}(): Argument #{argument_number} (${name}) must be of type ?string, {} given",
                given_type_name(value)
            ),
        );
        return Err(());
    };
    Ok(Some(value_string))
}

#[derive(Clone, Copy)]
enum ContextOwner {
    Resource(i64),
    Stream(i64),
}

fn stream_or_context_owner(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    argument_name: &str,
) -> Option<ContextOwner> {
    let value = argument(execute_data, 0);
    let Some(resource) = value.as_resource_id() else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "{function}(): Argument #1 (${argument_name}) must be of type resource, {} given",
                given_type_name(value)
            ),
        );
        return None;
    };
    if super::super::super::resource::with_request_payload_mut::<StreamContext, _>(
        eg,
        resource,
        |_| (),
    )
    .is_some()
    {
        return Some(ContextOwner::Resource(resource));
    }
    if with_stream(eg, resource, |_| ()).is_some() {
        return Some(ContextOwner::Stream(resource));
    }
    argument_error(
        eg,
        "TypeError",
        format!("{function}(): Argument #1 (${argument_name}) must be a valid stream/context"),
    );
    None
}

fn context_for_owner(eg: &mut ExecutorGlobals, owner: ContextOwner) -> StreamContext {
    match owner {
        ContextOwner::Resource(resource) => context_snapshot(eg, resource),
        ContextOwner::Stream(resource) => with_stream(eg, resource, |stream| {
            stream.context().cloned().unwrap_or_else(empty_context)
        }),
    }
    .unwrap_or_else(empty_context)
}

fn update_context(
    eg: &mut ExecutorGlobals,
    owner: ContextOwner,
    operation: impl FnOnce(&mut StreamContext),
) {
    match owner {
        ContextOwner::Resource(resource) => {
            let _ = super::super::super::resource::with_request_payload_mut::<StreamContext, _>(
                eg, resource, operation,
            );
        }
        ContextOwner::Stream(resource) => {
            let _ = with_stream(eg, resource, |stream| operation(stream.context_mut()));
        }
    }
}
