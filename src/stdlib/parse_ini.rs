use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{Function, FunctionType};

const INI_SCANNER_NORMAL: i64 = 0;
const INI_SCANNER_RAW: i64 = 1;
const INI_SCANNER_TYPED: i64 = 2;

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

    for (index, raw_line) in source.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
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
        let value = parse_value(&line[equals + 1..], mode, line_number)?;
        let destination = if let Some(section) = current_section.as_ref() {
            array_at_mut(&mut result, section)
        } else {
            &mut result
        };
        set_entry(destination, key, offset, value);
    }
    Ok(result)
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

fn parse_value(raw: &str, mode: i64, line: usize) -> Result<Value, ParseError> {
    let value = strip_comment(raw).trim();
    let quoted = value.starts_with('"');
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
    if let Some(evaluated) = evaluate_integer_expression(&value) {
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
fn evaluate_integer_expression(source: &str) -> Option<i64> {
    if !source
        .bytes()
        .any(|byte| matches!(byte, b'&' | b'|' | b'~' | b'!' | b'('))
    {
        return None;
    }
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
    use super::{INI_SCANNER_RAW, INI_SCANNER_TYPED, parse_ini};

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

        let normal = parse_ini("a=(1|2)&3\nb=E_ALL & ~E_NOTICE\n", false, 0).unwrap();
        assert_eq!(normal.get_str("a").unwrap().as_str(), Some("3"));
        assert_eq!(normal.get_str("b").unwrap().as_str(), Some("30711"));
    }
}
