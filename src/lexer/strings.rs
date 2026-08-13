use super::{Lexer, StringPart, Token};

impl<'a> Lexer<'a> {
    pub(super) fn read_string(&mut self, quote: u8) -> Result<String, String> {
        self.pos += 1;
        let mut result = String::new();
        while self.pos < self.src.len() && self.src[self.pos] != quote {
            if self.src[self.pos] == b'\\' && self.pos + 1 < self.src.len() {
                self.pos += 1;
                let escaped = self.src[self.pos];
                match (quote, escaped) {
                    (b'"', b'n') => result.push('\n'),
                    (b'"', b'r') => result.push('\r'),
                    (b'"', b't') => result.push('\t'),
                    (b'"', b'\\') => result.push('\\'),
                    (b'"', b'$') => result.push('$'),
                    (b'"', b'"') => result.push('"'),
                    (b'\'', b'\\') => result.push('\\'),
                    (b'\'', b'\'') => result.push('\''),
                    _ => {
                        result.push('\\');
                        result.push(escaped as char);
                    }
                }
                self.pos += 1;
            } else {
                Self::push_utf8_char(self.src, &mut self.pos, &mut result)?;
            }
        }
        if self.pos >= self.src.len() {
            return Err("Unterminated string literal".into());
        }
        self.pos += 1;
        Ok(result)
    }

    pub(super) fn read_double_quoted_string(&mut self) -> Result<Vec<StringPart>, String> {
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
            return Err("Unterminated string literal".into());
        }
        let parts = Self::interpolate_string_content(&self.src[start..self.pos])?;
        self.pos += 1;
        Ok(parts)
    }

    pub(super) fn read_document_string(&mut self) -> Result<Vec<StringPart>, String> {
        let opener = self.pos;
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
            return Err(format!(
                "Expected heredoc identifier at position {}",
                opener
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
                return Err("Unterminated quoted heredoc identifier".into());
            }
            self.pos += 1;
        }
        match self.src.get(self.pos..) {
            Some(rest) if rest.starts_with(b"\r\n") => self.pos += 2,
            Some(rest) if rest.starts_with(b"\n") => self.pos += 1,
            _ => return Err("Heredoc identifier must be followed by a line break".into()),
        }

        let content_start = self.pos;
        let mut line_start = content_start;
        while line_start <= self.src.len() {
            let newline = self.src[line_start..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|offset| line_start + offset);
            let line_end = newline.unwrap_or(self.src.len());
            let logical_end = if line_end > line_start && self.src[line_end - 1] == b'\r' {
                line_end - 1
            } else {
                line_end
            };
            let mut marker_start = line_start;
            while marker_start < logical_end && matches!(self.src[marker_start], b' ' | b'\t') {
                marker_start += 1;
            }

            let candidate = &self.src[marker_start..logical_end];
            if candidate.starts_with(&label)
                && candidate
                    .get(label.len())
                    .copied()
                    .is_none_or(|byte| !Self::is_identifier_continue(byte))
            {
                let indentation = &self.src[line_start..marker_start];
                if indentation.contains(&b' ') && indentation.contains(&b'\t') {
                    return Err("Heredoc closing indentation mixes spaces and tabs".into());
                }

                let mut content_end = line_start;
                if content_end > content_start && self.src[content_end - 1] == b'\n' {
                    content_end -= 1;
                    if content_end > content_start && self.src[content_end - 1] == b'\r' {
                        content_end -= 1;
                    }
                }
                let content = Self::strip_document_indentation(
                    &self.src[content_start..content_end],
                    indentation,
                )?;
                self.pos = marker_start + label.len();

                if nowdoc {
                    let literal = String::from_utf8(content)
                        .map_err(|_| "Nowdoc content is not valid UTF-8".to_string())?;
                    return Ok(vec![StringPart::Literal(literal)]);
                }
                return Self::interpolate_string_content(&content);
            }

            match newline {
                Some(newline) => line_start = newline + 1,
                None => break,
            }
        }

        Err(format!(
            "Unterminated heredoc starting at position {}",
            opener
        ))
    }

    fn strip_document_indentation(content: &[u8], indentation: &[u8]) -> Result<Vec<u8>, String> {
        if indentation.is_empty() || content.is_empty() {
            return Ok(content.to_vec());
        }
        let expected = indentation[0];
        let width = indentation.len();
        let mut output = Vec::with_capacity(content.len());
        let mut cursor = 0;

        loop {
            let newline = content[cursor..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|offset| cursor + offset);
            let line_end = newline.unwrap_or(content.len());
            let logical_end = if line_end > cursor && content[line_end - 1] == b'\r' {
                line_end - 1
            } else {
                line_end
            };
            let line = &content[cursor..logical_end];
            let whitespace_only = line.iter().all(|byte| matches!(byte, b' ' | b'\t'));
            let removable = line
                .iter()
                .take(width)
                .take_while(|byte| **byte == expected)
                .count();

            if whitespace_only {
                if line.get(removable).is_some_and(|byte| *byte != expected) {
                    return Err("Heredoc body indentation mixes spaces and tabs".into());
                }
            } else if removable != width {
                return Err("Heredoc body indentation is shallower than the closing marker".into());
            }

            output.extend_from_slice(&line[removable..]);
            if logical_end != line_end {
                output.push(b'\r');
            }
            if newline.is_some() {
                output.push(b'\n');
            } else {
                break;
            }
            cursor = line_end + 1;
        }

        Ok(output)
    }

    fn interpolate_string_content(content: &[u8]) -> Result<Vec<StringPart>, String> {
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut pos = 0;

        while pos < content.len() {
            if content[pos] == b'\\' && pos + 1 < content.len() {
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
                        current.push(char::from((value & 0xff) as u8));
                    }
                    b'n' => {
                        current.push('\n');
                        pos += 1;
                    }
                    b'r' => {
                        current.push('\r');
                        pos += 1;
                    }
                    b't' => {
                        current.push('\t');
                        pos += 1;
                    }
                    b'\\' => {
                        current.push('\\');
                        pos += 1;
                    }
                    b'$' => {
                        current.push('$');
                        pos += 1;
                    }
                    b'"' => {
                        current.push('"');
                        pos += 1;
                    }
                    _ => {
                        current.push('\\');
                        Self::push_utf8_char(content, &mut pos, &mut current)?;
                    }
                }
            } else if content[pos] == b'$' {
                let next = content.get(pos + 1).copied().unwrap_or(0);
                if Self::is_identifier_start(next) {
                    if !current.is_empty() {
                        parts.push(StringPart::Literal(std::mem::take(&mut current)));
                    }
                    pos += 1;
                    parts.push(StringPart::Variable(Self::read_content_identifier(
                        content, &mut pos,
                    )?));
                } else {
                    current.push('$');
                    pos += 1;
                }
            } else if content[pos] == b'{' && content.get(pos + 1) == Some(&b'$') {
                if !current.is_empty() {
                    parts.push(StringPart::Literal(std::mem::take(&mut current)));
                }
                pos += 2;
                let name = Self::read_content_identifier(content, &mut pos)?;
                if content.get(pos) == Some(&b'[') {
                    pos += 1;
                    let index_start = pos;
                    while pos < content.len() && content[pos] != b']' {
                        pos += 1;
                    }
                    let index = std::str::from_utf8(&content[index_start..pos])
                        .map_err(|_| "Interpolated array index is not valid UTF-8".to_string())?
                        .to_string();
                    if content.get(pos) == Some(&b']') {
                        pos += 1;
                    }
                    if content.get(pos) == Some(&b'}') {
                        pos += 1;
                    }
                    parts.push(StringPart::ArrayAccess(name, index));
                } else {
                    if content.get(pos) == Some(&b'}') {
                        pos += 1;
                    }
                    parts.push(StringPart::Variable(name));
                }
            } else {
                Self::push_utf8_char(content, &mut pos, &mut current)?;
            }
        }

        if !current.is_empty() || parts.is_empty() {
            parts.push(StringPart::Literal(current));
        }
        Ok(parts)
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

    fn push_utf8_char(bytes: &[u8], pos: &mut usize, output: &mut String) -> Result<(), String> {
        let rest = &bytes[*pos..];
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                let character = valid.chars().next().unwrap();
                output.push(character);
                *pos += character.len_utf8();
                Ok(())
            }
            Err(error) if error.valid_up_to() > 0 => {
                let valid = std::str::from_utf8(&rest[..error.valid_up_to()]).unwrap();
                let character = valid.chars().next().unwrap();
                output.push(character);
                *pos += character.len_utf8();
                Ok(())
            }
            Err(_) => Err(format!(
                "Invalid UTF-8 byte 0x{:02x} in string at position {}",
                bytes[*pos], *pos
            )),
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
                StringPart::Literal(value) => tokens.push(Token::StringLiteral(value.clone())),
                StringPart::Variable(name) => {
                    tokens.push(Token::LParen);
                    tokens.push(Token::StringLiteral(String::new()));
                    tokens.push(Token::Dot);
                    tokens.push(Token::Variable(name.clone()));
                    tokens.push(Token::RParen);
                }
                StringPart::ArrayAccess(name, index) => {
                    tokens.push(Token::LParen);
                    tokens.push(Token::StringLiteral(String::new()));
                    tokens.push(Token::Dot);
                    Self::emit_array_access_tokens(tokens, name, index);
                    tokens.push(Token::RParen);
                }
            }
            return;
        }

        tokens.push(Token::LParen);
        for (index, part) in parts.iter().enumerate() {
            if index != 0 {
                tokens.push(Token::Dot);
            }
            match part {
                StringPart::Literal(value) => tokens.push(Token::StringLiteral(value.clone())),
                StringPart::Variable(name) => tokens.push(Token::Variable(name.clone())),
                StringPart::ArrayAccess(name, index) => {
                    Self::emit_array_access_tokens(tokens, name, index);
                }
            }
        }
        tokens.push(Token::RParen);
    }

    fn emit_array_access_tokens(tokens: &mut Vec<Token>, name: &str, index: &str) {
        tokens.push(Token::Variable(name.to_string()));
        tokens.push(Token::LBracket);
        if let Ok(index) = index.parse::<i64>() {
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
}
