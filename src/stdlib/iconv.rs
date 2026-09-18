//! PHP 8.5 iconv extension backed by the process-native GNU conversion ABI.
//!
//! Conversion descriptors are intentionally opened only on extension calls.
//! Ordinary requests retain no iconv state; the three deprecated encoding
//! settings reuse the existing sparse request-local INI sidecar.

use std::borrow::Cow;

use super::native_process::{NativeIconvError, convert_encoding};
use crate::compiler::make_internal_function;
use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{
    FunctionCommon, InternalFunction, InternalFunctionHandler, ParamTypeHint,
};

const MAX_ENCODING_LENGTH: usize = 64;
const INTERNAL_ENCODING: &str = "iconv.internal_encoding";
const INPUT_ENCODING: &str = "iconv.input_encoding";
const OUTPUT_ENCODING: &str = "iconv.output_encoding";
const MIME_CONTINUE_ON_ERROR: i64 = 2;

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn write_value(return_value: *mut Value, value: Value) {
    super::write_return_value(return_value, value);
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn write_bytes(return_value: *mut Value, bytes: Vec<u8>) {
    write_value(return_value, super::php_byte_result(bytes, false));
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn required_string(
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

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn string_bytes(value: &Value) -> Vec<u8> {
    value.php_string_bytes().unwrap_or_default().into_owned()
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn request_encoding<'a>(eg: &'a ExecutorGlobals, key: &str) -> Cow<'a, str> {
    let legacy = public_setting_name(key);
    if let Some(encoding) = eg
        .ini_overrides
        .as_deref()
        .and_then(|settings| settings.get(key))
        .filter(|encoding| !encoding.is_empty())
    {
        return Cow::Borrowed(encoding);
    }
    if let Some(encoding) = eg
        .ini_overrides
        .as_deref()
        .and_then(|settings| settings.get(legacy))
        .filter(|encoding| !encoding.is_empty())
    {
        return Cow::Borrowed(encoding);
    }
    eg.ini_overrides
        .as_deref()
        .and_then(|settings| settings.get("default_charset"))
        .filter(|encoding| !encoding.is_empty())
        .map_or(Cow::Borrowed("UTF-8"), |encoding| Cow::Borrowed(encoding))
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn optional_encoding(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<Vec<u8>>, VmError> {
    let supplied = super::owned_argument(ed, index);
    match supplied.dereferenced().value_type() {
        ValueType::Undef | ValueType::Null => Ok(Some(
            request_encoding(eg, INTERNAL_ENCODING).as_bytes().to_vec(),
        )),
        _ => {
            let Some(value) = required_string(ed, eg, function, index, parameter)? else {
                return Ok(None);
            };
            let bytes = string_bytes(&value);
            if !validate_encoding(ed, eg, function, &bytes)? {
                return Ok(None);
            }
            Ok(Some(bytes))
        }
    }
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn validate_encoding(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    encoding: &[u8],
) -> Result<bool, VmError> {
    if encoding.len() < MAX_ENCODING_LENGTH {
        return Ok(true);
    }
    super::report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!(
            "{function}(): Encoding parameter exceeds the maximum allowed length of {MAX_ENCODING_LENGTH} characters"
        ),
    )?;
    Ok(false)
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn encoding_label(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn convert_with_diagnostic(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    from: &[u8],
    to: &[u8],
    input: &[u8],
) -> Result<Option<Vec<u8>>, VmError> {
    match convert_encoding(from, to, input) {
        Ok(result) => Ok(Some(result)),
        Err(NativeIconvError::UnknownEncoding) => {
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!(
                    "{function}(): Wrong encoding, conversion from \"{}\" to \"{}\" is not allowed",
                    encoding_label(from),
                    encoding_label(to)
                ),
            )?;
            Ok(None)
        }
        Err(NativeIconvError::IllegalSequence) => {
            super::report_internal_diagnostic(
                eg,
                ed,
                8,
                "Notice",
                &format!("{function}(): Detected an illegal character in input string"),
            )?;
            Ok(None)
        }
        Err(NativeIconvError::IncompleteSequence) => {
            super::report_internal_diagnostic(
                eg,
                ed,
                8,
                "Notice",
                &format!(
                    "{function}(): Detected an incomplete multibyte character in input string"
                ),
            )?;
            Ok(None)
        }
        Err(NativeIconvError::Other(error)) => {
            super::report_internal_diagnostic(
                eg,
                ed,
                8,
                "Notice",
                &format!("{function}(): Unknown error ({error})"),
            )?;
            Ok(None)
        }
        Err(NativeIconvError::None) => Ok(None),
    }
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_iconv(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(from) = required_string(ed, eg, "iconv", 0, "from_encoding")? else {
        return Ok(());
    };
    let Some(to) = required_string(ed, eg, "iconv", 1, "to_encoding")? else {
        return Ok(());
    };
    let Some(input) = required_string(ed, eg, "iconv", 2, "string")? else {
        return Ok(());
    };
    let from = string_bytes(&from);
    let to = string_bytes(&to);
    let input = string_bytes(&input);
    if !validate_encoding(ed, eg, "iconv", &from)? || !validate_encoding(ed, eg, "iconv", &to)? {
        write_value(rv, Value::bool(false));
        return Ok(());
    }
    match convert_with_diagnostic(ed, eg, "iconv", &from, &to, &input)? {
        Some(result) => write_bytes(rv, result),
        None => write_value(rv, Value::bool(false)),
    }
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn setting_key(setting_type: &str) -> Option<&'static str> {
    if setting_type.eq_ignore_ascii_case("input_encoding") {
        Some(INPUT_ENCODING)
    } else if setting_type.eq_ignore_ascii_case("output_encoding") {
        Some(OUTPUT_ENCODING)
    } else if setting_type.eq_ignore_ascii_case("internal_encoding") {
        Some(INTERNAL_ENCODING)
    } else {
        None
    }
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn public_setting_name(key: &str) -> &'static str {
    match key {
        INPUT_ENCODING => "input_encoding",
        OUTPUT_ENCODING => "output_encoding",
        _ => "internal_encoding",
    }
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_iconv_set_encoding(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(setting_type) = required_string(ed, eg, "iconv_set_encoding", 0, "type")? else {
        return Ok(());
    };
    let Some(encoding) = required_string(ed, eg, "iconv_set_encoding", 1, "encoding")? else {
        return Ok(());
    };
    let setting_type = setting_type.as_str().unwrap_or_default();
    let Some(key) = setting_key(setting_type) else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    let encoding_bytes = string_bytes(&encoding);
    if !validate_encoding(ed, eg, "iconv_set_encoding", &encoding_bytes)? {
        write_value(rv, Value::bool(false));
        return Ok(());
    }
    super::report_internal_deprecation(
        eg,
        ed,
        &format!("iconv_set_encoding(): Use of {key} is deprecated"),
    )?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let encoding = String::from_utf8_lossy(&encoding_bytes).into_owned();
    eg.ini_overrides
        .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
        .insert(key.to_string(), encoding);
    write_value(rv, Value::bool(true));
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn all_encodings(eg: &ExecutorGlobals) -> PhpArray {
    let mut values = PhpArray::new();
    for key in [INPUT_ENCODING, OUTPUT_ENCODING, INTERNAL_ENCODING] {
        values.set_str(
            public_setting_name(key),
            Value::string(request_encoding(eg, key).into_owned()),
        );
    }
    values
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_iconv_get_encoding(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let supplied = super::owned_argument(ed, 0);
    let setting_type = if supplied.dereferenced().value_type() == ValueType::Undef {
        Cow::Borrowed("all")
    } else {
        let Some(value) = required_string(ed, eg, "iconv_get_encoding", 0, "type")? else {
            return Ok(());
        };
        Cow::Owned(value.as_str().unwrap_or_default().to_string())
    };
    if setting_type.eq_ignore_ascii_case("all") {
        write_value(rv, Value::array(all_encodings(eg)));
    } else if let Some(key) = setting_key(&setting_type) {
        write_value(rv, Value::string(request_encoding(eg, key).into_owned()));
    } else {
        write_value(rv, Value::bool(false));
    }
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn to_ucs4(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    input: &[u8],
    encoding: &[u8],
) -> Result<Option<Vec<u8>>, VmError> {
    convert_with_diagnostic(ed, eg, function, encoding, b"UCS-4LE", input)
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_iconv_strlen(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) = required_string(ed, eg, "iconv_strlen", 0, "string")? else {
        return Ok(());
    };
    let Some(encoding) = optional_encoding(ed, eg, "iconv_strlen", 1, "encoding")? else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    let input = string_bytes(&input);
    match to_ucs4(ed, eg, "iconv_strlen", &input, &encoding)? {
        Some(converted) => write_value(rv, Value::long((converted.len() / 4) as i64)),
        None => write_value(rv, Value::bool(false)),
    }
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn substring_bounds(length: usize, offset: i64, requested: Option<i64>) -> (usize, usize) {
    let start = if offset >= 0 {
        usize::try_from(offset).unwrap_or(usize::MAX).min(length)
    } else {
        length.saturating_sub(offset.unsigned_abs() as usize)
    };
    let end = match requested {
        None => length,
        Some(count) if count >= 0 => start
            .saturating_add(usize::try_from(count).unwrap_or(usize::MAX))
            .min(length),
        Some(count) => length
            .saturating_sub(count.unsigned_abs() as usize)
            .max(start),
    };
    (start, end)
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_iconv_substr(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) = required_string(ed, eg, "iconv_substr", 0, "string")? else {
        return Ok(());
    };
    let Some(offset) = super::typed_internal_int_argument(ed, eg, "iconv_substr", 1, "offset")?
    else {
        return Ok(());
    };
    let length_value = super::owned_argument(ed, 2);
    let length = match length_value.dereferenced().value_type() {
        ValueType::Undef | ValueType::Null => None,
        _ => super::typed_internal_int_argument(ed, eg, "iconv_substr", 2, "length")?,
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    let Some(encoding) = optional_encoding(ed, eg, "iconv_substr", 3, "encoding")? else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    let input = string_bytes(&input);
    let Some(ucs4) = to_ucs4(ed, eg, "iconv_substr", &input, &encoding)? else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    let character_count = ucs4.len() / 4;
    let (start, end) = substring_bounds(character_count, offset, length);
    let selected = &ucs4[start * 4..end * 4];
    match convert_with_diagnostic(ed, eg, "iconv_substr", b"UCS-4LE", &encoding, selected)? {
        Some(result) => write_bytes(rv, result),
        None => write_value(rv, Value::bool(false)),
    }
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn aligned_position(haystack: &[u8], needle: &[u8], start: usize, reverse: bool) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() || start > haystack.len() / 4 {
        return None;
    }
    let first = start.saturating_mul(4);
    let mut candidates = haystack[first..]
        .windows(needle.len())
        .enumerate()
        .filter(|(index, bytes)| index % 4 == 0 && *bytes == needle)
        .map(|(index, _)| (first + index) / 4);
    if reverse {
        candidates.last()
    } else {
        candidates.next()
    }
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_iconv_strpos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(haystack) = required_string(ed, eg, "iconv_strpos", 0, "haystack")? else {
        return Ok(());
    };
    let Some(needle) = required_string(ed, eg, "iconv_strpos", 1, "needle")? else {
        return Ok(());
    };
    let offset_value = super::owned_argument(ed, 2);
    let offset = if offset_value.dereferenced().value_type() == ValueType::Undef {
        0
    } else {
        let Some(offset) = super::typed_internal_int_argument(ed, eg, "iconv_strpos", 2, "offset")?
        else {
            return Ok(());
        };
        offset
    };
    let Some(encoding) = optional_encoding(ed, eg, "iconv_strpos", 3, "encoding")? else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    let haystack = string_bytes(&haystack);
    let needle = string_bytes(&needle);
    let Some(haystack) = to_ucs4(ed, eg, "iconv_strpos", &haystack, &encoding)? else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    let Some(needle) = to_ucs4(ed, eg, "iconv_strpos", &needle, &encoding)? else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    let characters = haystack.len() / 4;
    let start = if offset >= 0 {
        usize::try_from(offset).unwrap_or(usize::MAX)
    } else {
        characters.saturating_sub(offset.unsigned_abs() as usize)
    };
    if start > characters || offset < -(characters as i64) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "iconv_strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)",
        ));
        return Ok(());
    }
    match aligned_position(&haystack, &needle, start, false) {
        Some(position) => write_value(rv, Value::long(position as i64)),
        None => write_value(rv, Value::bool(false)),
    }
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_iconv_strrpos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(haystack) = required_string(ed, eg, "iconv_strrpos", 0, "haystack")? else {
        return Ok(());
    };
    let Some(needle) = required_string(ed, eg, "iconv_strrpos", 1, "needle")? else {
        return Ok(());
    };
    let Some(encoding) = optional_encoding(ed, eg, "iconv_strrpos", 2, "encoding")? else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    let haystack = string_bytes(&haystack);
    let needle = string_bytes(&needle);
    let Some(haystack) = to_ucs4(ed, eg, "iconv_strrpos", &haystack, &encoding)? else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    let Some(needle) = to_ucs4(ed, eg, "iconv_strrpos", &needle, &encoding)? else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    match aligned_position(&haystack, &needle, 0, true) {
        Some(position) => write_value(rv, Value::long(position as i64)),
        None => write_value(rv, Value::bool(false)),
    }
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn decode_q_word(encoded: &[u8]) -> Option<Vec<u8>> {
    let mut result = Vec::with_capacity(encoded.len());
    let mut index = 0;
    while index < encoded.len() {
        match encoded[index] {
            b'_' => {
                result.push(b' ');
                index += 1;
            }
            b'=' if index + 2 < encoded.len() => {
                let high = (encoded[index + 1] as char).to_digit(16)? as u8;
                let low = (encoded[index + 2] as char).to_digit(16)? as u8;
                result.push((high << 4) | low);
                index += 3;
            }
            b'=' => return None,
            byte => {
                result.push(byte);
                index += 1;
            }
        }
    }
    Some(result)
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn encoded_word_at(input: &[u8], start: usize) -> Option<(usize, &[u8], u8, &[u8])> {
    if input.get(start..start + 2)? != b"=?" {
        return None;
    }
    let charset_end = input[start + 2..].iter().position(|byte| *byte == b'?')? + start + 2;
    let scheme = *input.get(charset_end + 1)?;
    if !matches!(scheme, b'B' | b'b' | b'Q' | b'q') || input.get(charset_end + 2) != Some(&b'?') {
        return None;
    }
    let payload_start = charset_end + 3;
    let payload_end = input[payload_start..]
        .windows(2)
        .position(|bytes| bytes == b"?=")?
        + payload_start;
    Some((
        payload_end + 2,
        &input[start + 2..charset_end],
        scheme,
        &input[payload_start..payload_end],
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MimeDecodeError {
    Malformed,
    Conversion(NativeIconvError),
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn has_adjacent_encoded_words(input: &[u8]) -> bool {
    let mut index = 0;
    while index < input.len() {
        if let Some((end, ..)) = encoded_word_at(input, index) {
            if encoded_word_at(input, end).is_some() {
                return true;
            }
            index = end;
        } else {
            index += 1;
        }
    }
    false
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn mime_decode_bytes(input: &[u8], mode: i64, target: &[u8]) -> Result<Vec<u8>, MimeDecodeError> {
    if mode & 1 != 0 && has_adjacent_encoded_words(input) {
        return Ok(input.to_vec());
    }
    let mut result = Vec::with_capacity(input.len());
    let mut index = 0;
    let mut previous_encoded = false;
    while index < input.len() {
        if input[index].is_ascii_whitespace() && previous_encoded {
            let whitespace_start = index;
            while index < input.len() && input[index].is_ascii_whitespace() {
                index += 1;
            }
            if encoded_word_at(input, index).is_some() {
                continue;
            }
            result.extend_from_slice(&input[whitespace_start..index]);
            previous_encoded = false;
            continue;
        }
        let Some((end, charset, scheme, payload)) = encoded_word_at(input, index) else {
            if input[index].is_ascii() {
                result.push(input[index]);
            } else if mode & MIME_CONTINUE_ON_ERROR == 0 {
                let error = convert_encoding(b"ASCII", target, &input[index..index + 1])
                    .err()
                    .unwrap_or(NativeIconvError::IllegalSequence);
                return Err(MimeDecodeError::Conversion(error));
            }
            previous_encoded = false;
            index += 1;
            continue;
        };
        let decoded = if matches!(scheme, b'B' | b'b') {
            crate::base64::decode(payload, true)
        } else {
            decode_q_word(payload)
        };
        let Some(decoded) = decoded else {
            // PHP historically drops malformed base64 encoded words even
            // without CONTINUE_ON_ERROR; malformed Q payloads remain errors.
            if matches!(scheme, b'B' | b'b') {
                index = end;
                previous_encoded = false;
                continue;
            }
            if mode & MIME_CONTINUE_ON_ERROR != 0 {
                result.extend_from_slice(&input[index..end]);
                index = end;
                previous_encoded = false;
                continue;
            }
            return Err(MimeDecodeError::Malformed);
        };
        let charset = charset
            .split(|byte| *byte == b'*')
            .next()
            .unwrap_or(charset);
        match convert_encoding(charset, target, &decoded) {
            Ok(converted) => result.extend_from_slice(&converted),
            Err(NativeIconvError::UnknownEncoding) if mode & MIME_CONTINUE_ON_ERROR != 0 => {
                result.extend_from_slice(&decoded);
            }
            Err(_) if mode & MIME_CONTINUE_ON_ERROR != 0 => {
                index = end;
                if matches!(scheme, b'Q' | b'q')
                    && result.last().is_some_and(u8::is_ascii_whitespace)
                {
                    while input.get(index).is_some_and(u8::is_ascii_whitespace) {
                        index += 1;
                    }
                }
                previous_encoded = false;
                continue;
            }
            Err(error) => return Err(MimeDecodeError::Conversion(error)),
        }
        index = end;
        previous_encoded = true;
    }
    Ok(result)
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn mime_arguments(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    encoding_index: u32,
) -> Result<Option<(i64, Vec<u8>)>, VmError> {
    let mode_value = super::owned_argument(ed, 1);
    let mode = if mode_value.dereferenced().value_type() == ValueType::Undef {
        0
    } else {
        let Some(mode) = super::typed_internal_int_argument(ed, eg, function, 1, "mode")? else {
            return Ok(None);
        };
        mode
    };
    let Some(encoding) = optional_encoding(ed, eg, function, encoding_index, "encoding")? else {
        return Ok(None);
    };
    if !validate_encoding(ed, eg, function, &encoding)? {
        return Ok(None);
    }
    Ok(Some((mode, encoding)))
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn report_malformed(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<(), VmError> {
    super::report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!("{function}(): Malformed string"),
    )?;
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn report_mime_conversion_error(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    error: NativeIconvError,
) -> Result<(), VmError> {
    let (level, message) = match error {
        NativeIconvError::IllegalSequence => (
            8,
            format!("{function}(): Detected an illegal character in input string"),
        ),
        NativeIconvError::IncompleteSequence => (
            8,
            format!("{function}(): Detected an incomplete multibyte character in input string"),
        ),
        NativeIconvError::UnknownEncoding => (2, format!("{function}(): Wrong encoding specified")),
        NativeIconvError::Other(error) => (8, format!("{function}(): Unknown error ({error})")),
        NativeIconvError::None => return Ok(()),
    };
    let label = if level == 2 { "Warning" } else { "Notice" };
    super::report_internal_diagnostic(eg, ed, level, label, &message)?;
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_iconv_mime_decode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) = required_string(ed, eg, "iconv_mime_decode", 0, "string")? else {
        return Ok(());
    };
    let Some((mode, encoding)) = mime_arguments(ed, eg, "iconv_mime_decode", 2)? else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    match mime_decode_bytes(&string_bytes(&input), mode, &encoding) {
        Ok(decoded) => write_bytes(rv, decoded),
        Err(MimeDecodeError::Malformed) => {
            report_malformed(ed, eg, "iconv_mime_decode")?;
            write_value(rv, Value::bool(false));
        }
        Err(MimeDecodeError::Conversion(error)) => {
            report_mime_conversion_error(ed, eg, "iconv_mime_decode", error)?;
            write_value(rv, Value::bool(false));
        }
    }
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn unfold_headers(headers: &[u8]) -> Vec<Vec<u8>> {
    let normalized = headers
        .split(|byte| *byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line));
    let mut logical: Vec<Vec<u8>> = Vec::new();
    for line in normalized {
        if line.first().is_some_and(u8::is_ascii_whitespace) && !logical.is_empty() {
            logical.last_mut().unwrap().push(b' ');
            logical
                .last_mut()
                .unwrap()
                .extend_from_slice(line.trim_ascii());
        } else if !line.is_empty() {
            logical.push(line.to_vec());
        }
    }
    logical
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn append_header(result: &mut PhpArray, name: &str, value: Value) {
    let existing = result.get_str(name).map(Value::clone_for_php_storage);
    match existing {
        None => result.set_str(name, value),
        Some(existing) => {
            let mut values = if let Some(existing_array) = existing.as_array() {
                let mut values = PhpArray::with_packed_capacity(existing_array.len() + 1);
                for entry in existing_array.values() {
                    values.push(entry.clone_for_php_storage());
                }
                values
            } else {
                let mut values = PhpArray::with_packed_capacity(2);
                values.push(existing);
                values
            };
            values.push(value);
            result.set_str(name, Value::array(values));
        }
    }
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_iconv_mime_decode_headers(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(headers) = required_string(ed, eg, "iconv_mime_decode_headers", 0, "headers")? else {
        return Ok(());
    };
    let Some((mode, encoding)) = mime_arguments(ed, eg, "iconv_mime_decode_headers", 2)? else {
        write_value(rv, Value::bool(false));
        return Ok(());
    };
    let mut result = PhpArray::new();
    for line in unfold_headers(&string_bytes(&headers)) {
        let Some(colon) = line.iter().position(|byte| *byte == b':') else {
            if mode & MIME_CONTINUE_ON_ERROR != 0 {
                continue;
            }
            report_malformed(ed, eg, "iconv_mime_decode_headers")?;
            write_value(rv, Value::bool(false));
            return Ok(());
        };
        let name = String::from_utf8_lossy(line[..colon].trim_ascii()).into_owned();
        let value = line[colon + 1..].trim_ascii();
        let decoded = match mime_decode_bytes(value, mode, &encoding) {
            Ok(decoded) => decoded,
            Err(_) if mode & MIME_CONTINUE_ON_ERROR != 0 => value.to_vec(),
            Err(MimeDecodeError::Malformed) => {
                report_malformed(ed, eg, "iconv_mime_decode_headers")?;
                write_value(rv, Value::bool(false));
                return Ok(());
            }
            Err(MimeDecodeError::Conversion(error)) => {
                report_mime_conversion_error(ed, eg, "iconv_mime_decode_headers", error)?;
                write_value(rv, Value::bool(false));
                return Ok(());
            }
        };
        append_header(&mut result, &name, super::php_byte_result(decoded, false));
    }
    write_value(rv, Value::array(result));
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn option_string(options: &PhpArray, name: &str) -> Option<Vec<u8>> {
    options
        .get_str(name)
        .and_then(Value::php_string_bytes)
        .map(Cow::into_owned)
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn q_encode(input: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(input.len());
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in input.iter().copied() {
        if (33..=126).contains(&byte) && !matches!(byte, b'=' | b'?' | b'_') {
            encoded.push(byte);
        } else {
            encoded.extend_from_slice(&[
                b'=',
                HEX[(byte >> 4) as usize],
                HEX[(byte & 15) as usize],
            ]);
        }
    }
    encoded
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn q_encode_units(input: &[u8], charset: &[u8]) -> Vec<Vec<u8>> {
    if charset.eq_ignore_ascii_case(b"UTF-8") {
        if let Ok(text) = std::str::from_utf8(input) {
            return text
                .chars()
                .map(|character| {
                    let mut buffer = [0_u8; 4];
                    q_encode(character.encode_utf8(&mut buffer).as_bytes())
                })
                .collect();
        }
    }
    input.iter().map(|byte| q_encode(&[*byte])).collect()
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn wrap_q_encoded_text(
    field: &[u8],
    charset: &[u8],
    units: &[Vec<u8>],
    line_length: usize,
    line_break: &[u8],
) -> Option<Vec<u8>> {
    let overhead = charset.len().checked_add(7)?;
    let first_prefix = field.len().checked_add(2)?;
    if line_length <= first_prefix + overhead || line_length <= 1 + overhead {
        return None;
    }
    let mut result = Vec::new();
    result.extend_from_slice(field);
    result.extend_from_slice(b": ");
    let mut index = 0_usize;
    let mut first = true;
    loop {
        let prefix = if first { first_prefix } else { 1 };
        let available = line_length - prefix - overhead;
        let begin = index;
        let mut used = 0_usize;
        while let Some(unit) = units.get(index) {
            if used.saturating_add(unit.len()) > available {
                break;
            }
            used += unit.len();
            index += 1;
        }
        if begin == index && !units.is_empty() {
            return None;
        }
        if !first {
            result.extend_from_slice(line_break);
            result.push(b' ');
        }
        result.extend_from_slice(b"=?");
        result.extend_from_slice(charset);
        result.extend_from_slice(b"?Q?");
        for unit in &units[begin..index] {
            result.extend_from_slice(unit);
        }
        result.extend_from_slice(b"?=");
        if index == units.len() {
            break;
        }
        first = false;
    }
    Some(result)
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn wrap_b_encoded_text(
    field: &[u8],
    input_charset: &[u8],
    output_charset: &[u8],
    input: &[u8],
    line_length: usize,
    line_break: &[u8],
) -> Result<Option<Vec<u8>>, NativeIconvError> {
    let ucs4 = convert_encoding(input_charset, b"UCS-4LE", input)?;
    let overhead = match output_charset.len().checked_add(7) {
        Some(overhead) => overhead,
        None => return Ok(None),
    };
    let first_prefix = match field.len().checked_add(2) {
        Some(prefix) => prefix,
        None => return Ok(None),
    };
    if line_length <= first_prefix + overhead || line_length <= 1 + overhead {
        return Ok(None);
    }
    let mut result = Vec::new();
    result.extend_from_slice(field);
    result.extend_from_slice(b": ");
    let character_count = ucs4.len() / 4;
    let mut offset = 0;
    let mut first = true;
    loop {
        let prefix = if first { first_prefix } else { 1 };
        let available = line_length - prefix - overhead;
        let (end, encoded) = if character_count == 0 {
            (0, String::new())
        } else {
            let mut best = None;
            for end in offset + 1..=character_count {
                let converted =
                    convert_encoding(b"UCS-4LE", output_charset, &ucs4[offset * 4..end * 4])?;
                let encoded = crate::base64::encode(&converted);
                if encoded.len() > available {
                    break;
                }
                if best.as_ref().is_none_or(|(_, previous): &(usize, String)| {
                    encoded.len() > previous.len() || end == character_count
                }) {
                    best = Some((end, encoded));
                }
            }
            let Some(best) = best else {
                return Ok(None);
            };
            best
        };
        if !first {
            result.extend_from_slice(line_break);
            result.push(b' ');
        }
        result.extend_from_slice(b"=?");
        result.extend_from_slice(output_charset);
        result.extend_from_slice(b"?B?");
        result.extend_from_slice(encoded.as_bytes());
        result.extend_from_slice(b"?=");
        if end == character_count {
            break;
        }
        offset = end;
        first = false;
    }
    Ok(Some(result))
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_iconv_mime_encode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(field) = required_string(ed, eg, "iconv_mime_encode", 0, "field_name")? else {
        return Ok(());
    };
    let Some(value) = required_string(ed, eg, "iconv_mime_encode", 1, "field_value")? else {
        return Ok(());
    };
    let options_value = super::owned_argument(ed, 2);
    let options_value = options_value.dereferenced();
    let empty_options = PhpArray::new();
    let options = if options_value.value_type() == ValueType::Undef {
        &empty_options
    } else if let Some(options) = options_value.as_array() {
        options
    } else {
        super::typed_internal_argument_error(
            eg,
            "iconv_mime_encode",
            options_value,
            3,
            "options",
            "array",
        );
        return Ok(());
    };
    let input_charset = option_string(options, "input-charset")
        .unwrap_or_else(|| request_encoding(eg, INTERNAL_ENCODING).as_bytes().to_vec());
    let output_charset = option_string(options, "output-charset")
        .unwrap_or_else(|| request_encoding(eg, INTERNAL_ENCODING).as_bytes().to_vec());
    if !validate_encoding(ed, eg, "iconv_mime_encode", &input_charset)?
        || !validate_encoding(ed, eg, "iconv_mime_encode", &output_charset)?
    {
        write_value(rv, Value::bool(false));
        return Ok(());
    }
    let scheme = option_string(options, "scheme")
        .and_then(|value| value.first().copied())
        .filter(|value| matches!(value, b'B' | b'b' | b'Q' | b'q'))
        .unwrap_or(b'B');
    let line_length = options
        .get_str("line-length")
        .map(Value::to_long_val)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(76);
    let line_break = option_string(options, "line-break-chars").unwrap_or_else(|| b"\r\n".to_vec());
    let field = string_bytes(&field);
    let value = string_bytes(&value);
    let wrapped = if matches!(scheme, b'Q' | b'q') {
        match convert_encoding(&input_charset, &output_charset, &value) {
            Ok(converted) => wrap_q_encoded_text(
                &field,
                &output_charset,
                &q_encode_units(&converted, &output_charset),
                line_length,
                &line_break,
            ),
            Err(_) => None,
        }
    } else {
        wrap_b_encoded_text(
            &field,
            &input_charset,
            &output_charset,
            &value,
            line_length,
            &line_break,
        )
        .ok()
        .flatten()
    };
    match wrapped {
        Some(result) => write_bytes(rv, result),
        None => {
            if convert_encoding(&input_charset, &output_charset, &value).is_err() {
                let _ = convert_with_diagnostic(
                    ed,
                    eg,
                    "iconv_mime_encode",
                    &input_charset,
                    &output_charset,
                    &value,
                )?;
                write_value(rv, Value::bool(false));
                return Ok(());
            }
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                "iconv_mime_encode(): Buffer length exceeded",
            )?;
            write_value(rv, Value::bool(false));
        }
    }
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

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn string_or_false() -> ParamTypeHint {
    ParamTypeHint::Union(vec![
        ParamTypeHint::String,
        ParamTypeHint::ClassName("false".to_string()),
    ])
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn int_or_false() -> ParamTypeHint {
    ParamTypeHint::Union(vec![
        ParamTypeHint::Int,
        ParamTypeHint::ClassName("false".to_string()),
    ])
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn array_or_false() -> ParamTypeHint {
    ParamTypeHint::Union(vec![
        ParamTypeHint::Array,
        ParamTypeHint::ClassName("false".to_string()),
    ])
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn array_string_or_false() -> ParamTypeHint {
    ParamTypeHint::Union(vec![
        ParamTypeHint::Array,
        ParamTypeHint::String,
        ParamTypeHint::ClassName("false".to_string()),
    ])
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn nullable_string() -> ParamTypeHint {
    ParamTypeHint::Nullable(Box::new(ParamTypeHint::String))
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let declarations = [
        Declaration {
            name: "iconv_strlen",
            handler: fn_iconv_strlen,
            required: 1,
            parameters: &["string", "encoding"],
            parameter_types: || vec![ParamTypeHint::String, nullable_string()],
            return_type: int_or_false,
            defaults: || vec![None, Some(Value::null())],
        },
        Declaration {
            name: "iconv_substr",
            handler: fn_iconv_substr,
            required: 2,
            parameters: &["string", "offset", "length", "encoding"],
            parameter_types: || {
                vec![
                    ParamTypeHint::String,
                    ParamTypeHint::Int,
                    ParamTypeHint::Nullable(Box::new(ParamTypeHint::Int)),
                    nullable_string(),
                ]
            },
            return_type: string_or_false,
            defaults: || vec![None, None, Some(Value::null()), Some(Value::null())],
        },
        Declaration {
            name: "iconv_strpos",
            handler: fn_iconv_strpos,
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
            name: "iconv_strrpos",
            handler: fn_iconv_strrpos,
            required: 2,
            parameters: &["haystack", "needle", "encoding"],
            parameter_types: || {
                vec![
                    ParamTypeHint::String,
                    ParamTypeHint::String,
                    nullable_string(),
                ]
            },
            return_type: int_or_false,
            defaults: || vec![None, None, Some(Value::null())],
        },
        Declaration {
            name: "iconv_mime_encode",
            handler: fn_iconv_mime_encode,
            required: 2,
            parameters: &["field_name", "field_value", "options"],
            parameter_types: || {
                vec![
                    ParamTypeHint::String,
                    ParamTypeHint::String,
                    ParamTypeHint::Array,
                ]
            },
            return_type: string_or_false,
            defaults: || vec![None, None, Some(Value::array(PhpArray::new()))],
        },
        Declaration {
            name: "iconv_mime_decode",
            handler: fn_iconv_mime_decode,
            required: 1,
            parameters: &["string", "mode", "encoding"],
            parameter_types: || vec![ParamTypeHint::String, ParamTypeHint::Int, nullable_string()],
            return_type: string_or_false,
            defaults: || vec![None, Some(Value::long(0)), Some(Value::null())],
        },
        Declaration {
            name: "iconv_mime_decode_headers",
            handler: fn_iconv_mime_decode_headers,
            required: 1,
            parameters: &["headers", "mode", "encoding"],
            parameter_types: || vec![ParamTypeHint::String, ParamTypeHint::Int, nullable_string()],
            return_type: array_or_false,
            defaults: || vec![None, Some(Value::long(0)), Some(Value::null())],
        },
        Declaration {
            name: "iconv",
            handler: fn_iconv,
            required: 3,
            parameters: &["from_encoding", "to_encoding", "string"],
            parameter_types: || vec![ParamTypeHint::String; 3],
            return_type: string_or_false,
            defaults: || vec![None; 3],
        },
        Declaration {
            name: "iconv_set_encoding",
            handler: fn_iconv_set_encoding,
            required: 2,
            parameters: &["type", "encoding"],
            parameter_types: || vec![ParamTypeHint::String; 2],
            return_type: || ParamTypeHint::Bool,
            defaults: || vec![None; 2],
        },
        Declaration {
            name: "iconv_get_encoding",
            handler: fn_iconv_get_encoding,
            required: 0,
            parameters: &["type"],
            parameter_types: || vec![ParamTypeHint::String],
            return_type: array_string_or_false,
            defaults: || vec![Some(Value::string("all"))],
        },
    ];

    let mut functions = Vec::with_capacity(declarations.len());
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
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(declaration.name, pointer)
            .expect("iconv function registration is unique");
        eg.register_internal_function_reflection_metadata(
            pointer,
            (declaration.defaults)(),
            "iconv",
        );
        functions.push(function);
    }
    functions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q_words_encode_spaces_and_non_ascii_bytes() {
        assert_eq!(q_encode("Prüfung test".as_bytes()), b"Pr=C3=BCfung=20test");
    }

    #[test]
    fn mime_q_decoder_preserves_plain_text_and_converts_words() {
        assert_eq!(
            mime_decode_bytes(b"Subject: =?ISO-8859-1?Q?Pr=FCfung?=", 0, b"UTF-8"),
            Ok("Subject: Prüfung".as_bytes().to_vec())
        );
    }

    #[test]
    fn substring_bounds_match_php_out_of_range_rules() {
        assert_eq!(substring_bounds(3, -4, Some(1)), (0, 1));
        assert_eq!(substring_bounds(3, 2, Some(-2)), (2, 2));
        assert_eq!(substring_bounds(3, 4, None), (3, 3));
    }
}
