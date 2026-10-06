//! PHPUnit-facing PHP 8.5 Mbstring foundation.
//!
//! This is an in-process implementation over PHP string bytes. It deliberately
//! does not delegate conversion or Unicode operations to the host PHP runtime.
//! The admitted surface is the functions used by PHPUnit 13 and the stable
//! extension-admission path exercised by PHP's own suite. Unsupported
//! Mbstring functions stay absent rather than advertising placeholder
//! behaviour.

use crate::compiler::{make_internal_function, make_internal_function_ref};
use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value, ValueType, php_byte_string_bytes};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{
    FunctionCommon, InternalFunction, InternalFunctionHandler, ParamTypeHint,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Encoding {
    Utf8,
    Ascii,
    Windows1252,
    Latin1,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    fn parse(name: &str) -> Option<Self> {
        let normalized: String = name
            .bytes()
            .filter(|byte| !matches!(byte, b'-' | b'_'))
            .map(|byte| byte.to_ascii_lowercase() as char)
            .collect();
        match normalized.as_str() {
            "utf8" => Some(Self::Utf8),
            "ascii" | "usascii" | "ansix3.41968" | "ansix3.41986" | "iso646us" | "ibm367"
            | "cp367" | "csascii" | "us" => Some(Self::Ascii),
            "windows1252" | "cp1252" => Some(Self::Windows1252),
            "iso88591" | "latin1" => Some(Self::Latin1),
            "utf16le" => Some(Self::Utf16Le),
            "utf16be" => Some(Self::Utf16Be),
            _ => None,
        }
    }

    fn canonical_name(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Ascii => "ASCII",
            Self::Windows1252 => "Windows-1252",
            Self::Latin1 => "ISO-8859-1",
            Self::Utf16Le => "UTF-16LE",
            Self::Utf16Be => "UTF-16BE",
        }
    }

    fn is_valid(self, bytes: &[u8]) -> bool {
        match self {
            Self::Utf8 => std::str::from_utf8(bytes).is_ok(),
            Self::Ascii => bytes.is_ascii(),
            Self::Windows1252 | Self::Latin1 => true,
            Self::Utf16Le | Self::Utf16Be => valid_utf16(bytes, self == Self::Utf16Le),
        }
    }

    fn decode(self, bytes: &[u8]) -> Vec<char> {
        match self {
            Self::Utf8 => decode_utf8(bytes),
            Self::Ascii => bytes
                .iter()
                .map(|byte| {
                    if byte.is_ascii() {
                        char::from(*byte)
                    } else {
                        '?'
                    }
                })
                .collect(),
            Self::Windows1252 => bytes
                .iter()
                .map(|byte| decode_windows_1252(*byte))
                .collect(),
            Self::Latin1 => bytes.iter().map(|byte| char::from(*byte)).collect(),
            Self::Utf16Le => decode_utf16(bytes, true),
            Self::Utf16Be => decode_utf16(bytes, false),
        }
    }

    fn encode(self, characters: &[char]) -> Vec<u8> {
        match self {
            Self::Utf8 => characters.iter().collect::<String>().into_bytes(),
            Self::Ascii => characters
                .iter()
                .map(|character| {
                    u8::try_from(u32::from(*character))
                        .ok()
                        .filter(u8::is_ascii)
                        .unwrap_or(b'?')
                })
                .collect(),
            Self::Windows1252 => characters
                .iter()
                .map(|character| encode_windows_1252(*character).unwrap_or(b'?'))
                .collect(),
            Self::Latin1 => characters
                .iter()
                .map(|character| u8::try_from(u32::from(*character)).unwrap_or(b'?'))
                .collect(),
            Self::Utf16Le | Self::Utf16Be => {
                let little_endian = self == Self::Utf16Le;
                let mut output = Vec::with_capacity(characters.len().saturating_mul(2));
                for character in characters {
                    let mut units = [0u16; 2];
                    for unit in character.encode_utf16(&mut units).iter().copied() {
                        let bytes = if little_endian {
                            unit.to_le_bytes()
                        } else {
                            unit.to_be_bytes()
                        };
                        output.extend_from_slice(&bytes);
                    }
                }
                output
            }
        }
    }
}

fn write_result(rv: *mut Value, value: Value) {
    super::write_return_value(rv, value);
}

fn value_error(eg: &mut ExecutorGlobals, message: impl AsRef<str>) {
    eg.exception = Some(crate::value::make_error_value(
        "ValueError",
        message.as_ref(),
    ));
}

fn decode_utf8(bytes: &[u8]) -> Vec<char> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut remaining = bytes;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(text) => {
                output.extend(text.chars());
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if valid != 0 {
                    output.extend(
                        std::str::from_utf8(&remaining[..valid])
                            .expect("valid_up_to is a verified UTF-8 prefix")
                            .chars(),
                    );
                }
                output.push('?');
                let invalid = error.error_len().unwrap_or(remaining.len() - valid);
                remaining = &remaining[valid.saturating_add(invalid).min(remaining.len())..];
            }
        }
    }
    output
}

fn utf16_units(bytes: &[u8], little_endian: bool) -> impl Iterator<Item = u16> + '_ {
    bytes.chunks_exact(2).map(move |pair| {
        let pair = [pair[0], pair[1]];
        if little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    })
}

fn valid_utf16(bytes: &[u8], little_endian: bool) -> bool {
    bytes.len().is_multiple_of(2)
        && char::decode_utf16(utf16_units(bytes, little_endian)).all(|item| item.is_ok())
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> Vec<char> {
    let mut output: Vec<char> = char::decode_utf16(utf16_units(bytes, little_endian))
        .map(|item| item.unwrap_or('?'))
        .collect();
    if !bytes.len().is_multiple_of(2) {
        output.push('?');
    }
    output
}

fn decode_windows_1252(byte: u8) -> char {
    const SPECIAL: [u16; 32] = [
        0x20ac, 0x0081, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x008d, 0x017d, 0x008f, 0x0090, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022,
        0x2013, 0x2014, 0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0x009d, 0x017e, 0x0178,
    ];
    if (0x80..=0x9f).contains(&byte) {
        char::from_u32(u32::from(SPECIAL[usize::from(byte - 0x80)])).unwrap()
    } else {
        char::from(byte)
    }
}

fn encode_windows_1252(character: char) -> Option<u8> {
    let codepoint = u32::from(character);
    if codepoint <= 0x7f || (0xa0..=0xff).contains(&codepoint) {
        return u8::try_from(codepoint).ok();
    }
    const SPECIAL: [(u16, u8); 32] = [
        (0x20ac, 0x80),
        (0x0081, 0x81),
        (0x201a, 0x82),
        (0x0192, 0x83),
        (0x201e, 0x84),
        (0x2026, 0x85),
        (0x2020, 0x86),
        (0x2021, 0x87),
        (0x02c6, 0x88),
        (0x2030, 0x89),
        (0x0160, 0x8a),
        (0x2039, 0x8b),
        (0x0152, 0x8c),
        (0x008d, 0x8d),
        (0x017d, 0x8e),
        (0x008f, 0x8f),
        (0x0090, 0x90),
        (0x2018, 0x91),
        (0x2019, 0x92),
        (0x201c, 0x93),
        (0x201d, 0x94),
        (0x2022, 0x95),
        (0x2013, 0x96),
        (0x2014, 0x97),
        (0x02dc, 0x98),
        (0x2122, 0x99),
        (0x0161, 0x9a),
        (0x203a, 0x9b),
        (0x0153, 0x9c),
        (0x009d, 0x9d),
        (0x017e, 0x9e),
        (0x0178, 0x9f),
    ];
    SPECIAL
        .iter()
        .find_map(|(candidate, byte)| (u32::from(*candidate) == codepoint).then_some(*byte))
}

fn string_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<Value>, VmError> {
    super::typed_internal_string_value_argument_expected(
        ed, eg, function, index, parameter, "string",
    )
}

fn optional_encoding(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<Encoding>, VmError> {
    let argument = super::owned_argument(ed, index);
    if matches!(argument.value_type(), ValueType::Undef | ValueType::Null) {
        return Ok(Some(Encoding::Utf8));
    }
    let Some(name) = super::typed_internal_string_argument(ed, eg, function, index, parameter)?
    else {
        return Ok(None);
    };
    match Encoding::parse(&name) {
        Some(encoding) => Ok(Some(encoding)),
        None => {
            value_error(
                eg,
                format!(
                    "{function}(): Argument #{} (${parameter}) must be a valid encoding, \"{name}\" given",
                    index + 1
                ),
            );
            Ok(None)
        }
    }
}

fn encoding_list_value(
    value: &Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    position: usize,
    parameter: &str,
) -> Option<Vec<Encoding>> {
    let value = value.dereferenced();
    if value.value_type() == ValueType::Null {
        return Some(vec![Encoding::Ascii, Encoding::Utf8]);
    }
    let names = if let Some(name) = value.as_str() {
        if name.trim().is_empty() {
            Vec::new()
        } else if name.eq_ignore_ascii_case("auto") {
            vec!["ASCII", "UTF-8"]
        } else {
            name.split(',').map(str::trim).collect()
        }
    } else if let Some(array) = value.as_array() {
        let mut names = Vec::with_capacity(array.len());
        for (_, item) in array.iter() {
            let Some(name) = item.dereferenced().as_str() else {
                super::typed_internal_argument_error(
                    eg,
                    function,
                    item,
                    position,
                    parameter,
                    "array|string|null",
                );
                return None;
            };
            names.push(name);
        }
        names
    } else {
        super::typed_internal_argument_error(
            eg,
            function,
            value,
            position,
            parameter,
            "array|string|null",
        );
        return None;
    };
    if names.is_empty() {
        value_error(
            eg,
            format!(
                "{function}(): Argument #{position} (${parameter}) must specify at least one encoding"
            ),
        );
        return None;
    }
    let mut encodings = Vec::with_capacity(names.len());
    for name in names {
        let Some(encoding) = Encoding::parse(name) else {
            value_error(
                eg,
                format!(
                    "{function}(): Argument #{position} (${parameter}) contains invalid encoding \"{name}\""
                ),
            );
            return None;
        };
        encodings.push(encoding);
    }
    Some(encodings)
}

fn optional_encoding_list(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Option<Vec<Encoding>> {
    let argument = super::owned_argument(ed, index);
    encoding_list_value(&argument, eg, function, index as usize + 1, parameter)
}

fn array_key_bytes(key: &ArrayKey, external: bool) -> Option<Vec<u8>> {
    match key {
        ArrayKey::Int(_) => None,
        ArrayKey::String(key) if external => Some(php_byte_string_bytes(key)),
        ArrayKey::String(key) => Some(key.as_bytes().to_vec()),
    }
}

fn value_has_valid_encoding(value: &Value, encoding: Encoding) -> bool {
    let value = value.dereferenced();
    if let Some(bytes) = value.php_string_bytes() {
        return encoding.is_valid(&bytes);
    }
    let Some(array) = value.as_array() else {
        return true;
    };
    let external = array.has_external_byte_keys();
    array.iter().all(|(key, item)| {
        array_key_bytes(&key, external).is_none_or(|bytes| encoding.is_valid(&bytes))
            && value_has_valid_encoding(item, encoding)
    })
}

fn convert_bytes(bytes: &[u8], from: Encoding, to: Encoding) -> Vec<u8> {
    to.encode(&from.decode(bytes))
}

fn convert_value(value: &Value, from: Encoding, to: Encoding) -> Value {
    let value = value.dereferenced();
    if let Some(bytes) = value.php_string_bytes() {
        return super::php_byte_result(convert_bytes(&bytes, from, to), false);
    }
    let Some(array) = value.as_array() else {
        return value.clone();
    };
    let mut converted = PhpArray::new();
    for (key, item) in array.iter() {
        converted.set(key, convert_value(item, from, to));
    }
    super::copy_array_key_provenance(array, &converted);
    Value::array(converted)
}

fn fn_mb_check_encoding(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(encoding) = optional_encoding(ed, eg, "mb_check_encoding", 1, "encoding")? else {
        return Ok(());
    };
    let value = super::owned_argument(ed, 0);
    let valid = matches!(value.value_type(), ValueType::Undef | ValueType::Null)
        || value_has_valid_encoding(&value, encoding);
    write_result(rv, Value::bool(valid));
    Ok(())
}

fn fn_mb_detect_encoding(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) = string_argument(ed, eg, "mb_detect_encoding", 0, "string")? else {
        return Ok(());
    };
    let Some(encodings) = optional_encoding_list(ed, eg, "mb_detect_encoding", 1, "encodings")
    else {
        return Ok(());
    };
    let strict = if arg_opt!(ed, 2).is_some() {
        let Some(strict) =
            super::typed_internal_bool_argument(ed, eg, "mb_detect_encoding", 2, "strict")?
        else {
            return Ok(());
        };
        strict
    } else {
        false
    };
    let bytes = string.php_string_bytes().unwrap_or_default();
    let detected = encodings
        .iter()
        .copied()
        .find(|encoding| encoding.is_valid(&bytes))
        .or_else(|| (!strict).then(|| encodings[0]));
    match detected {
        Some(encoding) => write_result(rv, Value::string(encoding.canonical_name())),
        None => write_result(rv, Value::bool(false)),
    }
    Ok(())
}

fn fn_mb_convert_encoding(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(value) =
        super::typed_internal_array_or_string_argument(ed, eg, "mb_convert_encoding", 0, "string")?
    else {
        return Ok(());
    };
    let Some(to_name) =
        super::typed_internal_string_argument(ed, eg, "mb_convert_encoding", 1, "to_encoding")?
    else {
        return Ok(());
    };
    let Some(to) = Encoding::parse(&to_name) else {
        value_error(
            eg,
            format!(
                "mb_convert_encoding(): Argument #2 ($to_encoding) must be a valid encoding, \"{to_name}\" given"
            ),
        );
        return Ok(());
    };
    let Some(from_encodings) =
        optional_encoding_list(ed, eg, "mb_convert_encoding", 2, "from_encoding")
    else {
        return Ok(());
    };
    let from = from_encodings
        .iter()
        .copied()
        .find(|encoding| value_has_valid_encoding(&value, *encoding))
        .unwrap_or(from_encodings[0]);
    write_result(rv, convert_value(&value, from, to));
    Ok(())
}

fn fn_mb_strlen(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) = string_argument(ed, eg, "mb_strlen", 0, "string")? else {
        return Ok(());
    };
    let Some(encoding) = optional_encoding(ed, eg, "mb_strlen", 1, "encoding")? else {
        return Ok(());
    };
    let length = encoding
        .decode(&string.php_string_bytes().unwrap_or_default())
        .len();
    write_result(rv, Value::long(length as i64));
    Ok(())
}

fn fn_mb_substr(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) = string_argument(ed, eg, "mb_substr", 0, "string")? else {
        return Ok(());
    };
    let Some(start) = super::typed_internal_int_argument(ed, eg, "mb_substr", 1, "start")? else {
        return Ok(());
    };
    let length = if arg_opt!(ed, 2).is_some()
        && arg!(ed, 2).dereferenced().value_type() != ValueType::Null
    {
        let Some(length) = super::typed_internal_int_argument(ed, eg, "mb_substr", 2, "length")?
        else {
            return Ok(());
        };
        Some(length)
    } else {
        None
    };
    let Some(encoding) = optional_encoding(ed, eg, "mb_substr", 3, "encoding")? else {
        return Ok(());
    };
    let characters = encoding.decode(&string.php_string_bytes().unwrap_or_default());
    let count = characters.len() as i128;
    let start = if start < 0 {
        (count + i128::from(start)).max(0)
    } else {
        i128::from(start).min(count)
    };
    let end = match length {
        None => count,
        Some(length) if length >= 0 => (start + i128::from(length)).min(count),
        Some(length) => (count + i128::from(length)).max(start).min(count),
    };
    let result = encoding.encode(&characters[start as usize..end as usize]);
    write_result(rv, super::php_byte_result(result, false));
    Ok(())
}

fn is_cased(character: char) -> bool {
    character.is_alphabetic()
        && (character.to_lowercase().to_string() != character.to_string()
            || character.to_uppercase().to_string() != character.to_string())
}

fn is_case_ignorable(character: char) -> bool {
    matches!(character, '\'' | '.' | ':' | '\u{00b7}' | '\u{0387}')
        || matches!(u32::from(character), 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f)
}

fn sigma_is_final(characters: &[char], index: usize) -> bool {
    let mut examined = 0usize;
    let mut before = false;
    for character in characters[..index].iter().rev() {
        if is_case_ignorable(*character) {
            examined += 1;
            if examined >= 64 {
                break;
            }
            continue;
        }
        before = is_cased(*character);
        break;
    }
    if !before {
        return false;
    }
    for character in &characters[index + 1..] {
        if is_case_ignorable(*character) {
            continue;
        }
        return !is_cased(*character);
    }
    true
}

fn lowercase(characters: &[char]) -> Vec<char> {
    let mut output = Vec::with_capacity(characters.len());
    for (index, character) in characters.iter().copied().enumerate() {
        if character == '\u{03a3}' && sigma_is_final(characters, index) {
            output.push('\u{03c2}');
        } else {
            output.extend(character.to_lowercase());
        }
    }
    output
}

fn fn_mb_strtolower(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) = string_argument(ed, eg, "mb_strtolower", 0, "string")? else {
        return Ok(());
    };
    let Some(encoding) = optional_encoding(ed, eg, "mb_strtolower", 1, "encoding")? else {
        return Ok(());
    };
    let characters = encoding.decode(&string.php_string_bytes().unwrap_or_default());
    write_result(
        rv,
        super::php_byte_result(encoding.encode(&lowercase(&characters)), false),
    );
    Ok(())
}

fn casefold_with_origins(characters: &[char]) -> (Vec<char>, Vec<usize>) {
    let mut folded = Vec::with_capacity(characters.len());
    let mut origins = Vec::with_capacity(characters.len());
    for (index, character) in characters.iter().copied().enumerate() {
        for lowered in character.to_lowercase() {
            folded.push(if lowered == '\u{03c2}' {
                '\u{03c3}'
            } else {
                lowered
            });
            origins.push(index);
        }
    }
    (folded, origins)
}

fn fn_mb_stripos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(haystack) = string_argument(ed, eg, "mb_stripos", 0, "haystack")? else {
        return Ok(());
    };
    let Some(needle) = string_argument(ed, eg, "mb_stripos", 1, "needle")? else {
        return Ok(());
    };
    let offset = if arg_opt!(ed, 2).is_some() {
        let Some(offset) = super::typed_internal_int_argument(ed, eg, "mb_stripos", 2, "offset")?
        else {
            return Ok(());
        };
        offset
    } else {
        0
    };
    let Some(encoding) = optional_encoding(ed, eg, "mb_stripos", 3, "encoding")? else {
        return Ok(());
    };
    let haystack = encoding.decode(&haystack.php_string_bytes().unwrap_or_default());
    let needle = encoding.decode(&needle.php_string_bytes().unwrap_or_default());
    let count = haystack.len() as i128;
    let start = if offset < 0 {
        count + i128::from(offset)
    } else {
        i128::from(offset)
    };
    if !(0..=count).contains(&start) {
        value_error(
            eg,
            "mb_stripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)",
        );
        return Ok(());
    }
    if needle.is_empty() {
        write_result(rv, Value::long(start as i64));
        return Ok(());
    }
    let (folded_haystack, origins) = casefold_with_origins(&haystack);
    let (folded_needle, _) = casefold_with_origins(&needle);
    let found = folded_haystack
        .windows(folded_needle.len())
        .enumerate()
        .find(|(index, window)| origins[*index] >= start as usize && *window == folded_needle)
        .map(|(index, _)| origins[index]);
    match found {
        Some(index) => write_result(rv, Value::long(index as i64)),
        None => write_result(rv, Value::bool(false)),
    }
    Ok(())
}

fn fn_mb_ord(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) = string_argument(ed, eg, "mb_ord", 0, "string")? else {
        return Ok(());
    };
    let Some(encoding) = optional_encoding(ed, eg, "mb_ord", 1, "encoding")? else {
        return Ok(());
    };
    let bytes = string.php_string_bytes().unwrap_or_default();
    if bytes.is_empty() {
        value_error(eg, "mb_ord(): Argument #1 ($string) must not be empty");
        return Ok(());
    }
    if !encoding.is_valid(&bytes) {
        write_result(rv, Value::bool(false));
        return Ok(());
    }
    match encoding.decode(&bytes).first() {
        Some(character) => write_result(rv, Value::long(i64::from(u32::from(*character)))),
        None => write_result(rv, Value::bool(false)),
    }
    Ok(())
}

fn fn_mb_parse_str(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) = super::typed_internal_string_value_argument_expected(
        ed,
        eg,
        "mb_parse_str",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let input = input.php_string_bytes().unwrap_or_default();
    let result = Value::array(super::parse_query_string_array(input.as_ref()));
    arg_mut!(ed, 1, result);
    write_result(rv, Value::bool(true));
    Ok(())
}

struct Declaration {
    name: &'static str,
    handler: InternalFunctionHandler,
    required: u32,
    parameters: &'static [&'static str],
    parameter_types: fn() -> Vec<ParamTypeHint>,
    return_type: fn() -> ParamTypeHint,
    defaults: fn() -> Vec<Option<Value>>,
}

fn nullable_string() -> ParamTypeHint {
    ParamTypeHint::Nullable(std::rc::Rc::new(ParamTypeHint::String))
}

fn string_and_nullable_string() -> Vec<ParamTypeHint> {
    vec![ParamTypeHint::String, nullable_string()]
}

fn int_or_false() -> ParamTypeHint {
    ParamTypeHint::Union(vec![ParamTypeHint::Int, ParamTypeHint::ClassName("false".into())].into())
}

fn string_or_false() -> ParamTypeHint {
    ParamTypeHint::Union(
        vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".into()),
        ]
        .into(),
    )
}

fn array_or_string() -> ParamTypeHint {
    ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::String].into())
}

fn array_string_or_null() -> ParamTypeHint {
    ParamTypeHint::Union(
        vec![
            ParamTypeHint::Array,
            ParamTypeHint::String,
            ParamTypeHint::ClassName("null".into()),
        ]
        .into(),
    )
}

#[cold]
#[inline(never)]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let declarations = [
        Declaration {
            name: "mb_strlen",
            handler: fn_mb_strlen,
            required: 1,
            parameters: &["string", "encoding"],
            parameter_types: string_and_nullable_string,
            return_type: || ParamTypeHint::Int,
            defaults: || vec![None, Some(Value::null())],
        },
        Declaration {
            name: "mb_stripos",
            handler: fn_mb_stripos,
            required: 2,
            parameters: &["haystack", "needle", "offset", "encoding"],
            parameter_types: || {
                vec![
                    ParamTypeHint::String,
                    ParamTypeHint::String,
                    ParamTypeHint::Int,
                    nullable_string(),
                ]
            },
            return_type: int_or_false,
            defaults: || vec![None, None, Some(Value::long(0)), Some(Value::null())],
        },
        Declaration {
            name: "mb_substr",
            handler: fn_mb_substr,
            required: 2,
            parameters: &["string", "start", "length", "encoding"],
            parameter_types: || {
                vec![
                    ParamTypeHint::String,
                    ParamTypeHint::Int,
                    ParamTypeHint::Nullable(std::rc::Rc::new(ParamTypeHint::Int)),
                    nullable_string(),
                ]
            },
            return_type: || ParamTypeHint::String,
            defaults: || vec![None, None, Some(Value::null()), Some(Value::null())],
        },
        Declaration {
            name: "mb_convert_encoding",
            handler: fn_mb_convert_encoding,
            required: 2,
            parameters: &["string", "to_encoding", "from_encoding"],
            parameter_types: || {
                vec![
                    array_or_string(),
                    ParamTypeHint::String,
                    array_string_or_null(),
                ]
            },
            return_type: || {
                ParamTypeHint::Union(
                    vec![
                        ParamTypeHint::Array,
                        ParamTypeHint::String,
                        ParamTypeHint::ClassName("false".into()),
                    ]
                    .into(),
                )
            },
            defaults: || vec![None, None, Some(Value::null())],
        },
        Declaration {
            name: "mb_strtolower",
            handler: fn_mb_strtolower,
            required: 1,
            parameters: &["string", "encoding"],
            parameter_types: string_and_nullable_string,
            return_type: || ParamTypeHint::String,
            defaults: || vec![None, Some(Value::null())],
        },
        Declaration {
            name: "mb_detect_encoding",
            handler: fn_mb_detect_encoding,
            required: 1,
            parameters: &["string", "encodings", "strict"],
            parameter_types: || {
                vec![
                    ParamTypeHint::String,
                    array_string_or_null(),
                    ParamTypeHint::Bool,
                ]
            },
            return_type: string_or_false,
            defaults: || vec![None, Some(Value::null()), Some(Value::bool(false))],
        },
        Declaration {
            name: "mb_check_encoding",
            handler: fn_mb_check_encoding,
            required: 0,
            parameters: &["value", "encoding"],
            parameter_types: || vec![array_string_or_null(), nullable_string()],
            return_type: || ParamTypeHint::Bool,
            defaults: || vec![Some(Value::null()), Some(Value::null())],
        },
        Declaration {
            name: "mb_ord",
            handler: fn_mb_ord,
            required: 1,
            parameters: &["string", "encoding"],
            parameter_types: string_and_nullable_string,
            return_type: int_or_false,
            defaults: || vec![None, Some(Value::null())],
        },
    ];

    let mut functions = Vec::with_capacity(declarations.len() + 1);

    let mut parse_str = Box::new(make_internal_function_ref(
        fn_mb_parse_str,
        2,
        2,
        0b10,
        vec!["string".into(), "result".into()],
    ));
    parse_str.common.sig.param_type_hints = vec![ParamTypeHint::String, ParamTypeHint::None];
    parse_str.common.sig.return_type_hint = ParamTypeHint::Bool;
    let pointer = &parse_str.common as *const FunctionCommon;
    eg.register_function("mb_parse_str", pointer)
        .expect("mb_parse_str registration is unique");
    eg.register_internal_function_reflection_metadata(pointer, vec![None, None], "mbstring");
    functions.push(parse_str);

    for declaration in declarations {
        let mut function = Box::new(
            make_internal_function(
                declaration.handler,
                declaration.parameters.len() as u32,
                declaration.required,
                Vec::new(),
            )
            .with_static_parameter_names(declaration.parameters),
        );
        function.common.sig.param_type_hints = (declaration.parameter_types)();
        function.common.sig.return_type_hint = (declaration.return_type)();
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(declaration.name, pointer)
            .expect("mbstring function registration is unique");
        eg.register_internal_function_reflection_metadata(
            pointer,
            (declaration.defaults)(),
            "mbstring",
        );
        functions.push(function);
    }
    functions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_encodings_round_trip_phpunit_boundaries() {
        let utf8: Vec<char> = "€ Žlu".chars().collect();
        let windows = Encoding::Windows1252.encode(&utf8);
        assert_eq!(windows[0], 0x80);
        assert_eq!(Encoding::Windows1252.decode(&windows), utf8);
        let latin = Encoding::Latin1.decode(&[0x80, 0xe9]);
        assert_eq!(latin, ['\u{80}', '\u{e9}']);
        assert!(!Encoding::Utf8.is_valid(&[0xff]));
        assert_eq!(Encoding::Utf8.decode(&[b'A', 0xff, b'B']), ['A', '?', 'B']);
    }

    #[test]
    fn unicode_slicing_casing_and_casefold_use_character_offsets() {
        let input: Vec<char> = "ŽLUŤOUČKÝ".chars().collect();
        assert_eq!(lowercase(&input).iter().collect::<String>(), "žluťoučký");
        assert_eq!(
            lowercase(&"aΣ b".chars().collect::<Vec<_>>()),
            ['a', 'ς', ' ', 'b']
        );
        let (folded, origins) = casefold_with_origins(&"😀AbC".chars().collect::<Vec<_>>());
        assert_eq!(folded.iter().collect::<String>(), "😀abc");
        assert_eq!(origins, [0, 1, 2, 3]);
    }
}
