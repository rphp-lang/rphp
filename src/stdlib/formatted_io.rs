//! Feature-gated formatted stream I/O and the scanf family.
//!
//! The four public handlers share two cold parsers: one for printf-style
//! output and one for scanf-style input.  Keeping these paths out of the
//! existing sprintf handlers preserves their admitted hot-code layout while
//! this compatibility surface is evaluated.

use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

#[derive(Clone, Copy)]
enum OutputCall {
    Variadic,
    Array,
}

#[derive(Default)]
struct OutputFlags {
    left: bool,
    plus: bool,
    space: bool,
    zero: bool,
    pad: Option<u8>,
}

#[cold]
fn set_error(eg: &mut ExecutorGlobals, class: &str, message: impl AsRef<str>) {
    eg.exception = Some(crate::value::make_error_value(class, message.as_ref()));
}

#[inline]
fn argument<'a>(execute_data: *mut ExecuteData, index: u32) -> &'a Value {
    arg!(execute_data, index)
}

#[cold]
fn typed_string_argument(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Option<String> {
    let value = argument(execute_data, index);
    match value.value_type() {
        ValueType::Array | ValueType::Closure => {
            set_error(
                eg,
                "TypeError",
                format!(
                    "{function}(): Argument #{} (${parameter}) must be of type string, {} given",
                    index + 1,
                    value.type_name()
                ),
            );
            None
        }
        _ => Some(
            value
                .as_str()
                .map_or_else(|| value.echo_to_string(), ToOwned::to_owned),
        ),
    }
}

#[cold]
fn stream_argument(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Option<i64> {
    let value = argument(execute_data, 0);
    let Some(resource) = value.as_resource_id() else {
        set_error(
            eg,
            "TypeError",
            format!(
                "{function}(): Argument #1 ($stream) must be of type resource, {} given",
                value.type_name()
            ),
        );
        return None;
    };
    if !super::resource::is_open_for_request(eg, resource)
        || super::resource::type_for_request(eg, resource) != "stream"
    {
        if function == "fscanf" {
            set_error(
                eg,
                "TypeError",
                "fscanf(): supplied resource is not a valid File-Handle resource",
            );
            return None;
        }
        set_error(
            eg,
            "TypeError",
            format!("{function}(): Argument #1 ($stream) must be an open stream resource"),
        );
        return None;
    }
    Some(resource)
}

#[cold]
fn output_values_from_array(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
) -> Option<Vec<Value>> {
    let value = argument(execute_data, index);
    let Some(values) = value.as_array() else {
        set_error(
            eg,
            "TypeError",
            format!(
                "{function}(): Argument #{} ($values) must be of type array, {} given",
                index + 1,
                value.type_name()
            ),
        );
        return None;
    };
    Some(
        values
            .values()
            .map(|value| value.dereferenced().clone())
            .collect(),
    )
}

#[cold]
fn output_format_error(eg: &mut ExecutorGlobals, message: impl AsRef<str>) -> Option<String> {
    set_error(eg, "ValueError", message);
    None
}

#[cold]
fn take_output_argument<'a>(
    values: &'a [Value],
    next: &mut usize,
    position: Option<usize>,
    function: &str,
    call: OutputCall,
    eg: &mut ExecutorGlobals,
) -> Option<&'a Value> {
    let index = position.unwrap_or_else(|| {
        let index = *next;
        *next += 1;
        index
    });
    if let Some(value) = values.get(index) {
        return Some(value);
    }
    match call {
        OutputCall::Variadic => set_error(
            eg,
            "ArgumentCountError",
            format!(
                "{} arguments are required, {} given",
                index + 3,
                values.len() + 2
            ),
        ),
        OutputCall::Array => set_error(
            eg,
            "ValueError",
            format!(
                "The arguments array must contain {} items, {} given",
                index + 1,
                values.len()
            ),
        ),
    }
    let _ = function;
    None
}

fn parse_decimal(bytes: &[u8], index: &mut usize) -> Option<usize> {
    let start = *index;
    let mut value = 0usize;
    while let Some(byte @ b'0'..=b'9') = bytes.get(*index).copied() {
        value = value
            .saturating_mul(10)
            .saturating_add((byte - b'0') as usize);
        *index += 1;
    }
    (*index > start).then_some(value)
}

fn add_sign(mut rendered: String, flags: &OutputFlags, nonnegative: bool) -> String {
    if nonnegative {
        if flags.plus {
            rendered.insert(0, '+');
        } else if flags.space {
            rendered.insert(0, ' ');
        }
    }
    rendered
}

fn apply_width(mut rendered: String, width: usize, flags: &OutputFlags, specifier: u8) -> String {
    let length = rendered.len();
    if length >= width {
        return rendered;
    }
    let count = width - length;
    let numeric = specifier != b's' && specifier != b'c';
    let zero_pad =
        flags.zero && (!flags.left || matches!(specifier, b'e' | b'E' | b'f' | b'F' | b'g' | b'G'));
    let pad = flags.pad.unwrap_or(if zero_pad { b'0' } else { b' ' }) as char;
    if flags.left {
        rendered.extend(std::iter::repeat_n(pad, count));
        return rendered;
    }
    if numeric && pad == '0' && matches!(rendered.as_bytes().first(), Some(b'+' | b'-' | b' ')) {
        let sign = rendered.remove(0);
        let mut padded = String::with_capacity(width);
        padded.push(sign);
        padded.extend(std::iter::repeat_n(pad, count));
        padded.push_str(&rendered);
        return padded;
    }
    let mut padded = String::with_capacity(width);
    padded.extend(std::iter::repeat_n(pad, count));
    padded.push_str(&rendered);
    padded
}

fn normalize_exponent(mut rendered: String, upper: bool) -> String {
    let marker = if upper { 'E' } else { 'e' };
    if upper {
        rendered = rendered.replace('e', "E");
    }
    if let Some(position) = rendered.find(marker) {
        let sign_position = position + 1;
        if !matches!(rendered.as_bytes().get(sign_position), Some(b'+' | b'-')) {
            rendered.insert(sign_position, '+');
        }
    }
    rendered
}

fn output_float_value(value: &Value) -> f64 {
    let Some(string) = value.as_str() else {
        return value.to_float_val();
    };
    let bytes = super::php_string_to_bytes(string);
    let mut position = 0usize;
    skip_input_whitespace(&bytes, &mut position);
    scan_float(&bytes, &mut position, usize::MAX)
        .and_then(|value| value.as_double())
        .unwrap_or(0.0)
}

#[cold]
fn render_output_value(
    value: &Value,
    specifier: u8,
    precision: Option<usize>,
    flags: &OutputFlags,
) -> String {
    match specifier {
        b's' => {
            let mut rendered = value.echo_to_string();
            if let Some(precision) = precision {
                rendered.truncate(precision.min(rendered.len()));
            }
            rendered
        }
        b'c' => super::bytes_to_php_string(&[(value.to_long_val() & 0xff) as u8]),
        b'd' => {
            let number = value.to_long_val();
            add_sign(number.to_string(), flags, number >= 0)
        }
        b'u' => (value.to_long_val() as u64).to_string(),
        b'b' => format!("{:b}", value.to_long_val()),
        b'o' => format!("{:o}", value.to_long_val()),
        b'x' => format!("{:x}", value.to_long_val()),
        b'X' => format!("{:X}", value.to_long_val()),
        b'f' | b'F' => {
            let number = output_float_value(value);
            let precision = precision.unwrap_or(6);
            let rendered = if number.is_nan() {
                "NaN".to_string()
            } else if number == f64::INFINITY {
                "INF".to_string()
            } else if number == f64::NEG_INFINITY {
                "-INF".to_string()
            } else {
                format!("{number:.precision$}")
            };
            add_sign(rendered, flags, number >= 0.0)
        }
        b'e' | b'E' => {
            let number = output_float_value(value);
            let precision = precision.unwrap_or(6);
            let rendered = if number.is_nan() {
                "NaN".to_string()
            } else if number == f64::INFINITY {
                "INF".to_string()
            } else if number == f64::NEG_INFINITY {
                "-INF".to_string()
            } else {
                normalize_exponent(format!("{number:.precision$e}"), specifier == b'E')
            };
            add_sign(rendered, flags, number >= 0.0)
        }
        b'g' | b'G' => {
            let number = output_float_value(value);
            let rendered = value.echo_to_string();
            add_sign(
                if specifier == b'G' {
                    rendered.replace('e', "E")
                } else {
                    rendered
                },
                flags,
                number >= 0.0,
            )
        }
        _ => unreachable!("validated output conversion"),
    }
}

#[cold]
fn format_output(
    format: &str,
    values: &[Value],
    function: &str,
    call: OutputCall,
    eg: &mut ExecutorGlobals,
) -> Option<String> {
    let bytes = format.as_bytes();
    let mut output = String::with_capacity(format.len().saturating_add(values.len() * 8));
    let mut index = 0usize;
    let mut next_argument = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            output.push(bytes[index] as char);
            index += 1;
            continue;
        }
        index += 1;
        if bytes.get(index) == Some(&b'%') {
            output.push('%');
            index += 1;
            continue;
        }
        if index >= bytes.len() {
            return output_format_error(eg, "Missing format specifier at end of string");
        }

        let number_start = index;
        let number = parse_decimal(bytes, &mut index);
        let position = if bytes.get(index) == Some(&b'$') {
            index += 1;
            let Some(position) = number else {
                return output_format_error(
                    eg,
                    "Argument number specifier must be greater than zero and less than 2147483647",
                );
            };
            if position == 0 || position >= i32::MAX as usize {
                return output_format_error(
                    eg,
                    "Argument number specifier must be greater than zero and less than 2147483647",
                );
            }
            Some(position - 1)
        } else {
            index = number_start;
            None
        };

        let mut flags = OutputFlags::default();
        loop {
            match bytes.get(index).copied() {
                Some(b'-') => flags.left = true,
                Some(b'+') => flags.plus = true,
                Some(b' ') => flags.space = true,
                Some(b'0') => flags.zero = true,
                Some(b'\'') => {
                    index += 1;
                    let Some(pad) = bytes.get(index).copied() else {
                        return output_format_error(eg, "Missing padding character");
                    };
                    flags.pad = Some(pad);
                }
                _ => break,
            }
            index += 1;
        }
        let width = parse_decimal(bytes, &mut index).unwrap_or(0);
        if width >= i32::MAX as usize {
            return output_format_error(eg, "Width must be between 0 and 2147483647");
        }
        let precision = if bytes.get(index) == Some(&b'.') {
            index += 1;
            Some(parse_decimal(bytes, &mut index).unwrap_or(0))
        } else {
            None
        };
        let Some(specifier) = bytes.get(index).copied() else {
            return output_format_error(eg, "Missing format specifier at end of string");
        };
        index += 1;
        if !matches!(
            specifier,
            b'b' | b'c'
                | b'd'
                | b'e'
                | b'E'
                | b'f'
                | b'F'
                | b'g'
                | b'G'
                | b'o'
                | b's'
                | b'u'
                | b'x'
                | b'X'
        ) {
            return output_format_error(
                eg,
                format!("Unknown format specifier \"{}\"", specifier as char),
            );
        }
        let value = take_output_argument(values, &mut next_argument, position, function, call, eg)?;
        let rendered = render_output_value(value, specifier, precision, &flags);
        output.push_str(&apply_width(rendered, width, &flags, specifier));
    }
    Some(output)
}

#[cold]
fn write_formatted(
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    resource: i64,
    format: String,
    values: Vec<Value>,
    call: OutputCall,
) -> Result<(), VmError> {
    let Some(rendered) = format_output(&format, &values, function, call, eg) else {
        return Ok(());
    };
    let bytes = super::php_string_to_bytes(&rendered);
    let result = super::streams::with_stream(eg, resource, |stream| stream.write(&bytes));
    let value = match result {
        Some(Ok(written)) => Value::long(written as i64),
        _ => Value::bool(false),
    };
    super::write_return_value(return_pointer, value);
    Ok(())
}

pub(super) fn fn_fprintf(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = stream_argument(execute_data, eg, "fprintf") else {
        return Ok(());
    };
    let Some(format) = typed_string_argument(execute_data, eg, "fprintf", 1, "format") else {
        return Ok(());
    };
    let values = output_values_from_array(execute_data, eg, "fprintf", 2).unwrap_or_default();
    if eg.exception.is_some() {
        return Ok(());
    }
    write_formatted(
        return_pointer,
        eg,
        "fprintf",
        resource,
        format,
        values,
        OutputCall::Variadic,
    )
}

pub(super) fn fn_vfprintf(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = stream_argument(execute_data, eg, "vfprintf") else {
        return Ok(());
    };
    let Some(format) = typed_string_argument(execute_data, eg, "vfprintf", 1, "format") else {
        return Ok(());
    };
    let Some(values) = output_values_from_array(execute_data, eg, "vfprintf", 2) else {
        return Ok(());
    };
    write_formatted(
        return_pointer,
        eg,
        "vfprintf",
        resource,
        format,
        values,
        OutputCall::Array,
    )
}

#[derive(Clone)]
enum ScanKind {
    String,
    Character,
    SignedDecimal,
    UnsignedDecimal,
    AutoInteger,
    Octal,
    Hexadecimal,
    Float,
    Count,
    Set(Box<[bool; 256]>),
}

#[derive(Clone)]
struct ScanConversion {
    kind: ScanKind,
    width: usize,
    slot: Option<usize>,
}

#[derive(Clone)]
enum ScanToken {
    Whitespace,
    Literal(u8),
    Conversion(ScanConversion),
}

struct ScanFormat {
    tokens: Vec<ScanToken>,
    fields: usize,
}

fn scan_bad_conversion(eg: &mut ExecutorGlobals, byte: Option<u8>) -> Option<ScanFormat> {
    let message = byte.map_or_else(
        || "Bad scan conversion character \"".to_string(),
        |byte| format!("Bad scan conversion character \"{}\"", byte as char),
    );
    set_error(eg, "ValueError", message);
    None
}

#[cold]
fn parse_scan_set(
    bytes: &[u8],
    index: &mut usize,
    eg: &mut ExecutorGlobals,
) -> Option<Box<[bool; 256]>> {
    let mut accepted = Box::new([false; 256]);
    let mut negated = false;
    if bytes.get(*index) == Some(&b'^') {
        negated = true;
        *index += 1;
    }
    let mut any = false;
    let mut previous = None;
    if bytes.get(*index) == Some(&b']') {
        accepted[b']' as usize] = true;
        previous = Some(b']');
        any = true;
        *index += 1;
    }
    while let Some(byte) = bytes.get(*index).copied() {
        if byte == b']' {
            *index += 1;
            if negated {
                for entry in accepted.iter_mut() {
                    *entry = !*entry;
                }
            }
            return Some(accepted);
        }
        if byte == b'-'
            && previous.is_some()
            && bytes.get(*index + 1).is_some_and(|next| *next != b']')
        {
            let end = bytes[*index + 1];
            let start = previous.expect("range start checked");
            if start <= end {
                for member in start..=end {
                    accepted[member as usize] = true;
                }
            } else {
                for member in end..=start {
                    accepted[member as usize] = true;
                }
            }
            previous = Some(end);
            any = true;
            *index += 2;
            continue;
        }
        accepted[byte as usize] = true;
        previous = Some(byte);
        any = true;
        *index += 1;
    }
    let _ = any;
    set_error(eg, "ValueError", "Unmatched [ in format string");
    None
}

#[cold]
fn compile_scan_format(format: &str, eg: &mut ExecutorGlobals) -> Option<ScanFormat> {
    let bytes = format.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut next_slot = 0usize;
    let mut fields = 0usize;
    while index < bytes.len() {
        if is_scan_whitespace(bytes[index]) {
            while bytes
                .get(index)
                .is_some_and(|byte| is_scan_whitespace(*byte))
            {
                index += 1;
            }
            tokens.push(ScanToken::Whitespace);
            continue;
        }
        if bytes[index] != b'%' {
            tokens.push(ScanToken::Literal(bytes[index]));
            index += 1;
            continue;
        }
        index += 1;
        if bytes.get(index) == Some(&b'%') {
            tokens.push(ScanToken::Literal(b'%'));
            index += 1;
            continue;
        }
        let suppressed = if bytes.get(index) == Some(&b'*') {
            index += 1;
            true
        } else {
            false
        };
        let number_start = index;
        let number = parse_decimal(bytes, &mut index);
        let explicit_slot = if bytes.get(index) == Some(&b'$') {
            index += 1;
            let Some(position) = number else {
                return scan_bad_conversion(eg, bytes.get(index).copied());
            };
            if position == 0 || position >= i32::MAX as usize {
                set_error(eg, "ValueError", "\"%n$\" argument index out of range");
                return None;
            }
            Some(position - 1)
        } else {
            index = number_start;
            None
        };
        let width = parse_decimal(bytes, &mut index).unwrap_or(usize::MAX);
        if matches!(bytes.get(index), Some(b'h' | b'l' | b'L')) {
            index += 1;
        }
        let Some(specifier) = bytes.get(index).copied() else {
            return scan_bad_conversion(eg, None);
        };
        index += 1;
        let kind = match specifier {
            b's' => ScanKind::String,
            b'c' => ScanKind::Character,
            b'd' => ScanKind::SignedDecimal,
            b'u' => ScanKind::UnsignedDecimal,
            b'i' => ScanKind::AutoInteger,
            b'o' => ScanKind::Octal,
            b'x' | b'X' => ScanKind::Hexadecimal,
            b'f' | b'F' | b'e' | b'E' | b'g' | b'G' => ScanKind::Float,
            b'n' => ScanKind::Count,
            b'[' => ScanKind::Set(parse_scan_set(bytes, &mut index, eg)?),
            _ => return scan_bad_conversion(eg, Some(specifier)),
        };
        let slot = if suppressed {
            None
        } else {
            let slot = explicit_slot.unwrap_or_else(|| {
                let slot = next_slot;
                next_slot += 1;
                slot
            });
            fields = fields.max(slot.saturating_add(1));
            Some(slot)
        };
        tokens.push(ScanToken::Conversion(ScanConversion { kind, width, slot }));
    }
    Some(ScanFormat { tokens, fields })
}

fn skip_input_whitespace(input: &[u8], position: &mut usize) {
    while input
        .get(*position)
        .is_some_and(|byte| is_scan_whitespace(*byte))
    {
        *position += 1;
    }
}

#[inline]
fn is_scan_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn digit_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn unsigned_scan_value(value: u64) -> Value {
    if value <= i64::MAX as u64 {
        Value::long(value as i64)
    } else {
        Value::string(value.to_string())
    }
}

fn scan_integer(
    input: &[u8],
    position: &mut usize,
    width: usize,
    kind: &ScanKind,
) -> Option<Value> {
    let start = *position;
    let limit = input.len().min(start.saturating_add(width));
    let mut cursor = start;
    let negative = match input.get(cursor).copied() {
        Some(b'+') => {
            cursor += 1;
            false
        }
        Some(b'-') => {
            cursor += 1;
            true
        }
        _ => false,
    };
    if cursor >= limit {
        return None;
    }
    let mut base = match kind {
        ScanKind::Octal => 8u32,
        ScanKind::Hexadecimal => 16,
        ScanKind::AutoInteger => 0,
        _ => 10,
    };
    if matches!(kind, ScanKind::Hexadecimal | ScanKind::AutoInteger)
        && cursor + 2 <= limit
        && input.get(cursor) == Some(&b'0')
        && matches!(input.get(cursor + 1), Some(b'x' | b'X'))
        && input
            .get(cursor + 2)
            .and_then(|byte| digit_value(*byte))
            .is_some_and(|digit| digit < 16)
    {
        base = 16;
        cursor += 2;
    } else if matches!(kind, ScanKind::AutoInteger) {
        base = if input.get(cursor) == Some(&b'0') {
            8
        } else {
            10
        };
    }
    let digits_start = cursor;
    let mut magnitude = 0u64;
    while cursor < limit {
        let Some(digit) = digit_value(input[cursor]) else {
            break;
        };
        if digit as u32 >= base {
            break;
        }
        magnitude = magnitude
            .wrapping_mul(base as u64)
            .wrapping_add(digit as u64);
        cursor += 1;
    }
    if cursor == digits_start {
        return None;
    }
    *position = cursor;
    match kind {
        ScanKind::UnsignedDecimal => {
            let value = if negative {
                magnitude.wrapping_neg()
            } else {
                magnitude
            };
            Some(unsigned_scan_value(value))
        }
        _ if negative => Some(Value::long(magnitude.wrapping_neg() as i64)),
        _ => Some(unsigned_scan_value(magnitude)),
    }
}

fn scan_float(input: &[u8], position: &mut usize, width: usize) -> Option<Value> {
    let start = *position;
    let limit = input.len().min(start.saturating_add(width));
    let mut cursor = start;
    if matches!(input.get(cursor), Some(b'+' | b'-')) {
        cursor += 1;
    }
    let integer_start = cursor;
    while cursor < limit && input[cursor].is_ascii_digit() {
        cursor += 1;
    }
    let mut digits = cursor - integer_start;
    if cursor < limit && input[cursor] == b'.' {
        cursor += 1;
        let fraction_start = cursor;
        while cursor < limit && input[cursor].is_ascii_digit() {
            cursor += 1;
        }
        digits += cursor - fraction_start;
    }
    if digits == 0 {
        return None;
    }
    if cursor < limit && matches!(input[cursor], b'e' | b'E') {
        let exponent_marker = cursor;
        cursor += 1;
        if cursor < limit && matches!(input[cursor], b'+' | b'-') {
            cursor += 1;
        }
        let exponent_start = cursor;
        while cursor < limit && input[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == exponent_start {
            cursor = exponent_marker;
        }
    }
    let parsed = std::str::from_utf8(&input[start..cursor])
        .ok()?
        .parse::<f64>()
        .ok()?;
    *position = cursor;
    Some(Value::double(parsed))
}

fn scan_conversion(
    input: &[u8],
    position: &mut usize,
    conversion: &ScanConversion,
) -> Option<Value> {
    if !matches!(
        conversion.kind,
        ScanKind::Character | ScanKind::Set(_) | ScanKind::Count
    ) {
        skip_input_whitespace(input, position);
    }
    match &conversion.kind {
        ScanKind::String => {
            let start = *position;
            let limit = input.len().min(start.saturating_add(conversion.width));
            while *position < limit && !is_scan_whitespace(input[*position]) {
                *position += 1;
            }
            (*position > start)
                .then(|| Value::string(super::bytes_to_php_string(&input[start..*position])))
        }
        ScanKind::Character => {
            let width = if conversion.width == usize::MAX {
                1
            } else {
                conversion.width
            };
            if *position >= input.len() {
                None
            } else {
                let start = *position;
                let limit = input.len().min(start.saturating_add(width));
                while *position < limit && !is_scan_whitespace(input[*position]) {
                    *position += 1;
                }
                let value = Value::string(super::bytes_to_php_string(&input[start..*position]));
                Some(value)
            }
        }
        ScanKind::SignedDecimal
        | ScanKind::UnsignedDecimal
        | ScanKind::AutoInteger
        | ScanKind::Octal
        | ScanKind::Hexadecimal => {
            scan_integer(input, position, conversion.width, &conversion.kind)
        }
        ScanKind::Float => scan_float(input, position, conversion.width),
        ScanKind::Count => Some(Value::long(*position as i64)),
        ScanKind::Set(accepted) => {
            let start = *position;
            let limit = input.len().min(start.saturating_add(conversion.width));
            while *position < limit && accepted[input[*position] as usize] {
                *position += 1;
            }
            (*position > start)
                .then(|| Value::string(super::bytes_to_php_string(&input[start..*position])))
        }
    }
}

struct ScanOutcome {
    values: Vec<Option<Value>>,
    matched: usize,
}

enum ScanTargets {
    Raw {
        execute_data: *mut ExecuteData,
        count: usize,
    },
    Packed {
        execute_data: *mut ExecuteData,
        values: PhpArray,
    },
}

impl ScanTargets {
    fn len(&self) -> usize {
        match self {
            Self::Raw { count, .. } => *count,
            Self::Packed { values, .. } => values.len(),
        }
    }
}

#[cold]
fn scan_input(input: &[u8], format: &ScanFormat) -> ScanOutcome {
    let mut position = 0usize;
    let mut failed = false;
    let mut matched = 0usize;
    let mut values: Vec<Option<Value>> = (0..format.fields).map(|_| None).collect();
    for token in &format.tokens {
        if failed {
            continue;
        }
        match token {
            ScanToken::Whitespace => skip_input_whitespace(input, &mut position),
            ScanToken::Literal(expected) => {
                if input.get(position) == Some(expected) {
                    position += 1;
                } else {
                    failed = true;
                }
            }
            ScanToken::Conversion(conversion) => {
                let Some(value) = scan_conversion(input, &mut position, conversion) else {
                    failed = true;
                    continue;
                };
                matched += 1;
                if let Some(slot) = conversion.slot {
                    values[slot] = Some(value);
                }
            }
        }
    }
    ScanOutcome { values, matched }
}

#[cold]
fn validate_scan_targets(fields: usize, targets: usize, eg: &mut ExecutorGlobals) -> bool {
    if fields == targets {
        return true;
    }
    let message = if targets > fields {
        "Variable is not assigned by any conversion specifiers"
    } else {
        "Different numbers of variable names and field specifiers"
    };
    set_error(eg, "ValueError", message);
    false
}

#[cold]
fn finish_scan(return_pointer: *mut Value, targets: Option<ScanTargets>, outcome: ScanOutcome) {
    if let Some(mut targets) = targets {
        for (index, value) in outcome.values.into_iter().enumerate() {
            if let Some(value) = value {
                match &mut targets {
                    ScanTargets::Raw { execute_data, .. } => {
                        arg_mut!(*execute_data, index as u32 + 2, value);
                    }
                    ScanTargets::Packed { values, .. } => {
                        let key = values
                            .get_at(index)
                            .expect("packed scanf target must exist")
                            .1;
                        values
                            .get_key_mut(&key)
                            .expect("packed scanf target key must remain stable")
                            .assign_dereferenced(value);
                    }
                }
            }
        }
        if let ScanTargets::Packed {
            execute_data,
            values,
        } = targets
        {
            arg_mut!(execute_data, 2, Value::array(values));
        }
        super::write_return_value(return_pointer, Value::long(outcome.matched as i64));
    } else {
        let mut result = PhpArray::new();
        for value in outcome.values {
            result.push(value.unwrap_or_else(Value::null));
        }
        super::write_return_value(return_pointer, Value::array(result));
    }
}

#[cold]
fn scan_string_call(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    targets: Option<ScanTargets>,
) -> Result<(), VmError> {
    let Some(input) = typed_string_argument(execute_data, eg, "sscanf", 0, "string") else {
        return Ok(());
    };
    let Some(format) = typed_string_argument(execute_data, eg, "sscanf", 1, "format") else {
        return Ok(());
    };
    let Some(format) = compile_scan_format(&format, eg) else {
        return Ok(());
    };
    if let Some(targets) = targets.as_ref()
        && !validate_scan_targets(format.fields, targets.len(), eg)
    {
        return Ok(());
    }
    let outcome = scan_input(&super::php_string_to_bytes(&input), &format);
    finish_scan(return_pointer, targets, outcome);
    Ok(())
}

#[cold]
fn scan_stream_call(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    targets: Option<ScanTargets>,
) -> Result<(), VmError> {
    let Some(resource) = stream_argument(execute_data, eg, "fscanf") else {
        return Ok(());
    };
    let Some(format) = typed_string_argument(execute_data, eg, "fscanf", 1, "format") else {
        return Ok(());
    };
    let mut input = Vec::new();
    let read =
        super::streams::with_stream(eg, resource, |stream| stream.read_line(&mut input, None));
    if !matches!(read, Some(Ok(Some(_)))) {
        super::write_return_value(return_pointer, Value::bool(false));
        return Ok(());
    }
    let nul_terminated = input.iter().position(|byte| *byte == 0);
    if let Some(nul) = nul_terminated {
        input.truncate(nul);
    }
    if input.last() == Some(&b'\n') {
        input.pop();
        if input.last() == Some(&b'\r') {
            input.pop();
        }
    }
    // PHP consumes the physical input line before reporting a bad scan
    // conversion. Variation loops rely on that cursor movement to reach EOF.
    let Some(format) = compile_scan_format(&format, eg) else {
        return Ok(());
    };
    if let Some(targets) = targets.as_ref()
        && !validate_scan_targets(format.fields, targets.len(), eg)
    {
        return Ok(());
    }
    let mut leading_conversion = None;
    let mut leading_format_whitespace = false;
    for token in &format.tokens {
        match token {
            ScanToken::Whitespace => leading_format_whitespace = true,
            ScanToken::Conversion(conversion) => {
                leading_conversion = Some(conversion);
                break;
            }
            ScanToken::Literal(_) => break,
        }
    }
    let leading_skips_whitespace = leading_conversion.is_some_and(|conversion| {
        matches!(
            conversion.kind,
            ScanKind::String
                | ScanKind::SignedDecimal
                | ScanKind::UnsignedDecimal
                | ScanKind::AutoInteger
                | ScanKind::Octal
                | ScanKind::Hexadecimal
                | ScanKind::Float
        )
    });
    if leading_skips_whitespace && input.iter().all(|byte| is_scan_whitespace(*byte)) {
        super::write_return_value(
            return_pointer,
            if targets.is_some() {
                Value::long(-1)
            } else {
                Value::null()
            },
        );
        return Ok(());
    }
    if nul_terminated == Some(0) {
        super::write_return_value(
            return_pointer,
            if targets.is_some() {
                Value::long(-1)
            } else {
                Value::null()
            },
        );
        return Ok(());
    }
    if input.is_empty() && leading_format_whitespace {
        super::write_return_value(
            return_pointer,
            if targets.is_some() {
                Value::long(-1)
            } else {
                Value::null()
            },
        );
        return Ok(());
    }
    if input.is_empty()
        && let Some(conversion) = leading_conversion
        && matches!(conversion.kind, ScanKind::Character)
    {
        let mut values: Vec<Option<Value>> = (0..format.fields).map(|_| None).collect();
        if let Some(slot) = conversion.slot {
            values[slot] = Some(Value::string(""));
        }
        finish_scan(return_pointer, targets, ScanOutcome { values, matched: 1 });
        return Ok(());
    }
    let outcome = scan_input(&input, &format);
    finish_scan(return_pointer, targets, outcome);
    Ok(())
}

fn packed_scan_targets(execute_data: *mut ExecuteData) -> Option<ScanTargets> {
    let values = argument(execute_data, 2);
    if values.value_type() == ValueType::Undef {
        return None;
    }
    let values = values
        .as_array()
        .expect("variadic scanf values must be packed into an array");
    if values.is_empty() {
        return None;
    }
    Some(ScanTargets::Packed {
        execute_data,
        values: values.clone(),
    })
}

pub(super) fn fn_sscanf(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    scan_string_call(
        execute_data,
        return_pointer,
        eg,
        packed_scan_targets(execute_data),
    )
}

pub(super) fn fn_sscanf_raw_variadic(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    supplied_num_args: u32,
) -> Result<(), VmError> {
    let targets = (supplied_num_args > 2).then_some(ScanTargets::Raw {
        execute_data,
        count: supplied_num_args.saturating_sub(2) as usize,
    });
    scan_string_call(execute_data, return_pointer, eg, targets)
}

pub(super) fn fn_fscanf(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    scan_stream_call(
        execute_data,
        return_pointer,
        eg,
        packed_scan_targets(execute_data),
    )
}

pub(super) fn fn_fscanf_raw_variadic(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    supplied_num_args: u32,
) -> Result<(), VmError> {
    let targets = (supplied_num_args > 2).then_some(ScanTargets::Raw {
        execute_data,
        count: supplied_num_args.saturating_sub(2) as usize,
    });
    scan_stream_call(execute_data, return_pointer, eg, targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_and_custom_padding_match_php_shapes() {
        let flags = OutputFlags {
            zero: true,
            ..OutputFlags::default()
        };
        assert_eq!(apply_width("-1".to_string(), 7, &flags, b'd'), "-000001");
        let flags = OutputFlags {
            pad: Some(b'#'),
            ..OutputFlags::default()
        };
        assert_eq!(apply_width("-1".to_string(), 7, &flags, b'd'), "#####-1");
    }

    #[test]
    fn integer_scanning_stops_at_the_first_invalid_digit() {
        let conversion = ScanConversion {
            kind: ScanKind::Octal,
            width: usize::MAX,
            slot: Some(0),
        };
        let mut position = 0;
        let value = scan_conversion(b"0129", &mut position, &conversion).unwrap();
        assert_eq!(value.as_long(), Some(10));
        assert_eq!(position, 3);
    }

    #[test]
    fn scansets_support_ranges_and_negation() {
        let mut eg = ExecutorGlobals::new();
        let format = compile_scan_format("%[a-z]%[^[]", &mut eg).unwrap();
        let outcome = scan_input(b"abc123[", &format);
        assert_eq!(outcome.values[0].as_ref().unwrap().as_str(), Some("abc"));
        assert_eq!(outcome.values[1].as_ref().unwrap().as_str(), Some("123"));
    }
}
