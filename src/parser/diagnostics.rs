use super::{Parser, Token};

// Error construction lives outside the grammar module's warmed code. The
// SAFETY: the Linux section is executable compiler-generated code, with no custom ABI,
// symbol identity, initialization order, or address-based behavior.
impl Parser {
    /// Reservation is case-insensitive, unlike the spelling of a member name.
    /// This is used only where an identifier is not a relaxed PHP member name.
    #[cold]
    #[inline(never)]
    pub(super) fn reserved_identifier(name: &str) -> Option<&'static str> {
        const WORDS: &[&str] = &[
            "abstract",
            "and",
            "array",
            "as",
            "break",
            "case",
            "catch",
            "class",
            "clone",
            "const",
            "continue",
            "declare",
            "default",
            "die",
            "do",
            "echo",
            "else",
            "elseif",
            "empty",
            "enddeclare",
            "endfor",
            "endforeach",
            "endif",
            "endswitch",
            "endwhile",
            "eval",
            "exit",
            "extends",
            "final",
            "finally",
            "fn",
            "for",
            "foreach",
            "function",
            "global",
            "goto",
            "if",
            "implements",
            "include",
            "include_once",
            "instanceof",
            "insteadof",
            "interface",
            "isset",
            "list",
            "match",
            "namespace",
            "new",
            "or",
            "print",
            "private",
            "protected",
            "public",
            "require",
            "require_once",
            "return",
            "static",
            "switch",
            "throw",
            "trait",
            "try",
            "unset",
            "use",
            "var",
            "while",
            "xor",
            "yield",
        ];
        WORDS
            .binary_search_by(|word| {
                word.bytes()
                    .cmp(name.bytes().map(|byte| byte.to_ascii_lowercase()))
            })
            .ok()
            .map(|index| WORDS[index])
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
    fn token_error_description(token: &Token) -> String {
        match token {
            Token::Identifier(name, _) => {
                if let Some(keyword) = Self::reserved_identifier(name) {
                    format!("token \"{keyword}\"")
                } else {
                    format!("identifier \"{name}\"")
                }
            }
            Token::Integer(value) => format!("integer \"{value}\""),
            Token::Float(value) => format!("floating-point number \"{value}\""),
            Token::Variable(name, _) => format!("variable \"${name}\""),
            Token::This(_) => "variable \"$this\"".into(),
            Token::Eof => "end of file".into(),
            token => {
                if let Some(symbol) = Self::diagnostic_token_spelling(token) {
                    format!("token \"{symbol}\"")
                } else if let Some(name) = Self::token_as_named_arg_label(token) {
                    format!("token \"{name}\"")
                } else {
                    format!("token {token:?}")
                }
            }
        }
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
    fn diagnostic_token_spelling(token: &Token) -> Option<&'static str> {
        Some(match token {
            Token::LParen(_) => "(",
            Token::RParen => ")",
            Token::LBracket(_) => "[",
            Token::RBracket => "]",
            Token::LBrace(_) => "{",
            Token::RBrace => "}",
            Token::Semicolon(_) => ";",
            Token::Comma(_) => ",",
            Token::Colon => ":",
            Token::DoubleColon => "::",
            Token::Backslash => "\\",
            Token::Arrow => "->",
            Token::NullSafe => "?->",
            Token::DoubleArrow => "=>",
            Token::Assign => "=",
            Token::Slash => "/",
            Token::Plus => "+",
            Token::Minus => "-",
            Token::Star => "*",
            Token::Percent(_) => "%",
            Token::Dot => ".",
            Token::Ampersand(_) => "&",
            Token::Pipe => "|",
            Token::Bang => "!",
            Token::Question => "?",
            Token::AttributeStart(_) => "#[",
            Token::Dollar(_) => "$",
            _ => return None,
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
    pub(super) fn unexpected_token_error(
        &self,
        token: &Token,
        expected: &str,
        line: usize,
    ) -> String {
        if let Token::ParseError(message, line) = token {
            return self.source_error(message, *line);
        }
        let description = Self::token_error_description(token);
        self.source_error(
            &format!("syntax error, unexpected {description}, expecting {expected}"),
            line,
        )
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
    pub(super) fn expected_token_error(
        &self,
        token: &Token,
        expected: &Token,
        line: usize,
    ) -> String {
        // Source-less embedders have an existing structural/debug error API.
        // PHP source units supply source_name and take the diagnostic path.
        if self.source_name.is_none()
            && !matches!(token, Token::ParseError(..))
            && !(matches!(token, Token::LBrace(_)) && matches!(expected, Token::RParen))
        {
            let expected = match expected {
                Token::LParen(_) => "LParen".to_string(),
                Token::LBracket(_) => "LBracket".to_string(),
                Token::LBrace(_) => "LBrace".to_string(),
                Token::Semicolon(_) => "Semicolon".to_string(),
                token => format!("{token:?}"),
            };
            let actual = match token {
                Token::LBrace(_) => "LBrace".to_string(),
                token => format!("{token:?}"),
            };
            return format!("Expected {expected}, got {actual}");
        }
        let expected = Self::diagnostic_token_spelling(expected)
            .map(|spelling| format!("\"{spelling}\""))
            .unwrap_or_else(|| format!("{expected:?}"));
        self.unexpected_token_error(token, &expected, line)
    }

    /// A list may end after a comma, but a statement keyword cannot start its
    /// next element. Retain the list's closer as the grammar expectation.
    #[cold]
    #[inline(never)]
    pub(super) fn is_statement_only_keyword(token: &Token) -> bool {
        let name = match token {
            Token::Identifier(name, _) => Self::reserved_identifier(name),
            token => {
                return matches!(
                    token,
                    Token::If
                        | Token::Else
                        | Token::ElseIf
                        | Token::While
                        | Token::Do
                        | Token::For
                        | Token::Foreach { .. }
                        | Token::As(_)
                        | Token::Switch
                        | Token::Case(_)
                        | Token::Default(_)
                        | Token::EndIf
                        | Token::EndWhile
                        | Token::EndFor
                        | Token::EndForeach
                        | Token::EndSwitch
                        | Token::Break { .. }
                        | Token::Continue { .. }
                        | Token::Try
                        | Token::Catch
                        | Token::Finally
                        | Token::Return { .. }
                        | Token::Echo { .. }
                        | Token::Const
                        | Token::Global
                        | Token::Use(_)
                        | Token::Declare
                        | Token::Interface
                        | Token::Trait
                        | Token::Extends
                        | Token::Implements
                        | Token::Public
                        | Token::Protected
                        | Token::Private
                        | Token::Abstract(_)
                        | Token::Final(_)
                        | Token::Goto { .. }
                );
            }
        };
        matches!(
            name,
            Some(
                "if" | "else"
                    | "elseif"
                    | "while"
                    | "do"
                    | "for"
                    | "foreach"
                    | "as"
                    | "switch"
                    | "case"
                    | "default"
                    | "endif"
                    | "endwhile"
                    | "endfor"
                    | "endforeach"
                    | "endswitch"
                    | "break"
                    | "continue"
                    | "try"
                    | "catch"
                    | "finally"
                    | "return"
                    | "echo"
                    | "const"
                    | "global"
                    | "use"
                    | "declare"
                    | "interface"
                    | "trait"
                    | "extends"
                    | "implements"
                    | "public"
                    | "protected"
                    | "private"
                    | "abstract"
                    | "final"
                    | "goto"
            )
        )
    }
}
