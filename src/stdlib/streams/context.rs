use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::super::stream::{PhpStream, StreamContext};
use super::checked_args::{argument_error, given_type_name};
use super::{
    argument, argument_string, insert_stream, optional_argument, return_value, with_stream,
};

mod default;
mod mutate;

pub(super) use default::{fn_stream_context_get_default, fn_stream_context_set_default};
pub(super) use mutate::{
    fn_stream_context_set_option, fn_stream_context_set_options, fn_stream_context_set_params,
};

#[cold]
pub(super) fn fn_stream_context_create(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let options = match nullable_array_argument(execute_data, 0, eg, "options") {
        Ok(Some(options)) => match normalize_options(options) {
            Some(options) => options,
            None => {
                argument_error(
                    eg,
                    "ValueError",
                    "Options should have the form [\"wrappername\"][\"optionname\"] = $value"
                        .to_string(),
                );
                return Ok(());
            }
        },
        Ok(None) => PhpArray::new(),
        Err(()) => return Ok(()),
    };
    let params = match nullable_array_argument(execute_data, 1, eg, "params") {
        Ok(params) => params,
        Err(()) => return Ok(()),
    };

    let mut context = StreamContext {
        options,
        params: PhpArray::new(),
    };
    if let Some(params) = params
        && !apply_params(
            &mut context,
            params,
            eg,
            execute_data,
            "stream_context_create",
            "options",
        )
    {
        return Ok(());
    }
    #[cfg(feature = "resource-lifetime")]
    let value = super::super::resource::insert_value_for_request(eg, "stream-context", context);
    #[cfg(not(feature = "resource-lifetime"))]
    let value = Value::resource(super::super::resource::insert_for_request(
        eg,
        "stream-context",
        context,
    ));
    return_value(return_pointer, value)
}

#[cold]
pub(super) fn fn_stream_context_get_options(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(context) = stream_or_context_argument(
        execute_data,
        eg,
        "stream_context_get_options",
        "stream_or_context",
    ) else {
        return Ok(());
    };
    return_value(return_pointer, Value::array(context.options))
}

#[cold]
pub(super) fn fn_stream_context_get_params(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(context) =
        stream_or_context_argument(execute_data, eg, "stream_context_get_params", "context")
    else {
        return Ok(());
    };
    let mut params = context.params;
    params.set_str("options", Value::array(context.options));
    return_value(return_pointer, Value::array(params))
}

#[cold]
pub(super) fn fn_fopen(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = argument_string(execute_data, 0);
    let mode = argument_string(execute_data, 1);
    if let Some(value) = optional_argument(execute_data, 2)
        && !matches!(
            value.value_type(),
            ValueType::Null
                | ValueType::False
                | ValueType::True
                | ValueType::Long
                | ValueType::Double
                | ValueType::String
        )
    {
        argument_error(
            eg,
            "TypeError",
            format!(
                "fopen(): Argument #3 ($use_include_path) must be of type bool, {} given",
                given_type_name(value)
            ),
        );
        return Ok(());
    }
    #[cfg(feature = "include-path")]
    let use_include_path = optional_argument(execute_data, 2).is_some_and(Value::is_truthy);
    let context_id = match optional_context_resource(execute_data, 3, eg, "fopen", 4) {
        Ok(context) => context,
        Err(()) => return Ok(()),
    };
    if let Some(context) = context_id
        && context_snapshot(eg, context).is_none()
    {
        invalid_context_error(eg, "fopen");
        return Ok(());
    }

    #[cfg(feature = "include-path")]
    let resolved_path =
        super::super::include_path::resolve_for_open(eg, path.as_ref(), use_include_path);
    #[cfg(feature = "include-path")]
    let open_path = resolved_path.as_str();
    #[cfg(not(feature = "include-path"))]
    let open_path = path.as_ref();
    #[cfg(feature = "stream-registry")]
    match super::user_wrapper::open_file(eg, open_path, mode.as_ref(), 0)? {
        super::user_wrapper::OpenResult::Opened(value) => {
            return return_value(return_pointer, value);
        }
        super::user_wrapper::OpenResult::Declined { class } => {
            if eg.exception.is_some() {
                return Ok(());
            }
            super::super::report_internal_diagnostic(
                eg,
                execute_data,
                2,
                "Warning",
                &format!(
                    "fopen({open_path}): Failed to open stream: \"{class}::stream_open\" call failed"
                ),
            )?;
            return return_value(return_pointer, Value::bool(false));
        }
        super::user_wrapper::OpenResult::NotRegistered => {}
    }
    let value = match PhpStream::open(open_path, mode.as_ref()) {
        Ok(stream) => {
            if stream.metadata().wrapper_type == "plainfile" {
                super::super::filesystem::clear_filesystem_stat_cache(eg);
            }
            #[cfg(feature = "resource-lifetime")]
            let value = insert_stream(eg, stream);
            #[cfg(not(feature = "resource-lifetime"))]
            let value = Value::resource(insert_stream(eg, stream));
            value
        }
        Err(_) => Value::bool(false),
    };
    return_value(return_pointer, value)
}

pub(in crate::stdlib) fn optional_context_resource(
    execute_data: *mut ExecuteData,
    index: u32,
    eg: &mut ExecutorGlobals,
    function: &str,
    argument_number: u32,
) -> Result<Option<i64>, ()> {
    let Some(value) = optional_argument(execute_data, index) else {
        return Ok(None);
    };
    if value.value_type() == ValueType::Null {
        return Ok(None);
    }
    let Some(resource) = value.as_resource_id() else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "{function}(): Argument #{argument_number} ($context) must be of type resource or null, {} given",
                given_type_name(value)
            ),
        );
        return Err(());
    };
    Ok(Some(resource))
}

pub(in crate::stdlib) fn context_snapshot(
    eg: &mut ExecutorGlobals,
    resource: i64,
) -> Option<StreamContext> {
    super::super::resource::with_request_payload_mut::<StreamContext, _>(eg, resource, |context| {
        context.clone()
    })
}

pub(in crate::stdlib) fn invalid_context_error(eg: &mut ExecutorGlobals, function: &str) {
    argument_error(
        eg,
        "TypeError",
        format!("{function}(): supplied resource is not a valid Stream-Context resource"),
    );
}

fn nullable_array_argument<'a>(
    execute_data: *mut ExecuteData,
    index: u32,
    eg: &mut ExecutorGlobals,
    name: &str,
) -> Result<Option<&'a PhpArray>, ()> {
    let Some(value) = optional_argument(execute_data, index) else {
        return Ok(None);
    };
    if value.value_type() == ValueType::Null {
        return Ok(None);
    }
    let Some(array) = value.as_array() else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "stream_context_create(): Argument #{} (${name}) must be of type ?array, {} given",
                index + 1,
                given_type_name(value)
            ),
        );
        return Err(());
    };
    Ok(Some(array))
}

pub(super) fn normalize_options(options: &PhpArray) -> Option<PhpArray> {
    let mut normalized = PhpArray::new();
    for (wrapper, value) in options.iter() {
        let ArrayKey::String(wrapper) = wrapper else {
            return None;
        };
        let wrapper_options = value.as_array()?;
        let mut normalized_wrapper = PhpArray::new();
        for (name, value) in wrapper_options.iter() {
            if let ArrayKey::String(name) = name {
                normalized_wrapper.set_str(&name, value.clone());
            }
        }
        if !normalized_wrapper.is_empty() {
            normalized.set_str(&wrapper, Value::array(normalized_wrapper));
        }
    }
    Some(normalized)
}

pub(super) fn merge_options(options: &mut PhpArray, updates: &PhpArray) {
    for (wrapper, value) in updates.iter() {
        let ArrayKey::String(wrapper) = wrapper else {
            continue;
        };
        let Some(updates) = value.as_array() else {
            continue;
        };
        let mut wrapper_options = options
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
            options.set_str(&wrapper, Value::array(wrapper_options));
        }
    }
}

pub(super) fn set_option(options: &mut PhpArray, wrapper: &str, name: &str, value: Value) {
    let mut wrapper_options = options
        .get_str(wrapper)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(PhpArray::new);
    wrapper_options.set_str(name, value);
    options.set_str(wrapper, Value::array(wrapper_options));
}

pub(super) fn apply_params(
    context: &mut StreamContext,
    params: &PhpArray,
    eg: &mut ExecutorGlobals,
    execute_data: *mut ExecuteData,
    function: &str,
    callback_argument: &str,
) -> bool {
    if let Some(notification) = params.get_str("notification") {
        if super::super::resolve_callback_at_callsite(notification, eg, execute_data).is_none() {
            let detail = super::super::ordinary_callback_invalid_reason(notification, eg);
            argument_error(
                eg,
                "TypeError",
                format!(
                    "{function}(): Argument #1 (${callback_argument}) must be an array with valid callbacks as values, {detail}"
                ),
            );
            return false;
        }
        context.params.set_str("notification", notification.clone());
    }

    if let Some(options) = params.get_str("options") {
        let Some(options) = options.as_array() else {
            argument_error(
                eg,
                "TypeError",
                "Invalid stream/context parameter".to_string(),
            );
            return false;
        };
        let Some(options) = normalize_options(options) else {
            invalid_options_error(eg);
            return false;
        };
        merge_options(&mut context.options, &options);
    }
    true
}

pub(super) fn invalid_options_error(eg: &mut ExecutorGlobals) {
    argument_error(
        eg,
        "ValueError",
        "Options should have the form [\"wrappername\"][\"optionname\"] = $value".to_string(),
    );
}

fn stream_or_context_argument(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    argument_name: &str,
) -> Option<StreamContext> {
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
    if let Some(context) = context_snapshot(eg, resource) {
        return Some(context);
    }
    if let Some(context) = with_stream(eg, resource, |stream| {
        stream.context().cloned().unwrap_or_else(empty_context)
    }) {
        return Some(context);
    }
    argument_error(
        eg,
        "TypeError",
        format!("{function}(): Argument #1 (${argument_name}) must be a valid stream/context"),
    );
    None
}

pub(super) fn empty_context() -> StreamContext {
    StreamContext {
        options: PhpArray::new(),
        params: PhpArray::new(),
    }
}
