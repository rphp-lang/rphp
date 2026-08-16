use std::fmt::Write as _;

use crate::value::{PhpArray, Value, ValueType};

#[inline]
fn displayed_trace_class_name(class: &str) -> &str {
    class
        .strip_prefix("class@anonymous#")
        .map_or(class, |_| "class@anonymous")
}

fn append_trace_argument(output: &mut String, value: &Value) {
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
            output.push('\'');
            for byte in value.as_bytes().iter().take(15) {
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
            if value.len() > 15 {
                output.push_str("...");
            }
            output.push('\'');
        }
        ValueType::Array => output.push_str("Array"),
        ValueType::Object => {
            output.push_str("Object(");
            let object = value.as_object().expect("Object-tagged trace argument");
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
fn format_trace(trace: &PhpArray, append_main: bool) -> String {
    let mut output = String::new();
    let mut index = 0usize;
    for (_, value) in trace.iter() {
        let Some(entry) = value.as_array() else {
            continue;
        };
        if index != 0 {
            output.push('\n');
        }
        let _ = write!(output, "#{index} ");
        match (
            entry.get_str("file").and_then(Value::as_str),
            entry.get_str("line").and_then(Value::as_long),
        ) {
            (Some(file), Some(line)) => {
                let _ = write!(output, "{file}({line}): ");
            }
            _ => output.push_str("[internal function]: "),
        }
        if let Some(class) = entry.get_str("class").and_then(Value::as_str) {
            output.push_str(displayed_trace_class_name(class));
            output.push_str(
                entry
                    .get_str("type")
                    .and_then(Value::as_str)
                    .unwrap_or("::"),
            );
        }
        output.push_str(
            entry
                .get_str("function")
                .and_then(Value::as_str)
                .unwrap_or("{unknown}"),
        );
        output.push('(');
        if let Some(arguments) = entry.get_str("args").and_then(Value::as_array) {
            for (argument_index, (_, argument)) in arguments.iter().enumerate() {
                if argument_index != 0 {
                    output.push_str(", ");
                }
                append_trace_argument(&mut output, argument);
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
pub(crate) fn format_debug_print_backtrace(trace: &PhpArray) -> String {
    format_trace(trace, false)
}

/// Render a stored Throwable trace. The final `{main}` sentinel is not part of
/// `getTrace()` itself but is always appended to Zend's string representation.
pub(crate) fn format_throwable_trace(trace: &PhpArray) -> String {
    format_trace(trace, true)
}
