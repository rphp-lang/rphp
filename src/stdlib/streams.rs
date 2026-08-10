use std::borrow::Cow;
use std::io::SeekFrom;

use crate::compiler::make_internal_function;
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction, InternalFunctionHandler};

use super::stream::PhpStream;

// Linking either extended CSV path into the default ARM64 binary changes
// unrelated hot-code placement enough to fail the runtime admission gate. Keep
// both dependency-free implementations separately selectable until that
// codegen boundary is solved.
#[cfg(any(
    feature = "csv-errors",
    feature = "stream-contents",
    feature = "stream-copy",
    feature = "stream-context",
    feature = "file-contents",
    feature = "file-write",
    feature = "file-lines"
))]
pub(super) mod checked_args;
#[cfg(feature = "stream-contents")]
mod contents;
#[cfg(feature = "stream-context")]
pub(in crate::stdlib) mod context;
#[cfg(feature = "stream-copy")]
mod copy;
#[cfg(feature = "csv-errors")]
mod csv_errors;
#[cfg(feature = "csv-write")]
mod csv_write;

#[cold]
pub(super) fn register(eg: &mut ExecutorGlobals, functions: &mut Vec<Box<InternalFunction>>) {
    for (name, handler, maximum, required, parameter_names) in [
        #[cfg(not(feature = "stream-context"))]
        (
            "fopen",
            fn_fopen as InternalFunctionHandler,
            4,
            2,
            &["filename", "mode", "use_include_path", "context"][..],
        ),
        #[cfg(feature = "stream-context")]
        (
            "fopen",
            context::fn_fopen as InternalFunctionHandler,
            4,
            2,
            &["filename", "mode", "use_include_path", "context"][..],
        ),
        #[cfg(feature = "stream-context")]
        (
            "stream_context_create",
            context::fn_stream_context_create,
            2,
            0,
            &["options", "params"],
        ),
        #[cfg(feature = "stream-context")]
        (
            "stream_context_get_default",
            context::fn_stream_context_get_default,
            1,
            0,
            &["options"],
        ),
        #[cfg(feature = "stream-context")]
        (
            "stream_context_get_options",
            context::fn_stream_context_get_options,
            1,
            1,
            &["stream_or_context"],
        ),
        #[cfg(feature = "stream-context")]
        (
            "stream_context_get_params",
            context::fn_stream_context_get_params,
            1,
            1,
            &["context"],
        ),
        #[cfg(feature = "stream-context")]
        (
            "stream_context_set_default",
            context::fn_stream_context_set_default,
            1,
            1,
            &["options"],
        ),
        #[cfg(feature = "stream-context")]
        (
            "stream_context_set_option",
            context::fn_stream_context_set_option,
            4,
            2,
            &["context", "wrapper_or_options", "option_name", "value"],
        ),
        #[cfg(feature = "stream-context")]
        (
            "stream_context_set_options",
            context::fn_stream_context_set_options,
            2,
            2,
            &["context", "options"],
        ),
        #[cfg(feature = "stream-context")]
        (
            "stream_context_set_params",
            context::fn_stream_context_set_params,
            2,
            2,
            &["context", "params"],
        ),
        ("fread", fn_fread, 2, 2, &["stream", "length"]),
        #[cfg(feature = "stream-contents")]
        (
            "stream_get_contents",
            contents::fn_stream_get_contents,
            3,
            1,
            &["stream", "length", "offset"],
        ),
        #[cfg(feature = "stream-copy")]
        (
            "stream_copy_to_stream",
            copy::fn_stream_copy_to_stream,
            4,
            2,
            &["from", "to", "length", "offset"],
        ),
        ("fgets", fn_fgets, 2, 1, &["stream", "length"]),
        #[cfg(all(not(target_vendor = "apple"), not(feature = "csv-errors")))]
        (
            "fgetcsv",
            fn_fgetcsv,
            5,
            1,
            &["stream", "length", "separator", "enclosure", "escape"],
        ),
        #[cfg(all(not(target_vendor = "apple"), feature = "csv-errors"))]
        (
            "fgetcsv",
            csv_errors::fn_fgetcsv,
            5,
            1,
            &["stream", "length", "separator", "enclosure", "escape"],
        ),
        #[cfg(all(feature = "csv-write", not(target_vendor = "apple")))]
        (
            "fputcsv",
            csv_write::fn_fputcsv,
            6,
            2,
            &[
                "stream",
                "fields",
                "separator",
                "enclosure",
                "escape",
                "eol",
            ],
        ),
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
        (
            "stream_get_meta_data",
            fn_stream_get_meta_data,
            1,
            1,
            &["stream"],
        ),
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

#[cfg(all(target_vendor = "apple", not(feature = "csv-errors")))]
#[cold]
// Appending the Apple-only registration preserves the measured ordering of
// the pre-existing handlers while Linux keeps the regular table registration.
pub(super) fn register_extensions(
    eg: &mut ExecutorGlobals,
    functions: &mut Vec<Box<InternalFunction>>,
) {
    let function = Box::new(make_internal_function(
        fn_fgetcsv,
        5,
        1,
        ["stream", "length", "separator", "enclosure", "escape"]
            .into_iter()
            .map(str::to_string)
            .collect(),
    ));
    let pointer = &function.common as *const FunctionCommon;
    eg.register_function("fgetcsv", pointer).unwrap();
    functions.push(function);
    #[cfg(feature = "csv-write")]
    {
        let function = Box::new(make_internal_function(
            csv_write::fn_fputcsv,
            6,
            2,
            [
                "stream",
                "fields",
                "separator",
                "enclosure",
                "escape",
                "eol",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        ));
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("fputcsv", pointer).unwrap();
        functions.push(function);
    }
}

#[cfg(all(target_vendor = "apple", feature = "csv-errors"))]
#[cold]
// A separate registrar keeps the accepted default body token-for-token while
// selecting the checked handler only in extended builds.
#[allow(clippy::vec_box)]
pub(super) fn register_extensions(
    eg: &mut ExecutorGlobals,
    functions: &mut Vec<Box<InternalFunction>>,
) {
    let function = Box::new(make_internal_function(
        csv_errors::fn_fgetcsv,
        5,
        1,
        ["stream", "length", "separator", "enclosure", "escape"]
            .into_iter()
            .map(str::to_string)
            .collect(),
    ));
    let pointer = &function.common as *const FunctionCommon;
    eg.register_function("fgetcsv", pointer).unwrap();
    functions.push(function);
    #[cfg(feature = "csv-write")]
    {
        let function = Box::new(make_internal_function(
            csv_write::fn_fputcsv,
            6,
            2,
            [
                "stream",
                "fields",
                "separator",
                "enclosure",
                "escape",
                "eol",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        ));
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("fputcsv", pointer).unwrap();
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
#[cfg(feature = "resource-lifetime")]
fn insert_stream(eg: &mut ExecutorGlobals, stream: PhpStream) -> Value {
    super::resource::insert_value_for_request(eg, "stream", stream)
}

#[cold]
#[cfg(not(feature = "resource-lifetime"))]
fn insert_stream(eg: &mut ExecutorGlobals, stream: PhpStream) -> i64 {
    super::resource::insert_for_request(eg, "stream", stream)
}

#[cold]
pub(super) fn with_stream<R>(
    eg: &mut ExecutorGlobals,
    id: i64,
    operation: impl FnOnce(&mut PhpStream) -> R,
) -> Option<R> {
    super::resource::with_request_payload_mut::<PhpStream, _>(eg, id, operation)
}

#[cold]
#[cfg(not(feature = "stream-context"))]
fn fn_fopen(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = argument_string(execute_data, 0);
    let mode = argument_string(execute_data, 1);
    let value = match PhpStream::open(path.as_ref(), mode.as_ref()) {
        #[cfg(feature = "resource-lifetime")]
        Ok(stream) => insert_stream(eg, stream),
        #[cfg(not(feature = "resource-lifetime"))]
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
fn fn_fgets(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let resource = argument(execute_data, 0).as_resource_id();
    let length = match optional_argument(execute_data, 1) {
        Some(length) => match usize::try_from(length.to_long_val()) {
            Ok(length) => Some(length),
            Err(_) => return return_value(return_pointer, Value::bool(false)),
        },
        None => None,
    };
    let mut bytes = Vec::new();
    let result = resource.and_then(|resource| {
        with_stream(eg, resource, |stream| stream.read_line(&mut bytes, length))
    });
    match result {
        Some(Ok(Some(read))) => {
            debug_assert_eq!(read, bytes.len());
            return_value(
                return_pointer,
                Value::string(super::bytes_to_php_string(&bytes)),
            )
        }
        _ => return_value(return_pointer, Value::bool(false)),
    }
}

#[cfg(not(feature = "csv-errors"))]
#[cold]
#[inline(never)]
#[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
fn fn_fgetcsv(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let resource = argument(execute_data, 0).as_resource_id();
    let length = match optional_argument(execute_data, 1) {
        Some(length) => {
            let length = length.to_long_val();
            let Ok(length) = usize::try_from(length) else {
                return return_value(return_pointer, Value::bool(false));
            };
            (length != 0).then_some(length)
        }
        None => None,
    };
    let Some(separator) = csv_character_argument(execute_data, 2, b',') else {
        return return_value(return_pointer, Value::bool(false));
    };
    let Some(enclosure) = csv_character_argument(execute_data, 3, b'"') else {
        return return_value(return_pointer, Value::bool(false));
    };
    let Some(escape) = csv_escape_argument(execute_data, 4, Some(b'\\')) else {
        return return_value(return_pointer, Value::bool(false));
    };

    let result = resource.and_then(|resource| {
        with_stream(eg, resource, |stream| {
            stream.read_csv_record(length, separator, enclosure, escape)
        })
    });
    match result {
        Some(Ok(Some(fields))) => {
            let mut record = PhpArray::with_packed_capacity(fields.len());
            for field in fields {
                record.push(match field {
                    Some(bytes) => Value::string(super::bytes_to_php_string(&bytes)),
                    None => Value::null(),
                });
            }
            return_value(return_pointer, Value::array(record))
        }
        _ => return_value(return_pointer, Value::bool(false)),
    }
}

#[cfg(not(feature = "csv-errors"))]
#[cold]
#[inline(never)]
#[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
fn csv_character_argument(execute_data: *mut ExecuteData, index: u32, default: u8) -> Option<u8> {
    let Some(_) = optional_argument(execute_data, index) else {
        return Some(default);
    };
    let value = argument_string(execute_data, index);
    let bytes = super::php_string_to_bytes(value.as_ref());
    match bytes.as_slice() {
        [byte] => Some(*byte),
        _ => None,
    }
}

#[cfg(not(feature = "csv-errors"))]
#[cold]
#[inline(never)]
#[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
fn csv_escape_argument(
    execute_data: *mut ExecuteData,
    index: u32,
    default: Option<u8>,
) -> Option<Option<u8>> {
    let Some(_) = optional_argument(execute_data, index) else {
        return Some(default);
    };
    let value = argument_string(execute_data, index);
    let bytes = super::php_string_to_bytes(value.as_ref());
    match bytes.as_slice() {
        [] => Some(None),
        [escape] => Some(Some(*escape)),
        _ => None,
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

#[cold]
fn fn_stream_get_meta_data(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = argument(execute_data, 0)
        .as_resource_id()
        .and_then(|resource| {
            with_stream(eg, resource, |stream| {
                let metadata = stream.metadata();
                let status_fields = usize::from(metadata.timed_out.is_some())
                    + usize::from(metadata.blocked.is_some())
                    + usize::from(metadata.eof.is_some());
                let mut result = PhpArray::with_hash_capacity(6 + status_fields);
                if let Some(timed_out) = metadata.timed_out {
                    result.set_str("timed_out", Value::bool(timed_out));
                }
                if let Some(blocked) = metadata.blocked {
                    result.set_str("blocked", Value::bool(blocked));
                }
                if let Some(eof) = metadata.eof {
                    result.set_str("eof", Value::bool(eof));
                }
                result.set_str("wrapper_type", Value::string(metadata.wrapper_type));
                result.set_str("stream_type", Value::string(metadata.stream_type));
                result.set_str("mode", Value::string(metadata.mode));
                result.set_str("unread_bytes", Value::long(metadata.unread_bytes as i64));
                result.set_str("seekable", Value::bool(metadata.seekable));
                result.set_str("uri", Value::string(metadata.uri));
                Value::array(result)
            })
        })
        .unwrap_or_else(|| Value::bool(false));
    return_value(return_pointer, value)
}
