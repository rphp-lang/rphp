//! Shared byte-level format contract for PHP's `pack()` and `unpack()`.
//!
//! PHP strings are currently represented by the runtime's lossless Latin-1
//! bridge. This module stays byte-oriented at its boundary so every format
//! code has one explicit width and endian rule.

use crate::value::{PhpArray, Value};
use crate::vm::execute::{
    ExplicitNumericCastTarget, explicit_float_conversion, explicit_long_conversion,
    explicit_numeric_cast_warning,
};

pub(super) struct Outcome<T> {
    pub(super) value: Result<T, String>,
    pub(super) warnings: Vec<String>,
}

impl<T> Outcome<T> {
    fn success(value: T, warnings: Vec<String>) -> Self {
        Self {
            value: Ok(value),
            warnings,
        }
    }

    fn failure(message: impl Into<String>, warnings: Vec<String>) -> Self {
        Self {
            value: Err(message.into()),
            warnings,
        }
    }
}

#[derive(Clone, Copy)]
enum Repeat {
    Count(usize),
    Star,
}

impl Repeat {
    fn count_or(self, star: usize) -> usize {
        match self {
            Self::Count(count) => count,
            Self::Star => star,
        }
    }

    fn is_repeated(self) -> bool {
        !matches!(self, Self::Count(1))
    }
}

fn parse_repeat(format: &[u8], cursor: &mut usize) -> Repeat {
    if format.get(*cursor) == Some(&b'*') {
        *cursor += 1;
        return Repeat::Star;
    }
    let start = *cursor;
    let mut count = 0usize;
    while let Some(digit @ b'0'..=b'9') = format.get(*cursor).copied() {
        count = count
            .saturating_mul(10)
            .saturating_add(usize::from(digit - b'0'));
        *cursor += 1;
    }
    if *cursor == start {
        Repeat::Count(1)
    } else {
        Repeat::Count(count)
    }
}

fn value_bytes(value: &Value) -> Vec<u8> {
    let value = value.dereferenced();
    if let Some(bytes) = value.php_string_bytes() {
        return bytes.into_owned();
    }
    value.echo_to_string().into_bytes()
}

fn push_integer(output: &mut Vec<u8>, code: u8, integer: i64) {
    match code {
        b'c' | b'C' => output.push(integer as u8),
        b's' | b'S' => output.extend_from_slice(&(integer as u16).to_ne_bytes()),
        b'n' => output.extend_from_slice(&(integer as u16).to_be_bytes()),
        b'v' => output.extend_from_slice(&(integer as u16).to_le_bytes()),
        b'i' | b'I' | b'l' | b'L' => {
            output.extend_from_slice(&(integer as u32).to_ne_bytes());
        }
        b'N' => output.extend_from_slice(&(integer as u32).to_be_bytes()),
        b'V' => output.extend_from_slice(&(integer as u32).to_le_bytes()),
        b'q' | b'Q' => output.extend_from_slice(&(integer as u64).to_ne_bytes()),
        b'J' => output.extend_from_slice(&(integer as u64).to_be_bytes()),
        b'P' => output.extend_from_slice(&(integer as u64).to_le_bytes()),
        _ => unreachable!("validated integer format"),
    }
}

fn push_float(output: &mut Vec<u8>, code: u8, number: f64) {
    match code {
        b'f' => output.extend_from_slice(&(number as f32).to_ne_bytes()),
        b'g' => output.extend_from_slice(&(number as f32).to_le_bytes()),
        b'G' => output.extend_from_slice(&(number as f32).to_be_bytes()),
        b'd' => output.extend_from_slice(&number.to_ne_bytes()),
        b'e' => output.extend_from_slice(&number.to_le_bytes()),
        b'E' => output.extend_from_slice(&number.to_be_bytes()),
        _ => unreachable!("validated float format"),
    }
}

fn push_hex(
    output: &mut Vec<u8>,
    code: u8,
    input: &[u8],
    repeat: Repeat,
    warnings: &mut Vec<String>,
) {
    let requested = repeat.count_or(input.len());
    let count = requested.min(input.len());
    if requested > input.len() {
        warnings.push(format!(
            "pack(): Type {}: not enough characters in string",
            code as char
        ));
    }
    let mut byte = 0u8;
    for (index, digit) in input.iter().copied().take(count).enumerate() {
        let nibble = match digit {
            b'0'..=b'9' => digit - b'0',
            b'a'..=b'f' => digit - b'a' + 10,
            b'A'..=b'F' => digit - b'A' + 10,
            _ => {
                warnings.push(format!(
                    "pack(): Type {}: illegal hex digit {}",
                    code as char, digit as char
                ));
                0
            }
        };
        let shift = if (code == b'H') == (index % 2 == 0) {
            4
        } else {
            0
        };
        byte |= nibble << shift;
        if index % 2 == 1 {
            output.push(byte);
            byte = 0;
        }
    }
    if count % 2 == 1 {
        output.push(byte);
    }
}

pub(super) fn pack_values(format: &str, values: Option<&PhpArray>) -> Outcome<Vec<u8>> {
    let format = format.as_bytes();
    let arguments: Vec<&Value> = values
        .map(|values| values.values().collect())
        .unwrap_or_default();
    let mut argument = 0usize;
    let mut cursor = 0usize;
    let mut output = Vec::new();
    let mut warnings = Vec::new();

    while cursor < format.len() {
        let code = format[cursor];
        cursor += 1;
        let repeat = parse_repeat(format, &mut cursor);
        match code {
            b'a' | b'A' | b'Z' | b'h' | b'H' => {
                let Some(value) = arguments.get(argument) else {
                    return Outcome::failure(
                        format!("Type {}: too few arguments", code as char),
                        warnings,
                    );
                };
                argument += 1;
                if value.dereferenced().value_type() == crate::value::ValueType::Array {
                    warnings.push("Array to string conversion".to_string());
                }
                let input = value_bytes(value);
                match code {
                    b'a' | b'A' => {
                        let count = repeat.count_or(input.len());
                        let copied = count.min(input.len());
                        output.extend_from_slice(&input[..copied]);
                        output.resize(
                            output.len().saturating_add(count - copied),
                            if code == b'A' { b' ' } else { 0 },
                        );
                    }
                    b'Z' => {
                        let count = repeat.count_or(input.len().saturating_add(1));
                        if count != 0 {
                            let copied = (count - 1).min(input.len());
                            output.extend_from_slice(&input[..copied]);
                            output.resize(output.len().saturating_add(count - copied), 0);
                        }
                    }
                    b'h' | b'H' => push_hex(&mut output, code, &input, repeat, &mut warnings),
                    _ => unreachable!(),
                }
            }
            b'c' | b'C' | b's' | b'S' | b'n' | b'v' | b'i' | b'I' | b'l' | b'L' | b'N' | b'V'
            | b'q' | b'Q' | b'J' | b'P' => {
                let count = repeat.count_or(arguments.len().saturating_sub(argument));
                if arguments.len().saturating_sub(argument) < count {
                    return Outcome::failure(
                        format!("Type {}: too few arguments", code as char),
                        warnings,
                    );
                }
                for value in &arguments[argument..argument + count] {
                    if let Some(warning) =
                        explicit_numeric_cast_warning(value, ExplicitNumericCastTarget::Int)
                    {
                        warnings.push(warning);
                    }
                    push_integer(&mut output, code, explicit_long_conversion(value));
                }
                argument += count;
            }
            b'f' | b'g' | b'G' | b'd' | b'e' | b'E' => {
                let count = repeat.count_or(arguments.len().saturating_sub(argument));
                if arguments.len().saturating_sub(argument) < count {
                    return Outcome::failure(
                        format!("Type {}: too few arguments", code as char),
                        warnings,
                    );
                }
                for value in &arguments[argument..argument + count] {
                    if let Some(warning) =
                        explicit_numeric_cast_warning(value, ExplicitNumericCastTarget::Float)
                    {
                        warnings.push(warning);
                    }
                    push_float(&mut output, code, explicit_float_conversion(value));
                }
                argument += count;
            }
            b'x' | b'X' | b'@' => {
                let count = match repeat {
                    Repeat::Star => {
                        warnings.push(format!("pack(): Type {}: '*' ignored", code as char));
                        1
                    }
                    Repeat::Count(count) => count,
                };
                match code {
                    b'x' => output.resize(output.len().saturating_add(count), 0),
                    b'X' => {
                        if count > output.len() {
                            warnings.push("pack(): Type X: outside of string".to_string());
                        }
                        output.truncate(output.len().saturating_sub(count));
                    }
                    b'@' => {
                        if count < output.len() {
                            output.truncate(count);
                        } else {
                            output.resize(count, 0);
                        }
                    }
                    _ => unreachable!(),
                }
            }
            _ => {
                return Outcome::failure(
                    format!("Type {}: unknown format code", code as char),
                    warnings,
                );
            }
        }
    }

    let unused = arguments.len().saturating_sub(argument);
    if unused != 0 {
        warnings.push(format!("pack(): {unused} arguments unused"));
    }
    Outcome::success(output, warnings)
}

fn integer_width(code: u8) -> Option<usize> {
    match code {
        b'c' | b'C' => Some(1),
        b's' | b'S' | b'n' | b'v' => Some(2),
        b'i' | b'I' | b'l' | b'L' | b'N' | b'V' => Some(4),
        b'q' | b'Q' | b'J' | b'P' => Some(8),
        _ => None,
    }
}

fn float_width(code: u8) -> Option<usize> {
    match code {
        b'f' | b'g' | b'G' => Some(4),
        b'd' | b'e' | b'E' => Some(8),
        _ => None,
    }
}

fn read_integer(code: u8, bytes: &[u8]) -> Value {
    let value = match code {
        b'c' => i64::from(i8::from_ne_bytes([bytes[0]])),
        b'C' => i64::from(bytes[0]),
        b's' => i64::from(i16::from_ne_bytes(bytes.try_into().unwrap())),
        b'S' => i64::from(u16::from_ne_bytes(bytes.try_into().unwrap())),
        b'n' => i64::from(u16::from_be_bytes(bytes.try_into().unwrap())),
        b'v' => i64::from(u16::from_le_bytes(bytes.try_into().unwrap())),
        b'i' | b'l' => i64::from(i32::from_ne_bytes(bytes.try_into().unwrap())),
        b'I' | b'L' => i64::from(u32::from_ne_bytes(bytes.try_into().unwrap())),
        b'N' => i64::from(u32::from_be_bytes(bytes.try_into().unwrap())),
        b'V' => i64::from(u32::from_le_bytes(bytes.try_into().unwrap())),
        b'q' => i64::from_ne_bytes(bytes.try_into().unwrap()),
        b'Q' => u64::from_ne_bytes(bytes.try_into().unwrap()) as i64,
        b'J' => u64::from_be_bytes(bytes.try_into().unwrap()) as i64,
        b'P' => u64::from_le_bytes(bytes.try_into().unwrap()) as i64,
        _ => unreachable!("validated integer format"),
    };
    Value::long(value)
}

fn read_float(code: u8, bytes: &[u8]) -> Value {
    let value = match code {
        b'f' => f64::from(f32::from_ne_bytes(bytes.try_into().unwrap())),
        b'g' => f64::from(f32::from_le_bytes(bytes.try_into().unwrap())),
        b'G' => f64::from(f32::from_be_bytes(bytes.try_into().unwrap())),
        b'd' => f64::from_ne_bytes(bytes.try_into().unwrap()),
        b'e' => f64::from_le_bytes(bytes.try_into().unwrap()),
        b'E' => f64::from_be_bytes(bytes.try_into().unwrap()),
        _ => unreachable!("validated float format"),
    };
    Value::double(value)
}

fn bytes_to_value(bytes: &[u8]) -> Value {
    Value::binary_string(bytes)
}

fn insert_result(result: &mut PhpArray, name: &str, repeated: bool, index: usize, value: Value) {
    if name.is_empty() {
        result.set_int(index as i64, value);
    } else if repeated {
        result.set_str(&format!("{name}{index}"), value);
    } else {
        result.set_str(name, value);
    }
}

fn insufficient(code: u8, need: usize, available: usize) -> String {
    format!(
        "unpack(): Type {}: not enough input values, need {need} values but only {available} {} provided",
        code as char,
        if available == 1 { "was" } else { "were" }
    )
}

pub(super) fn unpack_values(format: &str, data: &[u8], offset: usize) -> Outcome<Option<PhpArray>> {
    let mut result = PhpArray::new();
    let mut position = offset;
    let mut warnings = Vec::new();

    for segment in format.split('/') {
        if segment.is_empty() {
            continue;
        }
        let raw = segment.as_bytes();
        let code = raw[0];
        let mut cursor = 1usize;
        let repeat = parse_repeat(raw, &mut cursor);
        if cursor > 1
            && raw[1] != b'*'
            && raw[1..cursor].iter().fold(0u64, |value, digit| {
                value
                    .saturating_mul(10)
                    .saturating_add(u64::from(*digit - b'0'))
            }) > i32::MAX as u64
        {
            warnings.push(format!("unpack(): Type {}: integer overflow", code as char));
            return Outcome::success(None, warnings);
        }
        let name = &segment[cursor..];

        if let Some(width) = integer_width(code).or_else(|| float_width(code)) {
            let available = data.len().saturating_sub(position);
            let count = repeat.count_or(available / width);
            let repeated = repeat.is_repeated();
            for index in 1..=count {
                let available = data.len().saturating_sub(position);
                if available < width {
                    warnings.push(insufficient(code, width, available));
                    return Outcome::success(None, warnings);
                }
                let bytes = &data[position..position + width];
                let value = if integer_width(code).is_some() {
                    read_integer(code, bytes)
                } else {
                    read_float(code, bytes)
                };
                insert_result(&mut result, name, repeated, index, value);
                position += width;
            }
            continue;
        }

        match code {
            b'a' | b'A' | b'Z' | b'h' | b'H' => {
                let available = data.len().saturating_sub(position);
                let requested = match (code, repeat) {
                    (b'h' | b'H', Repeat::Star) => available.saturating_mul(2),
                    (_, Repeat::Star) => available,
                    (_, Repeat::Count(count)) => count,
                };
                let bytes_needed = if matches!(code, b'h' | b'H') {
                    requested.saturating_add(1) / 2
                } else {
                    requested
                };
                if available < bytes_needed {
                    warnings.push(insufficient(code, bytes_needed, available));
                    return Outcome::success(None, warnings);
                }
                let bytes = &data[position..position + bytes_needed];
                position += bytes_needed;
                let value = match code {
                    b'a' => bytes_to_value(bytes),
                    b'A' => {
                        let trimmed = bytes
                            .iter()
                            .rposition(|byte| !matches!(byte, 0 | b' ' | b'\t' | b'\r' | b'\n'))
                            .map_or(&[][..], |last| &bytes[..=last]);
                        bytes_to_value(trimmed)
                    }
                    b'Z' => {
                        let end = bytes
                            .iter()
                            .position(|byte| *byte == 0)
                            .unwrap_or(bytes.len());
                        bytes_to_value(&bytes[..end])
                    }
                    b'h' | b'H' => {
                        const HEX: &[u8; 16] = b"0123456789abcdef";
                        let mut output = Vec::with_capacity(requested);
                        for byte in bytes {
                            let pair = if code == b'H' {
                                [HEX[(byte >> 4) as usize], HEX[(byte & 15) as usize]]
                            } else {
                                [HEX[(byte & 15) as usize], HEX[(byte >> 4) as usize]]
                            };
                            output.extend_from_slice(&pair);
                        }
                        output.truncate(requested);
                        bytes_to_value(&output)
                    }
                    _ => unreachable!(),
                };
                insert_result(&mut result, name, false, 1, value);
            }
            b'x' => {
                let available = data.len().saturating_sub(position);
                let count = repeat.count_or(available);
                if count > available {
                    warnings.push(insufficient(b'x', 1, 0));
                    return Outcome::success(None, warnings);
                }
                position += count;
            }
            b'X' => {
                let count = match repeat {
                    Repeat::Star => {
                        warnings.push("unpack(): Type X: '*' ignored".to_string());
                        1
                    }
                    Repeat::Count(count) => count,
                };
                position = position.saturating_sub(count).max(offset);
            }
            b'@' => {
                let count = match repeat {
                    Repeat::Star => {
                        warnings.push("unpack(): Type @: '*' ignored".to_string());
                        1
                    }
                    Repeat::Count(count) => count,
                };
                let target = position.max(offset.saturating_add(count));
                if target > data.len() {
                    warnings.push("unpack(): Type @: outside of string".to_string());
                    position = data.len();
                } else {
                    position = target;
                }
            }
            _ => {
                return Outcome::failure(format!("Invalid format type {}", code as char), warnings);
            }
        }
    }
    Outcome::success(Some(result), warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endian_round_trip_and_named_repeat() {
        let mut arguments = PhpArray::new();
        arguments.push(Value::long(0x0102_0304));
        arguments.push(Value::long(0x0506_0708));
        let packed = pack_values("V2", Some(&arguments)).value.unwrap();
        assert_eq!(packed, [4, 3, 2, 1, 8, 7, 6, 5]);
        let unpacked = unpack_values("V2word", &packed, 0).value.unwrap().unwrap();
        assert_eq!(
            unpacked.get_str("word1").and_then(Value::as_long),
            Some(0x0102_0304)
        );
        assert_eq!(
            unpacked.get_str("word2").and_then(Value::as_long),
            Some(0x0506_0708)
        );
    }

    #[test]
    fn text_hex_and_cursor_codes_are_byte_oriented() {
        let mut arguments = PhpArray::new();
        arguments.push(Value::string("123"));
        let packed = pack_values("H*X@4", Some(&arguments));
        assert_eq!(packed.value.unwrap(), [0x12, 0, 0, 0]);
        let unpacked = unpack_values("H3hex/@0/x/Cfirst", &[0x12, 0x30, 0x40, 0x50], 0)
            .value
            .unwrap()
            .unwrap();
        assert_eq!(unpacked.get_str("hex").and_then(Value::as_str), Some("123"));
        assert_eq!(
            unpacked.get_str("first").and_then(Value::as_long),
            Some(0x50)
        );
    }
}
