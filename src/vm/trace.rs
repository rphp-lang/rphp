use std::fmt::Write as _;

use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value, ValueType};

#[inline]
fn displayed_trace_class_name(class: &str) -> &str {
    class
        .strip_prefix("class@anonymous#")
        .map_or(class, |_| "class@anonymous")
}

fn append_exception_string_argument(output: &mut String, value: &str, string_max_len: usize) {
    output.push('\'');
    for byte in value.as_bytes().iter().take(string_max_len) {
        match byte {
            b'\\' => output.push_str("\\\\"),
            b'\n' => output.push_str("\\n"),
            b'\r' => output.push_str("\\r"),
            b'\t' => output.push_str("\\t"),
            0x20..=0x7e => output.push(*byte as char),
            _ => {
                let _ = write!(output, "\\x{byte:02X}");
            }
        }
    }
    if value.len() > string_max_len {
        output.push_str("...");
    }
    output.push('\'');
}

fn append_trace_argument(
    output: &mut String,
    value: &Value,
    string_max_len: usize,
    eg: &ExecutorGlobals,
) {
    let value = value.dereferenced();
    match value.value_type() {
        ValueType::Undef | ValueType::Null => output.push_str("NULL"),
        ValueType::False => output.push_str("false"),
        ValueType::True => output.push_str("true"),
        ValueType::Long => {
            let _ = write!(output, "{}", value.as_long().unwrap());
        }
        ValueType::Double => {
            let number = value.as_double().unwrap();
            if number.is_nan() {
                output.push_str("NAN");
            } else if number == f64::INFINITY {
                output.push_str("INF");
            } else if number == f64::NEG_INFINITY {
                output.push_str("-INF");
            } else if number.fract() == 0.0 {
                let _ = write!(output, "{number:.1}");
            } else {
                let _ = write!(output, "{number}");
            }
        }
        ValueType::String => {
            let value = value.as_str().unwrap();
            append_exception_string_argument(output, value, string_max_len);
        }
        ValueType::Array => output.push_str("Array"),
        ValueType::Object => {
            let object = value.as_object().expect("Object-tagged trace argument");
            if eg
                .class_by_id(object.class_id)
                .is_some_and(|class| class.is_enum)
                && let Some(case) = object.get_property("name").and_then(Value::as_str)
            {
                output.push_str(object.class_name.trim_start_matches('\\'));
                output.push_str("::");
                output.push_str(case);
                return;
            }
            output.push_str("Object(");
            let class = object.class_name.as_ref();
            output.push_str(displayed_trace_class_name(class));
            output.push(')');
        }
        ValueType::Closure => output.push_str("Object(Closure)"),
        ValueType::Resource => {
            let _ = write!(output, "Resource id #{}", value.as_resource_id().unwrap());
        }
        ValueType::Reference => unreachable!("dereferenced trace argument remained a reference"),
    }
}

/// Render the shared PHP call-frame shape, with the caller deciding whether
/// the Throwable-only `{main}` sentinel belongs in the result.
fn format_trace(
    trace: &PhpArray,
    append_main: bool,
    string_max_len: usize,
    eg: &ExecutorGlobals,
    mut warnings: Option<&mut Vec<String>>,
) -> String {
    let mut output = String::new();
    let mut index = 0usize;
    for (frame_index, (_, value)) in trace.iter().enumerate() {
        let Some(entry) = value.as_array() else {
            if let Some(warnings) = warnings.as_deref_mut() {
                warnings.push(format!("Expected array for frame {frame_index}"));
            }
            continue;
        };
        if index != 0 {
            output.push('\n');
        }
        let _ = write!(output, "#{index} ");
        match (entry.get_str("file"), entry.get_str("line")) {
            (Some(file), Some(line)) if file.as_str().is_some() && line.as_long().is_some() => {
                let file = file.as_str().unwrap();
                let line = line.as_long().unwrap();
                let _ = write!(output, "{file}({line}): ");
            }
            (None, None) => output.push_str("[internal function]: "),
            (Some(file), _) if file.as_str().is_none() => {
                if let Some(warnings) = warnings.as_deref_mut() {
                    warnings.push("File name is not a string".to_string());
                }
                output.push_str("[unknown file]: ");
            }
            _ => output.push_str("[internal function]: "),
        }
        if let Some(class) = entry.get_str("class") {
            if let Some(class) = class.as_str() {
                output.push_str(displayed_trace_class_name(class));
            } else {
                if let Some(warnings) = warnings.as_deref_mut() {
                    warnings.push("Value for class is not a string".to_string());
                }
                output.push_str("[unknown]");
            }
        }
        if let Some(kind) = entry.get_str("type") {
            if let Some(kind) = kind.as_str() {
                output.push_str(kind);
            } else {
                if let Some(warnings) = warnings.as_deref_mut() {
                    warnings.push("Value for type is not a string".to_string());
                }
                output.push_str("[unknown]");
            }
        } else if entry.get_str("class").and_then(Value::as_str).is_some() {
            output.push_str("::");
        }
        if let Some(function) = entry.get_str("function") {
            if let Some(function) = function.as_str() {
                output.push_str(function);
            } else {
                if let Some(warnings) = warnings.as_deref_mut() {
                    warnings.push("Value for function is not a string".to_string());
                }
                output.push_str("[unknown]");
            }
        }
        output.push('(');
        if let Some(arguments) = entry.get_str("args") {
            if let Some(arguments) = arguments.as_array() {
                for (argument_index, (key, argument)) in arguments.iter().enumerate() {
                    if argument_index != 0 {
                        output.push_str(", ");
                    }
                    if let ArrayKey::String(name) = key {
                        output.push_str(&name);
                        output.push_str(": ");
                    }
                    append_trace_argument(&mut output, argument, string_max_len, eg);
                }
            } else if let Some(warnings) = warnings.as_deref_mut() {
                warnings.push("args element is not an array".to_string());
            }
        }
        output.push(')');
        index += 1;
    }
    if append_main {
        if !output.is_empty() {
            output.push('\n');
        }
        let _ = write!(output, "#{index} {{main}}");
    } else if !output.is_empty() {
        output.push('\n');
    }
    output
}

/// Render the live trace printed by `debug_print_backtrace()`. Unlike a
/// Throwable string, this contains only actual frames and ends each with a
/// newline; calling it directly from the main script therefore prints nothing.
pub(crate) fn format_debug_print_backtrace(
    trace: &PhpArray,
    string_max_len: usize,
    eg: &ExecutorGlobals,
) -> String {
    format_trace(trace, false, string_max_len, eg, None)
}

/// Render a stored Throwable trace. The final `{main}` sentinel is not part of
/// `getTrace()` itself but is always appended to Zend's string representation.
pub(crate) fn format_throwable_trace(
    trace: &PhpArray,
    string_max_len: usize,
    eg: &ExecutorGlobals,
) -> String {
    format_trace(trace, true, string_max_len, eg, None)
}

/// Render a user-mutated Throwable trace and retain the diagnostics that PHP
/// emits before returning the best-effort string representation.
pub(crate) fn format_throwable_trace_checked(
    trace: &PhpArray,
    string_max_len: usize,
    eg: &ExecutorGlobals,
) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    let rendered = format_trace(trace, true, string_max_len, eg, Some(&mut warnings));
    (rendered, warnings)
}

pub(crate) fn format_exception_string_argument(value: &str, string_max_len: usize) -> String {
    let mut output = String::new();
    append_exception_string_argument(&mut output, value, string_max_len);
    output
}
