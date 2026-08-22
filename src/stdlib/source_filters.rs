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
            index = (index + 2).min(bytes.len());
        } else if bytes[index] == quote {
            return index + 1;
        } else {
            index += 1;
        }
    }
    bytes.len()
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
            | "false"
            | "final"
            | "finally"
            | "fn"
            | "for"
            | "foreach"
            | "from"
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
            | "null"
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
            | "true"
            | "try"
            | "unset"
            | "use"
            | "var"
            | "while"
            | "xor"
            | "yield"
    )
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
            push_segment(
                &mut segments,
                HighlightKind::Default,
                &source[index..index + 2],
            );
            index += 2;
            in_php = false;
            continue;
        }
        if bytes[index..].starts_with(b"//") || bytes[index] == b'#' {
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
            push_segment(
                &mut segments,
                HighlightKind::String,
                &source[header_end..content_end],
            );
            push_segment(
                &mut segments,
                HighlightKind::Keyword,
                &source[content_end..terminator_end],
            );
            index = terminator_end;
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"' | b'`') {
            let end = consume_whitespace(bytes, quoted_end(bytes, index, bytes[index]));
            push_segment(&mut segments, HighlightKind::String, &source[index..end]);
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
        if bytes[index] == b'$' || bytes[index].is_ascii_digit() {
            let mut end = index + 1;
            while bytes.get(end).is_some_and(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(*byte, b'_' | b'$')
                    || (bytes[index].is_ascii_digit() && *byte == b'.')
            }) {
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

fn parse_allowed_tag_list(value: &str, names: &mut HashSet<String>) {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    let mut found_bracketed = false;
    while index < bytes.len() {
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        found_bracketed = true;
        index += 1;
        if bytes.get(index) == Some(&b'/') {
            index += 1;
        }
        let start = index;
        while bytes
            .get(index)
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        {
            index += 1;
        }
        if index > start {
            names.insert(value[start..index].to_ascii_lowercase());
        }
    }
    if !found_bracketed {
        let value = value.trim().trim_start_matches('/');
        let end = value
            .bytes()
            .position(|byte| !byte.is_ascii_alphanumeric())
            .unwrap_or(value.len());
        if end != 0 {
            names.insert(value[..end].to_ascii_lowercase());
        }
    }
}

fn allowed_tag_names(value: Option<&Value>) -> HashSet<String> {
    let mut names = HashSet::new();
    let Some(value) = value else {
        return names;
    };
    if value.value_type() == ValueType::Array {
        if let Some(array) = value.as_array() {
            for (_, item) in array.iter() {
                if let Some(item) = item.dereferenced().as_str() {
                    parse_allowed_tag_list(item, &mut names);
                }
            }
        }
    } else if let Some(value) = value.as_str() {
        parse_allowed_tag_list(value, &mut names);
    }
    names
}

fn tag_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    let mut quote = None;
    let mut depth = 1usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active_quote) = quote {
            if byte == active_quote {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'"') {
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

fn strip_tags_text(source: &str, allowed: &HashSet<String>) -> String {
    let source: String = source
        .chars()
        .filter(|character| *character != '\0')
        .collect();
    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'<' {
            let length = source[index..].chars().next().map_or(1, char::len_utf8);
            output.push_str(&source[index..index + length]);
            index += length;
            continue;
        }
        if bytes[index..].starts_with(b"<!--") {
            index = bytes[index + 4..]
                .windows(3)
                .position(|window| window == b"-->")
                .map_or(bytes.len(), |offset| index + 4 + offset + 3);
            continue;
        }
        if bytes[index..].starts_with(b"<?") {
            index = bytes[index + 2..]
                .windows(2)
                .position(|window| window == b"?>")
                .map_or(bytes.len(), |offset| index + 2 + offset + 2);
            continue;
        }
        if bytes[index..].starts_with(b"<%") {
            index = bytes[index + 2..]
                .windows(2)
                .position(|window| window == b"%>")
                .map_or(bytes.len(), |offset| index + 2 + offset + 2);
            continue;
        }
        let mut name_start = index + 1;
        if bytes.get(name_start) == Some(&b'/') {
            name_start += 1;
        }
        if bytes
            .get(name_start)
            .map_or(true, |byte| !byte.is_ascii_alphabetic())
        {
            output.push('<');
            index += 1;
            continue;
        }
        let mut name_end = name_start + 1;
        while bytes
            .get(name_end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        {
            name_end += 1;
        }
        let Some(end) = tag_end(bytes, name_end) else {
            break;
        };
        let name = source[name_start..name_end].to_ascii_lowercase();
        if allowed.contains(&name) {
            output.push_str(&source[index..end]);
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
        if bytes[index..].starts_with(b"//") || bytes[index] == b'#' {
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
            && let Some((_, _, end)) = heredoc_bounds(bytes, index)
        {
            output.push_str(&source[index..end]);
            index = end;
            last_was_collapsed_whitespace = false;
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
    Contents(String),
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
        Ok(bytes) => SourceFile::Contents(super::bytes_to_php_string(&bytes)),
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
    let Some(source) = super::typed_internal_string_argument(ed, eg, "strip_tags", 0, "string")?
    else {
        return Ok(());
    };
    let allowed_value = arg_opt!(ed, 1);
    let allowed = if allowed_value.is_some_and(|value| {
        !matches!(
            value.value_type(),
            ValueType::Null | ValueType::Array | ValueType::String
        )
    }) {
        let Some(value) = super::typed_internal_string_argument_expected(
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
        let value = Value::string(value);
        allowed_tag_names(Some(&value))
    } else {
        allowed_tag_names(allowed_value.filter(|value| value.value_type() != ValueType::Null))
    };
    ret!(rv, Value::string(strip_tags_text(&source, &allowed)));
}

pub(super) fn fn_highlight_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(source) =
        super::typed_internal_string_argument(ed, eg, "highlight_string", 0, "string")?
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
    let highlighted = highlight_source(&source, &HighlightColors::from_executor(eg));
    if return_output {
        ret!(rv, Value::string(highlighted));
    }
    eg.write_output(highlighted.as_bytes());
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
    let source = match read_source_file(eg, filename) {
        SourceFile::Contents(source) => source,
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
        ret!(rv, Value::string(highlighted));
    }
    eg.write_output(highlighted.as_bytes());
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
    let stripped = match read_source_file(eg, filename) {
        SourceFile::Contents(source) => strip_source_whitespace(&source),
        SourceFile::Unreadable { filename, reason } => {
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("php_strip_whitespace({filename}): Failed to open stream: {reason}"),
            )?;
            String::new()
        }
    };
    ret!(rv, Value::string(stripped));
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
    fn tag_filter_keeps_only_named_tags_and_removes_nuls() {
        let allowed = HashSet::from(["b".to_string()]);
        assert_eq!(
            strip_tags_text("a\0<p>one <B title=\">\">two</B></p>\0z", &allowed),
            "aone <B title=\">\">two</B>z"
        );
        assert_eq!(strip_tags_text("<foo<>bar>", &HashSet::new()), "");
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
}
