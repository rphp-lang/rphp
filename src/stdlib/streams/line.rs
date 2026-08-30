//! PHP-compatible `stream_get_line()` argument handling and return values.

use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::checked_args::{argument_error, given_type_name, weak_long_argument};
use super::{argument, optional_argument, return_value, with_stream};

#[cold]
pub(super) fn fn_stream_get_line(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = stream_argument(execute_data, eg) else {
        return Ok(());
    };

    let length_value = argument(execute_data, 1);
    let Some(length) = weak_long_argument(length_value) else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "stream_get_line(): Argument #2 ($length) must be of type int, {} given",
                given_type_name(length_value)
            ),
        );
        return Ok(());
    };
    if length < 0 {
        argument_error(
            eg,
            "ValueError",
            "stream_get_line(): Argument #2 ($length) must be greater than or equal to 0"
                .to_string(),
        );
        return Ok(());
    }
    let maximum = (length != 0).then_some(length as usize);

    let ending = match optional_argument(execute_data, 2) {
        Some(value) => match weak_string(value, eg)? {
            Some(value) => value,
            None => {
                argument_error(
                    eg,
                    "TypeError",
                    format!(
                        "stream_get_line(): Argument #3 ($ending) must be of type string, {} given",
                        given_type_name(value)
                    ),
                );
                return Ok(());
            }
        },
        None => String::new(),
    };
    let ending = super::super::php_string_to_bytes(&ending);

    let mut bytes = Vec::new();
    let result = with_stream(eg, resource, |stream| {
        stream.read_until(&mut bytes, maximum, &ending)
    });
    match result {
        Some(Ok(Some(read))) => {
            debug_assert_eq!(read, bytes.len());
            return_value(return_pointer, super::super::php_byte_result(bytes, false))
        }
        _ => return_value(return_pointer, Value::bool(false)),
    }
}

fn stream_argument(execute_data: *mut ExecuteData, eg: &mut ExecutorGlobals) -> Option<i64> {
    let value = argument(execute_data, 0);
    let Some(resource) = value.as_resource_id() else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "stream_get_line(): Argument #1 ($stream) must be of type resource, {} given",
                given_type_name(value)
            ),
        );
        return None;
    };
    if with_stream(eg, resource, |_| ()).is_none() {
        argument_error(
            eg,
            "TypeError",
            "stream_get_line(): supplied resource is not a valid stream resource".to_string(),
        );
        return None;
    }
    Some(resource)
}

fn weak_string(value: &Value, eg: &mut ExecutorGlobals) -> Result<Option<String>, VmError> {
    match value.value_type() {
        ValueType::Null
        | ValueType::False
        | ValueType::True
        | ValueType::Long
        | ValueType::Double
        | ValueType::String => Ok(Some(value.echo_to_string())),
        ValueType::Object => {
            let converted = crate::vm::execute::call_object_string_conversion(eg, value)?;
            Ok(converted.map(|value| value.echo_to_string()))
        }
        ValueType::Undef
        | ValueType::Array
        | ValueType::Resource
        | ValueType::Reference
        | ValueType::Closure => Ok(None),
    }
}
