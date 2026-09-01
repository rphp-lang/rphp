use std::borrow::Cow;

use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::checked_args::{argument_error, stream_argument, weak_long_argument};

// Keep the complete checked handler in this feature-gated module. Sharing its
// validation helpers with the baseline handler changed unrelated ARM64 hot
// code generation even though the error paths never executed.

#[cold]
pub(super) fn fn_fgetcsv(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(result) = try_fast_fgetcsv(execute_data, return_pointer, eg) {
        return result;
    }
    let Some(resource) = stream_argument(execute_data, eg, "fgetcsv") else {
        return Ok(());
    };
    let length = match super::optional_argument(execute_data, 1) {
        Some(length) => {
            let Some(length) = weak_long_argument(length) else {
                argument_error(
                    eg,
                    "TypeError",
                    format!(
                        "fgetcsv(): Argument #2 ($length) must be of type ?int, {} given",
                        length.type_name()
                    ),
                );
                return Ok(());
            };
            if !(0..i64::MAX).contains(&length) {
                argument_error(
                    eg,
                    "ValueError",
                    format!(
                        "fgetcsv(): Argument #2 ($length) must be between 0 and {}",
                        i64::MAX - 1
                    ),
                );
                return Ok(());
            }
            let length = length as usize;
            (length != 0).then_some(length)
        }
        None => None,
    };
    let Some(separator) =
        csv_character_argument(execute_data, eg, 2, b',', "fgetcsv", 3, "separator")
    else {
        return Ok(());
    };
    let Some(enclosure) =
        csv_character_argument(execute_data, eg, 3, b'"', "fgetcsv", 4, "enclosure")
    else {
        return Ok(());
    };
    let Some(escape) = csv_escape_argument(execute_data, eg, 4, Some(b'\\'), "fgetcsv", 5) else {
        return Ok(());
    };
    if super::optional_argument(execute_data, 4).is_none() {
        super::super::report_internal_deprecation(
            eg,
            execute_data,
            "fgetcsv(): the $escape parameter must be provided as its default value will change",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }

    let result = super::with_stream_io(eg, resource, |stream| {
        stream.read_csv_record(length, separator, enclosure, escape)
    });
    match result {
        Some(Ok(Some(fields))) => {
            let mut record = PhpArray::with_packed_capacity(fields.len());
            for field in fields {
                record.push(match field {
                    Some(bytes) => super::super::php_byte_result(bytes, false),
                    None => Value::null(),
                });
            }
            super::return_value(return_pointer, Value::array(record))
        }
        _ => super::return_value(return_pointer, Value::bool(false)),
    }
}

fn try_fast_fgetcsv(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Option<Result<(), VmError>> {
    let resource = super::argument(execute_data, 0).as_resource_id()?;
    if super::optional_argument(execute_data, 1).is_some()
        || super::optional_argument(execute_data, 2).is_some()
        || super::optional_argument(execute_data, 3).is_some()
    {
        return None;
    }
    let escape = super::optional_argument(execute_data, 4)?;
    let escape = escape.as_str()?;
    let escape = match super::super::php_string_to_bytes(escape).as_slice() {
        [] => None,
        [escape] => Some(*escape),
        _ => return None,
    };

    let result = super::with_stream_io(eg, resource, |stream| {
        stream.read_csv_record(None, b',', b'"', escape)
    })?;
    Some(match result {
        Ok(Some(fields)) => {
            let mut record = PhpArray::with_packed_capacity(fields.len());
            for field in fields {
                record.push(match field {
                    Some(bytes) => super::super::php_byte_result(bytes, false),
                    None => Value::null(),
                });
            }
            super::return_value(return_pointer, Value::array(record))
        }
        _ => super::return_value(return_pointer, Value::bool(false)),
    })
}

#[cold]
pub(super) fn string_argument(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    index: u32,
    function: &str,
    argument_number: u8,
    argument_name: &str,
) -> Option<Cow<'static, str>> {
    let value = super::argument(execute_data, index);
    if matches!(
        value.value_type(),
        ValueType::Array | ValueType::Object | ValueType::Resource | ValueType::Closure
    ) {
        argument_error(
            eg,
            "TypeError",
            format!(
                "{function}(): Argument #{argument_number} (${argument_name}) must be of type string, {} given",
                value.type_name()
            ),
        );
        return None;
    }
    Some(super::argument_string(execute_data, index))
}

#[cold]
pub(super) fn csv_character_argument(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    index: u32,
    default: u8,
    function: &str,
    argument_number: u8,
    argument_name: &str,
) -> Option<u8> {
    let Some(_) = super::optional_argument(execute_data, index) else {
        return Some(default);
    };
    let value = string_argument(
        execute_data,
        eg,
        index,
        function,
        argument_number,
        argument_name,
    )?;
    let bytes = super::super::php_string_to_bytes(value.as_ref());
    match bytes.as_slice() {
        [byte] => Some(*byte),
        _ => {
            argument_error(
                eg,
                "ValueError",
                format!(
                    "{function}(): Argument #{argument_number} (${argument_name}) must be a single character"
                ),
            );
            None
        }
    }
}

#[cold]
pub(super) fn csv_escape_argument(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    index: u32,
    default: Option<u8>,
    function: &str,
    argument_number: u8,
) -> Option<Option<u8>> {
    let Some(_) = super::optional_argument(execute_data, index) else {
        return Some(default);
    };
    let value = string_argument(execute_data, eg, index, function, argument_number, "escape")?;
    let bytes = super::super::php_string_to_bytes(value.as_ref());
    match bytes.as_slice() {
        [] => Some(None),
        [escape] => Some(Some(*escape)),
        _ => {
            argument_error(
                eg,
                "ValueError",
                format!(
                    "{function}(): Argument #{argument_number} ($escape) must be empty or a single character"
                ),
            );
            None
        }
    }
}
