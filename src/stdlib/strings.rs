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
    StringSearchDirection, bytes_to_php_string,
    html_entities::{HTML4_ENTITIES, HTML5_ENTITIES, html5_characters_for_entity},
    internal_call_source,
    legacy_encoding::LegacyEncoding,
    owned_argument, percent_decode_php_bytes, php_byte_result, push_percent_escape,
    report_internal_deprecation, report_internal_diagnostic, string_position_builtin,
    typed_internal_bool_argument, typed_internal_int_argument,
    typed_internal_int_value_argument_expected, typed_internal_string_argument,
    typed_internal_string_argument_expected, typed_internal_string_value_argument_expected,
};

// ============================================================================
// String encoding functions
// ============================================================================

const ENT_QUOTES_MASK: i64 = 3;
const ENT_IGNORE: i64 = 4;
const ENT_SUBSTITUTE: i64 = 8;
const ENT_DISALLOWED: i64 = 128;
const ENT_DOCUMENT_MASK: i64 = 48;
const ENT_XML1: i64 = 16;
const ENT_XHTML: i64 = 32;
const ENT_HTML5: i64 = 48;

#[derive(Clone, Copy)]
enum HtmlTranslationEncoding {
    Utf8,
    Legacy(LegacyEncoding),
    BasicOnly(BasicMultibyteEncoding),
}

#[derive(Clone, Copy)]
enum BasicMultibyteEncoding {
    Sjis,
    EucJp,
    Big5,
}

impl HtmlTranslationEncoding {
    fn parse(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("UTF-8") {
            return Some(Self::Utf8);
        }
        let basic = match name.to_ascii_lowercase().as_str() {
            "sjis" | "shift_jis" | "cp932" => Some(BasicMultibyteEncoding::Sjis),
            "euc-jp" | "eucjp" | "eucjp-win" => Some(BasicMultibyteEncoding::EucJp),
            "big5" => Some(BasicMultibyteEncoding::Big5),
            _ => None,
        };
        if let Some(encoding) = basic {
            return Some(Self::BasicOnly(encoding));
        }
        LegacyEncoding::parse(name).map(Self::Legacy)
    }

    fn key(self, characters: &str) -> Option<String> {
        match self {
            Self::Utf8 => Some(characters.to_string()),
            Self::BasicOnly(_) => characters.is_ascii().then(|| characters.to_string()),
            Self::Legacy(encoding) => {
                let bytes = characters
                    .chars()
                    .enumerate()
                    .map(|(position, character)| {
                        let codepoint = u32::from(character);
                        encoding.encode(codepoint).or_else(|| {
                            // PHP 8.5 retains the three HTML5 composite
                            // inequality keys in ISO-8859-1. Their trailing
                            // variation mark is represented by its low byte.
                            (encoding == LegacyEncoding::Iso88591
                                && position > 0
                                && matches!(codepoint, 0x20d2 | 0x20e5))
                            .then_some(codepoint as u8)
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(bytes_to_php_string(&bytes))
            }
        }
    }

    fn has_external_byte_keys(self) -> bool {
        matches!(self, Self::Legacy(_))
    }

    fn is_basic_only(self) -> bool {
        matches!(self, Self::BasicOnly(_))
    }
}

fn resolve_html_translation_encoding(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    name: &str,
) -> Result<Option<HtmlTranslationEncoding>, VmError> {
    // PHP's charset lookup consumes a C-style name. An explicit empty name,
    // or bytes after the first NUL, therefore select the request default. The
    // admitted default remains UTF-8; configurable default_charset handling is
    // a separate INI contract.
    let name = name.split('\0').next().unwrap_or_default();
    if name.is_empty() {
        return Ok(Some(HtmlTranslationEncoding::Utf8));
    }
    if let Some(encoding) = HtmlTranslationEncoding::parse(name) {
        return Ok(Some(encoding));
    }
    report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!("{function}(): Charset \"{name}\" is not supported, assuming UTF-8"),
    )?;
    if eg.exception.is_some() {
        return Ok(None);
    }
    Ok(Some(HtmlTranslationEncoding::Utf8))
}

fn valid_html_entity(src: &[u8], flags: i64) -> Option<usize> {
    let bytes = src;
    if bytes.first() != Some(&b'&') {
        return None;
    }
    if bytes.get(1) == Some(&b'#') {
        let (codepoint, consumed) = parse_numeric_html_reference(&bytes[1..])?;
        return html_numeric_reference_valid_for_encoding(codepoint, flags).then_some(consumed + 1);
    }
    // Named HTML entities are short. Bound that probe so repeated bare
    // ampersands remain linear instead of rescanning the tail.
    let probe_length = bytes.len().min(34);
    let semicolon = bytes[..probe_length]
        .iter()
        .position(|byte| *byte == b';')?;
    if semicolon < 2 {
        return None;
    }
    let document = flags & ENT_DOCUMENT_MASK;
    let named = named_html_reference(&bytes[1..], document)
        .is_some_and(|(_, consumed)| consumed == semicolon);
    named.then_some(semicolon + 1)
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
            && let Some(length) = valid_html_entity(&src.as_bytes()[position..], flags)
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

fn encode_html_special_chars_disallowed_utf8(
    source: &str,
    flags: i64,
    double_encode: bool,
) -> String {
    let document = flags & ENT_DOCUMENT_MASK;
    let quote_flags = flags & ENT_QUOTES_MASK;
    let mut output = String::with_capacity(source.len());
    let mut position = 0;
    while position < source.len() {
        if !double_encode
            && source.as_bytes()[position] == b'&'
            && let Some(consumed) = valid_html_entity(&source.as_bytes()[position..], flags)
        {
            output.push_str(&source[position..position + consumed]);
            position += consumed;
            continue;
        }
        let character = source[position..]
            .chars()
            .next()
            .expect("position remains on a character boundary");
        position += character.len_utf8();
        if !html_literal_codepoint_allowed(u32::from(character), document) {
            output.push('\u{fffd}');
            continue;
        }
        match character {
            '&' => output.push_str("&amp;"),
            '"' if quote_flags & 2 != 0 => output.push_str("&quot;"),
            '\'' if quote_flags & 1 != 0 => {
                if matches!(document, ENT_XML1 | ENT_XHTML | ENT_HTML5) {
                    output.push_str("&apos;");
                } else {
                    output.push_str("&#039;");
                }
            }
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(character),
        }
    }
    output
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

/// Validate PHP byte strings with the Unicode maximal-subpart behavior used by
/// PHP 8.5's HTML encoder. Structurally complete overlong, surrogate and
/// out-of-range sequences form one invalid unit; an invalid lead or isolated
/// continuation forms its own unit. ENT_IGNORE takes precedence when both
/// recovery flags are supplied.
fn sanitize_html_utf8(source: &[u8], flags: i64) -> Option<Cow<'_, str>> {
    if let Ok(source) = std::str::from_utf8(source) {
        return Some(Cow::Borrowed(source));
    }
    let ignore = flags & ENT_IGNORE != 0;
    let substitute = !ignore && flags & ENT_SUBSTITUTE != 0;
    if !ignore && !substitute {
        return None;
    }

    let mut output = String::with_capacity(source.len());
    let mut position = 0;
    while position < source.len() {
        let byte = source[position];
        if byte < 0x80 {
            output.push(char::from(byte));
            position += 1;
            continue;
        }

        let expected = match byte {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 1,
        };
        let mut consumed = 1;
        while consumed < expected
            && source
                .get(position + consumed)
                .is_some_and(|byte| matches!(byte, 0x80..=0xbf))
        {
            consumed += 1;
        }
        let structurally_complete = consumed == expected;
        let scalar_boundary_valid = match (byte, source.get(position + 1).copied()) {
            (0xe0, Some(second)) => second >= 0xa0,
            (0xed, Some(second)) => second <= 0x9f,
            (0xf0, Some(second)) => second >= 0x90,
            (0xf4, Some(second)) => second <= 0x8f,
            _ => expected > 1,
        };
        if structurally_complete && scalar_boundary_valid {
            let valid = std::str::from_utf8(&source[position..position + consumed])
                .expect("validated UTF-8 unit");
            output.push_str(valid);
        } else if substitute {
            output.push('\u{fffd}');
        }
        position += consumed;
    }
    Some(Cow::Owned(output))
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
) -> Result<Option<(String, i64, String, bool)>, VmError> {
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
    let Some(encoding) = html_encoding_argument(ed, eg, function)? else {
        return Ok(None);
    };
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
    Ok(Some((string, flags, encoding, double_encode)))
}

fn html_translation_encoding_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<Option<String>, VmError> {
    if arg_opt!(ed, 2).is_none() {
        return Ok(Some("UTF-8".to_string()));
    }
    typed_internal_string_argument_expected(
        ed,
        eg,
        "get_html_translation_table",
        2,
        "encoding",
        "string",
    )
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
        && !arg!(ed, 0).is_binary_string()
        && let Some(string) = arg!(ed, 0).as_str()
    {
        ret!(rv, Value::string(encode_html_special_chars_default(string)));
    }
    let Some((string, flags, encoding_name, double_encode)) =
        html_encode_arguments(ed, eg, "htmlspecialchars")?
    else {
        return Ok(());
    };
    let binary_input = arg!(ed, 0).is_binary_string();
    let binary_source = binary_input.then(|| {
        arg!(ed, 0)
            .php_string_bytes()
            .map(Cow::into_owned)
            .unwrap_or_else(|| string.as_bytes().to_vec())
    });
    let Some(encoding) =
        resolve_html_translation_encoding(ed, eg, "htmlspecialchars", &encoding_name)?
    else {
        return Ok(());
    };
    match encoding {
        HtmlTranslationEncoding::Utf8 => {
            let source = if let Some(source) = binary_source.as_deref() {
                let Some(source) = sanitize_html_utf8(&source, flags).map(Cow::into_owned) else {
                    ret!(rv, Value::string(""));
                };
                Cow::Owned(source)
            } else {
                Cow::Borrowed(string.as_str())
            };
            let encoded = if flags & ENT_DISALLOWED != 0 {
                encode_html_special_chars_disallowed_utf8(&source, flags, double_encode)
            } else {
                encode_html_special_chars(&source, flags, double_encode)
            };
            if binary_input {
                ret!(rv, Value::binary_string(encoded.as_bytes()));
            }
            ret!(rv, Value::string(encoded));
        }
        HtmlTranslationEncoding::Legacy(encoding) => {
            let source = binary_source
                .as_deref()
                .map(Cow::Borrowed)
                .unwrap_or_else(|| Cow::Borrowed(string.as_bytes()));
            let encoded =
                encode_html_entities_legacy(&source, flags, double_encode, encoding, false)
                    .unwrap_or_default();
            ret!(rv, Value::binary_string_from_storage(encoded));
        }
        HtmlTranslationEncoding::BasicOnly(encoding) => {
            let source = binary_source
                .as_deref()
                .map(Cow::Borrowed)
                .unwrap_or_else(|| Cow::Borrowed(string.as_bytes()));
            let encoded =
                encode_html_entities_basic_multibyte(&source, flags, double_encode, encoding)
                    .unwrap_or_default();
            ret!(rv, Value::binary_string_from_storage(encoded));
        }
    }
}

/// htmlspecialchars_decode($string, $flags = ENT_QUOTES | ENT_SUBSTITUTE): string
/// Decodes only one layer — `&amp;lt;` becomes `&lt;`, not `<`.
pub(super) fn fn_htmlspecialchars_decode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if arg_opt!(ed, 1).is_none() {
        let argument = arg!(ed, 0);
        if let Some(source) = argument.as_str()
            && !argument.is_binary_string()
        {
            ret!(
                rv,
                Value::string(decode_html_special_references_default_text(source))
            );
        }
        if let Some(source) = argument.php_string_bytes() {
            ret!(
                rv,
                Value::binary_string_from_storage(decode_html_special_references_default(&source))
            );
        }
    }
    let original_bytes = arg!(ed, 0).php_string_bytes().map(Cow::into_owned);
    let Some(string) =
        typed_internal_string_argument(ed, eg, "htmlspecialchars_decode", 0, "string")?
    else {
        return Ok(());
    };
    let flags = if arg_opt!(ed, 1).is_some() {
        let Some(flags) =
            typed_internal_int_argument(ed, eg, "htmlspecialchars_decode", 1, "flags")?
        else {
            return Ok(());
        };
        flags
    } else {
        ENT_QUOTES_MASK | ENT_SUBSTITUTE
    };
    let source = original_bytes.unwrap_or_else(|| string.into_bytes());
    ret!(
        rv,
        Value::binary_string_from_storage(decode_html_special_references(&source, flags))
    );
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

fn html_numeric_reference_valid_for_encoding(codepoint: u32, flags: i64) -> bool {
    if codepoint > 0x10ffff {
        return false;
    }
    if flags & ENT_DISALLOWED == 0 {
        return true;
    }
    match flags & ENT_DOCUMENT_MASK {
        ENT_XML1 | ENT_XHTML => {
            html_numeric_reference_allowed(codepoint, flags & ENT_DOCUMENT_MASK)
        }
        // PHP 8.5 preserves syntactically bounded surrogate references in
        // HTML5 even though the corresponding literal byte sequence is not
        // valid UTF-8.
        ENT_HTML5 if (0xd800..=0xdfff).contains(&codepoint) => true,
        ENT_HTML5 => html_numeric_reference_allowed(codepoint, ENT_HTML5),
        _ => true,
    }
}

fn html_literal_codepoint_allowed(codepoint: u32, document: i64) -> bool {
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
                && (matches!(codepoint, 0x09 | 0x0a | 0x0c | 0x0d)
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

fn html_basic_legacy_codepoint_allowed(codepoint: u32, document: i64) -> bool {
    if codepoint >= 0x20 {
        return true;
    }
    matches!(codepoint, 0x09 | 0x0a | 0x0d) || (document == ENT_HTML5 && codepoint == 0x0c)
}

fn html_quote_reference_allowed(codepoint: u32, flags: i64) -> bool {
    match codepoint {
        0x22 => flags & 2 != 0,
        0x27 => flags & 1 != 0,
        _ => true,
    }
}

fn html_quote_characters_allowed(characters: &str, flags: i64) -> bool {
    match characters {
        "\"" => flags & 2 != 0,
        "'" => flags & 1 != 0,
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
        // Keep scanning after overflow so an arbitrarily long leading-zero
        // reference can retain its small value. Values above the Unicode
        // ceiling collapse to one invalid sentinel.
        codepoint = codepoint
            .saturating_mul(radix)
            .saturating_add(digit)
            .min(0x11_0000);
        position += 1;
    }
    if position == digits_start || source.get(position) != Some(&b';') {
        return None;
    }
    Some((codepoint, position + 1))
}

fn decode_html_special_references_default_text(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut position = 0;
    while position < source.len() {
        if source.as_bytes()[position] == b'&' {
            let tail = &source[position..];
            if tail.starts_with("&amp;") {
                out.push('&');
                position += 5;
                continue;
            } else if tail.starts_with("&quot;") {
                out.push('"');
                position += 6;
                continue;
            } else if tail.starts_with("&#039;") {
                out.push('\'');
                position += 6;
                continue;
            } else if tail.starts_with("&lt;") {
                out.push('<');
                position += 4;
                continue;
            } else if tail.starts_with("&gt;") {
                out.push('>');
                position += 4;
                continue;
            } else if tail.as_bytes().get(1) == Some(&b'#')
                && let Some((codepoint, consumed)) =
                    parse_numeric_html_reference(&tail.as_bytes()[1..])
                && matches!(codepoint, 0x22 | 0x26 | 0x27 | 0x3c | 0x3e)
            {
                out.push(char::from(codepoint as u8));
                position += consumed + 1;
                continue;
            }
        }
        let character = source[position..]
            .chars()
            .next()
            .expect("position remains on a character boundary");
        out.push(character);
        position += character.len_utf8();
    }
    out
}

fn decode_html_special_references_default(source: &[u8]) -> String {
    let mut out = String::with_capacity(source.len());
    let mut position = 0;
    while position < source.len() {
        if source[position] == b'&' {
            let tail = &source[position..];
            if tail.starts_with(b"&amp;") {
                out.push('&');
                position += 5;
                continue;
            } else if tail.starts_with(b"&quot;") {
                out.push('"');
                position += 6;
                continue;
            } else if tail.starts_with(b"&#039;") {
                out.push('\'');
                position += 6;
                continue;
            } else if tail.starts_with(b"&lt;") {
                out.push('<');
                position += 4;
                continue;
            } else if tail.starts_with(b"&gt;") {
                out.push('>');
                position += 4;
                continue;
            } else if tail.get(1) == Some(&b'#')
                && let Some((codepoint, consumed)) = parse_numeric_html_reference(&tail[1..])
                && matches!(codepoint, 0x22 | 0x26 | 0x27 | 0x3c | 0x3e)
            {
                out.push(char::from(codepoint as u8));
                position += consumed + 1;
                continue;
            }
        }
        out.push(char::from(source[position]));
        position += 1;
    }
    out
}

fn decode_html_special_references(source: &[u8], flags: i64) -> String {
    let document = flags & ENT_DOCUMENT_MASK;
    let mut out = String::with_capacity(source.len());
    let mut position = 0;
    while position < source.len() {
        let Some(remaining) = source.get(position..) else {
            break;
        };
        let Some(first) = remaining.first() else {
            break;
        };
        if *first != b'&' {
            out.push(char::from(*first));
            position += 1;
            continue;
        }

        let tail = remaining.get(1..).unwrap_or_default();
        let parsed = if tail.first() == Some(&b'#') {
            parse_numeric_html_reference(tail)
                .filter(|(codepoint, _)| matches!(*codepoint, 0x22 | 0x26 | 0x27 | 0x3c | 0x3e))
        } else {
            Some(match tail {
                tail if tail.starts_with(b"amp;") => (0x26, 4),
                tail if tail.starts_with(b"lt;") => (0x3c, 3),
                tail if tail.starts_with(b"gt;") => (0x3e, 3),
                tail if tail.starts_with(b"quot;") => (0x22, 5),
                tail if tail.starts_with(b"apos;")
                    && matches!(document, ENT_XML1 | ENT_XHTML | ENT_HTML5) =>
                {
                    (0x27, 5)
                }
                _ => {
                    out.push('&');
                    position += 1;
                    continue;
                }
            })
        };
        if let Some((codepoint, consumed)) = parsed
            && html_quote_reference_allowed(codepoint, flags)
        {
            out.push(char::from_u32(codepoint).expect("special HTML code points are ASCII"));
            position += consumed + 1;
            continue;
        }
        out.push('&');
        position += 1;
    }
    out
}

fn html_entity_for_characters(
    entries: &'static [(&'static str, &'static str)],
    characters: &str,
) -> Option<&'static str> {
    entries
        .binary_search_by(|(candidate, _)| candidate.cmp(&characters))
        .ok()
        .map(|position| entries[position].1)
}

fn html4_characters_for_entity(name: &[u8]) -> Option<&'static str> {
    HTML4_ENTITIES.iter().find_map(|(characters, entity)| {
        entity
            .as_bytes()
            .strip_prefix(b"&")
            .filter(|candidate| *candidate == name)
            .map(|_| *characters)
    })
}

fn named_html_reference(source: &[u8], document: i64) -> Option<(&'static str, usize)> {
    let common = match source {
        source if source.starts_with(b"amp;") => Some(("&", 4)),
        source if source.starts_with(b"lt;") => Some(("<", 3)),
        source if source.starts_with(b"gt;") => Some((">", 3)),
        source if source.starts_with(b"quot;") => Some(("\"", 5)),
        source
            if source.starts_with(b"apos;")
                && matches!(document, ENT_XML1 | ENT_XHTML | ENT_HTML5) =>
        {
            Some(("'", 5))
        }
        source if source.starts_with(b"nbsp;") && document != ENT_XML1 => Some(("\u{a0}", 5)),
        source if source.starts_with(b"copy;") && document != ENT_XML1 => Some(("\u{a9}", 5)),
        source if source.starts_with(b"reg;") && document != ENT_XML1 => Some(("\u{ae}", 4)),
        _ => None,
    };
    if common.is_some() {
        return common;
    }
    let semicolon = source.iter().take(34).position(|byte| *byte == b';')?;
    let consumed = semicolon + 1;
    let name = &source[..consumed];
    let characters = match document {
        ENT_XML1 => match name {
            b"amp;" => "&",
            b"lt;" => "<",
            b"gt;" => ">",
            b"quot;" => "\"",
            b"apos;" => "'",
            _ => return None,
        },
        ENT_HTML5 => html5_characters_for_entity(std::str::from_utf8(name).ok()?)?,
        ENT_XHTML if name == b"apos;" => "'",
        ENT_XHTML | 0 => html4_characters_for_entity(name)?,
        _ => html4_characters_for_entity(name)?,
    };
    Some((characters, consumed))
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

fn encode_html_entity_characters(
    characters: &str,
    encoding: HtmlEntityOutputEncoding,
    out: &mut String,
) -> bool {
    if matches!(encoding, HtmlEntityOutputEncoding::Utf8) {
        for character in characters.chars() {
            let encoded = encode_html_entity_codepoint(u32::from(character), encoding, out);
            debug_assert!(encoded, "valid Unicode always has a UTF-8 representation");
        }
        return true;
    }
    let mut encoded = String::with_capacity(characters.len());
    for character in characters.chars() {
        if !encode_html_entity_codepoint(u32::from(character), encoding, &mut encoded) {
            return false;
        }
    }
    out.push_str(&encoded);
    true
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
        if tail.first() == Some(&b'#') {
            if let Some((codepoint, consumed)) = parse_numeric_html_reference(tail)
                && html_quote_reference_allowed(codepoint, flags)
                && html_numeric_reference_allowed(codepoint, document)
                && encode_html_entity_codepoint(codepoint, encoding, &mut out)
            {
                position += consumed + 1;
                continue;
            }
        } else if let Some((characters, consumed)) = named_html_reference(tail, document)
            && html_quote_characters_allowed(characters, flags)
            && encode_html_entity_characters(characters, encoding, &mut out)
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

fn decode_html_entities_default_utf8(source: &[u8]) -> String {
    let mut out = String::with_capacity(source.len());
    let mut position = 0;
    while position < source.len() {
        let tail = &source[position..];
        let common = if tail.starts_with(b"&amp;") {
            Some(('&', 5))
        } else if tail.starts_with(b"&quot;") {
            Some(('"', 6))
        } else if tail.starts_with(b"&#039;") {
            Some(('\'', 6))
        } else if tail.starts_with(b"&lt;") {
            Some(('<', 4))
        } else if tail.starts_with(b"&gt;") {
            Some(('>', 4))
        } else {
            None
        };
        if let Some((character, consumed)) = common {
            out.push(character);
            position += consumed;
        } else if tail.first() == Some(&b'&') {
            out.push_str(&decode_html_entities_for_encoding(
                tail,
                ENT_QUOTES_MASK | ENT_SUBSTITUTE,
                HtmlEntityOutputEncoding::Utf8,
            ));
            break;
        } else if let Some(byte) = tail.first() {
            out.push(char::from(*byte));
            position += 1;
        } else {
            break;
        }
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
        let decoded = decode_html_entities_default_utf8(&source);
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

fn html_translation_entries(table: i64, document: i64) -> &'static [(&'static str, &'static str)] {
    if table == 0 || document == ENT_XML1 {
        &HTML4_ENTITIES[..5]
    } else if document == ENT_HTML5 {
        HTML5_ENTITIES
    } else {
        HTML4_ENTITIES
    }
}

fn encode_named_html_entities_utf8(source: &str, flags: i64, double_encode: bool) -> String {
    let document = flags & ENT_DOCUMENT_MASK;
    let entries = html_translation_entries(1, document);
    let mut output = String::with_capacity(source.len());
    let mut position = 0;
    while position < source.len() {
        if !double_encode
            && source.as_bytes()[position] == b'&'
            && let Some(consumed) = valid_html_entity(&source.as_bytes()[position..], flags)
        {
            output.push_str(&source[position..position + consumed]);
            position += consumed;
            continue;
        }

        let character = source[position..]
            .chars()
            .next()
            .expect("position remains on a character boundary");
        let first_end = position + character.len_utf8();
        if flags & ENT_DISALLOWED != 0
            && !html_literal_codepoint_allowed(u32::from(character), document)
        {
            output.push('\u{fffd}');
            position = first_end;
            continue;
        }
        let mut matched_end = first_end;
        let mut entity = None;
        if document == ENT_HTML5
            && first_end < source.len()
            && let Some(next) = source[first_end..].chars().next()
        {
            let second_end = first_end + next.len_utf8();
            if let Some(candidate) =
                html_entity_for_characters(entries, &source[position..second_end])
            {
                matched_end = second_end;
                entity = Some(candidate);
            }
        }
        if entity.is_none() {
            entity = html_entity_for_characters(entries, &source[position..first_end]);
        }
        if let Some(mut entity) = entity
            && html_quote_characters_allowed(&source[position..matched_end], flags)
        {
            if document == ENT_XML1 && &source[position..matched_end] == "'" {
                entity = "&apos;";
            }
            output.push_str(entity);
            position = matched_end;
        } else {
            output.push(character);
            position = first_end;
        }
    }
    output
}

fn push_storage_bytes(output: &mut String, bytes: &[u8]) {
    output.extend(bytes.iter().copied().map(char::from));
}

fn push_basic_html_character(output: &mut String, byte: u8, flags: i64) {
    let document = flags & ENT_DOCUMENT_MASK;
    let quote_flags = flags & ENT_QUOTES_MASK;
    match byte {
        b'&' => output.push_str("&amp;"),
        b'"' if quote_flags & 2 != 0 => output.push_str("&quot;"),
        b'\'' if quote_flags & 1 != 0 => {
            if matches!(document, ENT_XML1 | ENT_XHTML | ENT_HTML5) {
                output.push_str("&apos;");
            } else {
                output.push_str("&#039;");
            }
        }
        b'<' => output.push_str("&lt;"),
        b'>' => output.push_str("&gt;"),
        _ => output.push(char::from(byte)),
    }
}

fn encode_html_entities_legacy(
    source: &[u8],
    flags: i64,
    double_encode: bool,
    encoding: LegacyEncoding,
    all_entities: bool,
) -> Option<String> {
    let document = flags & ENT_DOCUMENT_MASK;
    let entries = html_translation_entries(i64::from(all_entities), document);
    let mut output = String::with_capacity(source.len());
    let mut position = 0;
    while position < source.len() {
        if !double_encode
            && source[position] == b'&'
            && let Some(consumed) = valid_html_entity(&source[position..], flags)
        {
            push_storage_bytes(&mut output, &source[position..position + consumed]);
            position += consumed;
            continue;
        }

        let byte = source[position];
        let codepoint = encoding.decode(byte)?;
        if flags & ENT_DISALLOWED != 0
            && !(if all_entities {
                html_literal_codepoint_allowed(codepoint, document)
            } else {
                html_basic_legacy_codepoint_allowed(codepoint, document)
            })
        {
            output.push_str("&#xFFFD;");
            position += 1;
            continue;
        }

        if all_entities {
            let character = char::from_u32(codepoint)?;
            let mut buffer = [0; 4];
            let characters = character.encode_utf8(&mut buffer);
            if let Some(mut entity) = html_entity_for_characters(entries, characters)
                && html_quote_characters_allowed(characters, flags)
            {
                if document == ENT_XML1 && characters == "'" {
                    entity = "&apos;";
                }
                output.push_str(entity);
                position += 1;
                continue;
            }
        }
        push_basic_html_character(&mut output, byte, flags);
        position += 1;
    }
    Some(output)
}

fn encode_html_entities_basic_multibyte(
    source: &[u8],
    flags: i64,
    double_encode: bool,
    encoding: BasicMultibyteEncoding,
) -> Option<String> {
    let document = flags & ENT_DOCUMENT_MASK;
    let mut output = String::with_capacity(source.len());
    let mut position = 0;
    while position < source.len() {
        if !double_encode
            && source[position] == b'&'
            && let Some(consumed) = valid_html_entity(&source[position..], flags)
        {
            push_storage_bytes(&mut output, &source[position..position + consumed]);
            position += consumed;
            continue;
        }

        let byte = source[position];
        if byte < 0x80 {
            if flags & ENT_DISALLOWED != 0
                && !html_basic_legacy_codepoint_allowed(u32::from(byte), document)
            {
                output.push_str("&#xFFFD;");
            } else {
                push_basic_html_character(&mut output, byte, flags);
            }
            position += 1;
            continue;
        }
        let (valid, consumed) = basic_multibyte_unit(source, position, encoding);
        if valid {
            push_storage_bytes(&mut output, &source[position..position + consumed]);
            position += consumed;
            continue;
        }
        if !matches!(encoding, BasicMultibyteEncoding::Sjis) {
            if flags & ENT_IGNORE != 0 {
                position += consumed;
                continue;
            }
            if flags & ENT_SUBSTITUTE != 0 {
                output.push_str("&#xFFFD;");
                position += consumed;
                continue;
            }
        }
        return None;
    }
    Some(output)
}

fn basic_multibyte_unit(
    source: &[u8],
    position: usize,
    encoding: BasicMultibyteEncoding,
) -> (bool, usize) {
    let byte = source[position];
    match encoding {
        BasicMultibyteEncoding::Sjis => {
            if byte < 0x80 || (0xa1..=0xdf).contains(&byte) {
                return (true, 1);
            }
            if matches!(byte, 0x81..=0x9f | 0xe0..=0xfc)
                && source
                    .get(position + 1)
                    .is_some_and(|trail| matches!(trail, 0x40..=0x7e | 0x80..=0xfc))
            {
                return (true, 2);
            }
            (false, 1)
        }
        BasicMultibyteEncoding::EucJp => {
            if byte < 0x80 || matches!(byte, 0x80..=0x8d | 0x90..=0x9f) {
                return (true, 1);
            }
            let invalid_trail_width = |trail: Option<&u8>| {
                1 + usize::from(trail.is_some_and(|trail| matches!(trail, 0xa0 | 0xff)))
            };
            if byte == 0x8e {
                return match source.get(position + 1) {
                    Some(0xa1..=0xdf) => (true, 2),
                    trail => (false, invalid_trail_width(trail)),
                };
            }
            if byte == 0x8f {
                return match (source.get(position + 1), source.get(position + 2)) {
                    (Some(0xa1..=0xfe), Some(0xa1..=0xfe)) => (true, 3),
                    (Some(0xa1..=0xfe), _) => (false, 1),
                    (trail, _) => (false, invalid_trail_width(trail)),
                };
            }
            if (0xa1..=0xfe).contains(&byte) {
                return match source.get(position + 1) {
                    Some(0xa1..=0xfe) => (true, 2),
                    trail => (false, invalid_trail_width(trail)),
                };
            }
            (false, 1)
        }
        BasicMultibyteEncoding::Big5 => {
            if !(0x81..=0xfe).contains(&byte) {
                return (true, 1);
            }
            if source
                .get(position + 1)
                .is_some_and(|trail| matches!(trail, 0x40..=0x7e | 0xa1..=0xfe))
            {
                return (true, 2);
            }
            (false, 1)
        }
    }
}

/// get_html_translation_table($table = HTML_SPECIALCHARS,
///     $flags = ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401,
///     $encoding = null): array
pub(super) fn fn_get_html_translation_table(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let table = if arg_opt!(ed, 0).is_some() {
        let Some(table) =
            typed_internal_int_argument(ed, eg, "get_html_translation_table", 0, "table")?
        else {
            return Ok(());
        };
        table
    } else {
        0
    };
    let flags = if arg_opt!(ed, 1).is_some() {
        let Some(flags) =
            typed_internal_int_argument(ed, eg, "get_html_translation_table", 1, "flags")?
        else {
            return Ok(());
        };
        flags
    } else {
        ENT_QUOTES_MASK | ENT_SUBSTITUTE
    };
    let Some(encoding_name) = html_translation_encoding_argument(ed, eg)? else {
        return Ok(());
    };
    let Some(encoding) =
        resolve_html_translation_encoding(ed, eg, "get_html_translation_table", &encoding_name)?
    else {
        return Ok(());
    };

    let document = flags & ENT_DOCUMENT_MASK;
    let entries = if encoding.is_basic_only() {
        &HTML4_ENTITIES[..5]
    } else {
        html_translation_entries(table, document)
    };
    let mut result = PhpArray::with_deferred_hash_capacity(entries.len());
    for &(characters, mut entity) in entries {
        if characters == "\"" && flags & 2 == 0 {
            continue;
        }
        if characters == "'" {
            if flags & 1 == 0 {
                continue;
            }
            entity = if matches!(document, ENT_XML1 | ENT_HTML5)
                || (table == 0 && document == ENT_XHTML)
            {
                "&apos;"
            } else {
                "&#039;"
            };
        }
        let Some(key) = encoding.key(characters) else {
            continue;
        };
        result.set_streamed_owned_str(key, Value::string(entity));
    }
    if encoding.has_external_byte_keys() {
        result.mark_external_byte_keys();
    } else if matches!(encoding, HtmlTranslationEncoding::Utf8) {
        result.mark_utf8_text_keys();
    }
    ret!(rv, Value::array(result));
}

pub(super) fn fn_filter_var(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const FILTER_VALIDATE_INT: i64 = 257;
    const FILTER_VALIDATE_BOOL: i64 = 258;
    const FILTER_VALIDATE_FLOAT: i64 = 259;
    const FILTER_VALIDATE_IP: i64 = 275;
    const FILTER_FLAG_ALLOW_OCTAL: i64 = 1;
    const FILTER_FLAG_ALLOW_HEX: i64 = 2;
    const FILTER_NULL_ON_FAILURE: i64 = 134_217_728;
    const FILTER_FLAG_IPV4: i64 = 1_048_576;
    const FILTER_FLAG_IPV6: i64 = 2_097_152;

    const FILTER_DEFAULT: i64 = 516;

    let value = arg!(ed, 0);
    let filter = if arg_opt!(ed, 1).is_some() {
        let Some(filter) = typed_internal_int_argument(ed, eg, "filter_var", 1, "filter")? else {
            return Ok(());
        };
        filter
    } else {
        FILTER_DEFAULT
    };
    let options = arg!(ed, 2);
    let (flags, validator_options) = if options.value_type() == ValueType::Undef {
        (0, None)
    } else if let Some(options) = options.as_array() {
        (
            options
                .get_str("flags")
                .map(Value::to_long_val)
                .unwrap_or(0),
            options.get_str("options").and_then(Value::as_array),
        )
    } else {
        let Some(flags) = typed_internal_int_value_argument_expected(
            ed,
            eg,
            options,
            "filter_var",
            2,
            "options",
            "array|int",
        )?
        else {
            return Ok(());
        };
        (flags, None)
    };
    if filter == FILTER_VALIDATE_INT
        && value.value_type() == ValueType::Long
        && flags == 0
        && validator_options.is_none()
    {
        ret!(rv, value.clone());
    }
    let default = validator_options
        .and_then(|options| options.get_str("default"))
        .cloned();
    let invalid = || {
        default.clone().unwrap_or_else(|| {
            if flags & FILTER_NULL_ON_FAILURE != 0 {
                Value::null()
            } else {
                Value::bool(false)
            }
        })
    };
    let parse_integer = |source: &str| {
        let source = source.trim();
        if flags & FILTER_FLAG_ALLOW_HEX != 0
            && source.len() > 2
            && (source.starts_with("0x") || source.starts_with("0X"))
        {
            return i64::from_str_radix(&source[2..], 16).ok();
        }
        if flags & FILTER_FLAG_ALLOW_OCTAL != 0 && source.len() > 1 && source.starts_with('0') {
            return i64::from_str_radix(&source[1..], 8).ok();
        }
        let digits = source.strip_prefix(['+', '-']).unwrap_or(source);
        (!digits.is_empty()
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && (digits.len() == 1 || !digits.starts_with('0')))
        .then(|| source.parse::<i64>().ok())
        .flatten()
    };

    let result = match filter {
        FILTER_DEFAULT => match value.value_type() {
            ValueType::String => value.clone(),
            ValueType::Long | ValueType::Double | ValueType::True | ValueType::False => {
                Value::string(value.echo_to_string_with_precision(eg.precision))
            }
            ValueType::Null => Value::string(String::new()),
            _ => invalid(),
        },
        FILTER_VALIDATE_INT => {
            let parsed = match value.value_type() {
                ValueType::Long => value.as_long(),
                ValueType::True => Some(1),
                ValueType::Double => {
                    parse_integer(&value.echo_to_string_with_precision(eg.precision))
                }
                ValueType::String => value.as_str().and_then(parse_integer),
                _ => None,
            };
            let in_range = parsed.is_some_and(|parsed| {
                let minimum = validator_options
                    .and_then(|options| options.get_str("min_range"))
                    .map(Value::to_long_val);
                let maximum = validator_options
                    .and_then(|options| options.get_str("max_range"))
                    .map(Value::to_long_val);
                minimum.is_none_or(|minimum| parsed >= minimum)
                    && maximum.is_none_or(|maximum| parsed <= maximum)
            });
            if in_range {
                Value::long(parsed.expect("validated integer filter result"))
            } else {
                invalid()
            }
        }
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
        _ => {
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("filter_var(): Unknown filter with ID {filter}"),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            Value::bool(false)
        }
    };
    ret!(rv, result);
}

pub(super) fn fn_preg_quote(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source_argument = arg!(ed, 0);
    let mut source = if source_argument.value_type() == ValueType::String {
        Cow::Borrowed(source_argument)
    } else {
        let Some(converted) = typed_internal_string_value_argument_expected(
            ed,
            eg,
            "preg_quote",
            0,
            "str",
            "string",
        )?
        else {
            return Ok(());
        };
        Cow::Owned(converted)
    };
    let delimiter = match arg_opt!(ed, 1) {
        None => None,
        Some(value) if value.dereferenced().value_type() == ValueType::Null => None,
        Some(value) => {
            if value.value_type() == ValueType::String {
                value
                    .php_string_bytes()
                    .and_then(|bytes| bytes.first().copied())
            } else {
                // Weak delimiter conversion may call __toString() or a user
                // error handler. Snapshot an exact borrowed source before
                // crossing that reentrant boundary.
                source.to_mut();
                let Some(delimiter) = typed_internal_string_value_argument_expected(
                    ed,
                    eg,
                    "preg_quote",
                    1,
                    "delimiter",
                    "?string",
                )?
                else {
                    return Ok(());
                };
                delimiter
                    .php_string_bytes()
                    .and_then(|bytes| bytes.first().copied())
            }
        }
    };
    let binary = source.is_binary_string();
    let source_bytes = source.php_string_bytes().unwrap_or_default();
    let extra = source_bytes.iter().copied().fold(0usize, |extra, byte| {
        extra.saturating_add(preg_quote_extra_bytes(byte, delimiter))
    });
    if extra == 0 {
        drop(source_bytes);
        ret!(rv, source.into_owned());
    }
    let mut quoted = Vec::with_capacity(source_bytes.len().saturating_add(extra));
    for byte in source_bytes.iter().copied() {
        if byte == 0 {
            quoted.extend_from_slice(b"\\000");
            continue;
        }
        if matches!(
            byte,
            b'.' | b'\\'
                | b'+'
                | b'*'
                | b'?'
                | b'['
                | b'^'
                | b']'
                | b'$'
                | b'('
                | b')'
                | b'{'
                | b'}'
                | b'='
                | b'!'
                | b'<'
                | b'>'
                | b'|'
                | b':'
                | b'-'
                | b'#'
        ) || delimiter == Some(byte)
        {
            quoted.push(b'\\');
        }
        quoted.push(byte);
    }
    let result = if binary {
        Value::binary_string(&quoted)
    } else {
        match String::from_utf8(quoted) {
            Ok(quoted) => Value::string(quoted),
            Err(error) => Value::binary_string(&error.into_bytes()),
        }
    };
    ret!(rv, result);
}

#[inline(always)]
fn preg_quote_extra_bytes(byte: u8, delimiter: Option<u8>) -> usize {
    if byte == 0 {
        return 3;
    }
    usize::from(
        matches!(
            byte,
            b'.' | b'\\'
                | b'+'
                | b'*'
                | b'?'
                | b'['
                | b'^'
                | b']'
                | b'$'
                | b'('
                | b')'
                | b'{'
                | b'}'
                | b'='
                | b'!'
                | b'<'
                | b'>'
                | b'|'
                | b':'
                | b'-'
                | b'#'
        ) || delimiter == Some(byte),
    )
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
        && string.is_ascii()
    {
        ret!(rv, Value::string(encode_html_special_chars_default(string)));
    }
    let Some((string, flags, encoding, double_encode)) =
        html_encode_arguments(ed, eg, "htmlentities")?
    else {
        return Ok(());
    };
    let binary_input = arg!(ed, 0).is_binary_string();
    let binary_source = binary_input.then(|| {
        arg!(ed, 0)
            .php_string_bytes()
            .map(Cow::into_owned)
            .unwrap_or_else(|| string.as_bytes().to_vec())
    });
    let Some(encoding) = resolve_html_translation_encoding(ed, eg, "htmlentities", &encoding)?
    else {
        return Ok(());
    };
    match encoding {
        HtmlTranslationEncoding::Utf8 => {
            let source = if let Some(source) = binary_source.as_deref() {
                let Some(source) = sanitize_html_utf8(&source, flags).map(Cow::into_owned) else {
                    ret!(rv, Value::string(""));
                };
                Cow::Owned(source)
            } else {
                Cow::Borrowed(string.as_str())
            };
            let encoded = encode_named_html_entities_utf8(&source, flags, double_encode);
            if binary_input {
                ret!(rv, Value::binary_string(encoded.as_bytes()));
            }
            ret!(rv, Value::string(encoded));
        }
        HtmlTranslationEncoding::Legacy(legacy) => {
            let source = binary_source
                .as_deref()
                .map(Cow::Borrowed)
                .unwrap_or_else(|| Cow::Borrowed(string.as_bytes()));
            let encoded = encode_html_entities_legacy(&source, flags, double_encode, legacy, true)
                .unwrap_or_default();
            ret!(rv, Value::binary_string_from_storage(encoded));
        }
        HtmlTranslationEncoding::BasicOnly(encoding) => {
            report_internal_diagnostic(
                eg,
                ed,
                8,
                "Notice",
                "htmlentities(): Only basic entities substitution is supported for multi-byte encodings other than UTF-8; functionality is equivalent to htmlspecialchars",
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            let source = binary_source
                .as_deref()
                .map(Cow::Borrowed)
                .unwrap_or_else(|| Cow::Borrowed(string.as_bytes()));
            let encoded =
                encode_html_entities_basic_multibyte(&source, flags, double_encode, encoding)
                    .unwrap_or_default();
            ret!(rv, Value::binary_string_from_storage(encoded));
        }
    }
}

/// urlencode($string): string
#[inline]
fn url_byte_is_safe(byte: u8, raw: bool) -> bool {
    matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.')
        || raw && byte == b'~'
}

fn percent_encode_url_bytes(bytes: &[u8], raw: bool) -> String {
    let Some(first_changed) = bytes.iter().position(|byte| !url_byte_is_safe(*byte, raw)) else {
        // Every retained byte is ASCII, so this conversion cannot fail.
        return String::from_utf8(bytes.to_vec()).unwrap_or_default();
    };
    let headroom = bytes.len().saturating_mul(2).min(64);
    let mut output = String::with_capacity(bytes.len().saturating_add(headroom));
    output.extend(bytes[..first_changed].iter().map(|byte| char::from(*byte)));
    for byte in bytes[first_changed..].iter().copied() {
        if url_byte_is_safe(byte, raw) {
            output.push(char::from(byte));
        } else if !raw && byte == b' ' {
            output.push('+');
        } else {
            push_percent_escape(&mut output, byte);
        }
    }
    output
}

pub(super) fn fn_urlencode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let bytes = arg!(ed, 0)
        .php_string_bytes()
        .unwrap_or_else(|| Cow::Owned(arg_str!(ed, 0).into_owned().into_bytes()));
    ret!(rv, Value::string(percent_encode_url_bytes(&bytes, false)));
}

/// urldecode($string): string
pub(super) fn fn_urldecode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg!(ed, 0)
        .php_string_bytes()
        .unwrap_or_else(|| Cow::Owned(arg_str!(ed, 0).into_owned().into_bytes()));
    ret!(
        rv,
        php_byte_result(percent_decode_php_bytes(&input, true), false)
    );
}

/// rawurlencode($string): string — like urlencode but space → %20
pub(super) fn fn_rawurlencode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let bytes = arg!(ed, 0)
        .php_string_bytes()
        .unwrap_or_else(|| Cow::Owned(arg_str!(ed, 0).into_owned().into_bytes()));
    ret!(rv, Value::string(percent_encode_url_bytes(&bytes, true)));
}

/// rawurldecode($string): string
pub(super) fn fn_rawurldecode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg!(ed, 0)
        .php_string_bytes()
        .unwrap_or_else(|| Cow::Owned(arg_str!(ed, 0).into_owned().into_bytes()));
    ret!(
        rv,
        php_byte_result(percent_decode_php_bytes(&input, false), false)
    );
}

/// base64_encode($string): string
/// Uses Latin-1 byte mapping to handle binary PHP strings correctly.
pub(super) fn fn_base64_encode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "base64_encode",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    use crate::base64;
    let bytes = input.php_string_bytes().unwrap_or_default();
    ret!(rv, Value::string(base64::encode(&bytes)));
}

/// base64_decode($data): string|false
pub(super) fn fn_base64_decode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "base64_decode",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let strict = if arg_opt!(ed, 1).is_some() {
        let Some(strict) = typed_internal_bool_argument(ed, eg, "base64_decode", 1, "strict")?
        else {
            return Ok(());
        };
        strict
    } else {
        false
    };
    use crate::base64;
    let bytes = input.php_string_bytes().unwrap_or_default();
    match base64::decode(&bytes, strict) {
        Some(bytes) => ret!(rv, php_byte_result(bytes, false)),
        None => ret!(rv, Value::bool(false)),
    }
}

pub(super) fn fn_quoted_printable_encode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "quoted_printable_encode",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let bytes = input.php_string_bytes().unwrap_or_default();
    let encoded = crate::quoted_printable::encode(&bytes);
    ret!(
        rv,
        Value::string(
            String::from_utf8(encoded).expect("quoted-printable output contains only ASCII")
        )
    );
}

pub(super) fn fn_quoted_printable_decode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "quoted_printable_decode",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let bytes = input.php_string_bytes().unwrap_or_default();
    ret!(
        rv,
        php_byte_result(crate::quoted_printable::decode(&bytes), false)
    );
}

pub(super) fn fn_convert_uuencode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "convert_uuencode",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let encoded = crate::uuencode::encode(&input.php_string_bytes().unwrap_or_default());
    ret!(
        rv,
        Value::string(String::from_utf8(encoded).expect("UUencode output contains only ASCII"))
    );
}

pub(super) fn fn_convert_uudecode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "convert_uudecode",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let bytes = input.php_string_bytes().unwrap_or_default();
    let Some(decoded) = crate::uuencode::decode(&bytes) else {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "convert_uudecode(): Argument #1 ($data) is not a valid uuencoded string",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        ret!(rv, Value::bool(false));
    };
    ret!(rv, php_byte_result(decoded, false));
}

// ============================================================================
// Missing common string functions
// ============================================================================

/// stripos($haystack, $needle, $offset = 0): int|false
pub(super) fn fn_stripos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    string_position_builtin(ed, rv, eg, "stripos", StringSearchDirection::First, true)
}

/// strripos($haystack, $needle, $offset = 0): int|false
pub(super) fn fn_strripos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    string_position_builtin(ed, rv, eg, "strripos", StringSearchDirection::Last, true)
}

/// str_ireplace($search, $replace, $subject, &$count = null): array|string
pub(super) fn fn_str_ireplace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    super::string_replace_builtin(ed, rv, eg, "str_ireplace", true)
}

/// substr_replace($string, $replace, $offset, $length = null): array|string
pub(super) fn fn_substr_replace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    super::substr_replace_builtin(ed, rv, eg)
}

/// str_getcsv($string, $separator = ",", $enclosure = "\"", $escape = "\\"): array
fn str_getcsv_array(
    string: &Value,
    separator: u8,
    enclosure: u8,
    escape: Option<u8>,
) -> Result<PhpArray, VmError> {
    let binary = string.is_binary_string();
    let bytes = string.php_string_bytes().unwrap_or_default();
    let fields = super::stream::parse_csv_string(&bytes, separator, enclosure, escape)
        .map_err(|error| VmError::Fatal(error.to_string()))?;
    let mut array = PhpArray::with_packed_capacity(fields.len());
    for field in fields {
        array.push(field.map_or_else(Value::null, |field| php_byte_result(field, binary)));
    }
    Ok(array)
}

#[derive(Clone, Copy)]
enum TextCsvState {
    FieldStart,
    Unquoted,
    Quoted,
    AfterQuote,
}

/// Allocation-direct fast path for ordinary ASCII records. Binary, UTF-8,
/// NUL, multiline and diagnostic inputs stay in the canonical shared parser.
/// The guarded state transitions intentionally mirror that parser while
/// materializing the packed PHP result without an intermediate field vector.
fn str_getcsv_text_fast(
    string: &Value,
    separator: u8,
    enclosure: u8,
    escape: Option<u8>,
) -> Option<PhpArray> {
    if string.is_binary_string()
        || !separator.is_ascii()
        || !enclosure.is_ascii()
        || separator == b'\0'
        || enclosure == b'\0'
        || escape.is_some_and(|escape| !escape.is_ascii() || escape == b'\0')
    {
        return None;
    }
    let source = string.as_str()?.as_bytes();
    if source.is_empty() {
        let mut array = PhpArray::with_packed_capacity(1);
        array.push(Value::null());
        return Some(array);
    }

    let mut array = PhpArray::with_packed_capacity(4);
    let mut field = String::new();
    let mut state = TextCsvState::FieldStart;
    let mut index = 0usize;
    while index < source.len() {
        let byte = source[index];
        // No PHP-visible work has happened yet: an ineligible later byte may
        // discard this private partial result and restart canonically.
        if !byte.is_ascii() || matches!(byte, b'\0' | b'\r' | b'\n') {
            return None;
        }
        match state {
            TextCsvState::FieldStart => {
                if byte == enclosure {
                    field.clear();
                    state = TextCsvState::Quoted;
                } else if byte == separator {
                    array.push(Value::string(std::mem::take(&mut field)));
                } else {
                    field.push(char::from(byte));
                    if !matches!(byte, b' ' | b'\t') {
                        state = TextCsvState::Unquoted;
                    }
                }
            }
            TextCsvState::Unquoted => {
                if byte == separator {
                    array.push(Value::string(std::mem::take(&mut field)));
                    state = TextCsvState::FieldStart;
                } else {
                    field.push(char::from(byte));
                }
            }
            TextCsvState::Quoted => {
                if byte == enclosure {
                    if source.get(index + 1).copied() == Some(enclosure) {
                        field.push(char::from(enclosure));
                        index += 1;
                    } else {
                        state = TextCsvState::AfterQuote;
                    }
                } else if escape == Some(byte) {
                    field.push(char::from(byte));
                    if let Some(next) = source.get(index + 1).copied() {
                        field.push(char::from(next));
                        index += 1;
                    }
                } else {
                    field.push(char::from(byte));
                }
            }
            TextCsvState::AfterQuote => {
                if byte == separator {
                    array.push(Value::string(std::mem::take(&mut field)));
                    state = TextCsvState::FieldStart;
                } else {
                    field.push(char::from(byte));
                }
            }
        }
        index += 1;
    }
    array.push(Value::string(field));
    Some(array)
}

pub(super) fn fn_str_getcsv(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_string = arg!(ed, 0);
    if exact_string.value_type() == ValueType::String
        && let (Some(exact_separator), Some(exact_enclosure), Some(exact_escape)) =
            (arg_opt!(ed, 1), arg_opt!(ed, 2), arg_opt!(ed, 3))
        && exact_separator.value_type() == ValueType::String
        && exact_enclosure.value_type() == ValueType::String
        && exact_escape.value_type() == ValueType::String
    {
        let separator = exact_separator.php_string_bytes().unwrap_or_default();
        let enclosure = exact_enclosure.php_string_bytes().unwrap_or_default();
        let escape = exact_escape.php_string_bytes().unwrap_or_default();
        match (separator.as_ref(), enclosure.as_ref(), escape.as_ref()) {
            ([separator], [enclosure], []) => {
                if let Some(array) =
                    str_getcsv_text_fast(exact_string, *separator, *enclosure, None)
                {
                    ret!(rv, Value::array(array));
                }
                ret!(
                    rv,
                    Value::array(str_getcsv_array(
                        exact_string,
                        *separator,
                        *enclosure,
                        None
                    )?)
                );
            }
            ([separator], [enclosure], [escape]) => {
                if let Some(array) =
                    str_getcsv_text_fast(exact_string, *separator, *enclosure, Some(*escape))
                {
                    ret!(rv, Value::array(array));
                }
                ret!(
                    rv,
                    Value::array(str_getcsv_array(
                        exact_string,
                        *separator,
                        *enclosure,
                        Some(*escape)
                    )?)
                );
            }
            _ => {}
        }
    }

    let Some(string) =
        typed_internal_string_value_argument_expected(ed, eg, "str_getcsv", 0, "string", "string")?
    else {
        return Ok(());
    };
    let separator = if arg_opt!(ed, 1).is_some() {
        let Some(separator) = typed_internal_string_value_argument_expected(
            ed,
            eg,
            "str_getcsv",
            1,
            "separator",
            "string",
        )?
        else {
            return Ok(());
        };
        let bytes = separator.php_string_bytes().unwrap_or_default();
        let [separator] = bytes.as_ref() else {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "str_getcsv(): Argument #2 ($separator) must be a single character",
            ));
            return Ok(());
        };
        *separator
    } else {
        b','
    };
    let enclosure = if arg_opt!(ed, 2).is_some() {
        let Some(enclosure) = typed_internal_string_value_argument_expected(
            ed,
            eg,
            "str_getcsv",
            2,
            "enclosure",
            "string",
        )?
        else {
            return Ok(());
        };
        let bytes = enclosure.php_string_bytes().unwrap_or_default();
        let [enclosure] = bytes.as_ref() else {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "str_getcsv(): Argument #3 ($enclosure) must be a single character",
            ));
            return Ok(());
        };
        *enclosure
    } else {
        b'"'
    };
    let escape_supplied = arg_opt!(ed, 3).is_some();
    let escape = if escape_supplied {
        let Some(escape) = typed_internal_string_value_argument_expected(
            ed,
            eg,
            "str_getcsv",
            3,
            "escape",
            "string",
        )?
        else {
            return Ok(());
        };
        let bytes = escape.php_string_bytes().unwrap_or_default();
        match bytes.as_ref() {
            [] => None,
            [escape] => Some(*escape),
            _ => {
                eg.exception = Some(crate::value::make_error_value(
                    "ValueError",
                    "str_getcsv(): Argument #4 ($escape) must be empty or a single character",
                ));
                return Ok(());
            }
        }
    } else {
        Some(b'\\')
    };
    if !escape_supplied {
        report_internal_deprecation(
            eg,
            ed,
            "str_getcsv(): the $escape parameter must be provided as its default value will change",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }

    ret!(
        rv,
        Value::array(str_getcsv_array(&string, separator, enclosure, escape)?)
    );
}

const PHP_DEFAULT_MEMORY_LIMIT: usize = 128 * 1024 * 1024;

fn chunk_split_result_length(
    string_length: usize,
    length: usize,
    separator_length: usize,
) -> Result<usize, usize> {
    // PHP appends the separator even when the source is empty.
    let chunks = string_length.div_ceil(length).max(1);
    let separator_bytes = separator_length.checked_mul(chunks).ok_or(usize::MAX)?;
    let result_length = string_length
        .checked_add(separator_bytes)
        .ok_or(usize::MAX)?;
    if result_length > PHP_DEFAULT_MEMORY_LIMIT {
        Err(result_length)
    } else {
        Ok(result_length)
    }
}

fn chunk_split_php_bytes(string: &Value, length: usize, separator: &Value) -> Result<Value, usize> {
    let string_bytes = string.php_string_bytes().unwrap_or_default();
    let separator_bytes = separator.php_string_bytes().unwrap_or_default();
    let result_length =
        chunk_split_result_length(string_bytes.len(), length, separator_bytes.len())?;

    if !string.is_binary_string()
        && !separator.is_binary_string()
        && string_bytes.is_ascii()
        && separator_bytes.is_ascii()
    {
        let source = string.as_str().unwrap_or("");
        let separator = separator.as_str().unwrap_or("");
        let mut result = String::with_capacity(result_length);
        if source.is_empty() {
            result.push_str(separator);
        } else {
            for chunk in source.as_bytes().chunks(length) {
                result.push_str(std::str::from_utf8(chunk).expect("ASCII chunk is valid UTF-8"));
                result.push_str(separator);
            }
        }
        return Ok(Value::string(result));
    }

    let mut result = Vec::new();
    result
        .try_reserve_exact(result_length)
        .map_err(|_| result_length)?;
    if string_bytes.is_empty() {
        result.extend_from_slice(&separator_bytes);
    } else {
        for chunk in string_bytes.chunks(length) {
            result.extend_from_slice(chunk);
            result.extend_from_slice(&separator_bytes);
        }
    }
    if string.is_binary_string() || separator.is_binary_string() || !result.is_ascii() {
        Ok(Value::binary_string(&result))
    } else {
        Ok(Value::string(
            String::from_utf8(result).expect("ASCII chunk_split result is valid UTF-8"),
        ))
    }
}

pub(super) fn direct_chunk_split_default_string(argument: &Value) -> Option<Value> {
    let argument = argument.dereferenced();
    (argument.value_type() == ValueType::String)
        .then(|| chunk_split_php_bytes(argument, 76, &Value::string("\r\n")))?
        .ok()
}

pub(super) fn fn_chunk_split(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "chunk_split",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let length = match arg_opt!(ed, 1) {
        Some(_) => {
            let Some(length) = typed_internal_int_argument(ed, eg, "chunk_split", 1, "length")?
            else {
                return Ok(());
            };
            length
        }
        None => 76,
    };
    if length < 1 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "chunk_split(): Argument #2 ($length) must be greater than 0",
        ));
        return Ok(());
    }
    let separator = match arg_opt!(ed, 2) {
        Some(_) => {
            let Some(separator) = typed_internal_string_value_argument_expected(
                ed,
                eg,
                "chunk_split",
                2,
                "separator",
                "string",
            )?
            else {
                return Ok(());
            };
            separator
        }
        None => Value::string("\r\n"),
    };
    let length = usize::try_from(length).unwrap_or(usize::MAX);
    match chunk_split_php_bytes(&string, length, &separator) {
        Ok(result) => ret!(rv, result),
        Err(bytes) => {
            let (file, line) = internal_call_source(ed);
            Err(VmError::Fatal(format!(
                "Allowed memory size of {PHP_DEFAULT_MEMORY_LIMIT} bytes exhausted (tried to allocate {bytes} bytes) in {file} on line {line}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BasicMultibyteEncoding, ENT_HTML5, ENT_IGNORE, ENT_QUOTES_MASK, ENT_SUBSTITUTE,
        PHP_DEFAULT_MEMORY_LIMIT, basic_multibyte_unit, chunk_split_php_bytes,
        chunk_split_result_length, decode_html_special_references, percent_encode_url_bytes,
        sanitize_html_utf8, valid_html_entity,
    };
    use crate::value::Value;

    #[test]
    fn special_entity_decode_is_single_pass_flags_aware_and_bounded() {
        const ENT_NOQUOTES: i64 = 0;
        const ENT_COMPAT: i64 = 2;
        let source = b"&amp;lt;|&quot;|&#34;|&apos;|&#39;|&lt;|&#60;|&#65;";
        assert_eq!(
            decode_html_special_references(source, ENT_NOQUOTES),
            "&lt;|&quot;|&#34;|&apos;|&#39;|<|<|&#65;"
        );
        assert_eq!(
            decode_html_special_references(source, ENT_COMPAT),
            "&lt;|\"|\"|&apos;|&#39;|<|<|&#65;"
        );
        assert_eq!(
            decode_html_special_references(source, ENT_QUOTES_MASK | ENT_HTML5),
            "&lt;|\"|\"|'|'|<|<|&#65;"
        );
    }

    #[test]
    fn entity_preservation_accepts_long_leading_zero_numbers_without_unbounded_names() {
        for zero_count in [0, 31, 64, 4_096] {
            for prefix in ["&#", "&#x"] {
                let entity = format!("{prefix}{}5;", "0".repeat(zero_count));
                assert_eq!(
                    valid_html_entity(entity.as_bytes(), ENT_QUOTES_MASK),
                    Some(entity.len())
                );
            }
        }

        assert_eq!(
            valid_html_entity(b"&#999999999999999999999999;", ENT_QUOTES_MASK),
            None
        );
        assert_eq!(
            valid_html_entity(b"&#xFFFFFFFFFFFFFFFF;", ENT_QUOTES_MASK),
            None
        );
        assert_eq!(valid_html_entity(b"&amp;", ENT_QUOTES_MASK), Some(5));
        assert_eq!(
            valid_html_entity(
                format!("&{};", "a".repeat(4_096)).as_bytes(),
                ENT_QUOTES_MASK
            ),
            None
        );
    }

    #[test]
    fn url_encoders_preserve_safe_ascii_and_escape_php_bytes() {
        assert_eq!(
            percent_encode_url_bytes(b"a b~\0\xff", false),
            "a+b%7E%00%FF"
        );
        assert_eq!(
            percent_encode_url_bytes(b"a b~\0\xff", true),
            "a%20b~%00%FF"
        );
        assert_eq!(percent_encode_url_bytes(b"AZaz09-_.", false), "AZaz09-_.");
    }

    #[test]
    fn chunk_split_preserves_bytes_empty_input_and_ending_order() {
        let result =
            chunk_split_php_bytes(&Value::string("abcdef"), 2, &Value::string("|")).unwrap();
        assert_eq!(result.as_str(), Some("ab|cd|ef|"));

        let empty = chunk_split_php_bytes(&Value::string(""), 1, &Value::string("|")).unwrap();
        assert_eq!(empty.as_str(), Some("|"));

        let binary = chunk_split_php_bytes(
            &Value::binary_string(&[0, 0xff, b'A']),
            2,
            &Value::binary_string(&[0]),
        )
        .unwrap();
        assert_eq!(
            binary.php_string_bytes().as_deref(),
            Some(&[0, 0xff, 0, b'A', 0][..])
        );
    }

    #[test]
    fn chunk_split_rejects_oversized_and_overflowing_result_lengths() {
        assert_eq!(chunk_split_result_length(0, 7, 1), Ok(1));
        assert_eq!(chunk_split_result_length(5, 2, 2), Ok(11));
        assert_eq!(
            chunk_split_result_length(50_000_000, 1, 2),
            Err(150_000_000)
        );
        assert_eq!(
            chunk_split_result_length(PHP_DEFAULT_MEMORY_LIMIT, 1, 1),
            Err(PHP_DEFAULT_MEMORY_LIMIT * 2)
        );
        assert_eq!(
            chunk_split_result_length(usize::MAX, 1, usize::MAX),
            Err(usize::MAX)
        );
    }

    #[test]
    fn invalid_utf8_sanitizer_groups_only_valid_lead_sequences() {
        assert_eq!(
            sanitize_html_utf8(b"\xed\xa0\x80A\x80", ENT_SUBSTITUTE).as_deref(),
            Some("\u{fffd}A\u{fffd}")
        );
        assert_eq!(
            sanitize_html_utf8(b"\xf4\x90\x80\x80B", ENT_SUBSTITUTE).as_deref(),
            Some("\u{fffd}B")
        );
        assert_eq!(
            sanitize_html_utf8(b"\xe2\x82C", ENT_IGNORE | ENT_SUBSTITUTE).as_deref(),
            Some("C")
        );
        assert!(sanitize_html_utf8(b"A\x80", ENT_QUOTES_MASK).is_none());
    }

    #[test]
    fn legacy_multibyte_units_retain_restart_boundaries() {
        let euc = BasicMultibyteEncoding::EucJp;
        assert_eq!(basic_multibyte_unit(b"\x8e\xa1", 0, euc), (true, 2));
        assert_eq!(basic_multibyte_unit(b"\x8f\xa1\xa1", 0, euc), (true, 3));
        assert_eq!(basic_multibyte_unit(b"\x8f\xa0", 0, euc), (false, 2));
        assert_eq!(basic_multibyte_unit(b"\x8f\xa1", 0, euc), (false, 1));
        assert_eq!(basic_multibyte_unit(b"\xb2\xff", 0, euc), (false, 2));

        let big5 = BasicMultibyteEncoding::Big5;
        assert_eq!(basic_multibyte_unit(b"\x81\x40", 0, big5), (true, 2));
        assert_eq!(basic_multibyte_unit(b"\x81\x7f", 0, big5), (false, 1));
        assert_eq!(basic_multibyte_unit(b"\xff", 0, big5), (true, 1));
    }
}
