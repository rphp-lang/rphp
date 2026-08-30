#[cfg(feature = "file-contents")]
use std::io::SeekFrom;

#[cfg(feature = "file-contents")]
use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

#[cfg(feature = "file-contents")]
use super::stream::PhpStream;
#[cfg(feature = "file-contents")]
use super::streams::checked_args::{argument_error, given_type_name, weak_long_argument};

#[cfg(feature = "file-write")]
mod write;
#[cfg(feature = "file-write")]
pub(super) use write::fn_file_put_contents;
#[cfg(feature = "file-lines")]
mod lines;
#[cfg(feature = "file-lines")]
pub(super) use lines::fn_file;

#[cold]
#[cfg(feature = "file-contents")]
pub(super) fn fn_file_get_contents(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let filename_value = argument(execute_data, 0);
    let Some(filename) = string_argument(filename_value) else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "file_get_contents(): Argument #1 ($filename) must be of type string, {} given",
                given_type_name(filename_value)
            ),
        );
        return Ok(());
    };
    if let Some(value) = optional_argument(execute_data, 1)
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
                "file_get_contents(): Argument #2 ($use_include_path) must be of type bool, {} given",
                given_type_name(value)
            ),
        );
        return Ok(());
    }
    #[cfg(feature = "include-path")]
    let use_include_path = optional_argument(execute_data, 1).is_some_and(Value::is_truthy);

    #[cfg(not(feature = "stream-context"))]
    if let Some(context) = optional_argument(execute_data, 2)
        && context.value_type() != ValueType::Null
    {
        if context.value_type() == ValueType::Resource {
            argument_error(
                eg,
                "TypeError",
                "file_get_contents(): supplied resource is not a valid Stream-Context resource"
                    .to_string(),
            );
        } else {
            argument_error(
                eg,
                "TypeError",
                format!(
                    "file_get_contents(): Argument #3 ($context) must be of type resource or null, {} given",
                    given_type_name(context)
                ),
            );
        }
        return Ok(());
    }
    #[cfg(feature = "stream-context")]
    {
        let context = match super::streams::context::optional_context_resource(
            execute_data,
            2,
            eg,
            "file_get_contents",
            3,
        ) {
            Ok(context) => context,
            Err(()) => return Ok(()),
        };
        if let Some(context) = context
            && super::streams::context::context_snapshot(eg, context).is_none()
        {
            super::streams::context::invalid_context_error(eg, "file_get_contents");
            return Ok(());
        }
    }

    let offset = match optional_argument(execute_data, 3) {
        Some(value) => {
            let Some(offset) = weak_long_argument(value) else {
                argument_error(
                    eg,
                    "TypeError",
                    format!(
                        "file_get_contents(): Argument #4 ($offset) must be of type int, {} given",
                        given_type_name(value)
                    ),
                );
                return Ok(());
            };
            offset
        }
        None => 0,
    };

    let length = match optional_argument(execute_data, 4) {
        Some(value) if value.value_type() == ValueType::Null => None,
        Some(value) => {
            let Some(length) = weak_long_argument(value) else {
                argument_error(
                    eg,
                    "TypeError",
                    format!(
                        "file_get_contents(): Argument #5 ($length) must be of type ?int, {} given",
                        given_type_name(value)
                    ),
                );
                return Ok(());
            };
            if length < 0 {
                argument_error(
                    eg,
                    "ValueError",
                    "file_get_contents(): Argument #5 ($length) must be greater than or equal to 0"
                        .to_string(),
                );
                return Ok(());
            }
            let Ok(length) = usize::try_from(length) else {
                return return_value(return_pointer, Value::bool(false));
            };
            Some(length)
        }
        None => None,
    };

    if filename.is_empty() {
        argument_error(eg, "ValueError", "Path must not be empty".to_string());
        return Ok(());
    }

    if let Some(data) = super::filesystem::decode_data_uri(&filename) {
        let bytes = match data {
            Ok(bytes) => bytes,
            Err(error) => {
                super::filesystem::report_data_uri_error(execute_data, eg, &filename, error)?;
                if eg.exception.is_some() {
                    return Ok(());
                }
                return return_value(return_pointer, Value::bool(false));
            }
        };
        let start = if offset < 0 {
            let distance = usize::try_from(offset.unsigned_abs()).unwrap_or(usize::MAX);
            let Some(start) = bytes.len().checked_sub(distance) else {
                super::report_internal_diagnostic(
                    eg,
                    execute_data,
                    2,
                    "Warning",
                    &format!(
                        "file_get_contents(): Failed to seek to position {offset} in the stream"
                    ),
                )?;
                if eg.exception.is_some() {
                    return Ok(());
                }
                return return_value(return_pointer, Value::bool(false));
            };
            start
        } else {
            usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .min(bytes.len())
        };
        let end = length
            .and_then(|length| start.checked_add(length))
            .unwrap_or(bytes.len())
            .min(bytes.len());
        return return_value(
            return_pointer,
            super::php_byte_result(bytes[start..end].to_vec(), false),
        );
    }

    #[cfg(feature = "include-path")]
    let filename = super::include_path::resolve_for_open(eg, &filename, use_include_path);
    let mut stream = match PhpStream::open(&filename, "r") {
        Ok(stream) => stream,
        Err(_) => return return_value(return_pointer, Value::bool(false)),
    };
    let seek = if offset < 0 {
        stream.seek(SeekFrom::End(offset))
    } else {
        stream.seek(SeekFrom::Start(offset as u64))
    };
    if seek.is_err() {
        return return_value(return_pointer, Value::bool(false));
    }

    let mut bytes = Vec::new();
    match stream.read_contents(&mut bytes, length, None) {
        Ok(read) => {
            debug_assert_eq!(read, bytes.len());
            return_value(return_pointer, super::php_byte_result(bytes, false))
        }
        Err(_) => return_value(return_pointer, Value::bool(false)),
    }
}

#[inline]
fn argument<'a>(execute_data: *mut ExecuteData, index: u32) -> &'a Value {
    let value = unsafe { (*execute_data).cv(index) };
    if value.is_reference() {
        unsafe { &*value.as_ref_ptr() }
    } else {
        value
    }
}

#[inline]
fn optional_argument<'a>(execute_data: *mut ExecuteData, index: u32) -> Option<&'a Value> {
    let value = argument(execute_data, index);
    (value.value_type() != ValueType::Undef).then_some(value)
}

#[cold]
fn string_argument(value: &Value) -> Option<String> {
    match value.value_type() {
        ValueType::String => value.as_str().map(str::to_string),
        ValueType::Null
        | ValueType::False
        | ValueType::True
        | ValueType::Long
        | ValueType::Double => Some(value.echo_to_string()),
        ValueType::Undef
        | ValueType::Array
        | ValueType::Object
        | ValueType::Resource
        | ValueType::Reference
        | ValueType::Closure => None,
    }
}

#[inline]
fn return_value(pointer: *mut Value, value: Value) -> Result<(), VmError> {
    if !pointer.is_null() {
        unsafe { pointer.write(value) };
    }
    Ok(())
}
