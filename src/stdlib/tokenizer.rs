//! Cold PHP source tokenization used by framework metadata and dumpers.
//!
//! The runtime parser intentionally consumes a smaller token model. This
//! module instead preserves source spelling and PHP's public tokenizer shape,
//! so consumers can safely reconstruct code while inspecting declarations.

use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

const T_LNUMBER: i64 = 260;
const T_DNUMBER: i64 = 261;
const T_STRING: i64 = 262;
const T_NAME_FULLY_QUALIFIED: i64 = 263;
const T_NAME_RELATIVE: i64 = 264;
const T_NAME_QUALIFIED: i64 = 265;
const T_VARIABLE: i64 = 266;
const T_INLINE_HTML: i64 = 267;
const T_ENCAPSED_AND_WHITESPACE: i64 = 268;
const T_CONSTANT_ENCAPSED_STRING: i64 = 269;
const T_NEW: i64 = 284;
const T_CLASS: i64 = 336;
const T_NAMESPACE: i64 = 342;
const T_COMMENT: i64 = 392;
const T_DOC_COMMENT: i64 = 393;
const T_OPEN_TAG: i64 = 394;
const T_OPEN_TAG_WITH_ECHO: i64 = 395;
const T_CLOSE_TAG: i64 = 396;
const T_WHITESPACE: i64 = 397;
const T_START_HEREDOC: i64 = 398;
const T_END_HEREDOC: i64 = 399;
const T_DOUBLE_COLON: i64 = 402;

fn argument(ed: *mut ExecuteData, index: u32) -> Value {
    crate::stdlib::owned_argument(ed, index)
}

fn token(id: i64, text: &str, line: usize) -> Value {
    let mut fields = PhpArray::with_packed_capacity(3);
    fields.push(Value::long(id));
    fields.push(Value::string(text));
    fields.push(Value::long(line as i64));
    Value::array(fields)
}

fn push_token(output: &mut PhpArray, id: i64, text: &str, line: usize) {
    output.push(token(id, text, line));
}

fn count_lines(text: &str) -> usize {
    text.as_bytes()
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
}

fn is_identifier_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic() || byte >= 0x80
}

fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

fn next_php_tag(source: &str, from: usize) -> Option<(usize, bool)> {
    let bytes = source.as_bytes();
    let mut cursor = from;
    while cursor + 2 < bytes.len() {
        let relative = source[cursor..].find("<?")?;
        let position = cursor + relative;
        if source[position..].starts_with("<?=") {
            return Some((position, true));
        }
        if source[position..].starts_with("<?php")
            && bytes
                .get(position + 5)
                .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            return Some((position, false));
        }
        cursor = position + 2;
    }
    None
}

fn keyword_token(word: &str) -> i64 {
    if word.eq_ignore_ascii_case("class") {
        T_CLASS
    } else if word.eq_ignore_ascii_case("namespace") {
        T_NAMESPACE
    } else if word.eq_ignore_ascii_case("new") {
        T_NEW
    } else {
        T_STRING
    }
}

fn quoted_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' {
            cursor = (cursor + 2).min(bytes.len());
        } else if bytes[cursor] == quote {
            return cursor + 1;
        } else {
            cursor += 1;
        }
    }
    bytes.len()
}

fn heredoc_parts(source: &str, start: usize) -> Option<(usize, usize, usize, usize)> {
    let bytes = source.as_bytes();
    let opening_end = source[start..]
        .find('\n')
        .map_or(bytes.len(), |n| start + n + 1);
    let declaration = source[start + 3..opening_end].trim();
    let label = declaration.trim_matches(['\'', '"', '\r', '\n']);
    if label.is_empty() || !label.bytes().all(is_identifier_continue) {
        return None;
    }

    let mut line_start = opening_end;
    while line_start < bytes.len() {
        let line_end = source[line_start..]
            .find('\n')
            .map_or(bytes.len(), |n| line_start + n);
        let line = &source[line_start..line_end];
        let indentation = line.len() - line.trim_start_matches([' ', '\t']).len();
        let candidate = &line[indentation..];
        if candidate == label || candidate.starts_with(&format!("{label};")) {
            let label_start = line_start + indentation;
            return Some((
                opening_end,
                label_start,
                label_start + label.len(),
                line_end,
            ));
        }
        line_start = (line_end + 1).min(bytes.len());
    }
    None
}

fn tokenize(source: &str) -> PhpArray {
    let bytes = source.as_bytes();
    let mut output = PhpArray::new();
    let mut cursor = 0usize;
    let mut line = 1usize;
    let mut in_php = false;

    while cursor < bytes.len() {
        if !in_php {
            let Some((tag, short_echo)) = next_php_tag(source, cursor) else {
                push_token(&mut output, T_INLINE_HTML, &source[cursor..], line);
                break;
            };
            if tag > cursor {
                let html = &source[cursor..tag];
                push_token(&mut output, T_INLINE_HTML, html, line);
                line += count_lines(html);
            }
            if short_echo {
                push_token(&mut output, T_OPEN_TAG_WITH_ECHO, "<?=", line);
                cursor = tag + 3;
            } else {
                let end = tag + 6;
                let opening = &source[tag..end];
                push_token(&mut output, T_OPEN_TAG, opening, line);
                line += count_lines(opening);
                cursor = end;
            }
            in_php = true;
            continue;
        }

        let start = cursor;
        let start_line = line;
        if source[cursor..].starts_with("?>") {
            cursor += 2;
            if bytes.get(cursor) == Some(&b'\n') {
                cursor += 1;
            }
            let text = &source[start..cursor];
            push_token(&mut output, T_CLOSE_TAG, text, start_line);
            line += count_lines(text);
            in_php = false;
            continue;
        }
        if bytes[cursor].is_ascii_whitespace() {
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            let text = &source[start..cursor];
            push_token(&mut output, T_WHITESPACE, text, start_line);
            line += count_lines(text);
            continue;
        }
        if source[cursor..].starts_with("//") || bytes[cursor] == b'#' {
            cursor += if bytes[cursor] == b'#' { 1 } else { 2 };
            while cursor < bytes.len() && bytes[cursor] != b'\n' {
                cursor += 1;
            }
            push_token(&mut output, T_COMMENT, &source[start..cursor], start_line);
            continue;
        }
        if source[cursor..].starts_with("/*") {
            cursor += 2;
            while cursor + 1 < bytes.len() && &bytes[cursor..cursor + 2] != b"*/" {
                cursor += 1;
            }
            cursor = (cursor + 2).min(bytes.len());
            let text = &source[start..cursor];
            push_token(
                &mut output,
                if text.starts_with("/**") {
                    T_DOC_COMMENT
                } else {
                    T_COMMENT
                },
                text,
                start_line,
            );
            line += count_lines(text);
            continue;
        }
        if source[cursor..].starts_with("<<<")
            && let Some((opening_end, label_start, label_end, _line_end)) =
                heredoc_parts(source, cursor)
        {
            let opening = &source[start..opening_end];
            push_token(&mut output, T_START_HEREDOC, opening, start_line);
            line += count_lines(opening);
            if label_start > opening_end {
                let body = &source[opening_end..label_start];
                push_token(&mut output, T_ENCAPSED_AND_WHITESPACE, body, line);
                line += count_lines(body);
            }
            push_token(
                &mut output,
                T_END_HEREDOC,
                &source[label_start..label_end],
                line,
            );
            cursor = label_end;
            continue;
        }
        if matches!(bytes[cursor], b'\'' | b'"' | b'`') {
            cursor = quoted_end(bytes, cursor, bytes[cursor]);
            let text = &source[start..cursor];
            push_token(&mut output, T_CONSTANT_ENCAPSED_STRING, text, start_line);
            line += count_lines(text);
            continue;
        }
        if bytes[cursor] == b'$'
            && bytes
                .get(cursor + 1)
                .is_some_and(|byte| is_identifier_start(*byte))
        {
            cursor += 2;
            while cursor < bytes.len() && is_identifier_continue(bytes[cursor]) {
                cursor += 1;
            }
            push_token(&mut output, T_VARIABLE, &source[start..cursor], start_line);
            continue;
        }
        if is_identifier_start(bytes[cursor]) || bytes[cursor] == b'\\' {
            cursor += 1;
            while cursor < bytes.len()
                && (is_identifier_continue(bytes[cursor]) || bytes[cursor] == b'\\')
            {
                cursor += 1;
            }
            let text = &source[start..cursor];
            let id = if text.starts_with("namespace\\") {
                T_NAME_RELATIVE
            } else if text.starts_with('\\') {
                T_NAME_FULLY_QUALIFIED
            } else if text.contains('\\') {
                T_NAME_QUALIFIED
            } else {
                keyword_token(text)
            };
            push_token(&mut output, id, text, start_line);
            continue;
        }
        if bytes[cursor].is_ascii_digit() {
            cursor += 1;
            while cursor < bytes.len()
                && (bytes[cursor].is_ascii_alphanumeric()
                    || matches!(bytes[cursor], b'_' | b'.' | b'+' | b'-'))
            {
                cursor += 1;
            }
            let text = &source[start..cursor];
            push_token(
                &mut output,
                if text.contains(['.', 'e', 'E']) {
                    T_DNUMBER
                } else {
                    T_LNUMBER
                },
                text,
                start_line,
            );
            continue;
        }
        if source[cursor..].starts_with("::") {
            push_token(&mut output, T_DOUBLE_COLON, "::", start_line);
            cursor += 2;
            continue;
        }

        let character =
            &source[cursor..cursor + source[cursor..].chars().next().unwrap().len_utf8()];
        output.push(Value::string(character));
        cursor += character.len();
    }
    output
}

pub(super) fn token_get_all(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = argument(ed, 0).echo_to_string();
    let result = Value::array(tokenize(&source));
    crate::stdlib::write_return_value(rv, result);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_preserves_source_when_tokens_are_rejoined() {
        let source = "html<?php\n/** docs */ namespace Rphp\\Fixture; // comment\nnew class {}; Fixture::class;";
        let tokens = tokenize(source);
        let mut rebuilt = String::new();
        for value in tokens.values() {
            if let Some(fields) = value.as_array() {
                rebuilt.push_str(fields.get_int(1).unwrap().as_str().unwrap());
            } else {
                rebuilt.push_str(value.as_str().unwrap());
            }
        }
        assert_eq!(rebuilt, source);
    }
}
