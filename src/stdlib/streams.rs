use std::borrow::Cow;
use std::io::SeekFrom;

use crate::compiler::{make_internal_function, make_internal_function_ref};
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::ParamTypeHint;
use crate::vm::function::{FunctionCommon, InternalFunction, InternalFunctionHandler};

use super::stream::{PhpStream, StreamStat};

// Linking either extended CSV path into the default ARM64 binary changes
// unrelated hot-code placement enough to fail the runtime admission gate. Keep
// both dependency-free implementations separately selectable until that
// codegen boundary is solved.
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
#[cfg(feature = "stream-registry")]
pub(in crate::stdlib) mod filters;
#[cfg(feature = "stream-registry")]
mod info;
#[cfg(feature = "stream-line")]
mod line;
mod read_projections;
#[cfg(feature = "stream-truncate")]
mod truncate;
#[cfg(feature = "stream-registry")]
pub(crate) mod user_wrapper;

#[cold]
pub(super) fn register(eg: &mut ExecutorGlobals, functions: &mut Vec<Box<InternalFunction>>) {
    register_standard_streams(eg);
    #[cfg(feature = "stream-registry")]
    filters::register_functions(eg, functions);
    for (name, handler, maximum, required, parameter_names) in [
        (
            "fopen",
            fn_fopen as InternalFunctionHandler,
            4,
            2,
            &["filename", "mode", "use_include_path", "context"][..],
        ),
        ("fstat", fn_fstat, 1, 1, &["stream"]),
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
        #[cfg(feature = "stream-registry")]
        ("stream_get_filters", info::fn_stream_get_filters, 0, 0, &[]),
        #[cfg(feature = "stream-registry")]
        (
            "stream_get_transports",
            info::fn_stream_get_transports,
            0,
            0,
            &[],
        ),
        #[cfg(feature = "stream-registry")]
        (
            "stream_get_wrappers",
            info::fn_stream_get_wrappers,
            0,
            0,
            &[],
        ),
        #[cfg(feature = "stream-registry")]
        (
            "stream_is_local",
            info::fn_stream_is_local,
            1,
            1,
            &["stream"],
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
        #[cfg(feature = "stream-line")]
        (
            "stream_get_line",
            line::fn_stream_get_line,
            3,
            2,
            &["stream", "length", "ending"],
        ),
        #[cfg(feature = "stream-truncate")]
        (
            "ftruncate",
            truncate::fn_ftruncate,
            2,
            2,
            &["stream", "size"],
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
        (
            "flock",
            fn_flock,
            3,
            2,
            &["stream", "operation", "would_block"],
        ),
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
        let parameter_names = parameter_names
            .iter()
            .map(|name| (*name).to_string())
            .collect();
        let mut function = Box::new(if name == "flock" {
            make_internal_function_ref(handler, maximum, required, 0b100, parameter_names)
        } else {
            make_internal_function(handler, maximum, required, parameter_names)
        });
        if name == "flock" {
            function.common.sig.param_type_hints =
                vec![ParamTypeHint::None, ParamTypeHint::Int, ParamTypeHint::None];
            function.common.sig.return_type_hint = ParamTypeHint::Bool;
            function.handler_validates_types = true;
            // SendRef/SendVal enforce the optional output slot, so ordinary
            // two-argument calls retain the compact fixed-arity ABI.
            function.common.plan.call = crate::vm::function::CallStrategy::Fast;
        } else if name == "fstat" {
            function.common.sig.param_type_hints = vec![ParamTypeHint::None];
            function.common.sig.return_type_hint = ParamTypeHint::Union(vec![
                ParamTypeHint::Array,
                ParamTypeHint::ClassName("false".to_string()),
            ]);
            function.handler_validates_types = true;
        } else if name == "is_resource" {
            function.common.sig.param_type_hints = vec![ParamTypeHint::Mixed];
            function.common.sig.return_type_hint = ParamTypeHint::Bool;
            function.handler_validates_types = true;
        }
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(name, pointer).unwrap();
        if name == "flock" {
            eg.register_internal_function_reflection_metadata(
                pointer,
                vec![None, None, Some(Value::null())],
                "standard",
            );
        } else if name == "fstat" {
            eg.register_internal_function_reflection_metadata(pointer, vec![None], "standard");
        } else if name == "is_resource" {
            eg.register_internal_function_extension(pointer, "standard");
        }
        functions.push(function);
    }

    read_projections::register(eg, functions);

    #[cfg(feature = "stream-registry")]
    for (name, handler, maximum, required, parameter_names, parameter_types) in [
        (
            "stream_wrapper_register",
            user_wrapper::fn_stream_wrapper_register as InternalFunctionHandler,
            3,
            2,
            &["protocol", "class", "flags"][..],
            &[
                ParamTypeHint::String,
                ParamTypeHint::String,
                ParamTypeHint::Int,
            ][..],
        ),
        (
            "stream_register_wrapper",
            user_wrapper::fn_stream_wrapper_register as InternalFunctionHandler,
            3,
            2,
            &["protocol", "class", "flags"][..],
            &[
                ParamTypeHint::String,
                ParamTypeHint::String,
                ParamTypeHint::Int,
            ][..],
        ),
        (
            "stream_wrapper_unregister",
            user_wrapper::fn_stream_wrapper_unregister as InternalFunctionHandler,
            1,
            1,
            &["protocol"][..],
            &[ParamTypeHint::String][..],
        ),
        (
            "stream_wrapper_restore",
            user_wrapper::fn_stream_wrapper_restore as InternalFunctionHandler,
            1,
            1,
            &["protocol"][..],
            &[ParamTypeHint::String][..],
        ),
    ] {
        let mut function = Box::new(make_internal_function(
            handler,
            maximum,
            required,
            parameter_names
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        ));
        function.common.sig.param_type_hints = parameter_types.to_vec();
        function.common.sig.return_type_hint = ParamTypeHint::Bool;
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(name, pointer).unwrap();
        eg.register_internal_function_reflection_metadata(
            pointer,
            if maximum == 3 {
                vec![None, None, Some(Value::long(0))]
            } else {
                vec![None]
            },
            "standard",
        );
        functions.push(function);
    }
}

#[cold]
fn register_standard_streams(eg: &mut ExecutorGlobals) {
    for (name, stream) in [
        ("STDIN", PhpStream::standard_input()),
        ("STDOUT", PhpStream::standard_output()),
        ("STDERR", PhpStream::standard_error()),
    ] {
        #[cfg(feature = "resource-lifetime")]
        let value = insert_stream(eg, stream);
        #[cfg(not(feature = "resource-lifetime"))]
        let value = Value::resource(insert_stream(eg, stream));
        eg.define_constant(name, value)
            .expect("CLI standard stream constants are registered once");
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
fn argument_with_count<'a>(execute_data: *mut ExecuteData, index: u32) -> (&'a Value, u32) {
    // SAFETY: Internal-function handlers receive a live ExecuteData frame, and
    // the registered signature guarantees that `index` is inside its CV area.
    // SAFETY: a reference CV keeps its target live for the duration of this
    // internal call, so both branches return a valid request-scoped borrow.
    unsafe {
        let value = (*execute_data).cv(index);
        let value = if value.is_reference() {
            &*value.as_ref_ptr()
        } else {
            value
        };
        (value, (*execute_data).num_args)
    }
}

#[inline]
fn argument<'a>(execute_data: *mut ExecuteData, index: u32) -> &'a Value {
    argument_with_count(execute_data, index).0
}

#[inline]
fn optional_argument<'a>(execute_data: *mut ExecuteData, index: u32) -> Option<&'a Value> {
    let value = argument(execute_data, index);
    (value.value_type() != ValueType::Undef).then_some(value)
}

#[inline]
fn set_argument(execute_data: *mut ExecuteData, index: u32, value: Value) {
    // SAFETY: the registered signature reserves this CV, and a reference CV's
    // payload remains live for the duration of the internal call.
    unsafe {
        (*execute_data).cv_mut(index).assign_dereferenced(value);
    }
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
    let mut operation = Some(operation);
    let result = super::resource::with_request_payload_mut::<PhpStream, _>(eg, id, |stream| {
        operation.take().expect("backend operation")(stream)
    });
    #[cfg(feature = "stream-registry")]
    if result.is_none() {
        return filters::with_backend(eg, id, operation.expect("unconsumed backend operation"));
    }
    result
}

#[inline(always)]
pub(super) fn with_stream_io<R>(
    eg: &mut ExecutorGlobals,
    id: i64,
    operation: impl FnOnce(&mut PhpStream) -> R,
) -> Option<R> {
    if eg.static_vars.is_empty() {
        return super::resource::with_request_payload_mut::<PhpStream, _>(eg, id, operation);
    }

    with_stream_io_maybe_cached(eg, id, operation)
}

#[cold]
#[inline(never)]
fn with_stream_io_maybe_cached<R>(
    eg: &mut ExecutorGlobals,
    id: i64,
    operation: impl FnOnce(&mut PhpStream) -> R,
) -> Option<R> {
    if !super::filesystem::filesystem_stat_cache_is_populated(eg) {
        return super::resource::with_request_payload_mut::<PhpStream, _>(eg, id, operation);
    }
    let mut plain_file_io = false;
    let result = super::resource::with_request_payload_mut::<PhpStream, _>(eg, id, |stream| {
        stream.take_plain_file_io();
        let result = operation(stream);
        plain_file_io = stream.take_plain_file_io();
        result
    })?;
    if plain_file_io {
        super::filesystem::clear_filesystem_stat_cache(eg);
    }
    Some(result)
}

#[inline(always)]
pub(super) fn write_stream_bytes(
    eg: &mut ExecutorGlobals,
    _frame: *mut ExecuteData,
    id: i64,
    bytes: &[u8],
) -> Result<Option<std::io::Result<usize>>, VmError> {
    let native = with_stream_io(eg, id, |stream| stream.write(bytes));
    #[cfg(feature = "stream-registry")]
    if native.is_none() {
        return filters::with_source(eg, _frame, |eg| filters::write(eg, id, bytes));
    }
    Ok(native)
}

#[inline]
pub(super) fn read_stream_line(
    eg: &mut ExecutorGlobals,
    _frame: *mut ExecuteData,
    id: i64,
    length: Option<usize>,
) -> Result<Option<Vec<u8>>, VmError> {
    let mut bytes = Vec::new();
    match with_stream_io(eg, id, |stream| stream.read_line(&mut bytes, length)) {
        Some(Ok(Some(_))) => return Ok(Some(bytes)),
        Some(_) => return Ok(None),
        None => {}
    }
    #[cfg(feature = "stream-registry")]
    {
        let maximum = match length {
            Some(length) => length.saturating_sub(1),
            None => usize::MAX,
        };
        return filters::with_source(eg, _frame, |eg| filters::read_line(eg, id, maximum));
    }
    #[cfg(not(feature = "stream-registry"))]
    Ok(None)
}

#[cold]
fn read_stream_csv(
    eg: &mut ExecutorGlobals,
    _frame: *mut ExecuteData,
    id: i64,
    length: Option<usize>,
    separator: u8,
    enclosure: u8,
    escape: Option<u8>,
) -> Result<Option<Vec<Option<Vec<u8>>>>, VmError> {
    match with_stream_io(eg, id, |stream| {
        stream.read_csv_record(length, separator, enclosure, escape)
    }) {
        Some(Ok(fields)) => return Ok(fields),
        Some(Err(_)) => return Ok(None),
        None => {}
    }
    #[cfg(feature = "stream-registry")]
    {
        return filters::with_source(eg, _frame, |eg| {
            filters::read_csv(eg, id, length, separator, enclosure, escape)
        });
    }
    #[cfg(not(feature = "stream-registry"))]
    Ok(None)
}

#[inline]
fn retain_open_argument(value: &Value) -> Value {
    value.clone()
}

#[cold]
fn fn_fopen(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let (path_argument, _supplied) = argument_with_count(execute_data, 0);
    #[cfg(feature = "stream-context")]
    if _supplied > 2 {
        return context::fn_fopen(execute_data, return_pointer, eg);
    }

    // Keep the argument's string owner, not a borrowed reference cell: a
    // wrapper/filter callback may replace that cell while opening. String
    // snapshots share immutable storage instead of allocating two byte copies
    // for every native fopen; coercions retain their existing owned result.
    // Value::clone already follows reference targets. Do not repeat that
    // dispatch before retaining the same immutable argument snapshot.
    let path_snapshot = retain_open_argument(path_argument);
    let mode_snapshot = retain_open_argument(argument(execute_data, 1));
    let path: Cow<'_, str> = match path_snapshot.as_str() {
        Some(value) => Cow::Borrowed(value),
        None => Cow::Owned(path_snapshot.echo_to_string()),
    };
    let mode: Cow<'_, str> = match mode_snapshot.as_str() {
        Some(value) => Cow::Borrowed(value),
        None => Cow::Owned(mode_snapshot.echo_to_string()),
    };
    if !super::filesystem::validate_stream_path(eg, &path, "fopen") {
        return Ok(());
    }
    if !super::filesystem::url_open_allowed(execute_data, eg, &path, "fopen")? {
        return return_value(return_pointer, Value::bool(false));
    }
    #[cfg(feature = "stream-registry")]
    if filters::uri::recognizes(&path) {
        let value = filters::uri::open_internal(eg, execute_data, &path, &mode)?;
        return return_value(return_pointer, value);
    }
    #[cfg(feature = "stream-registry")]
    match user_wrapper::open_file(eg, path.as_ref(), mode.as_ref(), 0)? {
        user_wrapper::OpenResult::Opened(value) => return return_value(return_pointer, value),
        user_wrapper::OpenResult::Declined { class } => {
            if eg.exception.is_some() {
                return Ok(());
            }
            super::report_internal_diagnostic(
                eg,
                execute_data,
                2,
                "Warning",
                &format!(
                    "fopen({path}): Failed to open stream: \"{class}::stream_open\" call failed"
                ),
            )?;
            return return_value(return_pointer, Value::bool(false));
        }
        user_wrapper::OpenResult::NotRegistered => {}
    }
    let value = match PhpStream::open(path.as_ref(), mode.as_ref()) {
        Ok(stream) => {
            if stream.is_plain_file() {
                super::filesystem::clear_filesystem_stat_cache(eg);
            }
            #[cfg(feature = "resource-lifetime")]
            let value = insert_stream(eg, stream);
            #[cfg(not(feature = "resource-lifetime"))]
            let value = Value::resource(insert_stream(eg, stream));
            value
        }
        Err(error) => {
            let reason = match error.kind() {
                std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
                std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
                _ => error.to_string(),
            };
            super::report_internal_diagnostic(
                eg,
                execute_data,
                2,
                "Warning",
                &format!("fopen({path}): Failed to open stream: {reason}"),
            )?;
            Value::bool(false)
        }
    };
    return_value(return_pointer, value)
}

#[cfg(test)]
mod open_argument_snapshot_tests {
    use super::{PhpArray, Value, retain_open_argument};

    #[test]
    fn open_argument_snapshot_retains_binary_storage_after_reference_replacement() {
        let bytes = [0, 127, 128, 255];
        let mut reference = Value::owned_reference(Value::binary_string(&bytes));
        let snapshot = retain_open_argument(&reference);
        let second = retain_open_argument(&snapshot);
        reference.assign_dereferenced(Value::string("replacement"));
        assert!(snapshot.is_binary_string());
        assert!(second.is_binary_string());
        assert_eq!(snapshot.as_str(), Value::binary_string(&bytes).as_str());
        assert_eq!(second.as_str(), snapshot.as_str());
    }

    #[test]
    fn open_argument_snapshot_preserves_non_string_value_kinds() {
        for value in [
            Value::null(),
            Value::bool(false),
            Value::bool(true),
            Value::long(-7),
            Value::double(2.5),
            Value::array(PhpArray::new()),
        ] {
            let snapshot = retain_open_argument(&value);
            assert_eq!(snapshot.value_type(), value.value_type());
            assert_eq!(snapshot.echo_to_string(), value.echo_to_string());
        }
    }
}

#[cold]
fn read_error_message(function: &str, error: &std::io::Error) -> Option<String> {
    let errno = error.raw_os_error()?;
    let reason = error.to_string();
    let suffix = format!(" (os error {errno})");
    let reason = reason.strip_suffix(&suffix).unwrap_or(&reason);
    Some(format!(
        "{function}(): Read of 8192 bytes failed with errno={errno} {reason}"
    ))
}

#[cold]
fn fn_fread(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = checked_args::native_stream_id(execute_data, eg, "fread") else {
        return Ok(());
    };
    let Some(length) = checked_args::stream_long_argument(
        execute_data,
        eg,
        resource,
        "fread",
        1,
        "length",
        "int",
    )?
    else {
        return Ok(());
    };
    if length <= 0 {
        checked_args::positive_length_error(eg, resource, "fread");
        return Ok(());
    }
    let Ok(length) = usize::try_from(length) else {
        return return_value(return_pointer, Value::bool(false));
    };

    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(length).is_err() {
        return return_value(return_pointer, Value::bool(false));
    }
    let result = with_stream_io(eg, resource, |stream| {
        stream.read_into_vec(&mut bytes, length)
    });
    match result {
        Some(Ok(read)) => {
            bytes.truncate(read);
            return_value(return_pointer, super::php_byte_result(bytes, false))
        }
        Some(Err(error)) => {
            if let Some(message) = read_error_message("fread", &error) {
                super::report_internal_diagnostic(eg, execute_data, 8, "Notice", &message)?;
            }
            return_value(return_pointer, Value::bool(false))
        }
        _ => {
            #[cfg(feature = "stream-registry")]
            if let Some(bytes) =
                filters::with_source(eg, execute_data, |eg| filters::read(eg, resource, length))?
            {
                return return_value(return_pointer, super::php_byte_result(bytes, false));
            }
            #[cfg(feature = "stream-registry")]
            if let Some(bytes) = user_wrapper::read(eg, resource, length)? {
                return return_value(return_pointer, super::php_byte_result(bytes, false));
            }
            checked_args::ensure_open_stream(eg, resource, "fread");
            return_value(return_pointer, Value::bool(false))
        }
    }
}

#[cold]
fn fn_fgets(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = checked_args::native_stream_id(execute_data, eg, "fgets") else {
        return Ok(());
    };
    let length = match optional_argument(execute_data, 1) {
        Some(value) if value.value_type() == ValueType::Null => None,
        Some(_) => {
            let Some(length) = checked_args::stream_long_argument(
                execute_data,
                eg,
                resource,
                "fgets",
                1,
                "length",
                "?int",
            )?
            else {
                return Ok(());
            };
            if length <= 0 {
                checked_args::positive_length_error(eg, resource, "fgets");
                return Ok(());
            }
            let Ok(length) = usize::try_from(length) else {
                return return_value(return_pointer, Value::bool(false));
            };
            Some(length)
        }
        None => None,
    };
    let result = read_stream_line(eg, execute_data, resource, length)?;
    match result {
        Some(bytes) => return_value(return_pointer, super::php_byte_result(bytes, false)),
        _ => {
            checked_args::ensure_open_stream(eg, resource, "fgets");
            return_value(return_pointer, Value::bool(false))
        }
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

    let result = if let Some(resource) = resource {
        read_stream_csv(
            eg,
            execute_data,
            resource,
            length,
            separator,
            enclosure,
            escape,
        )?
    } else {
        None
    };
    match result {
        Some(fields) => {
            let mut record = PhpArray::with_packed_capacity(fields.len());
            for field in fields {
                record.push(match field {
                    Some(bytes) => super::php_byte_result(bytes, false),
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
    let Some(resource) = checked_args::native_stream_id(execute_data, eg, "fwrite") else {
        return Ok(());
    };
    // Retain immutable string storage across a filter/error callback instead
    // of copying it into a second String. Coercions use the canonical checker.
    let supplied = argument(execute_data, 1);
    let string_owner = if supplied.as_str().is_some() {
        supplied.clone()
    } else {
        let Some(value) = checked_args::coerce_write_data(execute_data, eg, resource)? else {
            return Ok(());
        };
        value
    };
    let data = string_owner.as_str().expect("validated string argument");
    // Source UTF-8 is already PHP byte storage: its provenance proves that
    // no scan/conversion is needed. Only externally materialized binary
    // storage uses the lossless bridge; ASCII binary data can still borrow.
    let storage = if !string_owner.is_binary_string() || data.is_ascii() {
        Cow::Borrowed(data.as_bytes())
    } else {
        Cow::Owned(super::php_string_to_bytes(data))
    };
    let mut bytes = storage.as_ref();
    if let Some(value) = optional_argument(execute_data, 2)
        && value.value_type() != ValueType::Null
    {
        let Some(length) = checked_args::stream_long_argument(
            execute_data,
            eg,
            resource,
            "fwrite",
            2,
            "length",
            "?int",
        )?
        else {
            return Ok(());
        };
        bytes = &bytes[..bytes
            .len()
            .min(usize::try_from(length.max(0)).unwrap_or(usize::MAX))];
    }
    let result = write_stream_bytes(eg, execute_data, resource, bytes)?;
    match result {
        Some(Ok(written)) => return_value(return_pointer, Value::long(written as i64)),
        Some(Err(error)) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            report_stream_not_writable(eg, execute_data)?;
            return_value(return_pointer, Value::bool(false))
        }
        None => {
            checked_args::ensure_open_stream(eg, resource, "fwrite");
            return_value(return_pointer, Value::bool(false))
        }
        _ => return_value(return_pointer, Value::bool(false)),
    }
}

// Keep the user-callback/error path out of the successful write body. In
// particular it must not inflate ordinary native or memory I/O through LTO.
#[cold]
#[inline(never)]
fn report_stream_not_writable(
    eg: &mut ExecutorGlobals,
    execute_data: *mut ExecuteData,
) -> Result<(), VmError> {
    super::report_internal_diagnostic(
        eg,
        execute_data,
        8,
        "Notice",
        "fwrite(): Stream is not writable",
    )?;
    Ok(())
}

#[cold]
fn fn_fclose(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = checked_args::native_stream_id(execute_data, eg, "fclose") else {
        return Ok(());
    };
    let closed = super::resource::close_for_request::<PhpStream>(eg, resource);
    if closed {
        return return_value(return_pointer, Value::bool(true));
    }
    #[cfg(feature = "stream-registry")]
    if let Some(closed) =
        filters::with_source(eg, execute_data, |eg| filters::close(eg, resource, false))?
    {
        return return_value(return_pointer, Value::bool(closed));
    }
    #[cfg(feature = "stream-registry")]
    if let Some(closed) = user_wrapper::close(eg, resource)? {
        return return_value(return_pointer, Value::bool(closed));
    }
    checked_args::ensure_open_stream(eg, resource, "fclose");
    return_value(return_pointer, Value::bool(false))
}

#[cold]
fn fn_fflush(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = checked_args::native_stream_id(execute_data, eg, "fflush") else {
        return Ok(());
    };
    if let Some(flushed) = with_stream_io(eg, resource, |stream| stream.flush().is_ok()) {
        return return_value(return_pointer, Value::bool(flushed));
    }
    #[cfg(feature = "stream-registry")]
    if let Some(flushed) =
        filters::with_source(eg, execute_data, |eg| filters::flush(eg, resource))?
    {
        return return_value(return_pointer, Value::bool(flushed));
    }
    #[cfg(feature = "stream-registry")]
    if let Some(flushed) = user_wrapper::flush(eg, resource)? {
        return return_value(return_pointer, Value::bool(flushed));
    }
    checked_args::ensure_open_stream(eg, resource, "fflush");
    return_value(return_pointer, Value::bool(false))
}

#[cold]
fn fn_flock(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let stream = argument(execute_data, 0);
    let Some(resource) = stream.as_resource_id() else {
        super::typed_internal_argument_error(eg, "flock", stream, 1, "stream", "resource");
        return Ok(());
    };
    let supplied_operation = argument(execute_data, 1);
    let operation = if supplied_operation.value_type() == ValueType::Long {
        supplied_operation.as_long().unwrap_or_default()
    } else {
        if !super::resource::is_open_for_request(eg, resource) {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                "flock(): Argument #1 ($stream) must be an open stream resource",
            ));
            return Ok(());
        }
        let Some(operation) =
            super::typed_internal_int_argument(execute_data, eg, "flock", 1, "operation")?
        else {
            return Ok(());
        };
        operation
    };
    let result = with_stream(eg, resource, |stream| {
        (1..=3)
            .contains(&(operation & 3))
            .then(|| stream.lock(operation))
    });
    let Some(result) = result else {
        if !super::resource::is_open_for_request(eg, resource) {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                "flock(): Argument #1 ($stream) must be an open stream resource",
            ));
            return Ok(());
        }
        if optional_argument(execute_data, 2).is_some() {
            set_argument(execute_data, 2, Value::long(0));
        }
        return return_value(return_pointer, Value::bool(false));
    };
    let Some(result) = result else {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "flock(): Argument #2 ($operation) must be one of LOCK_SH, LOCK_EX, or LOCK_UN",
        ));
        return Ok(());
    };
    let has_would_block = optional_argument(execute_data, 2).is_some();
    if has_would_block {
        set_argument(execute_data, 2, Value::long(0));
    }
    let locked = match result {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            if has_would_block {
                set_argument(execute_data, 2, Value::long(1));
            }
            false
        }
        Err(_) => false,
    };
    return_value(return_pointer, Value::bool(locked))
}

fn virtual_stream_stat_fields(size: u64, writable: bool) -> [i64; 13] {
    [
        12,
        0,
        if writable { 0o100666 } else { 0o100444 },
        1,
        0,
        0,
        -1,
        i64::try_from(size).unwrap_or(i64::MAX),
        0,
        0,
        0,
        -1,
        -1,
    ]
}

#[cold]
fn fn_fstat(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let stream = argument(execute_data, 0);
    let Some(resource) = stream.as_resource_id() else {
        super::typed_internal_argument_error(eg, "fstat", stream, 1, "stream", "resource");
        return Ok(());
    };
    if !super::resource::is_open_for_request(eg, resource)
        || super::resource::type_for_request(eg, resource) != "stream"
    {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "fstat(): Argument #1 ($stream) must be an open stream resource",
        ));
        return Ok(());
    }

    if let Some(stat) = with_stream(eg, resource, |stream| stream.stat()) {
        let value = match stat {
            Ok(Some(StreamStat::Filesystem(metadata))) => super::filesystem::stat_array_value(
                super::filesystem::metadata_stat_fields(&metadata),
            ),
            Ok(Some(StreamStat::Memory { size, writable })) => {
                super::filesystem::stat_array_value(virtual_stream_stat_fields(size, writable))
            }
            Ok(None) | Err(_) => Value::bool(false),
        };
        return return_value(return_pointer, value);
    }

    #[cfg(feature = "stream-registry")]
    let value = user_wrapper::stat(eg, execute_data, resource)?;
    #[cfg(feature = "stream-registry")]
    if eg.exception.is_some() {
        return Ok(());
    }
    #[cfg(feature = "stream-registry")]
    if let Some(value) = value {
        return return_value(
            return_pointer,
            super::filesystem::normalize_wrapper_stat(value),
        );
    }

    return_value(return_pointer, Value::bool(false))
}

#[cold]
fn fn_feof(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = checked_args::native_stream_id(execute_data, eg, "feof") else {
        return Ok(());
    };
    if let Some(eof) =
        super::resource::with_request_payload_mut::<PhpStream, _>(eg, resource, |stream| {
            stream.is_eof()
        })
    {
        return return_value(return_pointer, Value::bool(eof));
    }
    #[cfg(feature = "stream-registry")]
    if let Some(eof) = filters::eof(eg, resource) {
        return return_value(return_pointer, Value::bool(eof));
    }
    #[cfg(feature = "stream-registry")]
    if user_wrapper::is_user_stream(eg, resource) {
        let eof = user_wrapper::eof(eg, resource)?.unwrap_or(false);
        return return_value(return_pointer, Value::bool(eof));
    }
    checked_args::ensure_open_stream(eg, resource, "feof");
    return_value(return_pointer, Value::bool(false))
}

#[cold]
fn fn_ftell(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = checked_args::native_stream_id(execute_data, eg, "ftell") else {
        return Ok(());
    };
    let position = with_stream_io(eg, resource, |stream| stream.position());
    match position {
        Some(Ok(position)) if position <= i64::MAX as u64 => {
            return_value(return_pointer, Value::long(position as i64))
        }
        _ => {
            #[cfg(feature = "stream-registry")]
            if let Some(position) = filters::position(eg, resource) {
                return return_value(return_pointer, Value::long(position as i64));
            }
            #[cfg(feature = "stream-registry")]
            if let Some(position) = user_wrapper::position(eg, resource) {
                return return_value(return_pointer, Value::long(position));
            }
            checked_args::ensure_open_stream(eg, resource, "ftell");
            return_value(return_pointer, Value::bool(false))
        }
    }
}

#[cold]
fn fn_fseek(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = checked_args::native_stream_id(execute_data, eg, "fseek") else {
        return Ok(());
    };
    let Some(offset) = checked_args::stream_long_argument(
        execute_data,
        eg,
        resource,
        "fseek",
        1,
        "offset",
        "int",
    )?
    else {
        return Ok(());
    };
    let whence = if optional_argument(execute_data, 2).is_some() {
        let Some(whence) = checked_args::stream_long_argument(
            execute_data,
            eg,
            resource,
            "fseek",
            2,
            "whence",
            "int",
        )?
        else {
            return Ok(());
        };
        whence
    } else {
        0
    };
    let seek_from = match whence {
        0 => match u64::try_from(offset) {
            Ok(offset) => SeekFrom::Start(offset),
            Err(_) => {
                checked_args::ensure_open_stream(eg, resource, "fseek");
                return return_value(return_pointer, Value::long(-1));
            }
        },
        1 => SeekFrom::Current(offset),
        2 => SeekFrom::End(offset),
        _ => {
            checked_args::ensure_open_stream(eg, resource, "fseek");
            return return_value(return_pointer, Value::long(-1));
        }
    };
    let succeeded =
        super::resource::with_request_payload_mut::<PhpStream, _>(eg, resource, |stream| {
            stream.seek(seek_from).is_ok()
        });
    #[cfg(feature = "stream-registry")]
    let succeeded = match succeeded {
        None => filters::with_source(eg, execute_data, |eg| {
            filters::seek(eg, resource, seek_from)
        })?,
        value => value,
    };
    if succeeded.is_none() {
        checked_args::ensure_open_stream(eg, resource, "fseek");
    }
    let succeeded = succeeded.unwrap_or(false);
    return_value(return_pointer, Value::long(if succeeded { 0 } else { -1 }))
}

#[cold]
fn fn_rewind(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = checked_args::native_stream_id(execute_data, eg, "rewind") else {
        return Ok(());
    };
    let succeeded =
        super::resource::with_request_payload_mut::<PhpStream, _>(eg, resource, PhpStream::rewind);
    #[cfg(feature = "stream-registry")]
    let succeeded = match succeeded {
        None => filters::with_source(eg, execute_data, |eg| {
            filters::seek(eg, resource, SeekFrom::Start(0))
        })?,
        value => value,
    };
    if succeeded.is_none() {
        checked_args::ensure_open_stream(eg, resource, "rewind");
    }
    let succeeded = succeeded.unwrap_or(false);
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
    let resource = argument(execute_data, 0).as_resource_id();
    #[cfg(feature = "stream-registry")]
    let filtered_unread = resource.and_then(|id| filters::unread_bytes(eg, id));
    let value = resource.and_then(|resource| {
        with_stream(eg, resource, |stream| {
            let metadata = stream.metadata();
            let status_fields = usize::from(metadata.timed_out.is_some())
                + usize::from(metadata.blocked.is_some())
                + usize::from(metadata.eof.is_some());
            let mut result = if metadata.wrapper_type == "RFC2397" {
                super::filesystem::data_uri_metadata(metadata.uri).unwrap_or_else(PhpArray::new)
            } else {
                PhpArray::with_hash_capacity(6 + status_fields)
            };
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
            let unread = metadata.unread_bytes;
            #[cfg(feature = "stream-registry")]
            let unread = filtered_unread.unwrap_or(unread);
            result.set_str("unread_bytes", Value::long(unread as i64));
            result.set_str("seekable", Value::bool(metadata.seekable));
            result.set_str("uri", Value::string(metadata.uri));
            Value::array(result)
        })
    });
    #[cfg(feature = "stream-registry")]
    let value = if value.is_some() {
        value
    } else if let Some(resource) = resource {
        user_wrapper::metadata(eg, resource)?
    } else {
        None
    };
    let value = value.unwrap_or_else(|| Value::bool(false));
    return_value(return_pointer, value)
}
