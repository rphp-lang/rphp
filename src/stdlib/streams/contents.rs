use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::checked_args::{argument_error, given_type_name, stream_argument, weak_long_argument};

#[cold]
pub(super) fn fn_stream_get_contents(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = stream_argument(execute_data, eg, "stream_get_contents") else {
        return Ok(());
    };

    let length = match super::optional_argument(execute_data, 1) {
        Some(value) if value.value_type() == ValueType::Null => None,
        Some(value) => {
            let Some(length) = weak_long_argument(value) else {
                argument_error(
                    eg,
                    "TypeError",
                    format!(
                        "stream_get_contents(): Argument #2 ($length) must be of type ?int, {} given",
                        given_type_name(value)
                    ),
                );
                return Ok(());
            };
            if length < -1 {
                argument_error(
                    eg,
                    "ValueError",
                    "stream_get_contents(): Argument #2 ($length) must be greater than or equal to -1"
                        .to_string(),
                );
                return Ok(());
            }
            if length == -1 {
                None
            } else {
                let Ok(length) = usize::try_from(length) else {
                    return super::return_value(return_pointer, Value::bool(false));
                };
                Some(length)
            }
        }
        None => None,
    };

    let offset = match super::optional_argument(execute_data, 2) {
        Some(value) => {
            let Some(offset) = weak_long_argument(value) else {
                argument_error(
                    eg,
                    "TypeError",
                    format!(
                        "stream_get_contents(): Argument #3 ($offset) must be of type int, {} given",
                        given_type_name(value)
                    ),
                );
                return Ok(());
            };
            (offset >= 0).then_some(offset as u64)
        }
        None => None,
    };

    let mut bytes = Vec::new();
    let result = super::with_stream_io(eg, resource, |stream| {
        stream.read_contents(&mut bytes, length, offset)
    });
    match result {
        Some(Ok(read)) => {
            debug_assert_eq!(read, bytes.len());
            super::return_value(return_pointer, super::super::php_byte_result(bytes, false))
        }
        _ => {
            #[cfg(feature = "stream-registry")]
            if super::user_wrapper::is_user_stream(eg, resource) {
                if offset.is_some() {
                    return super::return_value(return_pointer, Value::bool(false));
                }
                let limit = length.unwrap_or(usize::MAX);
                while bytes.len() < limit {
                    let requested = limit.saturating_sub(bytes.len()).min(8192);
                    let Some(chunk) = super::user_wrapper::read(eg, resource, requested)? else {
                        return super::return_value(return_pointer, Value::bool(false));
                    };
                    let empty = chunk.is_empty();
                    bytes.extend_from_slice(&chunk);
                    let eof = super::user_wrapper::cached_eof(eg, resource).unwrap_or(true);
                    if empty || eof {
                        break;
                    }
                }
                return super::return_value(
                    return_pointer,
                    super::super::php_byte_result(bytes, false),
                );
            }
            super::return_value(return_pointer, Value::bool(false))
        }
    }
}
