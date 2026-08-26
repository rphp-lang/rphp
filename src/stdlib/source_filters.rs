//! Cold PHP source presentation and tag-filtering builtins.
//!
//! Compilation deliberately discards comments and insignificant whitespace,
//! so source presentation owns a small independent scanner and never feeds
//! compatibility-only tokens into the execution frontend.

use std::collections::HashSet;

use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

#[derive(Clone, Copy, PartialEq, Eq)]
enum HighlightKind {
    Html,
    Default,
    Keyword,
    String,
    Comment,
}

struct HighlightColors {
    string: String,
    comment: String,
    keyword: String,
    default: String,
    html: String,
}

impl HighlightColors {
    fn from_executor(eg: &ExecutorGlobals) -> Self {
        let color = |name: &str, fallback: &str| {
            super::ini_default(eg, name).unwrap_or_else(|| fallback.to_string())
        };
        Self {
            string: color("highlight.string", "#DD0000"),
            comment: color("highlight.comment", "#FF8000"),
            keyword: color("highlight.keyword", "#007700"),
            default: color("highlight.default", "#0000BB"),
            html: color("highlight.html", "#000000"),
        }
    }

    fn for_kind(&self, kind: HighlightKind) -> &str {
        match kind {
            HighlightKind::Html => &self.html,
            HighlightKind::Default => &self.default,
            HighlightKind::Keyword => &self.keyword,
            HighlightKind::String => &self.string,
            HighlightKind::Comment => &self.comment,
        }
    }
}

#[derive(Clone)]
struct HighlightSegment {
    kind: HighlightKind,
    text: String,
}

fn push_segment(segments: &mut Vec<HighlightSegment>, kind: HighlightKind, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = segments.last_mut()
        && last.kind == kind
    {
        last.text.push_str(text);
    } else {
        segments.push(HighlightSegment {
            kind,
            text: text.to_string(),
        });
    }
}

#[inline]
fn ascii_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn consume_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(|byte| ascii_whitespace(*byte)) {
        index += 1;
    }
    index
}

fn open_tag_length(bytes: &[u8], index: usize) -> Option<usize> {
    let remaining = bytes.get(index..)?;
    if remaining.len() >= 5
        && remaining[..5].eq_ignore_ascii_case(b"<?php")
        && remaining
            .get(5)
            .map_or(true, |byte| ascii_whitespace(*byte))
    {
        return Some(5);
    }
    remaining.starts_with(b"<?=").then_some(3)
}

fn find_next_open_tag(bytes: &[u8], mut index: usize) -> Option<(usize, usize)> {
    while index < bytes.len() {
        if bytes[index] == b'<'
            && let Some(length) = open_tag_length(bytes, index)
        {
            return Some((index, length));
        }
        index += 1;
    }
    None
}

fn quoted_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 1;
            if index < bytes.len() {
                index += utf8_char_width(bytes[index]);
            }
        } else if bytes[index] == quote {
            return index + 1;
        } else {
            index += utf8_char_width(bytes[index]);
        }
    }
    bytes.len()
}

#[inline]
fn utf8_char_width(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

fn line_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |offset| start + offset + 1)
}

fn heredoc_label(header: &[u8]) -> Option<&[u8]> {
    let mut value = header.strip_prefix(b"<<<")?;
    value = value.strip_suffix(b"\n").unwrap_or(value);
    value = value.strip_suffix(b"\r").unwrap_or(value);
    while value
        .first()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        value = &value[1..];
    }
    if matches!(value.first(), Some(b'\'' | b'"')) {
        let quote = value[0];
        value = &value[1..];
        let end = value.iter().position(|byte| *byte == quote)?;
        value = &value[..end];
    } else {
        let end = value
            .iter()
            .position(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
            .unwrap_or(value.len());
        value = &value[..end];
    }
    (!value.is_empty()).then_some(value)
}

/// Return header end, content end and terminator end for a heredoc/nowdoc.
/// The terminator end includes following whitespace because PHP assigns that
/// whitespace the keyword token's color.
fn heredoc_bounds(bytes: &[u8], start: usize) -> Option<(usize, usize, usize)> {
    let header_end = line_end(bytes, start);
    let label = heredoc_label(&bytes[start..header_end])?;
    let mut line_start = header_end;
    while line_start < bytes.len() {
        let current_end = line_end(bytes, line_start);
        let mut content_end = current_end;
        if content_end > line_start && bytes[content_end - 1] == b'\n' {
            content_end -= 1;
        }
        if content_end > line_start && bytes[content_end - 1] == b'\r' {
            content_end -= 1;
        }
        let line = &bytes[line_start..content_end];
        if line.starts_with(label) && matches!(line.get(label.len()), None | Some(b';')) {
            let token_end = consume_whitespace(bytes, current_end);
            return Some((header_end, line_start, token_end));
        }
        line_start = current_end;
    }
    Some((header_end, bytes.len(), bytes.len()))
}

fn is_keyword(identifier: &str) -> bool {
    matches!(
        identifier.to_ascii_lowercase().as_str(),
        "__halt_compiler"
            | "abstract"
            | "and"
            | "array"
            | "as"
            | "break"
            | "callable"
            | "case"
            | "catch"
            | "class"
            | "clone"
            | "const"
            | "continue"
            | "declare"
            | "default"
            | "die"
            | "do"
            | "echo"
            | "else"
            | "elseif"
            | "empty"
            | "enddeclare"
            | "endfor"
            | "endforeach"
            | "endif"
            | "endswitch"
            | "endwhile"
            | "enum"
            | "eval"
            | "exit"
            | "extends"
            | "final"
            | "finally"
            | "fn"
            | "for"
            | "foreach"
            | "function"
            | "global"
            | "goto"
            | "if"
            | "implements"
            | "include"
            | "include_once"
            | "instanceof"
            | "insteadof"
            | "interface"
            | "isset"
            | "list"
            | "match"
            | "namespace"
            | "new"
            | "or"
            | "print"
            | "private"
            | "protected"
            | "public"
            | "readonly"
            | "require"
            | "require_once"
            | "return"
            | "static"
            | "switch"
            | "throw"
            | "trait"
            | "try"
            | "unset"
            | "use"
            | "var"
            | "while"
            | "xor"
            | "yield"
    )
}

fn cast_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'(') {
        return None;
    }
    let mut index = consume_whitespace(bytes, start + 1);
    let type_start = index;
    while bytes
        .get(index)
        .is_some_and(|byte| byte.is_ascii_alphabetic())
    {
        index += 1;
    }
    if type_start == index {
        return None;
    }
    let cast = &bytes[type_start..index];
    const CASTS: [&[u8]; 12] = [
        b"array", b"binary", b"bool", b"boolean", b"double", b"float", b"int", b"integer",
        b"object", b"real", b"string", b"unset",
    ];
    if !CASTS
        .iter()
        .any(|expected| cast.eq_ignore_ascii_case(expected))
    {
        return None;
    }
    index = consume_whitespace(bytes, index);
    (bytes.get(index) == Some(&b')')).then_some(index + 1)
}

#[inline]
fn identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn identifier_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        end += 1;
    }
    end
}

fn push_interpolated_variable_suffix(
    segments: &mut Vec<HighlightSegment>,
    source: &str,
    mut index: usize,
    end: usize,
) -> usize {
    let bytes = source.as_bytes();
    if bytes
        .get(index..index.saturating_add(2))
        .is_some_and(|operator| operator == b"->")
        && bytes
            .get(index + 2)
            .is_some_and(|byte| identifier_start(*byte))
    {
        push_segment(segments, HighlightKind::Keyword, &source[index..index + 2]);
        let name_end = identifier_end(bytes, index + 2);
        push_segment(
            segments,
            HighlightKind::Default,
            &source[index + 2..name_end],
        );
        return name_end;
    }
    if bytes.get(index) != Some(&b'[') {
        return index;
    }
    push_segment(segments, HighlightKind::Keyword, &source[index..index + 1]);
    index += 1;
    while index < end && bytes[index] != b']' {
        let token_end = if identifier_start(bytes[index]) || bytes[index].is_ascii_digit() {
            let mut token_end = index + 1;
            while bytes
                .get(token_end)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'.'))
            {
                token_end += 1;
            }
            token_end
        } else if matches!(bytes[index], b'\'' | b'"') {
            quoted_end(bytes, index, bytes[index])
        } else {
            index + utf8_char_width(bytes[index])
        };
        let kind = if matches!(bytes[index], b'\'' | b'"') {
            HighlightKind::String
        } else if identifier_start(bytes[index]) || bytes[index].is_ascii_digit() {
            HighlightKind::Default
        } else {
            HighlightKind::Keyword
        };
        push_segment(segments, kind, &source[index..token_end.min(end)]);
        index = token_end.min(end);
    }
    if index < end && bytes[index] == b']' {
        push_segment(segments, HighlightKind::Keyword, &source[index..index + 1]);
        index += 1;
    }
    index
}

fn push_interpolated_content(
    segments: &mut Vec<HighlightSegment>,
    source: &str,
    start: usize,
    end: usize,
) {
    let bytes = source.as_bytes();
    let mut chunk_start = start;
    let mut index = start;
    while index < end {
        if bytes[index] == b'\\' {
            index += 1;
            if index < end {
                index += utf8_char_width(bytes[index]);
            }
            continue;
        }
        if bytes[index..].starts_with(b"{$")
            && bytes
                .get(index + 2)
                .is_some_and(|byte| identifier_start(*byte))
        {
            push_segment(segments, HighlightKind::String, &source[chunk_start..index]);
            push_segment(segments, HighlightKind::Keyword, &source[index..index + 1]);
            let variable_end = identifier_end(bytes, index + 2);
            push_segment(
                segments,
                HighlightKind::Default,
                &source[index + 1..variable_end],
            );
            index = push_interpolated_variable_suffix(segments, source, variable_end, end);
            if index < end && bytes[index] == b'}' {
                push_segment(segments, HighlightKind::Keyword, &source[index..index + 1]);
                index += 1;
            }
            chunk_start = index;
            continue;
        }
        if bytes[index..].starts_with(b"${")
            && bytes
                .get(index + 2)
                .is_some_and(|byte| identifier_start(*byte))
        {
            push_segment(segments, HighlightKind::String, &source[chunk_start..index]);
            push_segment(segments, HighlightKind::Keyword, &source[index..index + 2]);
            let name_end = identifier_end(bytes, index + 2);
            push_segment(
                segments,
                HighlightKind::Default,
                &source[index + 2..name_end],
            );
            index = name_end;
            if index < end && bytes[index] == b'}' {
                push_segment(segments, HighlightKind::Keyword, &source[index..index + 1]);
                index += 1;
            }
            chunk_start = index;
            continue;
        }
        if bytes[index] == b'$'
            && bytes
                .get(index + 1)
                .is_some_and(|byte| identifier_start(*byte))
        {
            push_segment(segments, HighlightKind::String, &source[chunk_start..index]);
            let variable_end = identifier_end(bytes, index + 2);
            push_segment(
                segments,
                HighlightKind::Default,
                &source[index..variable_end],
            );
            index = push_interpolated_variable_suffix(segments, source, variable_end, end);
            chunk_start = index;
            continue;
        }
        index += utf8_char_width(bytes[index]);
    }
    push_segment(segments, HighlightKind::String, &source[chunk_start..end]);
}

fn push_interpolated_string_segments(
    segments: &mut Vec<HighlightSegment>,
    source: &str,
    start: usize,
    token_end: usize,
    quote: u8,
) {
    let bytes = source.as_bytes();
    let quoted_end = quoted_end(bytes, start, quote);
    let closed = quoted_end > start + 1 && bytes.get(quoted_end - 1) == Some(&quote);
    let content_end = quoted_end.saturating_sub(usize::from(closed));
    if quote == b'`' {
        push_segment(segments, HighlightKind::Keyword, &source[start..start + 1]);
        push_interpolated_content(segments, source, start + 1, content_end);
        if closed {
            push_segment(
                segments,
                HighlightKind::Keyword,
                &source[quoted_end - 1..token_end],
            );
        }
    } else {
        push_segment(segments, HighlightKind::String, &source[start..start + 1]);
        push_interpolated_content(segments, source, start + 1, content_end);
        push_segment(
            segments,
            HighlightKind::String,
            &source[content_end..token_end],
        );
    }
}

fn highlight_segments(source: &str) -> Vec<HighlightSegment> {
    let bytes = source.as_bytes();
    let mut segments = Vec::new();
    let mut index = 0usize;
    let mut in_php = false;
    while index < bytes.len() {
        if !in_php {
            let Some((tag_start, tag_length)) = find_next_open_tag(bytes, index) else {
                push_segment(&mut segments, HighlightKind::Html, &source[index..]);
                break;
            };
            push_segment(
                &mut segments,
                HighlightKind::Html,
                &source[index..tag_start],
            );
            let end = consume_whitespace(bytes, tag_start + tag_length);
            push_segment(
                &mut segments,
                HighlightKind::Default,
                &source[tag_start..end],
            );
            index = end;
            in_php = true;
            continue;
        }

        if bytes[index..].starts_with(b"?>") {
            let mut end = index + 2;
            if bytes.get(end) == Some(&b'\r') && bytes.get(end + 1) == Some(&b'\n') {
                end += 2;
            } else if matches!(bytes.get(end), Some(b'\n' | b'\r')) {
                end += 1;
            }
            push_segment(&mut segments, HighlightKind::Default, &source[index..end]);
            index = end;
            in_php = false;
            continue;
        }
        if bytes[index..].starts_with(b"//")
            || (bytes[index] == b'#' && bytes.get(index + 1) != Some(&b'['))
        {
            let mut end = index;
            while end < bytes.len() && bytes[end] != b'\n' && !bytes[end..].starts_with(b"?>") {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'\n' {
                end = consume_whitespace(bytes, end + 1);
            }
            push_segment(&mut segments, HighlightKind::Comment, &source[index..end]);
            index = end;
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            let end = bytes[index + 2..]
                .windows(2)
                .position(|window| window == b"*/")
                .map_or(bytes.len(), |offset| index + 2 + offset + 2);
            let end = consume_whitespace(bytes, end);
            push_segment(&mut segments, HighlightKind::Comment, &source[index..end]);
            index = end;
            continue;
        }
        if bytes[index..].starts_with(b"<<<")
            && let Some((header_end, content_end, terminator_end)) = heredoc_bounds(bytes, index)
        {
            push_segment(
                &mut segments,
                HighlightKind::Keyword,
                &source[index..header_end],
            );
            let header = &bytes[index + 3..header_end];
            let nowdoc = header
                .iter()
                .copied()
                .find(|byte| !matches!(byte, b' ' | b'\t'))
                == Some(b'\'');
            if nowdoc {
                push_segment(
                    &mut segments,
                    HighlightKind::String,
                    &source[header_end..content_end],
                );
            } else {
                push_interpolated_content(&mut segments, source, header_end, content_end);
            }
            push_segment(
                &mut segments,
                HighlightKind::Keyword,
                &source[content_end..terminator_end],
            );
            index = terminator_end;
            continue;
        }
        if bytes[index] == b'\'' {
            let end = consume_whitespace(bytes, quoted_end(bytes, index, bytes[index]));
            push_segment(&mut segments, HighlightKind::String, &source[index..end]);
            index = end;
            continue;
        }
        if matches!(bytes[index], b'"' | b'`') {
            let end = consume_whitespace(bytes, quoted_end(bytes, index, bytes[index]));
            push_interpolated_string_segments(&mut segments, source, index, end, bytes[index]);
            index = end;
            continue;
        }
        if bytes[index] == b'('
            && let Some(cast_end) = cast_end(bytes, index)
        {
            let end = consume_whitespace(bytes, cast_end);
            push_segment(&mut segments, HighlightKind::Keyword, &source[index..end]);
            index = end;
            continue;
        }
        if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let mut end = index + 1;
            while bytes
                .get(end)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                end += 1;
            }
            let kind = if is_keyword(&source[index..end]) {
                HighlightKind::Keyword
            } else {
                HighlightKind::Default
            };
            end = consume_whitespace(bytes, end);
            push_segment(&mut segments, kind, &source[index..end]);
            index = end;
            continue;
        }
        if bytes[index] == b'$'
            && bytes
                .get(index + 1)
                .is_some_and(|byte| identifier_start(*byte))
        {
            let mut end = index + 1;
            while bytes
                .get(end)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                end += 1;
            }
            end = consume_whitespace(bytes, end);
            push_segment(&mut segments, HighlightKind::Default, &source[index..end]);
            index = end;
            continue;
        }
        if bytes[index].is_ascii_digit()
            || (bytes[index] == b'.' && bytes.get(index + 1).is_some_and(u8::is_ascii_digit))
        {
            let mut end = index + 1;
            while bytes
                .get(end)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'.'))
            {
                end += 1;
            }
            end = consume_whitespace(bytes, end);
            push_segment(&mut segments, HighlightKind::Default, &source[index..end]);
            index = end;
            continue;
        }
        if ascii_whitespace(bytes[index]) {
            let end = consume_whitespace(bytes, index);
            push_segment(&mut segments, HighlightKind::Default, &source[index..end]);
            index = end;
            continue;
        }
        if bytes[index].is_ascii_punctuation() {
            let end = consume_whitespace(bytes, index + 1);
            push_segment(&mut segments, HighlightKind::Keyword, &source[index..end]);
            index = end;
            continue;
        }
        let length = source[index..].chars().next().map_or(1, char::len_utf8);
        let end = consume_whitespace(bytes, index + length);
        push_segment(&mut segments, HighlightKind::Default, &source[index..end]);
        index = end;
    }
    segments
}

fn push_html_escaped(output: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(character),
        }
    }
}

fn highlight_source(source: &str, colors: &HighlightColors) -> String {
    let segments = highlight_segments(source);
    let mut output = String::with_capacity(source.len().saturating_mul(2).saturating_add(80));
    output.push_str("<pre><code style=\"color: ");
    output.push_str(&colors.html);
    output.push_str("\">");
    let mut active = None;
    for segment in segments {
        if segment.kind == HighlightKind::Html {
            if active.take().is_some() {
                output.push_str("</span>");
            }
            push_html_escaped(&mut output, &segment.text);
            continue;
        }
        if active != Some(segment.kind) {
            if active.is_some() {
                output.push_str("</span>");
            }
            output.push_str("<span style=\"color: ");
            output.push_str(colors.for_kind(segment.kind));
            output.push_str("\">");
            active = Some(segment.kind);
        }
        push_html_escaped(&mut output, &segment.text);
    }
    if active.is_some() {
        output.push_str("</span>");
    }
    output.push_str("</code></pre>");
    output
}

fn normalized_allowed_name(value: &[u8]) -> Option<Vec<u8>> {
    if value.is_empty()
        || value[0] == b'/'
        || value
            .iter()
            .any(|byte| ascii_whitespace(*byte) || matches!(byte, b'<' | b'>' | 0))
    {
        return None;
    }
    Some(value.iter().map(u8::to_ascii_lowercase).collect())
}

fn parse_allowed_tag_list(value: &[u8], names: &mut HashSet<Vec<u8>>) {
    for (start, byte) in value.iter().enumerate() {
        if *byte != b'<' {
            continue;
        }
        let Some(relative_end) = value[start + 1..].iter().position(|byte| *byte == b'>') else {
            continue;
        };
        let end = start + 1 + relative_end;
        if let Some(name) = normalized_allowed_name(&value[start + 1..end]) {
            names.insert(name);
        }
    }
}

fn insert_allowed_array_item(value: &[u8], names: &mut HashSet<Vec<u8>>) {
    if value.contains(&b'<') {
        parse_allowed_tag_list(value, names);
    } else if let Some(name) = normalized_allowed_name(value) {
        names.insert(name);
    }
}

fn allowed_item_bytes(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<Vec<u8>>, VmError> {
    let value = value.dereferenced();
    if value.value_type() == ValueType::String {
        return Ok(Some(
            value.php_string_bytes().unwrap_or_default().into_owned(),
        ));
    }
    if value.value_type() == ValueType::Array {
        super::report_internal_diagnostic(eg, ed, 2, "Warning", "Array to string conversion")?;
        if eg.exception.is_some() {
            return Ok(None);
        }
        return Ok(Some(b"Array".to_vec()));
    }
    if value.value_type() == ValueType::Closure {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "Object of class Closure could not be converted to string",
        ));
        return Ok(None);
    }
    if value.value_type() != ValueType::Object {
        return Ok(Some(
            value
                .echo_to_string_with_precision(eg.precision)
                .into_bytes(),
        ));
    }

    let class_name = value
        .as_object()
        .map(|object| object.class_name.to_string())
        .unwrap_or_else(|| "object".to_string());
    let rendered = crate::vm::execute::call_object_string_conversion(eg, value)?;
    if eg.exception.is_some() {
        return Ok(None);
    }
    let Some(rendered) = rendered else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            &format!("Object of class {class_name} could not be converted to string"),
        ));
        return Ok(None);
    };
    let rendered = rendered.dereferenced();
    if rendered.value_type() != ValueType::String {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("{class_name}::__toString(): Return value must be of type string"),
        ));
        return Ok(None);
    }
    Ok(Some(
        rendered.php_string_bytes().unwrap_or_default().into_owned(),
    ))
}

fn allowed_tag_names(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: Option<&Value>,
) -> Result<Option<HashSet<Vec<u8>>>, VmError> {
    let mut names = HashSet::new();
    let Some(value) = value else {
        return Ok(Some(names));
    };
    let value = value.dereferenced();
    if value.value_type() == ValueType::Array {
        let items = value
            .as_array()
            .map(|array| array.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for item in &items {
            let Some(item) = allowed_item_bytes(ed, eg, item)? else {
                return Ok(None);
            };
            insert_allowed_array_item(&item, &mut names);
        }
    } else if value.value_type() == ValueType::String {
        let bytes = value.php_string_bytes().unwrap_or_default();
        parse_allowed_tag_list(bytes.as_ref(), &mut names);
    }
    Ok(Some(names))
}

fn tag_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    let mut quote = None;
    let mut depth = 1usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active_quote) = quote {
            if byte == active_quote && bytes.get(index.wrapping_sub(1)) != Some(&b'\\') {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'"') && bytes.get(index.wrapping_sub(1)) != Some(&b'\\') {
            quote = Some(byte);
        } else if byte == b'<' {
            depth += 1;
        } else if byte == b'>' {
            depth -= 1;
            if depth == 0 {
                return Some(index + 1);
            }
        }
        index += 1;
    }
    None
}

fn quoted_delimiter_end(bytes: &[u8], start: usize, delimiter: &[u8]) -> Option<usize> {
    let mut index = start;
    let mut quote = None;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active_quote) = quote {
            if byte == active_quote && bytes.get(index.wrapping_sub(1)) != Some(&b'\\') {
                quote = None;
            }
            index += 1;
            continue;
        }
        if matches!(byte, b'\'' | b'"' | b'`') && bytes.get(index.wrapping_sub(1)) != Some(&b'\\') {
            quote = Some(byte);
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(delimiter) {
            return Some(index + delimiter.len());
        }
        index += 1;
    }
    None
}

fn effective_tag_bounds(bytes: &[u8], start: usize, end: usize) -> (usize, usize) {
    let leading = bytes[start..end]
        .iter()
        .take_while(|byte| **byte == b'<')
        .count()
        .max(1);
    (start + leading - 1, end.saturating_sub(leading - 1))
}

fn normalized_tag_name(tag: &[u8]) -> Option<Vec<u8>> {
    let mut index = usize::from(tag.first() == Some(&b'<'));
    if tag.get(index) == Some(&b'/') {
        index += 1;
    }
    let start = index;
    while tag
        .get(index)
        .is_some_and(|byte| !ascii_whitespace(*byte) && *byte != b'>')
    {
        index += 1;
    }
    if index == start {
        return None;
    }
    if tag.get(index.wrapping_sub(1)) == Some(&b'/') {
        index -= 1;
    }
    normalized_allowed_name(&tag[start..index])
}

fn strip_tags_bytes(bytes: &[u8], allowed: &HashSet<Vec<u8>>) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'<' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        if bytes
            .get(index + 1)
            .is_some_and(|byte| ascii_whitespace(*byte))
        {
            output.push(b'<');
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"<!--") {
            index = if bytes.get(index + 4) == Some(&b'>') {
                index + 5
            } else {
                bytes[index + 4..]
                    .windows(3)
                    .position(|window| window == b"-->")
                    .map_or(bytes.len(), |offset| index + 4 + offset + 3)
            };
            continue;
        }
        if bytes[index..].starts_with(b"<?") {
            if bytes[index..].starts_with(b"<?>") {
                index += 3;
            } else if bytes
                .get(index + 2..index + 5)
                .is_some_and(|name| name.eq_ignore_ascii_case(b"xml"))
            {
                index = quoted_delimiter_end(bytes, index + 2, b"?>")
                    .or_else(|| tag_end(bytes, index + 2))
                    .unwrap_or(bytes.len());
            } else {
                index = quoted_delimiter_end(bytes, index + 2, b"?>").unwrap_or(bytes.len());
            }
            continue;
        }
        if bytes[index..].starts_with(b"<%") {
            index = tag_end(bytes, index + 2).unwrap_or(bytes.len());
            continue;
        }
        let Some(end) = tag_end(bytes, index + 1) else {
            break;
        };
        if bytes.get(index + 1) != Some(&b'!') {
            let (tag_start, tag_end) = effective_tag_bounds(bytes, index, end);
            if normalized_tag_name(&bytes[tag_start..tag_end])
                .is_some_and(|name| allowed.contains(&name))
            {
                output.extend_from_slice(&bytes[tag_start..tag_end]);
            }
        }
        index = end;
    }
    output
}

fn strip_source_whitespace(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut index = 0usize;
    let mut in_php = false;
    let mut last_was_collapsed_whitespace = false;
    while index < bytes.len() {
        if !in_php {
            let Some((tag_start, tag_length)) = find_next_open_tag(bytes, index) else {
                output.push_str(&source[index..]);
                break;
            };
            output.push_str(&source[index..tag_start]);
            let end = consume_whitespace(bytes, tag_start + tag_length);
            output.push_str(&source[tag_start..end]);
            index = end;
            in_php = true;
            last_was_collapsed_whitespace = false;
            continue;
        }
        if bytes[index..].starts_with(b"?>") {
            output.push_str("?>");
            index += 2;
            in_php = false;
            last_was_collapsed_whitespace = false;
            continue;
        }
        if bytes[index..].starts_with(b"//")
            || (bytes[index] == b'#' && bytes.get(index + 1) != Some(&b'['))
        {
            while index < bytes.len() && bytes[index] != b'\n' && !bytes[index..].starts_with(b"?>")
            {
                index += 1;
            }
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            index = bytes[index + 2..]
                .windows(2)
                .position(|window| window == b"*/")
                .map_or(bytes.len(), |offset| index + 2 + offset + 2);
            continue;
        }
        if bytes[index..].starts_with(b"<<<")
            && let Some((_, content_end, end)) = heredoc_bounds(bytes, index)
        {
            let terminator_line_end = line_end(bytes, content_end);
            if terminator_line_end >= content_end + 2
                && bytes.get(terminator_line_end - 2..terminator_line_end) == Some(&b"\r\n"[..])
            {
                output.push_str(&source[index..terminator_line_end - 2]);
                output.push('\n');
                output.push_str(&source[terminator_line_end..end]);
            } else {
                output.push_str(&source[index..end]);
            }
            index = end;
            last_was_collapsed_whitespace = true;
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"' | b'`') {
            let end = quoted_end(bytes, index, bytes[index]);
            output.push_str(&source[index..end]);
            index = end;
            last_was_collapsed_whitespace = false;
            continue;
        }
        if ascii_whitespace(bytes[index]) {
            index = consume_whitespace(bytes, index);
            if !last_was_collapsed_whitespace {
                output.push(' ');
            }
            last_was_collapsed_whitespace = true;
            continue;
        }
        let length = source[index..].chars().next().map_or(1, char::len_utf8);
        output.push_str(&source[index..index + length]);
        index += length;
        last_was_collapsed_whitespace = false;
    }
    output
}

enum SourceFile {
    Contents { source: String, legacy_bytes: bool },
    Unreadable { filename: String, reason: String },
}

fn file_error_reason(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
        std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
        std::io::ErrorKind::IsADirectory => "Is a directory".to_string(),
        _ if error.raw_os_error() == Some(36) => "File name too long".to_string(),
        _ => error
            .to_string()
            .split_once(" (os error ")
            .map_or_else(|| error.to_string(), |(message, _)| message.to_string()),
    }
}

fn read_source_file(_eg: &ExecutorGlobals, filename: String) -> SourceFile {
    let requested = filename.clone();
    #[cfg(feature = "include-path")]
    let filename = super::include_path::resolve_for_open(_eg, &filename, true);
    match std::fs::read(filename) {
        Ok(bytes) => SourceFile::Contents {
            legacy_bytes: !bytes.is_ascii(),
            source: super::bytes_to_php_string(&bytes),
        },
        Err(error) => SourceFile::Unreadable {
            filename: requested,
            reason: file_error_reason(&error),
        },
    }
}

pub(super) fn fn_strip_tags(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(source) = super::typed_internal_string_value_argument_expected(
        ed,
        eg,
        "strip_tags",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let allowed_argument = arg_opt!(ed, 1).cloned();
    let allowed_value = match allowed_argument.as_ref().map(Value::dereferenced) {
        None => None,
        Some(value) if value.value_type() == ValueType::Null => None,
        Some(value) if matches!(value.value_type(), ValueType::Array | ValueType::String) => {
            Some(value.clone())
        }
        Some(_) => {
            let Some(value) = super::typed_internal_string_value_argument_expected(
                ed,
                eg,
                "strip_tags",
                1,
                "allowed_tags",
                "array|string|null",
            )?
            else {
                return Ok(());
            };
            Some(value)
        }
    };
    let Some(allowed) = allowed_tag_names(ed, eg, allowed_value.as_ref())? else {
        return Ok(());
    };
    let source_bytes = source.php_string_bytes().unwrap_or_default();
    if !source_bytes.contains(&b'<') && !source_bytes.contains(&0) {
        ret!(rv, source);
    }
    let cleaned = source_bytes
        .iter()
        .copied()
        .filter(|byte| *byte != 0)
        .collect::<Vec<_>>();
    let result = strip_tags_bytes(&cleaned, &allowed);
    ret!(
        rv,
        super::php_byte_result(result, source.is_binary_string())
    );
}

pub(super) fn fn_highlight_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(source) = super::typed_internal_string_value_argument_expected(
        ed,
        eg,
        "highlight_string",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let return_output = match arg_opt!(ed, 1) {
        Some(_) => {
            let Some(value) =
                super::typed_internal_bool_argument(ed, eg, "highlight_string", 1, "return")?
            else {
                return Ok(());
            };
            value
        }
        None => false,
    };
    reject_output_handler_reentry(ed, eg, "highlight_string")?;
    let legacy_bytes =
        source.is_binary_string() && source.as_str().is_some_and(|source| !source.is_ascii());
    let source = source.as_str().unwrap_or_default();
    let highlighted = highlight_source(&source, &HighlightColors::from_executor(eg));
    if return_output {
        if legacy_bytes {
            ret!(
                rv,
                super::php_byte_result(super::php_string_to_bytes(&highlighted), false)
            );
        }
        ret!(rv, Value::string(highlighted));
    }
    if legacy_bytes {
        eg.write_output(&super::php_string_to_bytes(&highlighted));
    } else {
        eg.write_output(highlighted.as_bytes());
    }
    ret!(rv, Value::bool(true));
}

fn highlight_file(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<(), VmError> {
    let Some(filename) = super::typed_internal_string_argument(ed, eg, function, 0, "filename")?
    else {
        return Ok(());
    };
    let return_output = match arg_opt!(ed, 1) {
        Some(_) => {
            let Some(value) = super::typed_internal_bool_argument(ed, eg, function, 1, "return")?
            else {
                return Ok(());
            };
            value
        }
        None => false,
    };
    reject_output_handler_reentry(ed, eg, function)?;
    let (source, legacy_bytes) = match read_source_file(eg, filename) {
        SourceFile::Contents {
            source,
            legacy_bytes,
        } => (source, legacy_bytes),
        SourceFile::Unreadable { filename, reason } => {
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("{function}({filename}): Failed to open stream: {reason}"),
            )?;
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("{function}(): Failed opening '{filename}' for highlighting"),
            )?;
            ret!(rv, Value::bool(false));
        }
    };
    let highlighted = highlight_source(&source, &HighlightColors::from_executor(eg));
    if return_output {
        if legacy_bytes {
            ret!(
                rv,
                super::php_byte_result(super::php_string_to_bytes(&highlighted), false)
            );
        }
        ret!(rv, Value::string(highlighted));
    }
    if legacy_bytes {
        eg.write_output(&super::php_string_to_bytes(&highlighted));
    } else {
        eg.write_output(highlighted.as_bytes());
    }
    ret!(rv, Value::bool(true));
}

pub(super) fn fn_highlight_file(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    highlight_file(ed, rv, eg, "highlight_file")
}

pub(super) fn fn_show_source(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    highlight_file(ed, rv, eg, "show_source")
}

pub(super) fn fn_php_strip_whitespace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(filename) =
        super::typed_internal_string_argument(ed, eg, "php_strip_whitespace", 0, "filename")?
    else {
        return Ok(());
    };
    reject_output_handler_reentry(ed, eg, "php_strip_whitespace")?;
    let (stripped, legacy_bytes) = match read_source_file(eg, filename) {
        SourceFile::Contents {
            source,
            legacy_bytes,
        } => (strip_source_whitespace(&source), legacy_bytes),
        SourceFile::Unreadable { filename, reason } => {
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("php_strip_whitespace({filename}): Failed to open stream: {reason}"),
            )?;
            (String::new(), false)
        }
    };
    if legacy_bytes {
        ret!(
            rv,
            super::php_byte_result(super::php_string_to_bytes(&stripped), false)
        );
    }
    ret!(rv, Value::string(stripped));
}

fn reject_output_handler_reentry(
    ed: *mut ExecuteData,
    eg: &ExecutorGlobals,
    function: &str,
) -> Result<(), VmError> {
    if !eg.is_output_handler_active() {
        return Ok(());
    }
    let (file, line) = super::internal_call_source(ed);
    Err(VmError::Fatal(format!(
        "{function}(): Cannot use output buffering in output buffering display handlers in {file} on line {line}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_colors() -> HighlightColors {
        HighlightColors {
            string: "#DD0000".to_string(),
            comment: "#FF8000".to_string(),
            keyword: "#007700".to_string(),
            default: "#0000BB".to_string(),
            html: "#000000".to_string(),
        }
    }

    #[test]
    fn highlighter_preserves_invalid_numeric_literals_as_default_text() {
        assert_eq!(
            highlight_source("<?php \n 09 09 09;", &default_colors()),
            "<pre><code style=\"color: #000000\"><span style=\"color: #0000BB\">&lt;?php \n 09 09 09</span><span style=\"color: #007700\">;</span></code></pre>"
        );
    }

    #[test]
    fn highlighter_keeps_attribute_cast_and_interpolation_token_boundaries() {
        assert_eq!(
            highlight_source(
                "<?php #[Attr] (int) $x; \"a{$x}b\"; `c$x`; ?>\r\nz",
                &default_colors(),
            ),
            concat!(
                "<pre><code style=\"color: #000000\">",
                "<span style=\"color: #0000BB\">&lt;?php </span>",
                "<span style=\"color: #007700\">#[</span>",
                "<span style=\"color: #0000BB\">Attr</span>",
                "<span style=\"color: #007700\">] (int) </span>",
                "<span style=\"color: #0000BB\">$x</span>",
                "<span style=\"color: #007700\">; </span>",
                "<span style=\"color: #DD0000\">\"a</span>",
                "<span style=\"color: #007700\">{</span>",
                "<span style=\"color: #0000BB\">$x</span>",
                "<span style=\"color: #007700\">}</span>",
                "<span style=\"color: #DD0000\">b\"</span>",
                "<span style=\"color: #007700\">; `</span>",
                "<span style=\"color: #DD0000\">c</span>",
                "<span style=\"color: #0000BB\">$x</span>",
                "<span style=\"color: #007700\">`; </span>",
                "<span style=\"color: #0000BB\">?&gt;\r\n</span>",
                "z</code></pre>",
            )
        );
    }

    #[test]
    fn tag_filter_keeps_only_named_tags_and_removes_nuls() {
        let allowed = HashSet::from([b"b".to_vec()]);
        let source = b"a\0<p>one <B title=\">\">two</B></p>\0z"
            .iter()
            .copied()
            .filter(|byte| *byte != 0)
            .collect::<Vec<_>>();
        assert_eq!(
            strip_tags_bytes(&source, &allowed),
            b"aone <B title=\">\">two</B>z"
        );
        assert_eq!(strip_tags_bytes(b"<foo<>bar>", &HashSet::new()), b"");
    }

    #[test]
    fn tag_filter_distinguishes_declarations_processing_instructions_and_literal_angles() {
        assert_eq!(
            strip_tags_bytes(
                b"A<!DOCTYPE q '>'>B<!-- I've gone -->C<?xml:n p=1 />D",
                &HashSet::new(),
            ),
            b"ABCD"
        );
        assert_eq!(
            strip_tags_bytes(b"A<?= '<?= 1 ?>' ?>B< ax", &HashSet::new()),
            b"AB< ax"
        );
    }

    #[test]
    fn tag_filter_normalizes_nested_and_exact_allowed_names() {
        let mut allowed = HashSet::new();
        parse_allowed_tag_list(b"junk<a><<html>><a-b>", &mut allowed);
        assert_eq!(
            strip_tags_bytes(b"A<<HtMl>>B<</HtMl>><a.>C</.a><a-b>D</a-b><a/b>E", &allowed,),
            b"A<HtMl>B</HtMl>C<a-b>D</a-b>E"
        );
    }

    #[test]
    fn whitespace_filter_removes_comments_without_joining_tokens() {
        assert_eq!(
            strip_source_whitespace("<?php $x = 1 /* comment */ + 2;"),
            "<?php $x = 1 + 2;"
        );
        assert_eq!(
            strip_source_whitespace("<?php __CLASS__ ?>"),
            "<?php __CLASS__ ?>"
        );
        assert_eq!(
            strip_source_whitespace("<?php /* comment */ ?>"),
            "<?php  ?>"
        );
    }

    #[test]
    fn whitespace_filter_preserves_attributes_and_normalizes_heredoc_terminator_crlf() {
        assert_eq!(
            strip_source_whitespace(
                "<?php\r\n#[Attr]\r\n$x=<<<TXT\r\nA\r\nTXT;\r\n/* c */\r\necho $x;\r\n?>\r\ntail",
            ),
            "<?php\r\n#[Attr] $x=<<<TXT\r\nA\r\nTXT;\necho $x; ?>\r\ntail"
        );
    }
}
