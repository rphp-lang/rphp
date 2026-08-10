use std::io::SeekFrom;

use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::stream::PhpStream;
use super::streams::checked_args::{argument_error, given_type_name, weak_long_argument};

#[cold]
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
            return_value(
                return_pointer,
                Value::string(super::bytes_to_php_string(&bytes)),
            )
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
