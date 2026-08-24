//! String encoding, URL, entity and adjacent string builtins.
//!
//! Registration and direct-dispatch ownership stay in their existing modules.
//! This module groups the handlers without changing signatures, byte-string
//! conversion rules or PHP-visible behavior.

use std::borrow::Cow;

use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::{
    bytes_to_php_string, direct_arg_opt, direct_arg_str, legacy_encoding::LegacyEncoding,
    owned_argument, percent_decode_bytes, php_string_to_bytes, push_percent_escape,
    report_internal_diagnostic, typed_internal_bool_argument, typed_internal_int_argument,
    typed_internal_string_argument, typed_internal_string_argument_expected,
};

// ============================================================================
// String encoding functions
// ============================================================================

const ENT_QUOTES_MASK: i64 = 3;
const ENT_SUBSTITUTE: i64 = 8;
const ENT_DOCUMENT_MASK: i64 = 48;
const ENT_XML1: i64 = 16;
const ENT_XHTML: i64 = 32;
const ENT_HTML5: i64 = 48;

fn valid_html_entity(src: &str, document: i64) -> Option<usize> {
    let bytes = src.as_bytes();
    if bytes.first() != Some(&b'&') {
        return None;
    }
    // HTML entity names and numeric codepoints are short. Bound the probe so
    // repeated bare ampersands remain linear instead of rescanning the tail.
    let probe_length = bytes.len().min(34);
    let semicolon = bytes[..probe_length]
        .iter()
        .position(|byte| *byte == b';')?;
    if semicolon < 2 {
        return None;
    }
    let body = &src[1..semicolon];
    let numeric = body
        .strip_prefix("#x")
        .or_else(|| body.strip_prefix("#X"))
        .is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        || body.strip_prefix('#').is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        });
    let named = match document {
        ENT_XML1 => matches!(body, "amp" | "lt" | "gt" | "quot" | "apos"),
        ENT_XHTML | ENT_HTML5 => matches!(
            body,
            "amp" | "lt" | "gt" | "quot" | "apos" | "copy" | "nbsp" | "reg"
        ),
        _ => matches!(body, "amp" | "lt" | "gt" | "quot" | "copy" | "nbsp" | "reg"),
    };
    (numeric || named).then_some(semicolon + 1)
}

fn encode_html_special_chars(src: &str, flags: i64, double_encode: bool) -> String {
    let document = flags & ENT_DOCUMENT_MASK;
    let quote_flags = flags & ENT_QUOTES_MASK;
    let mut out = String::with_capacity(src.len());
    if double_encode {
        for character in src.chars() {
            match character {
                '&' => out.push_str("&amp;"),
                '"' if quote_flags & 2 != 0 => out.push_str("&quot;"),
                '\'' if quote_flags & 1 != 0 => {
                    if matches!(document, ENT_XML1 | ENT_XHTML | ENT_HTML5) {
                        out.push_str("&apos;");
                    } else {
                        out.push_str("&#039;");
                    }
                }
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                _ => out.push(character),
            }
        }
        return out;
    }

    let mut position = 0;
    while position < src.len() {
        if src.as_bytes()[position] == b'&'
            && let Some(length) = valid_html_entity(&src[position..], document)
        {
            out.push_str(&src[position..position + length]);
            position += length;
            continue;
        }
        let character = src[position..]
            .chars()
            .next()
            .expect("position remains on a character boundary");
        match character {
            '&' => out.push_str("&amp;"),
            '"' if quote_flags & 2 != 0 => out.push_str("&quot;"),
            '\'' if quote_flags & 1 != 0 => {
                if matches!(document, ENT_XML1 | ENT_XHTML | ENT_HTML5) {
                    out.push_str("&apos;");
                } else {
                    out.push_str("&#039;");
                }
            }
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(character),
        }
        position += character.len_utf8();
    }
    out
}

fn encode_html_special_chars_default(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for character in src.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(character),
        }
    }
    out
}

fn html_encoding_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<Option<String>, VmError> {
    if arg_opt!(ed, 2).is_none() {
        return Ok(Some("UTF-8".to_string()));
    }
    let argument = owned_argument(ed, 2);
    if matches!(argument.dereferenced().value_type(), ValueType::Null) {
        return Ok(Some("UTF-8".to_string()));
    }
    typed_internal_string_argument_expected(ed, eg, function, 2, "encoding", "?string")
}

fn html_encode_arguments(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<Option<(String, i64, bool)>, VmError> {
    let Some(string) = typed_internal_string_argument(ed, eg, function, 0, "string")? else {
        return Ok(None);
    };
    let flags = if arg_opt!(ed, 1).is_some() {
        let Some(flags) = typed_internal_int_argument(ed, eg, function, 1, "flags")? else {
            return Ok(None);
        };
        flags
    } else {
        ENT_QUOTES_MASK | ENT_SUBSTITUTE
    };
    if html_encoding_argument(ed, eg, function)?.is_none() {
        return Ok(None);
    }
    let double_encode = if arg_opt!(ed, 3).is_some() {
        let Some(double_encode) =
            typed_internal_bool_argument(ed, eg, function, 3, "double_encode")?
        else {
            return Ok(None);
        };
        double_encode
    } else {
        true
    };
    Ok(Some((string, flags, double_encode)))
}

/// htmlspecialchars($string, $flags = ENT_QUOTES|ENT_SUBSTITUTE,
///     $encoding = null, $double_encode = true): string
pub(super) fn fn_htmlspecialchars(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if arg_opt!(ed, 1).is_none()
        && arg_opt!(ed, 2).is_none()
        && arg_opt!(ed, 3).is_none()
        && let Some(string) = arg!(ed, 0).as_str()
    {
        ret!(rv, Value::string(encode_html_special_chars_default(string)));
    }
    let Some((string, flags, double_encode)) = html_encode_arguments(ed, eg, "htmlspecialchars")?
    else {
        return Ok(());
    };
    ret!(
        rv,
        Value::string(encode_html_special_chars(&string, flags, double_encode))
    );
}

/// htmlspecialchars_decode($string): string
/// Decodes only one layer — `&amp;lt;` becomes `&lt;`, not `<`.
pub(super) fn fn_htmlspecialchars_decode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(decode_html_entities(s.as_ref(), false)));
}

fn decode_html_entities(src: &str, decode_numeric: bool) -> String {
    // Single-pass decode to avoid chaining issues (e.g. &amp;lt; → &lt; not <).
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let bytes = src.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if src[i..].starts_with("&amp;") {
                out.push('&');
                i += 5;
            } else if src[i..].starts_with("&quot;") {
                out.push('"');
                i += 6;
            } else if src[i..].starts_with("&#039;") {
                out.push('\'');
                i += 6;
            } else if src[i..].starts_with("&lt;") {
                out.push('<');
                i += 4;
            } else if src[i..].starts_with("&gt;") {
                out.push('>');
                i += 4;
            } else if src[i..].starts_with("&apos;") {
                out.push('\'');
                i += 6;
            } else if decode_numeric {
                let decoded = src[i + 1..].find(';').and_then(|relative_end| {
                    let entity = &src[i + 1..i + 1 + relative_end];
                    let codepoint = entity
                        .strip_prefix("#x")
                        .or_else(|| entity.strip_prefix("#X"))
                        .and_then(|digits| u32::from_str_radix(digits, 16).ok())
                        .or_else(|| {
                            entity
                                .strip_prefix('#')
                                .and_then(|digits| digits.parse::<u32>().ok())
                        });
                    codepoint
                        .and_then(char::from_u32)
                        .map(|character| (character, relative_end + 2))
                });
                if let Some((character, consumed)) = decoded {
                    out.push(character);
                    i += consumed;
                } else {
                    out.push('&');
                    i += 1;
                }
            } else {
                out.push('&');
                i += 1;
            }
        } else {
            let character = src[i..].chars().next().unwrap();
            out.push(character);
            i += character.len_utf8();
        }
    }
    out
}

#[derive(Clone, Copy)]
enum HtmlEntityOutputEncoding {
    Utf8,
    Legacy(LegacyEncoding),
}

fn html_numeric_reference_allowed(codepoint: u32, document: i64) -> bool {
    if codepoint > 0x10ffff || (0xd800..=0xdfff).contains(&codepoint) {
        return false;
    }
    match document {
        ENT_XML1 | ENT_XHTML => {
            matches!(codepoint, 0x09 | 0x0a | 0x0d)
                || (0x20..=0xd7ff).contains(&codepoint)
                || (0xe000..=0xfffd).contains(&codepoint)
                || (0x10000..=0x10ffff).contains(&codepoint)
        }
        ENT_HTML5 => {
            let noncharacter = (0xfdd0..=0xfdef).contains(&codepoint)
                || matches!(codepoint & 0xffff, 0xfffe | 0xffff);
            !noncharacter
                && (matches!(codepoint, 0x09 | 0x0a | 0x0c)
                    || (0x20..=0x7e).contains(&codepoint)
                    || (0xa0..=0x10ffff).contains(&codepoint))
        }
        _ => {
            matches!(codepoint, 0x09 | 0x0a | 0x0d)
                || (0x20..=0x7e).contains(&codepoint)
                || (0xa0..=0x10ffff).contains(&codepoint)
        }
    }
}

fn html_quote_reference_allowed(codepoint: u32, flags: i64) -> bool {
    match codepoint {
        0x22 => flags & 2 != 0,
        0x27 => flags & 1 != 0,
        _ => true,
    }
}

fn parse_numeric_html_reference(source: &[u8]) -> Option<(u32, usize)> {
    if source.first() != Some(&b'#') {
        return None;
    }
    let (radix, mut position) = if matches!(source.get(1), Some(b'x' | b'X')) {
        (16_u32, 2)
    } else {
        (10_u32, 1)
    };
    let digits_start = position;
    let mut codepoint = 0_u32;
    while let Some(byte) = source.get(position) {
        let digit = char::from(*byte).to_digit(radix);
        let Some(digit) = digit else {
            break;
        };
        codepoint = codepoint.checked_mul(radix)?.checked_add(digit)?;
        position += 1;
    }
    if position == digits_start || source.get(position) != Some(&b';') {
        return None;
    }
    Some((codepoint, position + 1))
}

fn named_html_reference(source: &[u8], document: i64) -> Option<(u32, usize)> {
    Some(match source {
        source if source.starts_with(b"amp;") => (0x26, 4),
        source if source.starts_with(b"lt;") => (0x3c, 3),
        source if source.starts_with(b"gt;") => (0x3e, 3),
        source if source.starts_with(b"quot;") => (0x22, 5),
        source
            if source.starts_with(b"apos;")
                && matches!(document, ENT_XML1 | ENT_XHTML | ENT_HTML5) =>
        {
            (0x27, 5)
        }
        source if source.starts_with(b"nbsp;") => (0xa0, 5),
        source if source.starts_with(b"copy;") => (0xa9, 5),
        source if source.starts_with(b"reg;") => (0xae, 4),
        _ => return None,
    })
}

fn encode_html_entity_codepoint(
    codepoint: u32,
    encoding: HtmlEntityOutputEncoding,
    out: &mut String,
) -> bool {
    match encoding {
        HtmlEntityOutputEncoding::Utf8 => {
            let Some(character) = char::from_u32(codepoint) else {
                return false;
            };
            let mut buffer = [0; 4];
            for byte in character.encode_utf8(&mut buffer).bytes() {
                out.push(char::from(byte));
            }
            true
        }
        HtmlEntityOutputEncoding::Legacy(encoding) => {
            let Some(byte) = encoding.encode(codepoint) else {
                return false;
            };
            out.push(char::from(byte));
            true
        }
    }
}

fn decode_html_entities_for_encoding(
    source: &[u8],
    flags: i64,
    encoding: HtmlEntityOutputEncoding,
) -> String {
    let document = flags & ENT_DOCUMENT_MASK;
    let mut out = String::with_capacity(source.len());
    let mut position = 0;
    while position < source.len() {
        if source[position] != b'&' {
            out.push(char::from(source[position]));
            position += 1;
            continue;
        }

        let tail = &source[position + 1..];
        let numeric = tail.first() == Some(&b'#');
        let parsed = if numeric {
            parse_numeric_html_reference(tail)
        } else {
            named_html_reference(tail, document)
        };
        if let Some((codepoint, consumed)) = parsed
            && html_quote_reference_allowed(codepoint, flags)
            && (!numeric || html_numeric_reference_allowed(codepoint, document))
            && encode_html_entity_codepoint(codepoint, encoding, &mut out)
        {
            position += consumed + 1;
            continue;
        }
        // An invalid prefix may contain another ampersand before the
        // semicolon (`&#x&amp;`). Preserve only this ampersand so the later
        // valid entity still gets its own decoding opportunity.
        out.push('&');
        position += 1;
    }
    out
}

fn html_entity_decoded_value(decoded: String) -> Value {
    Value::binary_string_from_storage(decoded)
}

pub(super) fn fn_html_entity_decode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if arg_opt!(ed, 1).is_none()
        && arg_opt!(ed, 2).is_none()
        && let Some(source) = arg!(ed, 0).php_string_bytes()
    {
        let decoded = decode_html_entities_for_encoding(
            &source,
            ENT_QUOTES_MASK | ENT_SUBSTITUTE,
            HtmlEntityOutputEncoding::Utf8,
        );
        ret!(rv, html_entity_decoded_value(decoded));
    }
    let original_bytes = arg!(ed, 0).php_string_bytes().map(Cow::into_owned);
    let Some(string) = typed_internal_string_argument(ed, eg, "html_entity_decode", 0, "string")?
    else {
        return Ok(());
    };
    let flags = if arg_opt!(ed, 1).is_some() {
        let Some(flags) = typed_internal_int_argument(ed, eg, "html_entity_decode", 1, "flags")?
        else {
            return Ok(());
        };
        flags
    } else {
        ENT_QUOTES_MASK | ENT_SUBSTITUTE
    };
    let Some(encoding_name) = html_encoding_argument(ed, eg, "html_entity_decode")? else {
        return Ok(());
    };
    let encoding = if encoding_name.eq_ignore_ascii_case("UTF-8") {
        HtmlEntityOutputEncoding::Utf8
    } else if let Some(encoding) = LegacyEncoding::parse(&encoding_name) {
        HtmlEntityOutputEncoding::Legacy(encoding)
    } else {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!(
                "html_entity_decode(): Charset \"{encoding_name}\" is not supported, assuming UTF-8"
            ),
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        HtmlEntityOutputEncoding::Utf8
    };
    let source = original_bytes.unwrap_or_else(|| string.into_bytes());
    let decoded = decode_html_entities_for_encoding(&source, flags, encoding);
    ret!(rv, html_entity_decoded_value(decoded));
}

pub(super) fn fn_filter_var(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const FILTER_VALIDATE_INT: i64 = 257;
    const FILTER_VALIDATE_BOOL: i64 = 258;
    const FILTER_VALIDATE_FLOAT: i64 = 259;
    const FILTER_VALIDATE_IP: i64 = 275;
    const FILTER_NULL_ON_FAILURE: i64 = 134_217_728;
    const FILTER_FLAG_IPV4: i64 = 1_048_576;
    const FILTER_FLAG_IPV6: i64 = 2_097_152;

    let value = arg!(ed, 0);
    let filter = arg_long!(ed, 1);
    let options = arg_opt!(ed, 2);
    let flags = options.map_or(0, |options| {
        options
            .as_array()
            .and_then(|array| array.get_str("flags"))
            .unwrap_or(options)
            .to_long_val()
    });
    let invalid = || {
        if flags & FILTER_NULL_ON_FAILURE != 0 {
            Value::null()
        } else {
            Value::bool(false)
        }
    };

    let result = match filter {
        FILTER_VALIDATE_INT => match value.value_type() {
            ValueType::Long => value.clone(),
            ValueType::String => value
                .as_str()
                .and_then(|source| source.parse::<i64>().ok())
                .map_or_else(invalid, Value::long),
            _ => invalid(),
        },
        FILTER_VALIDATE_BOOL => {
            let normalized = value.echo_to_string().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "on" | "yes" => Value::bool(true),
                "" | "0" | "false" | "off" | "no" => Value::bool(false),
                _ => invalid(),
            }
        }
        FILTER_VALIDATE_FLOAT => match value.value_type() {
            ValueType::Double => value.clone(),
            ValueType::Long => Value::double(value.as_long().unwrap_or_default() as f64),
            ValueType::String => value
                .as_str()
                .and_then(|source| source.parse::<f64>().ok())
                .map_or_else(invalid, Value::double),
            _ => invalid(),
        },
        FILTER_VALIDATE_IP => {
            let parsed = value
                .as_str()
                .and_then(|source| source.parse::<std::net::IpAddr>().ok());
            let valid = parsed.is_some_and(|address| {
                (flags & FILTER_FLAG_IPV4 == 0 || address.is_ipv4())
                    && (flags & FILTER_FLAG_IPV6 == 0 || address.is_ipv6())
            });
            if valid { value.clone() } else { invalid() }
        }
        _ => value.clone(),
    };
    ret!(rv, result);
}

pub(super) fn fn_preg_quote(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = arg_str!(ed, 0);
    let delimiter = arg_opt!(ed, 1)
        .and_then(Value::as_str)
        .and_then(|value| value.chars().next());
    let mut quoted = String::with_capacity(source.len());
    for character in source.chars() {
        if matches!(
            character,
            '.' | '\\'
                | '+'
                | '*'
                | '?'
                | '['
                | '^'
                | ']'
                | '$'
                | '('
                | ')'
                | '{'
                | '}'
                | '='
                | '!'
                | '<'
                | '>'
                | '|'
                | ':'
                | '-'
                | '#'
        ) || delimiter == Some(character)
        {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    ret!(rv, Value::string(quoted));
}

/// htmlentities() shares htmlspecialchars()'s ASCII/UTF-8 special-character
/// boundary here; the broader named-entity table is intentionally separate.
pub(super) fn fn_htmlentities(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if arg_opt!(ed, 1).is_none()
        && arg_opt!(ed, 2).is_none()
        && arg_opt!(ed, 3).is_none()
        && let Some(string) = arg!(ed, 0).as_str()
    {
        ret!(rv, Value::string(encode_html_special_chars_default(string)));
    }
    let Some((string, flags, double_encode)) = html_encode_arguments(ed, eg, "htmlentities")?
    else {
        return Ok(());
    };
    ret!(
        rv,
        Value::string(encode_html_special_chars(&string, flags, double_encode))
    );
}

/// urlencode($string): string
pub(super) fn fn_urlencode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let extra_bytes = s
        .bytes()
        .filter(
            |b| !matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b' '),
        )
        .count()
        * 2;
    let mut out = String::with_capacity(s.len() + extra_bytes);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(*b as char),
            b' ' => out.push('+'),
            _ => push_percent_escape(&mut out, *b),
        }
    }
    ret!(rv, Value::string(out));
}

/// urldecode($string): string
pub(super) fn fn_urldecode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(percent_decode_bytes(&s, true)));
}

/// rawurlencode($string): string — like urlencode but space → %20
pub(super) fn fn_rawurlencode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let extra_bytes = s
        .bytes()
        .filter(
            |b| !matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'),
        )
        .count()
        * 2;
    let mut out = String::with_capacity(s.len() + extra_bytes);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => push_percent_escape(&mut out, *b),
        }
    }
    ret!(rv, Value::string(out));
}

/// rawurldecode($string): string
pub(super) fn fn_rawurldecode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(percent_decode_bytes(&s, false)));
}

/// base64_encode($data): string
/// Uses Latin-1 byte mapping to handle binary PHP strings correctly.
pub(super) fn fn_base64_encode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    use crate::base64;
    let raw = php_string_to_bytes(s.as_ref());
    ret!(rv, Value::string(base64::encode(&raw)));
}

/// base64_decode($data): string|false
pub(super) fn fn_base64_decode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    use crate::base64;
    match base64::decode(s.as_ref()) {
        Some(bytes) => ret!(rv, Value::string(bytes_to_php_string(&bytes))),
        None => ret!(rv, Value::bool(false)),
    }
}

// ============================================================================
// Missing common string functions
// ============================================================================

/// stripos($haystack, $needle): int|false
pub(super) fn fn_stripos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let hay = arg_str!(ed, 0);
    let needle = arg_str!(ed, 1);
    let hay_lower = hay.to_lowercase();
    let needle_lower = needle.to_lowercase();
    match hay_lower.find(&needle_lower) {
        Some(pos) => {
            // Convert byte offset to char offset for consistency
            let char_pos = hay[..pos].chars().count();
            ret!(rv, Value::long(char_pos as i64));
        }
        None => ret!(rv, Value::bool(false)),
    }
}

/// strripos($haystack, $needle): int|false
pub(super) fn fn_strripos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let hay = arg_str!(ed, 0);
    let needle = arg_str!(ed, 1);
    let hay_lower = hay.to_lowercase();
    let needle_lower = needle.to_lowercase();
    match hay_lower.rfind(&needle_lower) {
        Some(pos) => ret!(rv, Value::long(hay[..pos].chars().count() as i64)),
        None => ret!(rv, Value::bool(false)),
    }
}

/// str_ireplace($search, $replace, $subject): string
pub(super) fn fn_str_ireplace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let search = arg_str!(ed, 0);
    let replace = arg_str!(ed, 1);
    let subject = arg_str!(ed, 2);
    if search.is_empty() {
        ret!(rv, Value::string(subject.into_owned()));
    }
    // Case-insensitive replace
    let search_lower = search.to_lowercase();
    let mut result = String::with_capacity(subject.len());
    let subject_lower = subject.to_lowercase();
    let mut start = 0;
    while let Some(pos) = subject_lower[start..].find(&search_lower) {
        result.push_str(&subject[start..start + pos]);
        result.push_str(replace.as_ref());
        start += pos + search.len();
    }
    result.push_str(&subject[start..]);
    ret!(rv, Value::string(result));
}

/// substr_replace($string, $replacement, $start, $length = null): string
pub(super) fn fn_substr_replace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let replacement = arg_str!(ed, 1);
    let start_raw = arg_long!(ed, 2);
    let len = s.len() as i64;
    let start = if start_raw < 0 {
        (len + start_raw).max(0) as usize
    } else {
        start_raw.min(len) as usize
    };
    let length = match arg_opt!(ed, 3) {
        Some(v) if !v.is_undef() => {
            let l = v.to_long_val();
            if l < 0 {
                ((len as i64 - start as i64) + l).max(0) as usize
            } else {
                l as usize
            }
        }
        _ => s.len() - start,
    };
    let end = (start + length).min(s.len());
    let mut result = String::with_capacity(start + replacement.len() + (s.len() - end));
    result.push_str(&s[..start]);
    result.push_str(replacement.as_ref());
    result.push_str(&s[end..]);
    ret!(rv, Value::string(result));
}

/// str_getcsv($string, $separator = ",", $enclosure = "\"", $escape = "\\"): array
pub(super) fn fn_str_getcsv(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let sep = arg_opt!(ed, 1)
        .map(|v| v.echo_to_string().chars().next().unwrap_or(','))
        .unwrap_or(',');
    let enc = arg_opt!(ed, 2)
        .map(|v| v.echo_to_string().chars().next().unwrap_or('"'))
        .unwrap_or('"');

    let mut arr = PhpArray::new();
    let mut field = String::new();
    let mut in_enclosure = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == enc {
            if in_enclosure {
                if chars.peek() == Some(&enc) {
                    field.push(enc);
                    chars.next();
                } else {
                    in_enclosure = false;
                }
            } else {
                in_enclosure = true;
            }
        } else if c == sep && !in_enclosure {
            arr.push(Value::string(std::mem::take(&mut field)));
        } else {
            field.push(c);
        }
    }
    arr.push(Value::string(field));
    ret!(rv, Value::array(arr));
}

/// chunk_split($string, $chunklen = 76, $end = "\r\n"): string
pub(super) fn direct_chunk_split(args: &[Value]) -> Result<Value, VmError> {
    let s = direct_arg_str(args, 0);
    let chunklen = direct_arg_opt(args, 1)
        .map(|v| v.to_long_val() as usize)
        .unwrap_or(76);
    let end = direct_arg_opt(args, 2)
        .map(|v| v.echo_to_string())
        .unwrap_or_else(|| "\r\n".to_string());
    if chunklen == 0 {
        return Err(VmError::Fatal(
            "chunk_split(): Argument #2 ($chunklen) must be greater than 0".into(),
        ));
    }
    let mut result = String::new();
    for chunk in s.as_bytes().chunks(chunklen) {
        result.push_str(&String::from_utf8_lossy(chunk));
        result.push_str(&end);
    }
    Ok(Value::string(result))
}

pub(super) fn fn_chunk_split(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // SAFETY: registry metadata reserves three CV slots for chunk_split;
    // the internal frame stays live for the complete handler invocation.
    let args = unsafe { std::slice::from_raw_parts((*ed).cv(0), 3) };
    let result = direct_chunk_split(args)?;
    ret!(rv, result);
}

#[cfg(test)]
mod tests {
    use super::{decode_html_entities, direct_chunk_split};
    use crate::value::Value;

    #[test]
    fn entity_decode_is_single_pass_and_numeric_mode_is_explicit() {
        let source = "&amp;lt;&lt;&#65;&#x41;";
        assert_eq!(decode_html_entities(source, false), "&lt;<&#65;&#x41;");
        assert_eq!(decode_html_entities(source, true), "&lt;<AA");
    }

    #[test]
    fn direct_chunk_split_preserves_chunk_and_ending_order() {
        let args = [Value::string("abcdef"), Value::long(2), Value::string("|")];
        let result = direct_chunk_split(&args).unwrap();
        assert_eq!(result.as_str(), Some("ab|cd|ef|"));
    }
}
