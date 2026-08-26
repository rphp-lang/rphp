use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{Function, FunctionType};

const INI_SCANNER_NORMAL: i64 = 0;
const INI_SCANNER_RAW: i64 = 1;
const INI_SCANNER_TYPED: i64 = 2;

pub(super) fn fn_ini_parse_quantity(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(shorthand) = quantity_string_argument(execute_data, eg)? else {
        return Ok(());
    };
    let parsed = parse_quantity(&shorthand);
    if let Some(warning) = parsed.warning {
        super::report_internal_diagnostic(eg, execute_data, 2, "Warning", &warning)?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    return_value_with(return_value, Value::long(parsed.value))
}

fn quantity_string_argument(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<Option<String>, VmError> {
    let argument = super::owned_argument(execute_data, 0);
    let argument = argument.dereferenced();
    let strict = super::internal_call_is_strict(execute_data);

    let converted = match argument.value_type() {
        ValueType::String => Some(argument.as_str().unwrap_or("").to_string()),
        ValueType::Null if !strict => {
            super::report_internal_deprecation(
                eg,
                execute_data,
                "ini_parse_quantity(): Passing null to parameter #1 ($shorthand) of type string is deprecated",
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            Some(String::new())
        }
        ValueType::False if !strict => Some(String::new()),
        ValueType::True if !strict => Some("1".to_string()),
        ValueType::Long | ValueType::Double if !strict => {
            Some(argument.echo_to_string_with_precision(eg.precision))
        }
        ValueType::Object if !strict => {
            let rendered = crate::vm::execute::call_object_string_conversion(eg, argument)?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            let Some(rendered) = rendered else {
                return quantity_argument_type_error(eg, argument);
            };
            let Some(rendered) = rendered.as_str() else {
                let class_name = argument.diagnostic_type_name();
                eg.exception = Some(crate::value::make_error_value(
                    "TypeError",
                    &format!("{class_name}::__toString(): Return value must be of type string"),
                ));
                return Ok(None);
            };
            Some(rendered.to_string())
        }
        _ => return quantity_argument_type_error(eg, argument),
    };
    Ok(converted)
}

fn quantity_argument_type_error(
    eg: &mut ExecutorGlobals,
    argument: &Value,
) -> Result<Option<String>, VmError> {
    let actual = match argument.value_type() {
        ValueType::True => "true".to_string(),
        ValueType::False => "false".to_string(),
        _ => argument.diagnostic_type_name().into_owned(),
    };
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "ini_parse_quantity(): Argument #1 ($shorthand) must be of type string, {actual} given"
        ),
    ));
    Ok(None)
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedQuantity {
    value: i64,
    warning: Option<String>,
}

/// Parse the byte-oriented quantity grammar used by PHP's public
/// `ini_parse_quantity()` function. This is intentionally separate from the
/// INI expression parser below: quantities have base prefixes and K/M/G
/// multipliers, but do not evaluate constants or operators.
fn parse_quantity(shorthand: &str) -> ParsedQuantity {
    let bytes = shorthand.as_bytes();
    let escaped = || escape_quantity_bytes(bytes);
    let mut offset = skip_quantity_whitespace(bytes, 0);
    if offset == bytes.len() {
        return ParsedQuantity {
            value: 0,
            warning: None,
        };
    }

    let negative = match bytes[offset] {
        b'+' => {
            offset += 1;
            false
        }
        b'-' => {
            offset += 1;
            true
        }
        _ => false,
    };

    let mut base = 10_u32;
    let mut explicit_prefix = false;
    if bytes.get(offset) == Some(&b'0') {
        if let Some(second) = bytes.get(offset + 1).copied() {
            let recognized_after_zero = second.is_ascii_digit()
                || matches!(
                    second,
                    b'x' | b'X'
                        | b'b'
                        | b'B'
                        | b'o'
                        | b'O'
                        | b'k'
                        | b'K'
                        | b'm'
                        | b'M'
                        | b'g'
                        | b'G'
                )
                || (second.is_ascii_whitespace()
                    && bytes[offset + 1..].iter().all(u8::is_ascii_whitespace));
            if !recognized_after_zero {
                let prefix = String::from_utf8_lossy(&bytes[offset..offset + 2]);
                return ParsedQuantity {
                    value: 0,
                    warning: Some(format!(
                        "Invalid prefix \"{prefix}\", interpreting as \"0\" for backwards compatibility"
                    )),
                };
            }
        }
        base = 8;
        if let Some(prefix) = bytes.get(offset + 1).copied() {
            let explicit_base = match prefix {
                b'x' | b'X' => Some(16),
                b'b' | b'B' => Some(2),
                b'o' | b'O' => Some(8),
                _ => None,
            };
            if let Some(explicit_base) = explicit_base {
                base = explicit_base;
                explicit_prefix = true;
                offset += 2;
            }
        }
    }
    let digit_start = offset;

    let mut magnitude = 0_u64;
    let mut digit_overflow = false;
    while let Some(digit) = bytes.get(offset).copied().and_then(quantity_digit) {
        if u32::from(digit) >= base {
            break;
        }
        magnitude = magnitude
            .checked_mul(u64::from(base))
            .and_then(|value| value.checked_add(u64::from(digit)))
            .unwrap_or_else(|| {
                digit_overflow = true;
                u64::MAX
            });
        offset += 1;
    }

    if offset == digit_start {
        let missing_after_prefix = explicit_prefix
            && bytes
                .get(digit_start)
                .is_none_or(|byte| byte.is_ascii_whitespace() || matches!(byte, b'+' | b'-'));
        let reason = if missing_after_prefix {
            "no digits after base prefix"
        } else {
            "no valid leading digits"
        };
        return ParsedQuantity {
            value: 0,
            warning: Some(format!(
                "Invalid quantity \"{}\": {reason}, interpreting as \"0\" for backwards compatibility",
                escaped()
            )),
        };
    }

    let digit_end = offset;
    let prefix_end = skip_quantity_whitespace(bytes, digit_end);
    let last_non_whitespace = bytes.iter().rposition(|byte| !byte.is_ascii_whitespace());
    let multiplier = last_non_whitespace.and_then(|index| match bytes[index] {
        b'k' | b'K' => Some((index, 10_u32)),
        b'm' | b'M' => Some((index, 20_u32)),
        b'g' | b'G' => Some((index, 30_u32)),
        _ => None,
    });
    let clean_syntax = match multiplier {
        Some((multiplier_offset, _)) => {
            prefix_end == multiplier_offset
                && bytes[multiplier_offset + 1..]
                    .iter()
                    .all(u8::is_ascii_whitespace)
        }
        None => prefix_end == bytes.len(),
    };
    let shift = multiplier.map_or(0, |(_, shift)| shift);

    let mut signed = magnitude as i64;
    if negative && magnitude <= (1_u64 << 63) {
        signed = signed.wrapping_neg();
    }
    let value = signed.wrapping_shl(shift);

    let warning = if !clean_syntax {
        let mut interpreted = bytes[..prefix_end].to_vec();
        if let Some((multiplier_offset, _)) = multiplier {
            interpreted.push(bytes[multiplier_offset]);
            Some(format!(
                "Invalid quantity \"{}\", interpreting as \"{}\" for backwards compatibility",
                escaped(),
                escape_quantity_bytes(&interpreted)
            ))
        } else {
            let unknown = last_non_whitespace
                .map(|index| escape_quantity_bytes(&bytes[index..index + 1]))
                .unwrap_or_default();
            Some(format!(
                "Invalid quantity \"{}\": unknown multiplier \"{unknown}\", interpreting as \"{}\" for backwards compatibility",
                escaped(),
                escape_quantity_bytes(&interpreted)
            ))
        }
    } else {
        let limit = if negative {
            (1_u64 << 63) >> shift
        } else {
            (i64::MAX as u64) >> shift
        };
        (digit_overflow || magnitude > limit).then(|| {
            format!(
                "Invalid quantity \"{}\": value is out of range, using overflow result for backwards compatibility",
                escaped()
            )
        })
    };

    ParsedQuantity { value, warning }
}

fn quantity_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn skip_quantity_whitespace(bytes: &[u8], mut offset: usize) -> usize {
    while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
        offset += 1;
    }
    offset
}

fn escape_quantity_bytes(bytes: &[u8]) -> String {
    let mut escaped = String::with_capacity(bytes.len());
    for byte in bytes.iter().copied() {
        match byte {
            b'\\' => escaped.push_str("\\\\"),
            b'\0' => escaped.push_str("\\x00"),
            0x1b => escaped.push_str("\\e"),
            b'\t' => escaped.push_str("\\t"),
            b'\n' => escaped.push_str("\\n"),
            0x0b => escaped.push_str("\\v"),
            0x0c => escaped.push_str("\\f"),
            b'\r' => escaped.push_str("\\r"),
            0x20..=0x7e => escaped.push(char::from(byte)),
            _ => escaped.push_str(&format!("\\x{byte:02X}")),
        }
    }
    escaped
}

pub(super) fn fn_parse_ini_string(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = super::owned_argument(execute_data, 0).echo_to_string();
    let process_sections =
        optional_argument(execute_data, 1).is_some_and(|value| value.is_truthy());
    let Some(mode) = scanner_mode(execute_data, eg) else {
        return return_value_with(return_value, Value::bool(false));
    };
    match parse_ini(&source, process_sections, mode) {
        Ok(array) => return_value_with(return_value, Value::array(array)),
        Err(error) => {
            syntax_warning(eg, execute_data, "Unknown", &error);
            return_value_with(return_value, Value::bool(false))
        }
    }
}

pub(super) fn fn_parse_ini_file(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let filename = super::owned_argument(execute_data, 0).echo_to_string();
    let process_sections =
        optional_argument(execute_data, 1).is_some_and(|value| value.is_truthy());
    let Some(mode) = scanner_mode(execute_data, eg) else {
        return return_value_with(return_value, Value::bool(false));
    };
    let bytes = match std::fs::read(&filename) {
        Ok(bytes) => bytes,
        Err(_) => return return_value_with(return_value, Value::bool(false)),
    };
    let source: String = bytes.into_iter().map(char::from).collect();
    match parse_ini(&source, process_sections, mode) {
        Ok(array) => return_value_with(return_value, Value::array(array)),
        Err(error) => {
            syntax_warning(eg, execute_data, &filename, &error);
            return_value_with(return_value, Value::bool(false))
        }
    }
}

fn scanner_mode(execute_data: *mut ExecuteData, eg: &mut ExecutorGlobals) -> Option<i64> {
    let mode = optional_argument(execute_data, 2)
        .and_then(|value| value.as_long())
        .unwrap_or(INI_SCANNER_NORMAL);
    if matches!(
        mode,
        INI_SCANNER_NORMAL | INI_SCANNER_RAW | INI_SCANNER_TYPED
    ) {
        Some(mode)
    } else {
        let (file, line) = caller_location(execute_data);
        eg.write_output(
            format!("Warning: Invalid scanner mode in {file} on line {line}\n").as_bytes(),
        );
        None
    }
}

#[derive(Debug)]
struct ParseError {
    line: usize,
    message: &'static str,
}

fn parse_ini(source: &str, process_sections: bool, mode: i64) -> Result<PhpArray, ParseError> {
    let mut result = PhpArray::new();
    let mut current_section: Option<ArrayKey> = None;
    let source = source.split_once('\0').map_or(source, |(prefix, _)| prefix);

    for (index, (raw_line, terminated)) in IniLines::new(source).enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim_start();
        if line.trim_end().is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            let Some(close) = find_section_end(line) else {
                let message = if line.contains("${") {
                    "syntax error, unexpected end of file, expecting '}'"
                } else {
                    "syntax error, unexpected end of file, expecting ']'"
                };
                return Err(ParseError {
                    line: line_number,
                    message,
                });
            };
            if !line[close + 1..].trim().is_empty() {
                return Err(ParseError {
                    line: line_number,
                    message: "syntax error, unexpected token after section",
                });
            }
            let section = unquote(line[1..close].trim())?;
            let key = ini_key(&section);
            if process_sections {
                ensure_array(&mut result, &key);
                current_section = Some(key);
            } else {
                current_section = None;
            }
            continue;
        }

        let Some(equals) = line.find('=') else {
            return Err(ParseError {
                line: line_number,
                message: "syntax error, unexpected end of line",
            });
        };
        let raw_key = line[..equals].trim();
        if raw_key.is_empty() {
            return Err(ParseError {
                line: line_number,
                message: "syntax error, unexpected '='",
            });
        }
        let (key, offset) = parse_entry_key(raw_key, line_number)?;
        let value = parse_value(&line[equals + 1..], mode, line_number, terminated)?;
        let destination = if let Some(section) = current_section.as_ref() {
            array_at_mut(&mut result, section)
        } else {
            &mut result
        };
        set_entry(destination, key, offset, value);
    }
    Ok(result)
}

struct IniLines<'a> {
    source: &'a str,
    offset: usize,
}

impl<'a> IniLines<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, offset: 0 }
    }
}

impl<'a> Iterator for IniLines<'a> {
    type Item = (&'a str, bool);

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.source.len() {
            return None;
        }
        let start = self.offset;
        let bytes = self.source.as_bytes();
        let relative_end = memchr::memchr2(b'\r', b'\n', &bytes[start..]);
        let Some(relative_end) = relative_end else {
            self.offset = self.source.len();
            return Some((&self.source[start..], false));
        };
        let end = start + relative_end;
        self.offset = end + 1;
        if bytes[end] == b'\r' && bytes.get(self.offset) == Some(&b'\n') {
            self.offset += 1;
        }
        Some((&self.source[start..end], true))
    }
}

fn find_section_end(line: &str) -> Option<usize> {
    let mut quoted = false;
    for (index, byte) in line.as_bytes().iter().copied().enumerate().skip(1) {
        if byte == b'"' {
            quoted = !quoted;
        } else if byte == b']' && !quoted {
            return Some(index);
        }
    }
    None
}

fn parse_entry_key(
    raw: &str,
    line: usize,
) -> Result<(ArrayKey, Option<Option<ArrayKey>>), ParseError> {
    if let Some(open) = raw.find('[') {
        if !raw.ends_with(']') || open == 0 {
            return Err(ParseError {
                line,
                message: "syntax error, malformed array key",
            });
        }
        let key = ini_key(raw[..open].trim());
        let nested = raw[open + 1..raw.len() - 1].trim();
        let offset = if nested.is_empty() {
            None
        } else {
            Some(ini_key(nested))
        };
        Ok((key, Some(offset)))
    } else {
        Ok((ini_key(raw), None))
    }
}

fn parse_value(
    raw: &str,
    mode: i64,
    line: usize,
    line_terminated: bool,
) -> Result<Value, ParseError> {
    let without_comment = strip_comment(raw);
    let had_comment = without_comment.len() != raw.len();
    let value = without_comment.trim_start();
    let quoted = value.starts_with('"');
    let value = if quoted || had_comment || line_terminated || mode == INI_SCANNER_RAW {
        value.trim_end()
    } else {
        value
    };
    let value = if quoted {
        if value.len() < 2 || !value.ends_with('"') {
            return Err(ParseError {
                line,
                message: "syntax error, unexpected '\"'",
            });
        }
        unescape_quoted(&value[1..value.len() - 1])
    } else {
        value.to_string()
    };
    if mode == INI_SCANNER_RAW || quoted {
        return Ok(Value::string(value));
    }
    if let Some(evaluated) = evaluate_ini_integer_expression(&value) {
        return Ok(if mode == INI_SCANNER_TYPED {
            Value::long(evaluated)
        } else {
            Value::string(evaluated.to_string())
        });
    }
    let lower = value.to_ascii_lowercase();
    if mode == INI_SCANNER_TYPED {
        return Ok(match lower.as_str() {
            "true" | "on" | "yes" => Value::bool(true),
            "false" | "off" | "no" | "none" => Value::bool(false),
            "null" => Value::null(),
            _ => value
                .parse::<i64>()
                .map(Value::long)
                .or_else(|_| value.parse::<f64>().map(Value::double))
                .unwrap_or_else(|_| Value::string(value)),
        });
    }
    Ok(Value::string(match lower.as_str() {
        "true" | "on" | "yes" => "1".to_string(),
        "false" | "off" | "no" | "none" | "null" => String::new(),
        _ => value,
    }))
}

/// The INI grammar admits its small historical integer-expression language in
/// NORMAL and TYPED modes. Keep this parser deliberately separate from PHP
/// expression evaluation: operands are decimal integers or integer constants,
/// with unary `~`/`!`, parentheses, `&`, and `|`.
pub(super) fn evaluate_ini_integer_expression(source: &str) -> Option<i64> {
    let mut parser = IniIntegerExpression { source, offset: 0 };
    let value = parser.parse_or()?;
    parser.skip_whitespace();
    (parser.offset == source.len()).then_some(value)
}

struct IniIntegerExpression<'a> {
    source: &'a str,
    offset: usize,
}

impl IniIntegerExpression<'_> {
    fn parse_or(&mut self) -> Option<i64> {
        let mut value = self.parse_and()?;
        loop {
            self.skip_whitespace();
            if !self.consume(b'|') {
                return Some(value);
            }
            value |= self.parse_and()?;
        }
    }

    fn parse_and(&mut self) -> Option<i64> {
        let mut value = self.parse_unary()?;
        loop {
            self.skip_whitespace();
            if !self.consume(b'&') {
                return Some(value);
            }
            value &= self.parse_unary()?;
        }
    }

    fn parse_unary(&mut self) -> Option<i64> {
        self.skip_whitespace();
        if self.consume(b'~') {
            return self.parse_unary().map(|value| !value);
        }
        if self.consume(b'!') {
            return self.parse_unary().map(|value| i64::from(value == 0));
        }
        if self.consume(b'(') {
            let value = self.parse_or()?;
            self.skip_whitespace();
            return self.consume(b')').then_some(value);
        }
        self.parse_operand()
    }

    fn parse_operand(&mut self) -> Option<i64> {
        self.skip_whitespace();
        let start = self.offset;
        if self.peek() == Some(b'-') {
            self.offset += 1;
        }
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            self.offset += 1;
        }
        if self.offset == start
            || (self.offset == start + 1 && &self.source[start..self.offset] == "-")
        {
            return None;
        }
        let operand = &self.source[start..self.offset];
        operand.parse::<i64>().ok().or_else(|| {
            // PHP's INI grammar retains its historical E_ALL value excluding
            // E_STRICT even though ordinary PHP code exposes the current mask.
            if operand.eq_ignore_ascii_case("E_ALL") {
                Some(30_719)
            } else {
                crate::builtin_constant(operand).and_then(|value| value.as_long())
            }
        })
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.offset += 1;
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.offset).copied()
    }
}

fn strip_comment(raw: &str) -> &str {
    let bytes = raw.as_bytes();
    let mut quoted = false;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if byte == b'"' {
            // A quote begins a quoted value only at the first non-whitespace
            // byte; later quotes in an unquoted value are literal PHP bytes.
            if quoted || raw[..index].trim().is_empty() {
                quoted = !quoted;
            }
        } else if byte == b';' && !quoted {
            return &raw[..index];
        }
    }
    raw
}

fn unquote(value: &str) -> Result<String, ParseError> {
    if value.starts_with('"') {
        if value.len() < 2 || !value.ends_with('"') {
            return Err(ParseError {
                line: 1,
                message: "syntax error, unexpected '\"'",
            });
        }
        Ok(unescape_quoted(&value[1..value.len() - 1]))
    } else {
        Ok(value.to_string())
    }
}

fn unescape_quoted(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('"') => result.push('"'),
                Some('\\') => result.push('\\'),
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(character);
        }
    }
    result
}

fn ini_key(key: &str) -> ArrayKey {
    let canonical_integer = key
        .parse::<i64>()
        .ok()
        .filter(|_| key == "0" || (!key.starts_with('0') && !key.starts_with("-0")));
    canonical_integer.map_or_else(|| ArrayKey::String(key.to_string()), ArrayKey::Int)
}

fn set_entry(array: &mut PhpArray, key: ArrayKey, offset: Option<Option<ArrayKey>>, value: Value) {
    match offset {
        None => set_key(array, key, value),
        Some(offset) => {
            ensure_array(array, &key);
            let nested = array_at_mut(array, &key);
            match offset {
                Some(offset) => set_key(nested, offset, value),
                None => nested.push(value),
            }
        }
    }
}

fn ensure_array(array: &mut PhpArray, key: &ArrayKey) {
    let is_array = get_key_mut(array, key).is_some_and(|value| value.as_array().is_some());
    if !is_array {
        set_key(array, key.clone(), Value::array(PhpArray::new()));
    }
}

fn array_at_mut<'a>(array: &'a mut PhpArray, key: &ArrayKey) -> &'a mut PhpArray {
    get_key_mut(array, key).unwrap().as_array_mut().unwrap()
}

fn set_key(array: &mut PhpArray, key: ArrayKey, value: Value) {
    match key {
        ArrayKey::Int(key) => array.set_int(key, value),
        ArrayKey::String(key) => array.set_str(&key, value),
    }
}

fn get_key_mut<'a>(array: &'a mut PhpArray, key: &ArrayKey) -> Option<&'a mut Value> {
    match key {
        ArrayKey::Int(key) => array.get_int_mut(*key),
        ArrayKey::String(key) => array.get_str_mut(key),
    }
}

fn syntax_warning(
    eg: &mut ExecutorGlobals,
    execute_data: *mut ExecuteData,
    input: &str,
    error: &ParseError,
) {
    let (caller_file, caller_line) = caller_location(execute_data);
    eg.write_output(
        format!(
            "Warning: {} in {} on line {}\n in {} on line {}\n",
            error.message, input, error.line, caller_file, caller_line
        )
        .as_bytes(),
    );
}

fn caller_location(execute_data: *mut ExecuteData) -> (String, u32) {
    // SAFETY: an internal handler receives its live ExecuteData frame. Its
    // predecessor and user op-array remain owned by the synchronous VM stack
    // for the complete duration of this diagnostic lookup.
    unsafe {
        let caller = (*execute_data).prev_execute_data;
        if caller.is_null() || (*caller).func.is_null() {
            return ("Unknown".to_string(), 0);
        }
        let function = Function::from_common_ptr((*caller).func);
        if function.fn_type() != FunctionType::User {
            return ("Unknown".to_string(), 0);
        }
        let op_array = &function.as_user().op_array;
        let next = (*caller).opline.offset_from(op_array.instructions.as_ptr());
        let line = usize::try_from(next)
            .ok()
            .and_then(|next| next.checked_sub(1))
            .and_then(|index| op_array.source_line(index))
            .unwrap_or(0);
        (op_array.source_file.to_string(), line as u32)
    }
}

fn optional_argument(execute_data: *mut ExecuteData, index: u32) -> Option<Value> {
    let value = super::owned_argument(execute_data, index);
    (value.value_type() != ValueType::Undef).then_some(value)
}

fn return_value_with(pointer: *mut Value, value: Value) -> Result<(), VmError> {
    super::write_return_value(pointer, value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        INI_SCANNER_NORMAL, INI_SCANNER_RAW, INI_SCANNER_TYPED, parse_ini, parse_quantity,
    };

    #[test]
    fn quantity_parser_supports_signs_bases_multipliers_and_signed_boundaries() {
        let cases = [
            ("", 0),
            ("  \t", 0),
            ("+17", 17),
            ("-17", -17),
            ("077", 63),
            ("0o77", 63),
            ("0b101", 5),
            ("0x0b", 11),
            ("-0XBEEF", -48_879),
            ("1K", 1_024),
            ("0x10 m", 16_777_216),
            ("0b10G", 2_147_483_648),
            ("9223372036854775807", i64::MAX),
            ("-9223372036854775808", i64::MIN),
        ];
        for (source, expected) in cases {
            let parsed = parse_quantity(source);
            assert_eq!(parsed.value, expected, "{source}");
            assert_eq!(parsed.warning, None, "{source}");
        }
    }

    #[test]
    fn quantity_parser_reports_invalid_syntax_without_losing_legacy_results() {
        let cases = [
            (
                "0x+0",
                0,
                "Invalid quantity \"0x+0\": no digits after base prefix, interpreting as \"0\" for backwards compatibility",
            ),
            (
                "0b2",
                0,
                "Invalid quantity \"0b2\": no valid leading digits, interpreting as \"0\" for backwards compatibility",
            ),
            (
                "08",
                0,
                "Invalid quantity \"08\": unknown multiplier \"8\", interpreting as \"0\" for backwards compatibility",
            ),
            (
                "0a7",
                0,
                "Invalid prefix \"0a\", interpreting as \"0\" for backwards compatibility",
            ),
            (
                "0 K",
                0,
                "Invalid prefix \"0 \", interpreting as \"0\" for backwards compatibility",
            ),
            (
                "1.5K",
                1_024,
                "Invalid quantity \"1.5K\", interpreting as \"1K\" for backwards compatibility",
            ),
            (
                " 123 junk K ",
                125_952,
                "Invalid quantity \" 123 junk K \", interpreting as \" 123 K\" for backwards compatibility",
            ),
            (
                "123 abc",
                123,
                "Invalid quantity \"123 abc\": unknown multiplier \"c\", interpreting as \"123 \" for backwards compatibility",
            ),
        ];
        for (source, expected, warning) in cases {
            let parsed = parse_quantity(source);
            assert_eq!(parsed.value, expected, "{source}");
            assert_eq!(parsed.warning.as_deref(), Some(warning), "{source}");
        }
    }

    #[test]
    fn quantity_parser_uses_php_overflow_results_and_warning_escaping() {
        let positive = parse_quantity("9223372036854775808");
        assert_eq!(positive.value, i64::MIN);
        assert_eq!(
            positive.warning.as_deref(),
            Some(
                "Invalid quantity \"9223372036854775808\": value is out of range, using overflow result for backwards compatibility"
            )
        );

        let saturated = parse_quantity("999999999999999999999999G");
        assert_eq!(saturated.value, -1_073_741_824);
        assert!(saturated.warning.is_some());

        let escaped = parse_quantity("1\\\n\0x");
        assert_eq!(escaped.value, 1);
        assert_eq!(
            escaped.warning.as_deref(),
            Some(
                "Invalid quantity \"1\\\\\\n\\x00x\": unknown multiplier \"x\", interpreting as \"1\" for backwards compatibility"
            )
        );
    }

    #[test]
    fn parses_sections_typed_values_comments_and_raw_quotes() {
        let typed =
            parse_ini("[0]\na=1\nb=on\nc=null\nd=\"1\"\n", true, INI_SCANNER_TYPED).unwrap();
        let section = typed.get_int(0).unwrap().as_array().unwrap();
        assert_eq!(section.get_str("a").unwrap().as_long(), Some(1));
        assert!(section.get_str("b").unwrap().is_truthy());
        assert_eq!(
            section.get_str("c").unwrap().value_type(),
            crate::value::ValueType::Null
        );
        assert_eq!(section.get_str("d").unwrap().as_str(), Some("1"));

        let raw = parse_ini("a=\"foo;bar\" ; tail\nb= baz\n", false, INI_SCANNER_RAW).unwrap();
        assert_eq!(raw.get_str("a").unwrap().as_str(), Some("foo;bar"));
        assert_eq!(raw.get_str("b").unwrap().as_str(), Some("baz"));

        let normal = parse_ini("a=(1|2)&3\nb=E_ALL & ~E_NOTICE\nc=E_ALL\n", false, 0).unwrap();
        assert_eq!(normal.get_str("a").unwrap().as_str(), Some("3"));
        assert_eq!(normal.get_str("b").unwrap().as_str(), Some("30711"));
        assert_eq!(normal.get_str("c").unwrap().as_str(), Some("30719"));
    }

    #[test]
    fn ini_parser_distinguishes_unterminated_values_from_line_padding() {
        let normal = parse_ini(
            "a=alpha \nb=bravo\t\rc=charlie \r\nd=delta\t",
            false,
            INI_SCANNER_NORMAL,
        )
        .unwrap();
        assert_eq!(normal.get_str("a").unwrap().as_str(), Some("alpha"));
        assert_eq!(normal.get_str("b").unwrap().as_str(), Some("bravo"));
        assert_eq!(normal.get_str("c").unwrap().as_str(), Some("charlie"));
        assert_eq!(normal.get_str("d").unwrap().as_str(), Some("delta\t"));

        let raw = parse_ini("a=alpha ", false, INI_SCANNER_RAW).unwrap();
        assert_eq!(raw.get_str("a").unwrap().as_str(), Some("alpha"));
        let typed = parse_ini("a=alpha ", false, INI_SCANNER_TYPED).unwrap();
        assert_eq!(typed.get_str("a").unwrap().as_str(), Some("alpha "));
    }

    #[test]
    fn ini_parser_treats_nul_as_end_of_input_and_retains_error_line_numbers() {
        let empty = parse_ini("\0a=ignored", false, INI_SCANNER_NORMAL).unwrap();
        assert!(empty.is_empty());

        let parsed = parse_ini("a=value \0b=ignored", false, INI_SCANNER_NORMAL).unwrap();
        assert_eq!(parsed.get_str("a").unwrap().as_str(), Some("value "));
        assert!(parsed.get_str("b").is_none());

        let error = parse_ini("a=1\rmalformed", false, INI_SCANNER_NORMAL).unwrap_err();
        assert_eq!(error.line, 2);
        assert_eq!(error.message, "syntax error, unexpected end of line");
    }
}
