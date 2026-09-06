use super::{Lexer, Token, decode_php_source};

impl Lexer<'_> {
    /// A bare sigil must not consume a number as a variable name or discard the
    /// lexeme which made it invalid. This path runs only after normal $name,
    /// $$name and ${expression} have been ruled out.
    #[cold]
    #[inline(never)]
    // SAFETY: this is ordinary compiler-generated executable code; the
    // section changes placement only, not ABI, identity or initialization.
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
    pub(super) fn invalid_variable_token(&self) -> Token {
        let start = match self.skip_halt_trivia(self.pos) {
            Ok(start) => start,
            Err(error) => return Token::ParseError(error.message, error.line),
        };
        let line = self.source_line_at(start);
        let tail = &self.src[start..];
        let description = match tail.first().copied() {
            None => "end of file".to_string(),
            Some(quote @ (b'\'' | b'"')) => {
                let mut end = 1;
                while end < tail.len() && tail[end] != quote {
                    if tail[end] == b'\\' && end + 1 < tail.len() {
                        end += 1;
                    }
                    end += 1;
                }
                let label = if quote == b'\'' { "single" } else { "double" };
                let content = &tail[1..end];
                // Do not make a short token longer just to attach an ellipsis.
                let truncate = content.len() > 33;
                let mut display =
                    decode_php_source(if truncate { &content[..30] } else { content });
                if truncate {
                    display.push_str("...");
                }
                format!("{label}-quoted string \"{display}\"")
            }
            Some(byte) if byte.is_ascii_digit() => {
                let end = tail
                    .iter()
                    .position(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
                    .unwrap_or(tail.len());
                format!("integer \"{}\"", decode_php_source(&tail[..end]))
            }
            Some(byte) if byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80 => {
                let end = tail
                    .iter()
                    .position(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_' && *byte < 0x80)
                    .unwrap_or(tail.len());
                format!("identifier \"{}\"", decode_php_source(&tail[..end]))
            }
            Some(_) => {
                let width = if matches!(
                    tail.get(..2),
                    Some(b"->" | b"::" | b"=>" | b"??" | b"++" | b"--")
                ) {
                    2
                } else {
                    1
                };
                format!("token \"{}\"", decode_php_source(&tail[..width]))
            }
        };
        Token::ParseError(
            format!(
                "syntax error, unexpected {description}, expecting variable or \"{{\" or \"$\""
            ),
            line,
        )
    }
}
