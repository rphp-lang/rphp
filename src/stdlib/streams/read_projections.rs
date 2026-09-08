//! Byte and output/count projections of the existing stream cursor. Reading
//! uses the same prebuffer, EOF and stat-cache ownership as fread. No resource
//! borrow survives a diagnostic, filter callback, or output operation.

use crate::runtime::ExecutorGlobals;
use crate::value::Value;
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{
    FunctionCommon, InternalFunction, InternalFunctionHandler, ParamTypeHint,
};

use super::{checked_args, return_value, with_stream_io};

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn register(eg: &mut ExecutorGlobals, functions: &mut Vec<Box<InternalFunction>>) {
    // Keep the existing native registration loop and allocation order stable.
    // New signatures belong to their own small read-projection boundary.
    for (name, handler, result) in [
        (
            "fgetc",
            fn_fgetc as InternalFunctionHandler,
            ParamTypeHint::Union(vec![
                ParamTypeHint::String,
                ParamTypeHint::ClassName("false".to_string()),
            ]),
        ),
        ("fpassthru", fn_fpassthru, ParamTypeHint::Int),
    ] {
        let mut function = Box::new(crate::compiler::make_internal_function(
            handler,
            1,
            1,
            vec!["stream".to_string()],
        ));
        function.common.sig.param_type_hints = vec![ParamTypeHint::None];
        function.common.sig.return_type_hint = result;
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(name, pointer).unwrap();
        eg.register_internal_function_reflection_metadata(pointer, vec![None], "standard");
        functions.push(function);
    }
}

#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn fn_fgetc(
    frame: *mut ExecuteData,
    result: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(id) = checked_args::native_stream_id(frame, eg, "fgetc") else {
        return Ok(());
    };
    let mut byte = [0];
    let value = match read(eg, frame, id, &mut byte, "fgetc")? {
        Some(1) => Value::binary_string(&byte),
        _ => Value::bool(false),
    };
    return_value(result, value)
}

#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn fn_fpassthru(
    frame: *mut ExecuteData,
    result: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(id) = checked_args::native_stream_id(frame, eg, "fpassthru") else {
        return Ok(());
    };
    let mut bytes = [0; 8192];
    let mut total = 0usize;
    loop {
        match read(eg, frame, id, &mut bytes, "fpassthru")? {
            Some(0) => break,
            Some(count) => {
                if eg.exception.is_some() {
                    return Ok(());
                }
                eg.write_output(&bytes[..count]);
                total += count;
                if eg.exception.is_some() {
                    return Ok(());
                }
            }
            None => {
                // PHP reports a read failure only when nothing was emitted;
                // after partial output it returns the already delivered count.
                let count = if total == 0 { -1 } else { total as i64 };
                return return_value(result, Value::long(count));
            }
        }
    }
    return_value(result, Value::long(total as i64))
}

#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn read(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    id: i64,
    bytes: &mut [u8],
    function: &str,
) -> Result<Option<usize>, VmError> {
    match read_native(eg, id, bytes) {
        Some(Ok(count)) => Ok(Some(count)),
        Some(Err(error)) => {
            report_read_error(eg, frame, function, &error)?;
            Ok(None)
        }
        None => read_callback_stream(eg, frame, id, bytes, function),
    }
}

// Share the native payload/cursor projection with fread instead of generating
// a second copy of its resource and stat-cache lookup for scalar readers.
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn read_native(
    eg: &mut ExecutorGlobals,
    id: i64,
    bytes: &mut [u8],
) -> Option<std::io::Result<usize>> {
    with_stream_io(eg, id, |stream| stream.read(bytes))
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn report_read_error(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    function: &str,
    error: &std::io::Error,
) -> Result<(), VmError> {
    if let Some(message) = super::read_error_message(function, error) {
        super::super::report_internal_diagnostic(eg, frame, 8, "Notice", &message)?;
    }
    Ok(())
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn read_callback_stream(
    eg: &mut ExecutorGlobals,
    _frame: *mut ExecuteData,
    id: i64,
    _bytes: &mut [u8],
    function: &str,
) -> Result<Option<usize>, VmError> {
    #[cfg(feature = "stream-registry")]
    {
        let bytes = super::filters::with_source(eg, _frame, |eg| {
            super::filters::read(eg, id, _bytes.len())
        })?;
        let bytes = match bytes {
            Some(bytes) => Some(bytes),
            None if eg.exception.is_some() => return Ok(None),
            None => super::user_wrapper::read(eg, id, _bytes.len())?,
        };
        if let Some(bytes) = bytes {
            _bytes[..bytes.len()].copy_from_slice(&bytes);
            return Ok(Some(bytes.len()));
        }
    }
    checked_args::ensure_open_stream(eg, id, function);
    Ok(None)
}
