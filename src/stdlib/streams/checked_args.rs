use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::frame::ExecuteData;

#[cold]
pub(super) fn weak_long_argument(value: &Value) -> Option<i64> {
    match value.value_type() {
        ValueType::Long => value.as_long(),
        ValueType::Double => value.as_double().map(|value| value as i64),
        ValueType::True => Some(1),
        ValueType::False | ValueType::Null => Some(0),
        ValueType::String => {
            let value = value.as_str()?.trim();
            if value.is_empty() {
                return None;
            }
            value.parse::<i64>().ok().or_else(|| {
                value
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .map(|value| value as i64)
            })
        }
        ValueType::Undef
        | ValueType::Array
        | ValueType::Object
        | ValueType::Resource
        | ValueType::Reference
        | ValueType::Closure => None,
    }
}

#[cold]
pub(super) fn argument_error(eg: &mut ExecutorGlobals, class: &str, message: String) {
    debug_assert!(eg.exception.is_none());
    eg.exception = Some(crate::value::make_error_value(class, &message));
}

#[cold]
pub(super) fn given_type_name(value: &Value) -> String {
    match value.value_type() {
        ValueType::False => "false".to_string(),
        ValueType::True => "true".to_string(),
        ValueType::Object => value.as_object().map_or_else(
            || "object".to_string(),
            |object| object.class_name.to_string(),
        ),
        _ => value.type_name().to_string(),
    }
}

#[cold]
#[cfg(any(feature = "csv-errors", feature = "stream-contents"))]
pub(super) fn stream_argument(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Option<i64> {
    stream_argument_at(execute_data, eg, function, 0, "stream")
}

#[cold]
pub(super) fn stream_argument_at(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Option<i64> {
    let value = super::argument(execute_data, index);
    let argument = index + 1;
    let Some(resource) = value.as_resource_id() else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "{function}(): Argument #{argument} (${parameter}) must be of type resource, {} given",
                given_type_name(value)
            ),
        );
        return None;
    };
    if !super::super::resource::is_open_for_request(eg, resource)
        || super::super::resource::type_for_request(eg, resource) != "stream"
    {
        argument_error(
            eg,
            "TypeError",
            format!(
                "{function}(): Argument #{argument} (${parameter}) must be an open stream resource"
            ),
        );
        return None;
    }
    Some(resource)
}
