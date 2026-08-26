use super::{
    DeferredCompileDiagnostic, DeferredCompileDiagnosticKind, Lexer, StringPart, Token,
    decode_php_source,
};

const MAX_NESTED_DOCUMENT_DEPTH: usize = 256;

pub(super) struct InterpolatedString {
    pub(super) parts: Vec<StringPart>,
    pub(super) diagnostics: Vec<DeferredCompileDiagnostic>,
}

pub(super) struct StringLexError {
    pub(super) message: String,
    pub(super) line: usize,
}

struct DocumentStringEnd {
    line_start: usize,
    marker_start: usize,
}

impl StringLexError {
    fn new(message: impl Into<String>, line: usize) -> Self {
        Self {
            message: message.into(),
            line,
        }
    }
}

impl<'a> Lexer<'a> {
    pub(super) fn read_string(&mut self, quote: u8) -> Result<(String, bool), String> {
        self.pos += 1;
        let mut result = Vec::new();
        let mut binary = false;
        while self.pos < self.src.len() && self.src[self.pos] != quote {
            if self.src[self.pos] == b'\\' && self.pos + 1 < self.src.len() {
                self.pos += 1;
                let escaped = self.src[self.pos];
                match (quote, escaped) {
                    (b'"', b'n') => result.push(b'\n'),
                    (b'"', b'r') => result.push(b'\r'),
                    (b'"', b't') => result.push(b'\t'),
                    (b'"', b'\\') => result.push(b'\\'),
                    (b'"', b'$') => result.push(b'$'),
                    (b'"', b'"') => result.push(b'"'),
                    (b'\'', b'\\') => result.push(b'\\'),
                    (b'\'', b'\'') => result.push(b'\''),
                    _ => {
                        result.push(b'\\');
                        result.push(escaped);
                        binary |= escaped >= 0x80;
                    }
                }
                self.pos += 1;
            } else {
                binary |= Self::push_utf8_bytes(self.src, &mut self.pos, &mut result)?;
            }
        }
        if self.pos >= self.src.len() {
            return Err("Unterminated string literal".into());
        }
        self.pos += 1;
        if binary {
            return Ok((result.into_iter().map(char::from).collect(), true));
        }
        match String::from_utf8(result) {
            Ok(result) => Ok((result, false)),
            Err(error) => Ok((
                error.into_bytes().into_iter().map(char::from).collect(),
                true,
            )),
        }
    }

    pub(super) fn read_double_quoted_string(
        &mut self,
    ) -> Result<InterpolatedString, StringLexError> {
        let opener = self.pos;
        let opener_line = self.source_line_at(opener);
        self.pos += 1;
        let start = self.pos;
        while self.pos < self.src.len() {
            match self.src[self.pos] {
                b'"' => break,
                b'\\' if self.pos + 1 < self.src.len() => self.pos += 2,
                _ => self.pos += 1,
            }
        }
        if self.pos >= self.src.len() {
            return Err(StringLexError::new(
                "Unterminated string literal",
                opener_line,
            ));
        }
        let content = &self.src[start..self.pos];
        let parts = Self::interpolate_string_content(content, opener_line, 0)?;
        self.pos += 1;
        Ok(parts)
    }

    pub(super) fn read_document_string(&mut self) -> Result<InterpolatedString, StringLexError> {
        let opener = self.pos;
        let opener_line = self.source_line_at(opener);
        self.pos += 3;

        let quote = match self.src.get(self.pos).copied() {
            Some(b'\'') | Some(b'"') => {
                let quote = self.src[self.pos];
                self.pos += 1;
                Some(quote)
            }
            _ => None,
        };
        let nowdoc = quote == Some(b'\'');
        let label_start = self.pos;
        if !self
            .src
            .get(self.pos)
            .copied()
            .is_some_and(Self::is_identifier_start)
        {
            return Err(StringLexError::new(
                format!("Expected heredoc identifier at position {opener}"),
                opener_line,
            ));
        }
        self.pos += 1;
        while self
            .src
            .get(self.pos)
            .copied()
            .is_some_and(Self::is_identifier_continue)
        {
            self.pos += 1;
        }
        let label = self.src[label_start..self.pos].to_vec();

        if let Some(quote) = quote {
            if self.src.get(self.pos) != Some(&quote) {
                return Err(StringLexError::new(
                    "syntax error, unexpected token \"<<\"",
                    opener_line,
                ));
            }
            self.pos += 1;
        }
        match self.src.get(self.pos..) {
            Some(rest) if rest.starts_with(b"\r\n") => self.pos += 2,
            Some(rest) if rest.starts_with(b"\n") || rest.starts_with(b"\r") => self.pos += 1,
            _ => {
                return Err(StringLexError::new(
                    "Heredoc identifier must be followed by a line break",
                    opener_line,
                ));
            }
        }

        let content_start = self.pos;
        let content_start_line = self.source_line_at(content_start);
        let Some(document_end) =
            Self::find_document_string_end(self.src, &label, content_start, !nowdoc, 0)
        else {
            let message = if content_start == self.src.len() {
                "syntax error, unexpected end of file"
            } else {
                "syntax error, unexpected end of file, expecting variable or heredoc end or \"${\" or \"{$\""
            };
            return Err(StringLexError::new(
                message,
                self.source_line_at(self.src.len()),
            ));
        };

        let indentation = &self.src[document_end.line_start..document_end.marker_start];
        if indentation.contains(&b' ') && indentation.contains(&b'\t') {
            return Err(StringLexError::new(
                "Invalid indentation - tabs and spaces cannot be mixed",
                self.source_line_at(document_end.line_start),
            ));
        }

        let mut content_end = document_end.line_start;
        if content_end > content_start && self.src[content_end - 1] == b'\n' {
            content_end -= 1;
            if content_end > content_start && self.src[content_end - 1] == b'\r' {
                content_end -= 1;
            }
        } else if content_end > content_start && self.src[content_end - 1] == b'\r' {
            content_end -= 1;
        }
        let content = Self::strip_document_indentation(
            &self.src[content_start..content_end],
            indentation,
            content_start_line,
        )?;
        self.pos = document_end.marker_start + label.len();

        if nowdoc {
            return Ok(InterpolatedString {
                parts: vec![Self::literal_part(content, false)],
                diagnostics: Vec::new(),
            });
        }
        Self::interpolate_string_content(&content, content_start_line, 1)
    }

    fn find_document_string_end(
        source: &[u8],
        label: &[u8],
        mut line_start: usize,
        scan_interpolation: bool,
        document_depth: usize,
    ) -> Option<DocumentStringEnd> {
        if document_depth > MAX_NESTED_DOCUMENT_DEPTH {
            return None;
        }
        let mut expression_depth = 0_usize;
        let mut quote = None;

        while line_start <= source.len() {
            let (logical_end, next_line_start) = Self::document_line_bounds(source, line_start);
            let mut marker_start = line_start;
            while marker_start < logical_end && matches!(source[marker_start], b' ' | b'\t') {
                marker_start += 1;
            }
            if expression_depth == 0
                && Self::is_document_marker(&source[marker_start..logical_end], label)
            {
                return Some(DocumentStringEnd {
                    line_start,
                    marker_start,
                });
            }
            if !scan_interpolation {
                match next_line_start {
                    Some(next_line_start) => line_start = next_line_start,
                    None => break,
                }
                continue;
            }

            let mut cursor = line_start;
            let mut active_logical_end = logical_end;
            let mut active_next_line_start = next_line_start;
            loop {
                if cursor >= active_logical_end {
                    break;
                }
                let byte = source[cursor];
                if let Some(active_quote) = quote {
                    if byte == b'\\' && cursor + 1 < active_logical_end {
                        cursor += 2;
                        continue;
                    }
                    if byte == active_quote {
                        quote = None;
                    }
                    cursor += 1;
                    continue;
                }

                if expression_depth == 0 {
                    if byte == b'\\' && cursor + 1 < active_logical_end {
                        cursor += 2;
                    } else if (byte == b'$' && source.get(cursor + 1) == Some(&b'{'))
                        || (byte == b'{' && source.get(cursor + 1) == Some(&b'$'))
                    {
                        expression_depth = 1;
                        cursor += 2;
                    } else {
                        cursor += 1;
                    }
                    continue;
                }

                if byte == b'<'
                    && source.get(cursor..cursor + 3) == Some(b"<<<")
                    && let Some((nested_label, nested_content_start, nested_nowdoc)) =
                        Self::document_opener_at(source, cursor)
                    && let Some(nested_end) = Self::find_document_string_end(
                        source,
                        nested_label,
                        nested_content_start,
                        !nested_nowdoc,
                        document_depth + 1,
                    )
                {
                    line_start = nested_end.line_start;
                    (active_logical_end, active_next_line_start) =
                        Self::document_line_bounds(source, line_start);
                    cursor = nested_end.marker_start + nested_label.len();
                    continue;
                }

                match byte {
                    b'\'' | b'"' => quote = Some(byte),
                    b'$' if source.get(cursor + 1) == Some(&b'{') => {
                        expression_depth += 1;
                        cursor += 2;
                        continue;
                    }
                    b'{' => expression_depth += 1,
                    b'}' => expression_depth = expression_depth.saturating_sub(1),
                    _ => {}
                }
                cursor += 1;
            }

            match active_next_line_start {
                Some(next_line_start) => line_start = next_line_start,
                None => break,
            }
        }
        None
    }

    fn is_document_marker(candidate: &[u8], label: &[u8]) -> bool {
        candidate.starts_with(label)
            && candidate
                .get(label.len())
                .copied()
                .is_none_or(|byte| !Self::is_identifier_continue(byte))
    }

    fn document_opener_at(source: &[u8], opener: usize) -> Option<(&[u8], usize, bool)> {
        let mut cursor = opener + 3;
        let quote = match source.get(cursor).copied() {
            Some(b'\'' | b'"') => {
                let quote = source[cursor];
                cursor += 1;
                Some(quote)
            }
            _ => None,
        };
        let label_start = cursor;
        if !source
            .get(cursor)
            .copied()
            .is_some_and(Self::is_identifier_start)
        {
            return None;
        }
        cursor += 1;
        while source
            .get(cursor)
            .copied()
            .is_some_and(Self::is_identifier_continue)
        {
            cursor += 1;
        }
        let label = &source[label_start..cursor];
        if let Some(quote) = quote {
            if source.get(cursor) != Some(&quote) {
                return None;
            }
            cursor += 1;
        }
        if source.get(cursor..cursor + 2) == Some(b"\r\n") {
            Some((label, cursor + 2, quote == Some(b'\'')))
        } else if matches!(source.get(cursor), Some(b'\n' | b'\r')) {
            Some((label, cursor + 1, quote == Some(b'\'')))
        } else {
            None
        }
    }

    fn document_line_bounds(source: &[u8], line_start: usize) -> (usize, Option<usize>) {
        let Some(offset) = source[line_start..]
            .iter()
            .position(|byte| matches!(byte, b'\r' | b'\n'))
        else {
            return (source.len(), None);
        };
        let line_end = line_start + offset;
        let terminator_width = if source.get(line_end..line_end + 2) == Some(b"\r\n") {
            2
        } else {
            1
        };
        (line_end, Some(line_end + terminator_width))
    }

    fn strip_document_indentation(
        content: &[u8],
        indentation: &[u8],
        content_start_line: usize,
    ) -> Result<Vec<u8>, StringLexError> {
        if indentation.is_empty() || content.is_empty() {
            return Ok(content.to_vec());
        }
        let expected = indentation[0];
        let width = indentation.len();
        let mut output = Vec::with_capacity(content.len());
        let mut cursor = 0;
        let mut line_number = content_start_line;

        loop {
            let (logical_end, next_line_start) = Self::document_line_bounds(content, cursor);
            let line = &content[cursor..logical_end];
            let whitespace_only = line.iter().all(|byte| matches!(byte, b' ' | b'\t'));
            let leading_whitespace = line
                .iter()
                .take_while(|byte| matches!(byte, b' ' | b'\t'))
                .count();
            let removable = line
                .iter()
                .take(width)
                .take_while(|byte| **byte == expected)
                .count();

            if line[..leading_whitespace.min(width)]
                .iter()
                .any(|byte| *byte != expected)
            {
                return Err(StringLexError::new(
                    "Invalid indentation - tabs and spaces cannot be mixed",
                    line_number,
                ));
            }

            if whitespace_only {
                if line.get(removable).is_some_and(|byte| *byte != expected) {
                    return Err(StringLexError::new(
                        "Invalid indentation - tabs and spaces cannot be mixed",
                        line_number,
                    ));
                }
            } else if removable != width {
                return Err(StringLexError::new(
                    format!(
                        "Invalid body indentation level (expecting an indentation level of at least {width})"
                    ),
                    line_number,
                ));
            }

            output.extend_from_slice(&line[removable..]);
            match next_line_start {
                Some(next_line_start) => {
                    output.extend_from_slice(&content[logical_end..next_line_start]);
                    cursor = next_line_start;
                    line_number += 1;
                }
                None => break,
            }
        }

        Ok(output)
    }

    pub(super) fn source_line_at(&self, position: usize) -> usize {
        1 + Self::count_logical_line_breaks(&self.src[..position.min(self.src.len())])
    }

    fn count_logical_line_breaks(content: &[u8]) -> usize {
        let mut count = 0;
        let mut cursor = 0;
        while cursor < content.len() {
            match content[cursor] {
                b'\r' => {
                    count += 1;
                    cursor += usize::from(content.get(cursor + 1) == Some(&b'\n')) + 1;
                }
                b'\n' => {
                    count += 1;
                    cursor += 1;
                }
                _ => cursor += 1,
            }
        }
        count
    }

    pub(super) fn document_start_display(&self) -> String {
        let start = self.pos;
        let mut cursor = start + 3;
        if matches!(self.src.get(cursor), Some(b'\'' | b'"')) {
            cursor += 1;
        }
        while self
            .src
            .get(cursor)
            .copied()
            .is_some_and(Self::is_identifier_continue)
        {
            cursor += 1;
        }
        decode_php_source(&self.src[start..cursor])
    }

    fn interpolate_string_content(
        content: &[u8],
        source_line: usize,
        heredoc_line_adjustment: usize,
    ) -> Result<InterpolatedString, StringLexError> {
        let mut parts = Vec::new();
        let mut diagnostics = Vec::new();
        // PHP source strings are byte sequences. Accumulate bytes first so
        // `\xNN` and octal escapes can retain invalid UTF-8 without confusing
        // them with a literal Unicode code point having the same value.
        let mut current = Vec::new();
        let mut current_is_binary = false;
        let mut pos = 0;

        while pos < content.len() {
            if content[pos] == b'\\' && pos + 1 < content.len() {
                let escape_start = pos;
                pos += 1;
                match content[pos] {
                    b'0'..=b'7' => {
                        // PHP consumes up to three octal digits in a
                        // double-quoted escape. Its strings are byte-oriented;
                        // keep the low byte for values such as `\777`.
                        let mut value = 0_u16;
                        let mut digits = 0;
                        while digits < 3
                            && content
                                .get(pos)
                                .is_some_and(|byte| matches!(byte, b'0'..=b'7'))
                        {
                            value = value * 8 + u16::from(content[pos] - b'0');
                            pos += 1;
                            digits += 1;
                        }
                        if value > 0xff {
                            let escape = decode_php_source(&content[escape_start..pos]);
                            let line = source_line
                                + Self::count_logical_line_breaks(&content[..escape_start]);
                            diagnostics.push(DeferredCompileDiagnostic {
                                kind: DeferredCompileDiagnosticKind::Warning,
                                message: format!(
                                    "Octal escape sequence overflow {escape} is greater than \\377"
                                ),
                                line,
                            });
                        }
                        let byte = (value & 0xff) as u8;
                        current.push(byte);
                        current_is_binary |= byte >= 0x80;
                    }
                    b'x' => {
                        let escape_start = pos;
                        pos += 1;
                        let mut value = 0_u8;
                        let mut digits = 0;
                        while digits < 2 {
                            let Some(digit) = content.get(pos).copied().and_then(Self::hex_value)
                            else {
                                break;
                            };
                            value = value * 16 + digit;
                            pos += 1;
                            digits += 1;
                        }
                        if digits == 0 {
                            current.push(b'\\');
                            current.push(content[escape_start]);
                        } else {
                            current.push(value);
                            current_is_binary |= value >= 0x80;
                        }
                    }
                    b'u' if content.get(pos + 1) == Some(&b'{') => {
                        pos += 2;
                        let digits_start = pos;
                        let mut value = 0_u32;
                        while let Some(digit) = content.get(pos).copied().and_then(Self::hex_value)
                        {
                            let Some(next_value) = value
                                .checked_mul(16)
                                .and_then(|value| value.checked_add(u32::from(digit)))
                            else {
                                return Err(Self::string_lex_error_at(
                                    content,
                                    escape_start,
                                    source_line,
                                    "Invalid UTF-8 codepoint escape sequence: Codepoint too large",
                                ));
                            };
                            value = next_value;
                            pos += 1;
                        }
                        if pos == digits_start || content.get(pos) != Some(&b'}') {
                            return Err(Self::string_lex_error_at(
                                content,
                                escape_start,
                                source_line,
                                "Invalid UTF-8 codepoint escape sequence",
                            ));
                        }
                        pos += 1;
                        if value > 0x10ffff {
                            return Err(Self::string_lex_error_at(
                                content,
                                escape_start,
                                source_line,
                                "Invalid UTF-8 codepoint escape sequence: Codepoint too large",
                            ));
                        }
                        let Some(character) = char::from_u32(value) else {
                            // PHP strings can carry CESU-8 surrogate-half bytes. RPHP's
                            // current UTF-8 String representation cannot yet preserve
                            // those bytes, so keep the unsupported case visible instead
                            // of manufacturing a different byte sequence.
                            return Err(Self::string_lex_error_at(
                                content,
                                escape_start,
                                source_line,
                                "Invalid UTF-8 codepoint escape sequence",
                            ));
                        };
                        let mut encoded = [0; 4];
                        current.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
                    }
                    b'n' => {
                        current.push(b'\n');
                        pos += 1;
                    }
                    b'r' => {
                        current.push(b'\r');
                        pos += 1;
                    }
                    b't' => {
                        current.push(b'\t');
                        pos += 1;
                    }
                    b'e' => {
                        current.push(0x1b);
                        pos += 1;
                    }
                    b'f' => {
                        current.push(0x0c);
                        pos += 1;
                    }
                    b'v' => {
                        current.push(0x0b);
                        pos += 1;
                    }
                    b'\\' => {
                        current.push(b'\\');
                        pos += 1;
                    }
                    b'$' => {
                        current.push(b'$');
                        pos += 1;
                    }
                    b'"' => {
                        if heredoc_line_adjustment != 0 {
                            current.push(b'\\');
                        }
                        current.push(b'"');
                        pos += 1;
                    }
                    _ => {
                        current.push(b'\\');
                        let escaped_offset = pos;
                        current_is_binary |= Self::push_utf8_bytes(content, &mut pos, &mut current)
                            .map_err(|message| {
                                Self::string_lex_error_at(
                                    content,
                                    escaped_offset,
                                    source_line,
                                    message,
                                )
                            })?;
                    }
                }
            } else if content[pos] == b'$' {
                let variable_offset = pos;
                let next = content.get(pos + 1).copied().unwrap_or(0);
                if next == b'{' {
                    if !current.is_empty() {
                        parts.push(Self::take_literal_part(
                            &mut current,
                            &mut current_is_binary,
                        ));
                    }
                    let expression_line =
                        source_line + Self::count_logical_line_breaks(&content[..variable_offset]);
                    let deprecation_line = expression_line.saturating_sub(heredoc_line_adjustment);
                    let expression_start = pos + 2;
                    let expression_end = Self::complex_interpolation_end(content, expression_start)
                        .map_err(|message| StringLexError::new(message, expression_line))?;
                    let expression = &content[expression_start..expression_end];
                    let trimmed = expression.trim_ascii();
                    if trimmed.len() == expression.len()
                        && !trimmed.is_empty()
                        && Self::is_identifier_start(trimmed[0])
                        && trimmed[1..]
                            .iter()
                            .all(|byte| Self::is_identifier_continue(*byte))
                    {
                        let name = std::str::from_utf8(trimmed)
                            .map_err(|_| {
                                StringLexError::new(
                                    "Interpolated variable name is not valid UTF-8",
                                    expression_line,
                                )
                            })?
                            .to_string();
                        parts.push(StringPart::Variable(name, expression_line));
                        diagnostics.push(DeferredCompileDiagnostic {
                            kind: DeferredCompileDiagnosticKind::Deprecation,
                            message: "Using ${var} in strings is deprecated, use {$var} instead"
                                .to_string(),
                            line: deprecation_line,
                        });
                    } else {
                        let (tokens, nested_diagnostics) =
                            Self::tokenize_interpolation_expression(expression, expression_line)
                                .map_err(|message| StringLexError::new(message, expression_line))?;
                        parts.push(StringPart::DynamicVariable(tokens, expression_line));
                        diagnostics.extend(nested_diagnostics);
                        diagnostics.push(DeferredCompileDiagnostic {
                            kind: DeferredCompileDiagnosticKind::Deprecation,
                            message: "Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead"
                                .to_string(),
                            line: deprecation_line,
                        });
                    }
                    pos = expression_end + 1;
                } else if Self::is_identifier_start(next) {
                    if !current.is_empty() {
                        parts.push(Self::take_literal_part(
                            &mut current,
                            &mut current_is_binary,
                        ));
                    }
                    pos += 1;
                    let interpolation_line =
                        source_line + Self::count_logical_line_breaks(&content[..variable_offset]);
                    let name = Self::read_content_identifier(content, &mut pos)
                        .map_err(|message| StringLexError::new(message, interpolation_line))?;
                    if content.get(pos) == Some(&b'[') {
                        pos += 1;
                        let index = match content.get(pos).copied() {
                            Some(b'-') => {
                                let index_start = pos;
                                pos += 1;
                                let digits_start = pos;
                                while content
                                    .get(pos)
                                    .copied()
                                    .is_some_and(Self::is_identifier_continue)
                                {
                                    pos += 1;
                                }
                                if pos == digits_start || !content[digits_start].is_ascii_digit() {
                                    return Err(StringLexError::new(
                                        Self::simple_offset_parse_error(),
                                        interpolation_line,
                                    ));
                                }
                                (false, decode_php_source(&content[index_start..pos]))
                            }
                            Some(byte) if byte.is_ascii_digit() => {
                                let index_start = pos;
                                while content
                                    .get(pos)
                                    .copied()
                                    .is_some_and(Self::is_identifier_continue)
                                {
                                    pos += 1;
                                }
                                (false, decode_php_source(&content[index_start..pos]))
                            }
                            Some(b'$')
                                if content
                                    .get(pos + 1)
                                    .copied()
                                    .is_some_and(Self::is_identifier_start) =>
                            {
                                pos += 1;
                                (
                                    true,
                                    Self::read_content_identifier(content, &mut pos).map_err(
                                        |message| StringLexError::new(message, interpolation_line),
                                    )?,
                                )
                            }
                            Some(byte) if Self::is_identifier_start(byte) => (
                                false,
                                Self::read_content_identifier(content, &mut pos).map_err(
                                    |message| StringLexError::new(message, interpolation_line),
                                )?,
                            ),
                            _ => {
                                return Err(StringLexError::new(
                                    Self::simple_offset_parse_error(),
                                    interpolation_line,
                                ));
                            }
                        };
                        if content.get(pos) != Some(&b']') {
                            return Err(StringLexError::new(
                                Self::simple_offset_parse_error(),
                                interpolation_line,
                            ));
                        }
                        pos += 1;
                        if index.0 {
                            parts.push(StringPart::DynamicArrayAccess(
                                name,
                                index.1,
                                interpolation_line,
                            ));
                        } else {
                            parts.push(StringPart::ArrayAccess(name, index.1, interpolation_line));
                        }
                        continue;
                    }
                    let (nullsafe, operator_len) = if content.get(pos..pos + 3) == Some(b"?->") {
                        (true, 3)
                    } else if content.get(pos..pos + 2) == Some(b"->") {
                        (false, 2)
                    } else {
                        (false, 0)
                    };
                    if operator_len != 0
                        && content
                            .get(pos + operator_len)
                            .is_some_and(|byte| Self::is_identifier_start(*byte))
                    {
                        pos += operator_len;
                        let property = Self::read_content_identifier(content, &mut pos)
                            .map_err(|message| StringLexError::new(message, interpolation_line))?;
                        parts.push(StringPart::PropertyAccess(
                            name,
                            property,
                            nullsafe,
                            interpolation_line,
                        ));
                    } else {
                        parts.push(StringPart::Variable(name, interpolation_line));
                    }
                } else {
                    current.push(b'$');
                    pos += 1;
                }
            } else if content[pos] == b'{' && content.get(pos + 1) == Some(&b'$') {
                if !current.is_empty() {
                    parts.push(Self::take_literal_part(
                        &mut current,
                        &mut current_is_binary,
                    ));
                }
                let expression_start = pos + 1;
                let expression_line =
                    source_line + Self::count_logical_line_breaks(&content[..expression_start]);
                pos += 2;
                let name = Self::read_content_identifier(content, &mut pos)
                    .map_err(|message| StringLexError::new(message, expression_line))?;
                if content.get(pos) == Some(&b'[') {
                    pos += 1;
                    let index_start = pos;
                    while pos < content.len() && content[pos] != b']' {
                        pos += 1;
                    }
                    let index = std::str::from_utf8(&content[index_start..pos])
                        .map_err(|_| {
                            StringLexError::new(
                                "Interpolated array index is not valid UTF-8",
                                expression_line,
                            )
                        })?
                        .to_string();
                    if content.get(pos) == Some(&b']') {
                        pos += 1;
                    }
                    if content.get(pos) == Some(&b'}') {
                        pos += 1;
                    }
                    parts.push(StringPart::ArrayAccess(name, index, expression_line));
                } else if content.get(pos) == Some(&b'}') {
                    pos += 1;
                    parts.push(StringPart::Variable(name, expression_line));
                } else {
                    let expression_end = Self::complex_interpolation_end(content, pos)
                        .map_err(|message| StringLexError::new(message, expression_line))?;
                    let (tokens, nested_diagnostics) = Self::tokenize_interpolation_expression(
                        &content[expression_start..expression_end],
                        expression_line,
                    )
                    .map_err(|message| StringLexError::new(message, expression_line))?;
                    diagnostics.extend(nested_diagnostics);
                    pos = expression_end + 1;
                    parts.push(StringPart::Expression(tokens));
                }
            } else {
                let character_offset = pos;
                current_is_binary |= Self::push_utf8_bytes(content, &mut pos, &mut current)
                    .map_err(|message| {
                        Self::string_lex_error_at(content, character_offset, source_line, message)
                    })?;
            }
        }

        if !current.is_empty() || parts.is_empty() {
            parts.push(Self::literal_part(current, current_is_binary));
        }
        Ok(InterpolatedString { parts, diagnostics })
    }

    fn take_literal_part(bytes: &mut Vec<u8>, binary: &mut bool) -> StringPart {
        Self::literal_part(std::mem::take(bytes), std::mem::take(binary))
    }

    fn literal_part(bytes: Vec<u8>, binary: bool) -> StringPart {
        if binary {
            return StringPart::Literal(bytes.into_iter().map(char::from).collect(), true);
        }
        match String::from_utf8(bytes) {
            Ok(text) => StringPart::Literal(text, false),
            Err(error) => StringPart::Literal(
                error.into_bytes().into_iter().map(char::from).collect(),
                true,
            ),
        }
    }

    fn string_lex_error_at(
        content: &[u8],
        offset: usize,
        source_line: usize,
        message: impl Into<String>,
    ) -> StringLexError {
        StringLexError::new(
            message,
            source_line + Self::count_logical_line_breaks(&content[..offset]),
        )
    }

    fn simple_offset_parse_error() -> String {
        "syntax error, unexpected string content \"\", expecting \"-\" or identifier or variable or number"
            .to_string()
    }

    fn complex_interpolation_end(content: &[u8], mut pos: usize) -> Result<usize, String> {
        let mut nested_braces = 0_usize;
        let mut quote = None;

        while pos < content.len() {
            let byte = content[pos];
            if let Some(active_quote) = quote {
                if byte == b'\\' && pos + 1 < content.len() {
                    pos += 2;
                    continue;
                }
                if byte == active_quote {
                    quote = None;
                }
                pos += 1;
                continue;
            }

            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'{' => nested_braces += 1,
                b'}' if nested_braces == 0 => return Ok(pos),
                b'}' => nested_braces -= 1,
                _ => {}
            }
            pos += 1;
        }

        Err("Unterminated complex string interpolation".into())
    }

    fn tokenize_interpolation_expression(
        expression: &[u8],
        source_line: usize,
    ) -> Result<(Vec<Token>, Vec<DeferredCompileDiagnostic>), String> {
        let line_prefix = "\n".repeat(source_line.saturating_sub(1));
        let mut source = format!("<?php {line_prefix}").into_bytes();
        source.extend_from_slice(expression);
        let mut tokens = Lexer::new_bytes(&source).tokenize()?;
        if tokens.first() != Some(&Token::OpenTag) || tokens.last() != Some(&Token::Eof) {
            return Err("Invalid complex string interpolation".into());
        }
        tokens.remove(0);
        tokens.pop();
        let mut diagnostics = Vec::new();
        tokens.retain(|token| match token {
            Token::CompileWarning(message, line) => {
                diagnostics.push(DeferredCompileDiagnostic {
                    kind: DeferredCompileDiagnosticKind::Warning,
                    message: message.clone(),
                    line: *line,
                });
                false
            }
            Token::CompileDeprecation(message, line) => {
                diagnostics.push(DeferredCompileDiagnostic {
                    kind: DeferredCompileDiagnosticKind::Deprecation,
                    message: message.clone(),
                    line: *line,
                });
                false
            }
            _ => true,
        });
        if tokens.is_empty() {
            return Err("Empty complex string interpolation".into());
        }
        Ok((tokens, diagnostics))
    }

    fn read_content_identifier(content: &[u8], pos: &mut usize) -> Result<String, String> {
        let start = *pos;
        while content
            .get(*pos)
            .copied()
            .is_some_and(Self::is_identifier_continue)
        {
            *pos += 1;
        }
        std::str::from_utf8(&content[start..*pos])
            .map(str::to_string)
            .map_err(|_| "Interpolated variable name is not valid UTF-8".to_string())
    }

    fn push_utf8_bytes(
        bytes: &[u8],
        pos: &mut usize,
        output: &mut Vec<u8>,
    ) -> Result<bool, String> {
        let rest = &bytes[*pos..];
        let valid = match std::str::from_utf8(rest) {
            Ok(valid) => valid,
            Err(error) if error.valid_up_to() > 0 => {
                std::str::from_utf8(&rest[..error.valid_up_to()]).unwrap()
            }
            Err(_) => {
                output.push(bytes[*pos]);
                *pos += 1;
                return Ok(true);
            }
        };
        let length = valid
            .chars()
            .next()
            .expect("non-empty source tail")
            .len_utf8();
        output.extend_from_slice(&rest[..length]);
        *pos += length;
        Ok(false)
    }

    fn hex_value(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    fn is_identifier_start(byte: u8) -> bool {
        byte == b'_' || byte.is_ascii_alphabetic() || byte >= b'\x80'
    }

    fn is_identifier_continue(byte: u8) -> bool {
        byte == b'_' || byte.is_ascii_alphanumeric() || byte >= b'\x80'
    }

    pub(super) fn emit_string_parts(tokens: &mut Vec<Token>, parts: &[StringPart]) {
        if parts.len() == 1 {
            match &parts[0] {
                StringPart::Literal(value, binary) => tokens.push(if *binary {
                    Token::BinaryStringLiteral(value.clone())
                } else {
                    Token::StringLiteral(value.clone())
                }),
                StringPart::Variable(name, line) => {
                    tokens.push(Token::LParen(0));
                    tokens.push(Token::StringLiteral(String::new()));
                    tokens.push(Token::Dot);
                    tokens.push(Token::Variable(name.clone(), *line));
                    tokens.push(Token::RParen);
                }
                StringPart::PropertyAccess(name, property, nullsafe, line) => {
                    tokens.push(Token::LParen(0));
                    tokens.push(Token::StringLiteral(String::new()));
                    tokens.push(Token::Dot);
                    Self::emit_property_access_tokens(tokens, name, property, *nullsafe, *line);
                    tokens.push(Token::RParen);
                }
                StringPart::ArrayAccess(name, index, line) => {
                    tokens.push(Token::LParen(0));
                    tokens.push(Token::StringLiteral(String::new()));
                    tokens.push(Token::Dot);
                    Self::emit_array_access_tokens(tokens, name, index, *line);
                    tokens.push(Token::RParen);
                }
                StringPart::DynamicArrayAccess(name, index, line) => {
                    tokens.push(Token::LParen(0));
                    tokens.push(Token::StringLiteral(String::new()));
                    tokens.push(Token::Dot);
                    Self::emit_dynamic_array_access_tokens(tokens, name, index, *line);
                    tokens.push(Token::RParen);
                }
                StringPart::Expression(expression) => {
                    tokens.push(Token::LParen(0));
                    tokens.push(Token::StringLiteral(String::new()));
                    tokens.push(Token::Dot);
                    tokens.push(Token::LParen(0));
                    tokens.extend(expression.iter().cloned());
                    tokens.push(Token::RParen);
                    tokens.push(Token::RParen);
                }
                StringPart::DynamicVariable(expression, line) => {
                    tokens.push(Token::LParen(0));
                    tokens.push(Token::StringLiteral(String::new()));
                    tokens.push(Token::Dot);
                    Self::emit_dynamic_variable_tokens(tokens, expression, *line);
                    tokens.push(Token::RParen);
                }
            }
            return;
        }

        tokens.push(Token::LParen(0));
        for (index, part) in parts.iter().enumerate() {
            if index != 0 {
                tokens.push(Token::Dot);
            }
            match part {
                StringPart::Literal(value, binary) => tokens.push(if *binary {
                    Token::BinaryStringLiteral(value.clone())
                } else {
                    Token::StringLiteral(value.clone())
                }),
                StringPart::Variable(name, line) => {
                    tokens.push(Token::Variable(name.clone(), *line));
                }
                StringPart::PropertyAccess(name, property, nullsafe, line) => {
                    Self::emit_property_access_tokens(tokens, name, property, *nullsafe, *line);
                }
                StringPart::ArrayAccess(name, index, line) => {
                    Self::emit_array_access_tokens(tokens, name, index, *line);
                }
                StringPart::DynamicArrayAccess(name, index, line) => {
                    Self::emit_dynamic_array_access_tokens(tokens, name, index, *line);
                }
                StringPart::Expression(expression) => {
                    tokens.push(Token::LParen(0));
                    tokens.extend(expression.iter().cloned());
                    tokens.push(Token::RParen);
                }
                StringPart::DynamicVariable(expression, line) => {
                    Self::emit_dynamic_variable_tokens(tokens, expression, *line);
                }
            }
        }
        tokens.push(Token::RParen);
    }

    fn emit_property_access_tokens(
        tokens: &mut Vec<Token>,
        name: &str,
        property: &str,
        nullsafe: bool,
        line: usize,
    ) {
        if name == "this" {
            tokens.push(Token::This(line));
        } else {
            tokens.push(Token::Variable(name.to_string(), line));
        }
        tokens.push(if nullsafe {
            Token::NullSafe
        } else {
            Token::Arrow
        });
        tokens.push(Token::Identifier(property.to_string(), line));
    }

    fn emit_dynamic_variable_tokens(tokens: &mut Vec<Token>, expression: &[Token], line: usize) {
        tokens.push(Token::Dollar(line));
        tokens.push(Token::LBrace);
        tokens.extend(expression.iter().cloned());
        tokens.push(Token::RBrace);
    }

    fn emit_array_access_tokens(tokens: &mut Vec<Token>, name: &str, index: &str, line: usize) {
        tokens.push(Token::Variable(name.to_string(), line));
        tokens.push(Token::LBracket(line));
        if let Some(index) = Self::canonical_integer_string(index) {
            tokens.push(Token::Integer(index));
        } else {
            let key = if (index.starts_with('\'') && index.ends_with('\''))
                || (index.starts_with('"') && index.ends_with('"'))
            {
                &index[1..index.len() - 1]
            } else {
                index
            };
            tokens.push(Token::StringLiteral(key.to_string()));
        }
        tokens.push(Token::RBracket);
    }

    fn canonical_integer_string(index: &str) -> Option<i64> {
        let bytes = index.as_bytes();
        let digits = bytes.strip_prefix(b"-").unwrap_or(bytes);
        if digits.is_empty()
            || !digits.iter().all(u8::is_ascii_digit)
            || (digits.len() > 1 && digits[0] == b'0')
            || (bytes.starts_with(b"-") && digits == b"0")
        {
            return None;
        }
        index.parse().ok()
    }

    fn emit_dynamic_array_access_tokens(
        tokens: &mut Vec<Token>,
        name: &str,
        index: &str,
        line: usize,
    ) {
        tokens.push(Token::Variable(name.to_string(), line));
        tokens.push(Token::LBracket(line));
        tokens.push(Token::Variable(index.to_string(), line));
        tokens.push(Token::RBracket);
    }
}
