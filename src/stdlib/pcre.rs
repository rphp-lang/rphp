//! Cold PHP 8.5 PCRE surface that builds on the shared regex engine.
//!
//! The engine intentionally remains a separately bounded compatibility layer:
//! these handlers expose the missing collection/callback/error contracts
//! without claiming complete PCRE2 syntax, backtracking limits or JIT.

use std::borrow::Cow;
use std::rc::Rc;

use super::{ResolvedCallback, regex_callback};
use crate::compiler::{make_internal_function, make_internal_function_ref};
use crate::regex::Regex;
use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction, ParamTypeHint};

const PREG_NO_ERROR: u8 = 0;
const PREG_INTERNAL_ERROR: u8 = 1;
pub(super) const PREG_BAD_UTF8_ERROR: u8 = 4;
pub(super) const PREG_BAD_UTF8_OFFSET_ERROR: u8 = 5;
const PREG_OFFSET_CAPTURE: i64 = 256;
const PREG_UNMATCHED_AS_NULL: i64 = 512;
const PREG_GREP_INVERT: i64 = 1;

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn write_value(return_value: *mut Value, value: Value) {
    super::write_return_value(return_value, value);
}

fn preg_error_message(code: u8) -> &'static str {
    match code {
        0 => "No error",
        1 => "Internal error",
        2 => "Backtrack limit exhausted",
        3 => "Recursion limit exhausted",
        4 => "Malformed UTF-8 characters, possibly incorrectly encoded",
        5 => "The offset did not correspond to the beginning of a valid UTF-8 code point",
        6 => "JIT stack limit exhausted",
        _ => "Unknown error",
    }
}

fn rendered_compile_error(pattern: &str, error: &str) -> String {
    if matches!(
        error,
        "Empty regular expression" | "Delimiter must not be alphanumeric, backslash, or NUL byte"
    ) || error.starts_with("No ending delimiter")
        || error.starts_with("Unknown modifier")
    {
        return error.to_string();
    }
    if error == "Unterminated character class" {
        let offset = pattern.len().saturating_sub(2);
        return format!(
            "Compilation failed: missing terminating ] for character class at offset {offset}"
        );
    }
    format!("Compilation failed: {error}")
}

/// Compile one public preg pattern, updating the request-local error slot and
/// routing syntax diagnostics through PHP's user error handler.
pub(super) fn compile_pattern(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    function: &str,
    pattern: &str,
) -> Result<Option<Rc<Regex>>, VmError> {
    eg.regex_cache.set_last_error(PREG_NO_ERROR);
    match eg.regex_cache.get_or_compile(pattern) {
        Ok(regex) => Ok(Some(regex)),
        Err(error) => {
            eg.regex_cache.set_last_error(PREG_INTERNAL_ERROR);
            // A valid PCRE construct that the custom engine does not yet
            // implement is an explicit engine non-claim, not a PHP pattern
            // compilation failure. Preserve the pre-existing false/null
            // result without adding a warning that reference PHP would never
            // emit. Truly malformed patterns still publish the exact warning.
            if !error.starts_with("Unsupported PCRE ") {
                super::report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    &format!("{function}(): {}", rendered_compile_error(pattern, &error)),
                )?;
            }
            Ok(None)
        }
    }
}

/// Project a PHP byte string into the engine's Unicode scalar input without
/// losing PHP's byte-offset contract. PCRE validates only the suffix beginning
/// at the supplied offset, but an offset on a UTF-8 continuation byte is a
/// distinct error from malformed subject data.
pub(super) fn prepare_utf_subject<'a>(
    value: &'a Value,
    rendered: Cow<'a, str>,
    offset: usize,
) -> Result<Cow<'a, str>, u8> {
    let value = value.dereferenced();
    let Some(bytes) = value.php_string_bytes() else {
        if !rendered.is_char_boundary(offset) {
            return Err(PREG_BAD_UTF8_OFFSET_ERROR);
        }
        return Ok(match rendered {
            Cow::Borrowed(subject) => Cow::Borrowed(&subject[offset..]),
            Cow::Owned(subject) => Cow::Owned(subject[offset..].to_string()),
        });
    };
    if offset < bytes.len() && bytes[offset] & 0b1100_0000 == 0b1000_0000 {
        return Err(PREG_BAD_UTF8_OFFSET_ERROR);
    }

    match bytes {
        Cow::Borrowed(bytes) => std::str::from_utf8(&bytes[offset..])
            .map(Cow::Borrowed)
            .map_err(|_| PREG_BAD_UTF8_ERROR),
        Cow::Owned(mut bytes) => {
            if offset != 0 {
                bytes = bytes.split_off(offset);
            }
            String::from_utf8(bytes)
                .map(Cow::Owned)
                .map_err(|_| PREG_BAD_UTF8_ERROR)
        }
    }
}

#[inline]
pub(super) fn set_last_error(eg: &mut ExecutorGlobals, error: u8) {
    eg.regex_cache.set_last_error(error);
}

fn required_string(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<String>, VmError> {
    super::typed_internal_string_argument(ed, eg, function, index, parameter)
}

fn required_array(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Option<Value> {
    let value = super::owned_argument(ed, index);
    let value = value.dereferenced();
    if value.value_type() == ValueType::Array {
        return Some(value.clone());
    }
    super::typed_internal_argument_error(
        eg,
        function,
        value,
        index as usize + 1,
        parameter,
        "array",
    );
    None
}

fn optional_int(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
    default: i64,
) -> Result<Option<i64>, VmError> {
    if arg_opt!(ed, index).is_none() {
        return Ok(Some(default));
    }
    super::typed_internal_int_argument(ed, eg, function, index, parameter)
}

fn replacement_limit(limit: i64) -> usize {
    if limit < 0 {
        usize::MAX
    } else {
        usize::try_from(limit).unwrap_or(usize::MAX)
    }
}

fn set_count(ed: *mut ExecuteData, index: u32, count: usize) {
    if arg_opt!(ed, index).is_some() {
        arg_mut!(ed, index, Value::long(count as i64));
    }
}

fn array_keyed_value(result: &mut PhpArray, key: ArrayKey, value: Value) {
    match key {
        ArrayKey::Int(key) => result.set_int(key, value),
        ArrayKey::String(key) => result.set_str(&key, value),
    }
}

fn argument_strings(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<(Vec<String>, bool)>, VmError> {
    if let Some(values) = value.as_array() {
        let mut rendered = Vec::with_capacity(values.len());
        for (_, value) in values.iter() {
            let Some(value) = super::internal_value_to_string(ed, eg, value)? else {
                return Ok(None);
            };
            rendered.push(value);
        }
        Ok(Some((rendered, true)))
    } else {
        let Some(rendered) = super::internal_value_to_string(ed, eg, value)? else {
            return Ok(None);
        };
        Ok(Some((vec![rendered], false)))
    }
}

fn replace_strings(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    function: &str,
    patterns: &[String],
    replacements: &[String],
    replacement_is_array: bool,
    subject: &str,
    limit: usize,
) -> Result<(Option<String>, usize), VmError> {
    let mut result = subject.to_string();
    let mut count = 0;
    for (index, pattern) in patterns.iter().enumerate() {
        let replacement = if replacement_is_array {
            replacements.get(index).map_or("", String::as_str)
        } else {
            replacements.first().map_or("", String::as_str)
        };
        let Some(regex) = compile_pattern(eg, ed, function, pattern)? else {
            return Ok((None, count));
        };
        let (replaced, replacements) = regex.replace_limit(&result, replacement, limit);
        result = replaced;
        count += replacements;
    }
    Ok((Some(result), count))
}

pub(super) fn fn_preg_filter(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(pattern_value) =
        super::typed_internal_array_or_string_argument(ed, eg, "preg_filter", 0, "pattern")?
    else {
        return Ok(());
    };
    let Some(replacement_value) =
        super::typed_internal_array_or_string_argument(ed, eg, "preg_filter", 1, "replacement")?
    else {
        return Ok(());
    };
    let Some(subject_value) =
        super::typed_internal_array_or_string_argument(ed, eg, "preg_filter", 2, "subject")?
    else {
        return Ok(());
    };
    let Some(limit) = optional_int(ed, eg, "preg_filter", 3, "limit", -1)? else {
        return Ok(());
    };
    let Some((patterns, _)) = argument_strings(ed, eg, &pattern_value)? else {
        return Ok(());
    };
    let Some((replacements, replacement_is_array)) = argument_strings(ed, eg, &replacement_value)?
    else {
        return Ok(());
    };
    let limit = replacement_limit(limit);
    let mut total_count = 0usize;

    if let Some(subjects) = subject_value.as_array() {
        let mut result = PhpArray::new();
        for (key, subject) in subjects.iter() {
            let Some(subject) = super::internal_value_to_string(ed, eg, subject)? else {
                return Ok(());
            };
            let (replaced, count) = replace_strings(
                eg,
                ed,
                "preg_filter",
                &patterns,
                &replacements,
                replacement_is_array,
                &subject,
                limit,
            )?;
            total_count += count;
            let Some(replaced) = replaced else {
                set_count(ed, 4, total_count);
                write_value(rv, Value::null());
                return Ok(());
            };
            if count != 0 {
                array_keyed_value(&mut result, key, Value::string(replaced));
            }
        }
        super::copy_array_key_provenance(subjects, &result);
        set_count(ed, 4, total_count);
        write_value(rv, Value::array(result));
        return Ok(());
    }

    let Some(subject) = super::internal_value_to_string(ed, eg, &subject_value)? else {
        return Ok(());
    };
    let (replaced, count) = replace_strings(
        eg,
        ed,
        "preg_filter",
        &patterns,
        &replacements,
        replacement_is_array,
        &subject,
        limit,
    )?;
    let Some(replaced) = replaced else {
        set_count(ed, 4, count);
        write_value(rv, Value::null());
        return Ok(());
    };
    set_count(ed, 4, count);
    write_value(
        rv,
        if count == 0 {
            Value::null()
        } else {
            Value::string(replaced)
        },
    );
    Ok(())
}

pub(super) fn fn_preg_grep(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(pattern) = required_string(ed, eg, "preg_grep", 0, "pattern")? else {
        return Ok(());
    };
    let Some(values) = required_array(ed, eg, "preg_grep", 1, "array") else {
        return Ok(());
    };
    let Some(flags) = optional_int(ed, eg, "preg_grep", 2, "flags", 0)? else {
        return Ok(());
    };
    let Some(regex) = compile_pattern(eg, ed, "preg_grep", &pattern)? else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    let values = values.as_array().expect("preg_grep array was validated");
    let invert = flags & PREG_GREP_INVERT != 0;
    let mut result = PhpArray::new();
    for (key, value) in values.iter() {
        let Some(subject_value) = super::internal_value_to_string_value(ed, eg, value)? else {
            return Ok(());
        };
        let rendered = Cow::Borrowed(
            subject_value
                .as_str()
                .expect("preg_grep string conversion must produce a string"),
        );
        let subject = if regex.is_unicode() {
            match prepare_utf_subject(&subject_value, rendered, 0) {
                Ok(subject) => subject,
                Err(error) => {
                    set_last_error(eg, error);
                    break;
                }
            }
        } else {
            rendered
        };
        if regex.is_match(&subject) != invert {
            array_keyed_value(&mut result, key, value.clone_for_php_storage());
        }
    }
    super::copy_array_key_provenance(values, &result);
    write_value(rv, Value::array(result));
    Ok(())
}

pub(super) fn fn_preg_last_error(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write_value(rv, Value::long(i64::from(eg.regex_cache.last_error())));
    Ok(())
}

pub(super) fn fn_preg_last_error_msg(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write_value(
        rv,
        Value::string(preg_error_message(eg.regex_cache.last_error())),
    );
    Ok(())
}

fn callback_error(eg: &mut ExecutorGlobals, message: &str) {
    eg.exception = Some(crate::value::make_error_value("TypeError", message));
}

fn resolve_array_callback(
    callback: &Value,
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<Option<ResolvedCallback>, VmError> {
    let resolved = super::resolve_callback_at_callsite_checked(callback, eg, ed)?;
    if resolved.is_none() && eg.exception.is_none() {
        callback_error(
            eg,
            "preg_replace_callback_array(): Argument #1 ($pattern) must contain only valid callbacks",
        );
    }
    Ok(resolved)
}

fn replace_callback_value(
    value: &Value,
    regex: &Regex,
    callback: &ResolvedCallback,
    limit: usize,
    flags: i64,
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<Option<(Value, usize)>, VmError> {
    let Some(subject) = super::internal_value_to_string(ed, eg, value)? else {
        return Ok(None);
    };
    let result = regex_callback::replace(
        regex,
        subject,
        callback,
        limit,
        flags & PREG_UNMATCHED_AS_NULL != 0,
        flags & PREG_OFFSET_CAPTURE != 0,
        eg,
    )?;
    Ok(result.map(|(value, count)| (Value::string(value), count)))
}

pub(super) fn fn_preg_replace_callback_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(patterns) = required_array(ed, eg, "preg_replace_callback_array", 0, "pattern") else {
        return Ok(());
    };
    let Some(subject) = super::typed_internal_array_or_string_argument(
        ed,
        eg,
        "preg_replace_callback_array",
        1,
        "subject",
    )?
    else {
        return Ok(());
    };
    let Some(limit) = optional_int(ed, eg, "preg_replace_callback_array", 2, "limit", -1)? else {
        return Ok(());
    };
    let Some(flags) = optional_int(ed, eg, "preg_replace_callback_array", 4, "flags", 0)? else {
        return Ok(());
    };
    let limit = replacement_limit(limit);
    let patterns = patterns
        .as_array()
        .expect("preg_replace_callback_array patterns were validated");
    let mut total_count = 0usize;
    let mut scalar = subject.as_array().is_none().then_some(subject.clone());
    let mut array = subject.as_array().map(|values| {
        values
            .iter()
            .map(|(key, value)| (key, value.clone()))
            .collect::<Vec<_>>()
    });

    for (key, callback) in patterns.iter() {
        let ArrayKey::String(pattern) = key else {
            callback_error(
                eg,
                "preg_replace_callback_array(): Argument #1 ($pattern) must contain only string patterns as keys",
            );
            return Ok(());
        };
        let Some(callback) = resolve_array_callback(callback, ed, eg)? else {
            return Ok(());
        };
        let Some(regex) = compile_pattern(eg, ed, "preg_replace_callback_array", &pattern)? else {
            write_value(rv, Value::null());
            return Ok(());
        };

        if let Some(value) = scalar.as_mut() {
            let Some((replaced, count)) =
                replace_callback_value(value, &regex, &callback, limit, flags, ed, eg)?
            else {
                return Ok(());
            };
            *value = replaced;
            total_count += count;
        } else if let Some(values) = array.as_mut() {
            for (_, value) in values.iter_mut() {
                let Some((replaced, count)) =
                    replace_callback_value(value, &regex, &callback, limit, flags, ed, eg)?
                else {
                    return Ok(());
                };
                *value = replaced;
                total_count += count;
            }
        }
    }

    set_count(ed, 3, total_count);
    if let Some(value) = scalar {
        write_value(rv, value);
    } else {
        let mut result = PhpArray::new();
        for (key, value) in array.unwrap_or_default() {
            array_keyed_value(&mut result, key, value);
        }
        if let Some(subject) = subject.as_array() {
            super::copy_array_key_provenance(subject, &result);
        }
        write_value(rv, Value::array(result));
    }
    Ok(())
}

fn array_string_union() -> ParamTypeHint {
    ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::String])
}

fn array_string_null_union() -> ParamTypeHint {
    ParamTypeHint::Union(vec![
        ParamTypeHint::Array,
        ParamTypeHint::String,
        ParamTypeHint::ClassName("null".into()),
    ])
}

pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    struct Declaration {
        name: &'static str,
        handler: crate::vm::function::InternalFunctionHandler,
        required: u32,
        parameters: &'static [&'static str],
        parameter_types: fn() -> Vec<ParamTypeHint>,
        return_type: fn() -> ParamTypeHint,
        defaults: fn() -> Vec<Option<Value>>,
        reference_arguments: u64,
    }

    let declarations = [
        Declaration {
            name: "preg_filter",
            handler: fn_preg_filter,
            required: 3,
            parameters: &["pattern", "replacement", "subject", "limit", "count"],
            parameter_types: || {
                vec![
                    array_string_union(),
                    array_string_union(),
                    array_string_union(),
                    ParamTypeHint::Int,
                    ParamTypeHint::None,
                ]
            },
            return_type: array_string_null_union,
            defaults: || vec![None, None, None, Some(Value::long(-1)), Some(Value::null())],
            reference_arguments: 0b1_0000,
        },
        Declaration {
            name: "preg_grep",
            handler: fn_preg_grep,
            required: 2,
            parameters: &["pattern", "array", "flags"],
            parameter_types: || {
                vec![
                    ParamTypeHint::String,
                    ParamTypeHint::Array,
                    ParamTypeHint::Int,
                ]
            },
            return_type: || {
                ParamTypeHint::Union(vec![
                    ParamTypeHint::Array,
                    ParamTypeHint::ClassName("false".into()),
                ])
            },
            defaults: || vec![None, None, Some(Value::long(0))],
            reference_arguments: 0,
        },
        Declaration {
            name: "preg_last_error",
            handler: fn_preg_last_error,
            required: 0,
            parameters: &[],
            parameter_types: Vec::new,
            return_type: || ParamTypeHint::Int,
            defaults: Vec::new,
            reference_arguments: 0,
        },
        Declaration {
            name: "preg_last_error_msg",
            handler: fn_preg_last_error_msg,
            required: 0,
            parameters: &[],
            parameter_types: Vec::new,
            return_type: || ParamTypeHint::String,
            defaults: Vec::new,
            reference_arguments: 0,
        },
        Declaration {
            name: "preg_replace_callback_array",
            handler: fn_preg_replace_callback_array,
            required: 2,
            parameters: &["pattern", "subject", "limit", "count", "flags"],
            parameter_types: || {
                vec![
                    ParamTypeHint::Array,
                    array_string_union(),
                    ParamTypeHint::Int,
                    ParamTypeHint::None,
                    ParamTypeHint::Int,
                ]
            },
            return_type: array_string_null_union,
            defaults: || {
                vec![
                    None,
                    None,
                    Some(Value::long(-1)),
                    Some(Value::null()),
                    Some(Value::long(0)),
                ]
            },
            reference_arguments: 0b1000,
        },
    ];

    let mut functions = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let parameter_names = declaration
            .parameters
            .iter()
            .map(|name| (*name).to_string())
            .collect();
        let mut function = if declaration.reference_arguments == 0 {
            Box::new(make_internal_function(
                declaration.handler,
                declaration.parameters.len() as u32,
                declaration.required,
                parameter_names,
            ))
        } else {
            Box::new(make_internal_function_ref(
                declaration.handler,
                declaration.parameters.len() as u32,
                declaration.required,
                declaration.reference_arguments,
                parameter_names,
            ))
        };
        function.common.sig.param_type_hints = (declaration.parameter_types)();
        function.common.sig.return_type_hint = (declaration.return_type)();
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(declaration.name, pointer)
            .expect("PCRE function registration is unique");
        eg.register_internal_function_reflection_metadata(
            pointer,
            (declaration.defaults)(),
            "pcre",
        );
        functions.push(function);
    }
    functions
}
