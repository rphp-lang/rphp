use std::borrow::Cow;
use std::io::SeekFrom;

use crate::compiler::make_internal_function;
use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction, InternalFunctionHandler};

use super::stream::PhpStream;

#[cold]
pub(super) fn register(eg: &mut ExecutorGlobals, functions: &mut Vec<Box<InternalFunction>>) {
    for (name, handler, maximum, required, parameter_names) in [
        (
            "fopen",
            fn_fopen as InternalFunctionHandler,
            4,
            2,
            &["filename", "mode", "use_include_path", "context"][..],
        ),
        ("fread", fn_fread, 2, 2, &["stream", "length"]),
        ("fwrite", fn_fwrite, 3, 2, &["stream", "data", "length"]),
        ("fclose", fn_fclose, 1, 1, &["stream"]),
        ("fflush", fn_fflush, 1, 1, &["stream"]),
        ("feof", fn_feof, 1, 1, &["stream"]),
        ("ftell", fn_ftell, 1, 1, &["stream"]),
        ("fseek", fn_fseek, 3, 2, &["stream", "offset", "whence"]),
        ("rewind", fn_rewind, 1, 1, &["stream"]),
        ("is_resource", fn_is_resource, 1, 1, &["value"]),
        (
            "get_resource_type",
            fn_get_resource_type,
            1,
            1,
            &["resource"],
        ),
        ("get_resource_id", fn_get_resource_id, 1, 1, &["resource"]),
    ] {
        let function = Box::new(make_internal_function(
            handler,
            maximum,
            required,
            parameter_names
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        ));
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(name, pointer).unwrap();
        functions.push(function);
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

fn argument_string(execute_data: *mut ExecuteData, index: u32) -> Cow<'static, str> {
    let value = argument(execute_data, index);
    match value.as_str() {
        Some(value) => Cow::Owned(value.to_string()),
        None => Cow::Owned(value.echo_to_string()),
    }
}

#[inline]
fn return_value(pointer: *mut Value, value: Value) -> Result<(), VmError> {
    if !pointer.is_null() {
        unsafe { pointer.write(value) };
    }
    Ok(())
}

#[cold]
fn insert_stream(eg: &mut ExecutorGlobals, stream: PhpStream) -> i64 {
    super::resource::insert_for_request(eg, "stream", stream)
}

#[cold]
fn with_stream<R>(
    eg: &mut ExecutorGlobals,
    id: i64,
    operation: impl FnOnce(&mut PhpStream) -> R,
) -> Option<R> {
    super::resource::with_request_payload_mut::<PhpStream, _>(eg, id, operation)
}

#[cold]
fn fn_fopen(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = argument_string(execute_data, 0);
    let mode = argument_string(execute_data, 1);
    let value = match PhpStream::open(path.as_ref(), mode.as_ref()) {
        Ok(stream) => Value::resource(insert_stream(eg, stream)),
        Err(_) => Value::bool(false),
    };
    return_value(return_pointer, value)
}

#[cold]
fn fn_fread(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let resource = argument(execute_data, 0).as_resource_id();
    let length = argument(execute_data, 1).to_long_val();
    let Ok(length) = usize::try_from(length) else {
        return return_value(return_pointer, Value::bool(false));
    };
    if length == 0 {
        return return_value(return_pointer, Value::bool(false));
    }

    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(length).is_err() {
        return return_value(return_pointer, Value::bool(false));
    }
    bytes.resize(length, 0);
    let result =
        resource.and_then(|resource| with_stream(eg, resource, |stream| stream.read(&mut bytes)));
    match result {
        Some(Ok(read)) => {
            bytes.truncate(read);
            return_value(
                return_pointer,
                Value::string(super::bytes_to_php_string(&bytes)),
            )
        }
        _ => return_value(return_pointer, Value::bool(false)),
    }
}

#[cold]
fn fn_fwrite(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let resource = argument(execute_data, 0).as_resource_id();
    let data = argument_string(execute_data, 1);
    let mut bytes = super::php_string_to_bytes(data.as_ref());
    if let Some(length) = optional_argument(execute_data, 2) {
        let Ok(length) = usize::try_from(length.to_long_val()) else {
            return return_value(return_pointer, Value::bool(false));
        };
        bytes.truncate(length);
    }
    let result =
        resource.and_then(|resource| with_stream(eg, resource, |stream| stream.write(&bytes)));
    match result {
        Some(Ok(written)) => return_value(return_pointer, Value::long(written as i64)),
        _ => return_value(return_pointer, Value::bool(false)),
    }
}

#[cold]
fn fn_fclose(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let closed = argument(execute_data, 0)
        .as_resource_id()
        .is_some_and(|resource| super::resource::close_for_request::<PhpStream>(eg, resource));
    return_value(return_pointer, Value::bool(closed))
}

#[cold]
fn fn_fflush(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let flushed = argument(execute_data, 0)
        .as_resource_id()
        .and_then(|resource| with_stream(eg, resource, |stream| stream.flush().is_ok()))
        .unwrap_or(false);
    return_value(return_pointer, Value::bool(flushed))
}

#[cold]
fn fn_feof(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let eof = argument(execute_data, 0)
        .as_resource_id()
        .and_then(|resource| with_stream(eg, resource, |stream| stream.is_eof()))
        .unwrap_or(false);
    return_value(return_pointer, Value::bool(eof))
}

#[cold]
fn fn_ftell(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let position = argument(execute_data, 0)
        .as_resource_id()
        .and_then(|resource| with_stream(eg, resource, |stream| stream.position()));
    match position {
        Some(Ok(position)) if position <= i64::MAX as u64 => {
            return_value(return_pointer, Value::long(position as i64))
        }
        _ => return_value(return_pointer, Value::bool(false)),
    }
}

#[cold]
fn fn_fseek(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let offset = argument(execute_data, 1).to_long_val();
    let whence = optional_argument(execute_data, 2)
        .map(Value::to_long_val)
        .unwrap_or(0);
    let seek_from = match whence {
        0 => match u64::try_from(offset) {
            Ok(offset) => SeekFrom::Start(offset),
            Err(_) => return return_value(return_pointer, Value::long(-1)),
        },
        1 => SeekFrom::Current(offset),
        2 => SeekFrom::End(offset),
        _ => return return_value(return_pointer, Value::long(-1)),
    };
    let succeeded = argument(execute_data, 0)
        .as_resource_id()
        .and_then(|resource| with_stream(eg, resource, |stream| stream.seek(seek_from).is_ok()))
        .unwrap_or(false);
    return_value(return_pointer, Value::long(if succeeded { 0 } else { -1 }))
}

#[cold]
fn fn_rewind(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let succeeded = argument(execute_data, 0)
        .as_resource_id()
        .and_then(|resource| {
            with_stream(eg, resource, |stream| {
                stream.seek(SeekFrom::Start(0)).is_ok()
            })
        })
        .unwrap_or(false);
    return_value(return_pointer, Value::bool(succeeded))
}

#[cold]
fn fn_is_resource(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let is_resource = argument(execute_data, 0)
        .as_resource_id()
        .is_some_and(|resource| super::resource::is_open_for_request(eg, resource));
    return_value(return_pointer, Value::bool(is_resource))
}

#[cold]
fn fn_get_resource_type(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    match argument(execute_data, 0).as_resource_id() {
        Some(resource) => return_value(
            return_pointer,
            Value::string(super::resource::type_for_request(eg, resource).to_string()),
        ),
        None => return_value(return_pointer, Value::bool(false)),
    }
}

#[cold]
fn fn_get_resource_id(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    match argument(execute_data, 0).as_resource_id() {
        Some(resource) => return_value(return_pointer, Value::long(resource)),
        None => return_value(return_pointer, Value::bool(false)),
    }
}
