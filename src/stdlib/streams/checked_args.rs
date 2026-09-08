use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

// Successful native I/O validates the resource through its existing payload
// lookup. Only type/coercion failures and an operation miss need the cold
// open-stream check, before a later argument can emit a diagnostic/callback.
#[inline]
pub(super) fn native_stream_id(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Option<i64> {
    let value = super::argument(execute_data, 0);
    match value.as_resource_id() {
        Some(id) => Some(id),
        None => {
            stream_type_error(eg, function, 0, "stream", value);
            None
        }
    }
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn stream_type_error(
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
    value: &Value,
) {
    let argument = index + 1;
    argument_error(
        eg,
        "TypeError",
        format!(
            "{function}(): Argument #{argument} (${parameter}) must be of type resource, {} given",
            given_type_name(value)
        ),
    );
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn ensure_open_stream(eg: &mut ExecutorGlobals, resource: i64, function: &str) -> bool {
    ensure_open_stream_at(eg, resource, function, 0, "stream")
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn ensure_open_stream_at(
    eg: &mut ExecutorGlobals,
    resource: i64,
    function: &str,
    index: u32,
    parameter: &str,
) -> bool {
    if eg.exception.is_some() {
        return false;
    }
    if super::super::resource::type_for_request(eg, resource) == "stream" {
        return true;
    }
    let argument = index + 1;
    argument_error(
        eg,
        "TypeError",
        format!(
            "{function}(): Argument #{argument} (${parameter}) must be an open stream resource"
        ),
    );
    false
}

#[inline]
pub(super) fn stream_long_argument(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resource: i64,
    function: &str,
    index: u32,
    parameter: &str,
    expected: &str,
) -> Result<Option<i64>, VmError> {
    let value = super::argument(execute_data, index);
    if let Some(integer) = value.as_long() {
        return Ok(Some(integer));
    }
    coerce_stream_long_argument(
        execute_data,
        eg,
        resource,
        function,
        index,
        parameter,
        expected,
    )
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn coerce_stream_long_argument(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resource: i64,
    function: &str,
    index: u32,
    parameter: &str,
    expected: &str,
) -> Result<Option<i64>, VmError> {
    // Exact weak conversions cannot call user code or emit a diagnostic, so
    // the operation's payload lookup can still validate the stream once.
    // Fractional/null/invalid inputs must validate it before entering the
    // canonical coercer, which can invoke an error handler.
    if !super::super::internal_call_is_strict(execute_data) {
        let value = super::argument(execute_data, index);
        let converted = match value.value_type() {
            ValueType::String => value.as_str().and_then(|source| {
                source
                    .trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}'])
                    .parse::<i64>()
                    .ok()
            }),
            ValueType::True => Some(1),
            ValueType::False => Some(0),
            ValueType::Double => value.as_double().and_then(|number| {
                if number >= i64::MIN as f64
                    && number < -(i64::MIN as f64)
                    && (number as i64) as f64 == number
                {
                    Some(number as i64)
                } else {
                    None
                }
            }),
            _ => None,
        };
        if converted.is_some() {
            return Ok(converted);
        }
    }
    if !ensure_open_stream(eg, resource, function) {
        return Ok(None);
    }
    super::super::typed_internal_int_argument_expected(
        execute_data,
        eg,
        function,
        index,
        parameter,
        expected,
    )
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn positive_length_error(eg: &mut ExecutorGlobals, resource: i64, function: &str) {
    if ensure_open_stream(eg, resource, function) {
        argument_error(
            eg,
            "ValueError",
            format!("{function}(): Argument #2 ($length) must be greater than 0"),
        );
    }
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn coerce_write_data(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resource: i64,
) -> Result<Option<Value>, VmError> {
    if !ensure_open_stream(eg, resource, "fwrite") {
        return Ok(None);
    }
    super::super::typed_internal_string_value_argument_expected(
        execute_data,
        eg,
        "fwrite",
        1,
        "data",
        "string",
    )
}

#[cold]
#[cfg(any(
    feature = "csv-errors",
    feature = "stream-contents",
    feature = "stream-copy",
    feature = "stream-line",
    feature = "stream-truncate",
    feature = "file-contents",
    feature = "file-write",
    feature = "file-lines"
))]
pub(in crate::stdlib) fn weak_long_argument(value: &Value) -> Option<i64> {
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
pub(in crate::stdlib) fn argument_error(eg: &mut ExecutorGlobals, class: &str, message: String) {
    debug_assert!(eg.exception.is_none());
    eg.exception = Some(crate::value::make_error_value(class, &message));
}

#[cold]
pub(in crate::stdlib) fn given_type_name(value: &Value) -> String {
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
#[cfg(any(
    feature = "csv-errors",
    feature = "stream-contents",
    feature = "stream-copy"
))]
pub(super) fn stream_argument_at(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Option<i64> {
    let value = super::argument(execute_data, index);
    let Some(resource) = value.as_resource_id() else {
        stream_type_error(eg, function, index, parameter, value);
        return None;
    };
    ensure_open_stream_at(eg, resource, function, index, parameter).then_some(resource)
}
