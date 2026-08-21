/// Minimal PHP lexer — just enough tokens for the vertical slice.
mod strings;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    OpenTag, // <?php
    // Keywords
    Echo {
        line: usize,
    }, // echo
    Function(usize), // function with source line
    Return {
        line: usize,
    }, // return
    If,              // if
    Else,            // else
    While,           // while
    For,             // for
    ElseIf,          // elseif
    Do,              // do
    Break {
        line: usize,
    }, // break
    Continue {
        line: usize,
    }, // continue
    Switch,          // switch
    Case,            // case
    Default,         // default
    Null,            // null
    True,            // true
    False,           // false
    ArrayKw,         // array
    Foreach {
        line: usize,
    }, // foreach
    As,              // as
    Insteadof,       // insteadof (trait precedence adaptation)
    Isset,           // isset
    Empty,           // empty
    Unset,           // unset
    Match(usize),    // match with source line
    Try,             // try
    Catch,           // catch
    Finally,         // finally
    Throw(u32),      // throw with source line
    Class,           // class
    New(u32),        // new with source line
    Public,          // public
    Protected,       // protected
    Private,         // private
    This(usize),     // $this with source line (handled as special variable)
    Extends,         // extends
    Static,          // static
    Instanceof,      // instanceof
    Const,           // const
    Interface,       // interface
    Trait,           // trait
    Implements,      // implements
    Abstract,        // abstract
    Declare,         // declare
    Final,           // final
    Enum,            // enum
    Namespace,       // namespace
    Backslash,       // \ (namespace separator)
    Yield(usize),    // yield, source line
    From,            // from (used after yield)
    Print,           // print
    Global,          // global
    Clone(usize),    // clone, source line
    /// Start of a PHP attribute group (`#[`) together with its source line.
    AttributeStart(usize),
    Include,     // include
    IncludeOnce, // include_once
    Require,     // require
    RequireOnce, // require_once
    Goto {
        /// Preserve the lexeme because `goto` is also legal as a contextual
        /// identifier whose spelling can remain observably case-sensitive.
        name: String,
        line: usize,
    },
    // Literals
    Integer(i64),          // 42, -1
    Float(f64),            // 3.14, 1.5e10
    StringLiteral(String), // "hello", 'world'
    /// One indirect-variable sigil. Ordinary `$name` remains a single
    /// `Variable` token; `$$name` and `${expr}` retain the leading sigil so
    /// the parser can build PHP's right-associated variable-variable tree.
    Dollar(usize),
    Variable(String, usize),   // $a, $foo with source line
    Identifier(String, usize), // identifier with source line
    MagicConstant {
        name: String,
        line: usize,
    }, // __FILE__, __LINE__, ...
    // Operators
    Assign,                 // =
    Plus,                   // +
    Minus,                  // -
    Star,                   // *
    Slash,                  // /
    Percent,                // %
    Dot,                    // . (concat)
    PlusPlus,               // ++
    MinusMinus,             // --
    EqualEqual,             // ==
    IdenticalEqual,         // ===
    NotEqual,               // !=
    NotIdentical,           // !==
    Less,                   // <
    LessEqual,              // <=
    Greater,                // >
    GreaterEqual,           // >=
    AmpAmp,                 // &&
    PipePipe,               // ||
    LogicalAnd,             // and
    LogicalOr,              // or
    PipeGreater(usize),     // |> (PHP 8.5 pipe operator)
    LogicalXor,             // xor
    Bang,                   // !
    PlusAssign,             // +=
    MinusAssign,            // -=
    StarAssign,             // *=
    StarStarAssign,         // **=
    SlashAssign,            // /=
    PercentAssign,          // %=
    DotAssign,              // .=
    AmpAssign,              // &=
    PipeAssign,             // |=
    CaretAssign,            // ^=
    ShiftLeftAssign,        // <<=
    ShiftRightAssign,       // >>=
    Question,               // ?
    QuestionQuestion,       // ??
    QuestionQuestionAssign, // ??=
    At,                     // @ (error-control operator)
    Colon,                  // :
    // Punctuation
    Semicolon,        // ;
    LParen(usize),    // ( with source line
    RParen,           // )
    LBrace,           // {
    RBrace,           // }
    Comma,            // ,
    LBracket(usize),  // [ with source line
    RBracket,         // ]
    DoubleArrow,      // =>
    Arrow,            // ->
    NullSafe,         // ?->
    DoubleColon,      // ::
    Fn(usize),        // fn (arrow functions), with source line
    Use(usize),       // use (imports and closure capture), with source line
    Pipe,             // | (bitwise or, multi-catch separator)
    Ampersand,        // & (bitwise and, reference)
    Caret,            // ^ (bitwise xor)
    Tilde,            // ~ (bitwise not)
    StarStar,         // ** (power)
    Spaceship,        // <=> (spaceship)
    ShiftLeft,        // <<
    ShiftRight,       // >>
    DotDotDot(usize), // ... (variadic / spread) with source line
    CompileError(String, usize),
    CompileWarning(String, usize),
    CompileDeprecation(String, usize),
    ParseError(String, usize),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeferredCompileDiagnosticKind {
    Warning,
    Deprecation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeferredCompileDiagnostic {
    kind: DeferredCompileDiagnosticKind,
    message: String,
    line: usize,
}

enum StringPart {
    Literal(String),
    Variable(String, usize),
    PropertyAccess(String, String, bool, usize), // var_name, property_name, nullsafe, source line
    ArrayAccess(String, String, usize), // var_name, index (string or integer literal), line
    DynamicArrayAccess(String, String, usize), // var_name, index variable, line
    Expression(Vec<Token>),
    DynamicVariable(Vec<Token>, usize),
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    deferred_compile_errors: Vec<(String, usize)>,
    deferred_compile_diagnostics: Vec<DeferredCompileDiagnostic>,
}

/// PHP source is byte-oriented and may legally contain non-UTF-8 bytes in
/// identifiers and string literals. Preserve ordinary UTF-8 while mapping an
/// isolated legacy byte to the Unicode code point with the same value. This
/// gives the String-based frontend a deterministic representation and keeps a
/// raw identifier and the same raw class-map key equal.
pub fn decode_php_source(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len());
    let mut remaining = bytes;

    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                output.push_str(valid);
                break;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to > 0 {
                    output.push_str(std::str::from_utf8(&remaining[..valid_up_to]).unwrap());
                    remaining = &remaining[valid_up_to..];
                }

                let invalid_length = error.error_len().unwrap_or(remaining.len());
                for byte in &remaining[..invalid_length] {
                    output.push(char::from(*byte));
                }
                remaining = &remaining[invalid_length..];
            }
        }
    }

    output
}

fn decode_numeric_literal(bytes: &[u8]) -> String {
    bytes
        .iter()
        .copied()
        .filter(|byte| *byte != b'_')
        .map(char::from)
        .collect()
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            src: source.as_bytes(),
            pos: 0,
            deferred_compile_errors: Vec::new(),
            deferred_compile_diagnostics: Vec::new(),
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();

        self.skip_whitespace()?;

        // Expect <?php opening tag
        if self.starts_with(b"<?php") {
            self.pos += 5;
            tokens.push(Token::OpenTag);
        } else {
            return Err("Expected <?php opening tag".into());
        }

        loop {
            self.skip_whitespace()?;

            if self.pos >= self.src.len() {
                tokens.extend(
                    self.deferred_compile_diagnostics
                        .drain(..)
                        .map(|diagnostic| match diagnostic.kind {
                            DeferredCompileDiagnosticKind::Warning => {
                                Token::CompileWarning(diagnostic.message, diagnostic.line)
                            }
                            DeferredCompileDiagnosticKind::Deprecation => {
                                Token::CompileDeprecation(diagnostic.message, diagnostic.line)
                            }
                        }),
                );
                tokens.extend(
                    self.deferred_compile_errors
                        .drain(..)
                        .map(|(message, line)| Token::CompileError(message, line)),
                );
                tokens.push(Token::Eof);
                break;
            }

            let ch = self.src[self.pos];

            match ch {
                b'#' if self.peek_next() == Some(b'[') => {
                    let line = 1 + self.src[..self.pos]
                        .iter()
                        .filter(|byte| **byte == b'\n')
                        .count();
                    tokens.push(Token::AttributeStart(line));
                    self.pos += 2;
                }
                b'=' => {
                    if self.peek_next() == Some(b'=') {
                        if self.src.get(self.pos + 2) == Some(&b'=') {
                            tokens.push(Token::IdenticalEqual);
                            self.pos += 3;
                        } else {
                            tokens.push(Token::EqualEqual);
                            self.pos += 2;
                        }
                    } else if self.peek_next() == Some(b'>') {
                        tokens.push(Token::DoubleArrow);
                        self.pos += 2;
                    } else {
                        tokens.push(Token::Assign);
                        self.pos += 1;
                    }
                }
                b'!' => {
                    if self.peek_next() == Some(b'=') {
                        if self.src.get(self.pos + 2) == Some(&b'=') {
                            tokens.push(Token::NotIdentical);
                            self.pos += 3;
                        } else {
                            tokens.push(Token::NotEqual);
                            self.pos += 2;
                        }
                    } else {
                        tokens.push(Token::Bang);
                        self.pos += 1;
                    }
                }
                b'<' => {
                    if self.starts_with(b"<<<") {
                        if Self::is_value_token(tokens.last()) {
                            let line = self.source_line_at(self.pos);
                            let display = self.document_start_display();
                            tokens.push(Token::ParseError(
                                format!("syntax error, unexpected heredoc start \"{display}\""),
                                line,
                            ));
                            self.pos = self.src.len();
                            continue;
                        }
                        match self.read_document_string() {
                            Ok(interpolated) => {
                                self.deferred_compile_diagnostics
                                    .extend(interpolated.diagnostics);
                                Self::emit_string_parts(&mut tokens, &interpolated.parts);
                            }
                            Err(error) => {
                                tokens.push(Token::ParseError(error.message, error.line));
                                self.pos = self.src.len();
                            }
                        }
                    } else if self.peek_next() == Some(b'=') {
                        if self.src.get(self.pos + 2) == Some(&b'>') {
                            tokens.push(Token::Spaceship);
                            self.pos += 3;
                        } else {
                            tokens.push(Token::LessEqual);
                            self.pos += 2;
                        }
                    } else if self.peek_next() == Some(b'<') {
                        if self.src.get(self.pos + 2) == Some(&b'=') {
                            tokens.push(Token::ShiftLeftAssign);
                            self.pos += 3;
                        } else {
                            tokens.push(Token::ShiftLeft);
                            self.pos += 2;
                        }
                    } else {
                        tokens.push(Token::Less);
                        self.pos += 1;
                    }
                }
                b'>' => {
                    if self.peek_next() == Some(b'>') {
                        if self.src.get(self.pos + 2) == Some(&b'=') {
                            tokens.push(Token::ShiftRightAssign);
                            self.pos += 3;
                        } else {
                            tokens.push(Token::ShiftRight);
                            self.pos += 2;
                        }
                    } else if self.peek_next() == Some(b'=') {
                        tokens.push(Token::GreaterEqual);
                        self.pos += 2;
                    } else {
                        tokens.push(Token::Greater);
                        self.pos += 1;
                    }
                }
                b'+' => {
                    if self.peek_next() == Some(b'+') {
                        tokens.push(Token::PlusPlus);
                        self.pos += 2;
                    } else if self.peek_next() == Some(b'=') {
                        tokens.push(Token::PlusAssign);
                        self.pos += 2;
                    } else {
                        tokens.push(Token::Plus);
                        self.pos += 1;
                    }
                }
                b'*' => {
                    if self.peek_next() == Some(b'*') {
                        if self.src.get(self.pos + 2) == Some(&b'=') {
                            tokens.push(Token::StarStarAssign);
                            self.pos += 3;
                        } else {
                            tokens.push(Token::StarStar);
                            self.pos += 2;
                        }
                    } else if self.peek_next() == Some(b'=') {
                        tokens.push(Token::StarAssign);
                        self.pos += 2;
                    } else {
                        tokens.push(Token::Star);
                        self.pos += 1;
                    }
                }
                b'/' => {
                    if self.peek_next() == Some(b'=') {
                        tokens.push(Token::SlashAssign);
                        self.pos += 2;
                    } else {
                        tokens.push(Token::Slash);
                        self.pos += 1;
                    }
                }
                b'%' => {
                    if self.peek_next() == Some(b'=') {
                        tokens.push(Token::PercentAssign);
                        self.pos += 2;
                    } else {
                        tokens.push(Token::Percent);
                        self.pos += 1;
                    }
                }
                b'.' => {
                    if self.peek_next() == Some(b'.') && self.src.get(self.pos + 2) == Some(&b'.') {
                        let line = 1 + self.src[..self.pos]
                            .iter()
                            .filter(|&&byte| byte == b'\n')
                            .count();
                        tokens.push(Token::DotDotDot(line));
                        self.pos += 3;
                    } else if self.peek_next() == Some(b'=') {
                        tokens.push(Token::DotAssign);
                        self.pos += 2;
                    } else {
                        tokens.push(Token::Dot);
                        self.pos += 1;
                    }
                }
                b'&' => {
                    if self.peek_next() == Some(b'&') {
                        tokens.push(Token::AmpAmp);
                        self.pos += 2;
                    } else if self.peek_next() == Some(b'=') {
                        tokens.push(Token::AmpAssign);
                        self.pos += 2;
                    } else {
                        // Single & — bitwise and / reference
                        tokens.push(Token::Ampersand);
                        self.pos += 1;
                    }
                }
                b'@' => {
                    tokens.push(Token::At);
                    self.pos += 1;
                }
                b'|' => {
                    if self.peek_next() == Some(b'>') {
                        let line = 1 + self.src[..self.pos]
                            .iter()
                            .filter(|&&byte| byte == b'\n')
                            .count();
                        tokens.push(Token::PipeGreater(line));
                        self.pos += 2;
                    } else if self.peek_next() == Some(b'|') {
                        tokens.push(Token::PipePipe);
                        self.pos += 2;
                    } else if self.peek_next() == Some(b'=') {
                        tokens.push(Token::PipeAssign);
                        self.pos += 2;
                    } else {
                        tokens.push(Token::Pipe);
                        self.pos += 1;
                    }
                }
                b'\'' => {
                    let s = self.read_string(b'\'')?;
                    tokens.push(Token::StringLiteral(s));
                }
                b'"' => match self.read_double_quoted_string() {
                    Ok(interpolated) => {
                        self.deferred_compile_diagnostics
                            .extend(interpolated.diagnostics);
                        Self::emit_string_parts(&mut tokens, &interpolated.parts);
                    }
                    Err(error) => {
                        tokens.push(Token::ParseError(error.message, error.line));
                        self.pos = self.src.len();
                    }
                },
                b'-' => {
                    if self.peek_next() == Some(b'>') {
                        tokens.push(Token::Arrow);
                        self.pos += 2;
                    } else if self.peek_next() == Some(b'-') {
                        tokens.push(Token::MinusMinus);
                        self.pos += 2;
                    } else if self.peek_next() == Some(b'=') {
                        tokens.push(Token::MinusAssign);
                        self.pos += 2;
                    } else if self.pos + 1 < self.src.len()
                        && self.src[self.pos + 1].is_ascii_digit()
                        && !Self::is_value_token(tokens.last())
                        && !Self::is_loop_control_operand_sign(&tokens)
                    {
                        self.pos += 1;
                        match self.read_number()? {
                            Token::Integer(n) => tokens.push(Token::Integer(-n)),
                            Token::Float(f) => tokens.push(Token::Float(-f)),
                            _ => unreachable!(),
                        }
                    } else {
                        tokens.push(Token::Minus);
                        self.pos += 1;
                    }
                }
                b';' => {
                    tokens.push(Token::Semicolon);
                    self.pos += 1;
                }
                b'(' => {
                    let line = 1 + self.src[..self.pos]
                        .iter()
                        .filter(|&&byte| byte == b'\n')
                        .count();
                    tokens.push(Token::LParen(line));
                    self.pos += 1;
                }
                b')' => {
                    tokens.push(Token::RParen);
                    self.pos += 1;
                }
                b'{' => {
                    tokens.push(Token::LBrace);
                    self.pos += 1;
                }
                b'}' => {
                    tokens.push(Token::RBrace);
                    self.pos += 1;
                }
                b',' => {
                    tokens.push(Token::Comma);
                    self.pos += 1;
                }
                b'[' => {
                    let line = 1 + self.src[..self.pos]
                        .iter()
                        .filter(|byte| **byte == b'\n')
                        .count();
                    tokens.push(Token::LBracket(line));
                    self.pos += 1;
                }
                b']' => {
                    tokens.push(Token::RBracket);
                    self.pos += 1;
                }
                b'?' => {
                    if self.peek_next() == Some(b'>') {
                        self.pos += 2;
                        self.finish_php_segment(&mut tokens)?;
                    } else if self.peek_next() == Some(b'?') {
                        if self.src.get(self.pos + 2) == Some(&b'=') {
                            tokens.push(Token::QuestionQuestionAssign);
                            self.pos += 3;
                        } else {
                            tokens.push(Token::QuestionQuestion);
                            self.pos += 2;
                        }
                    } else if self.peek_next() == Some(b'-')
                        && self.src.get(self.pos + 2) == Some(&b'>')
                    {
                        tokens.push(Token::NullSafe);
                        self.pos += 3;
                    } else {
                        tokens.push(Token::Question);
                        self.pos += 1;
                    }
                }
                b':' => {
                    if self.peek_next() == Some(b':') {
                        tokens.push(Token::DoubleColon);
                        self.pos += 2;
                    } else {
                        tokens.push(Token::Colon);
                        self.pos += 1;
                    }
                }
                b'$' => {
                    let variable_start = self.pos;
                    self.pos += 1;
                    let line = 1 + self.src[..variable_start]
                        .iter()
                        .filter(|byte| **byte == b'\n')
                        .count();
                    if matches!(self.src.get(self.pos), Some(b'$' | b'{')) {
                        tokens.push(Token::Dollar(line));
                        continue;
                    }
                    let name = self.read_identifier();
                    if name.is_empty() {
                        return Err("Expected variable name after $".into());
                    }
                    if name == "this" {
                        tokens.push(Token::This(line));
                    } else {
                        tokens.push(Token::Variable(name, line));
                    }
                }
                b'0'..=b'9' => {
                    tokens.push(self.read_number()?);
                }
                b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'\x80'..=b'\xff' => {
                    let identifier_start = self.pos;
                    let ident = self.read_identifier();
                    let line = 1 + self.src[..identifier_start]
                        .iter()
                        .filter(|byte| **byte == b'\n')
                        .count();
                    let is_member_name = matches!(
                        tokens.last(),
                        Some(
                            Token::Backslash | Token::DoubleColon | Token::Arrow | Token::NullSafe
                        )
                    );
                    if !is_member_name
                        && ident.eq_ignore_ascii_case("b")
                        && self.starts_with(b"<<<")
                    {
                        continue;
                    }
                    if !is_member_name
                        && matches!(
                            ident.to_ascii_uppercase().as_str(),
                            "__LINE__"
                                | "__FILE__"
                                | "__DIR__"
                                | "__FUNCTION__"
                                | "__METHOD__"
                                | "__CLASS__"
                                | "__TRAIT__"
                                | "__PROPERTY__"
                                | "__NAMESPACE__"
                        )
                    {
                        tokens.push(Token::MagicConstant { name: ident, line });
                        continue;
                    }
                    if !is_member_name && ident.eq_ignore_ascii_case("goto") {
                        tokens.push(Token::Goto { name: ident, line });
                        continue;
                    }
                    if !is_member_name && ident.eq_ignore_ascii_case("array") {
                        let next_non_whitespace = self.src[self.pos..]
                            .iter()
                            .copied()
                            .find(|byte| !byte.is_ascii_whitespace());
                        if next_non_whitespace == Some(b'(') {
                            tokens.push(Token::ArrayKw);
                            continue;
                        }
                    }
                    match ident.as_str() {
                        "echo" => {
                            tokens.push(Token::Echo { line });
                        }
                        "function" => tokens.push(Token::Function(line)),
                        "return" => tokens.push(Token::Return { line }),
                        "if" => tokens.push(Token::If),
                        "else" => tokens.push(Token::Else),
                        "elseif" => tokens.push(Token::ElseIf),
                        "while" => tokens.push(Token::While),
                        "for" => tokens.push(Token::For),
                        "do" => tokens.push(Token::Do),
                        "break" => tokens.push(Token::Break { line }),
                        "continue" => tokens.push(Token::Continue { line }),
                        "switch" => tokens.push(Token::Switch),
                        "case" => tokens.push(Token::Case),
                        "default" => tokens.push(Token::Default),
                        "null" | "NULL" => tokens.push(Token::Null),
                        "true" | "TRUE" => tokens.push(Token::True),
                        "false" | "FALSE" => tokens.push(Token::False),
                        "array" => tokens.push(Token::ArrayKw),
                        "foreach" => tokens.push(Token::Foreach { line }),
                        "as" => tokens.push(Token::As),
                        "insteadof" => tokens.push(Token::Insteadof),
                        "isset" => tokens.push(Token::Isset),
                        "empty" => tokens.push(Token::Empty),
                        "unset" => tokens.push(Token::Unset),
                        "match" => tokens.push(Token::Match(line)),
                        "try" => tokens.push(Token::Try),
                        "catch" => tokens.push(Token::Catch),
                        "finally" => tokens.push(Token::Finally),
                        "throw" => {
                            tokens.push(Token::Throw(u32::try_from(line).unwrap_or(u32::MAX)));
                        }
                        "class" => tokens.push(Token::Class),
                        "new" => {
                            tokens.push(Token::New(u32::try_from(line).unwrap_or(u32::MAX)));
                        }
                        "public" => tokens.push(Token::Public),
                        "protected" => tokens.push(Token::Protected),
                        "private" => tokens.push(Token::Private),
                        "extends" => tokens.push(Token::Extends),
                        "static" => tokens.push(Token::Static),
                        "instanceof" => tokens.push(Token::Instanceof),
                        "const" => tokens.push(Token::Const),
                        "interface" => tokens.push(Token::Interface),
                        "trait" => tokens.push(Token::Trait),
                        "implements" => tokens.push(Token::Implements),
                        "abstract" => tokens.push(Token::Abstract),
                        "final" => tokens.push(Token::Final),
                        "enum" => tokens.push(Token::Enum),
                        "declare" => tokens.push(Token::Declare),
                        "namespace" => tokens.push(Token::Namespace),
                        "yield" => tokens.push(Token::Yield(line)),
                        "from" => tokens.push(Token::From),
                        "fn" => tokens.push(Token::Fn(line)),
                        "use" => tokens.push(Token::Use(line)),
                        "print" => tokens.push(Token::Print),
                        "global" => tokens.push(Token::Global),
                        "clone" => tokens.push(Token::Clone(line)),
                        "include" => tokens.push(Token::Include),
                        "include_once" => tokens.push(Token::IncludeOnce),
                        "require" => tokens.push(Token::Require),
                        "require_once" => tokens.push(Token::RequireOnce),
                        "and" => tokens.push(Token::LogicalAnd),
                        "or" => tokens.push(Token::LogicalOr),
                        "xor" => tokens.push(Token::LogicalXor),
                        _ => tokens.push(Token::Identifier(ident, line)),
                    }
                }
                b'^' => {
                    if self.peek_next() == Some(b'=') {
                        tokens.push(Token::CaretAssign);
                        self.pos += 2;
                    } else {
                        tokens.push(Token::Caret);
                        self.pos += 1;
                    }
                }
                b'~' => {
                    tokens.push(Token::Tilde);
                    self.pos += 1;
                }
                b'\\' => {
                    tokens.push(Token::Backslash);
                    self.pos += 1;
                }
                _ => {
                    return Err(format!(
                        "Unexpected character '{}' at position {}",
                        ch as char, self.pos
                    ));
                }
            }
        }

        Ok(tokens)
    }

    fn skip_whitespace(&mut self) -> Result<(), String> {
        loop {
            // Skip whitespace
            while self.pos < self.src.len() && self.src[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            if self.pos >= self.src.len() {
                break;
            }
            // Attribute groups are ordinary syntax. Leave `#[` for the main
            // token loop while retaining `#` as PHP's line-comment marker.
            if self.starts_with(b"#[") {
                break;
            }
            // Skip // and # line comments
            if self.starts_with(b"//") || self.src[self.pos] == b'#' {
                while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            // Skip /* */ block comments
            if self.starts_with(b"/*") {
                let start = self.pos;
                self.pos += 2;
                loop {
                    if self.pos + 1 >= self.src.len() {
                        return Err(format!(
                            "Unterminated comment starting at position {}",
                            start
                        ));
                    }
                    if self.src[self.pos] == b'*' && self.src[self.pos + 1] == b'/' {
                        self.pos += 2;
                        break;
                    }
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
        Ok(())
    }

    fn starts_with(&self, prefix: &[u8]) -> bool {
        self.src[self.pos..].starts_with(prefix)
    }

    fn peek_next(&self) -> Option<u8> {
        self.src.get(self.pos + 1).copied()
    }

    fn read_number(&mut self) -> Result<Token, String> {
        let start = self.pos;
        // Check for 0x (hex), 0b (binary), 0o (octal) prefixes
        if self.src[self.pos] == b'0' && self.pos + 1 < self.src.len() {
            match self.src[self.pos + 1] {
                b'x' | b'X'
                    if self
                        .src
                        .get(self.pos + 2)
                        .is_some_and(u8::is_ascii_hexdigit) =>
                {
                    self.pos += 2; // skip 0x
                    let hex_start = self.pos;
                    self.consume_numeric_digits(|byte| byte.is_ascii_hexdigit());
                    let s = decode_numeric_literal(&self.src[hex_start..self.pos]);
                    let n = i64::from_str_radix(&s, 16)
                        .map_err(|_| format!("Invalid hex literal at position {}", start))?;
                    return Ok(Token::Integer(n));
                }
                b'b' | b'B'
                    if self
                        .src
                        .get(self.pos + 2)
                        .is_some_and(|byte| matches!(byte, b'0' | b'1')) =>
                {
                    self.pos += 2; // skip 0b
                    let bin_start = self.pos;
                    self.consume_numeric_digits(|byte| matches!(byte, b'0' | b'1'));
                    let s = decode_numeric_literal(&self.src[bin_start..self.pos]);
                    let n = i64::from_str_radix(&s, 2)
                        .map_err(|_| format!("Invalid binary literal at position {}", start))?;
                    return Ok(Token::Integer(n));
                }
                b'o' | b'O'
                    if self
                        .src
                        .get(self.pos + 2)
                        .is_some_and(|byte| matches!(byte, b'0'..=b'7')) =>
                {
                    self.pos += 2; // skip 0o
                    let octal_start = self.pos;
                    self.consume_numeric_digits(|byte| matches!(byte, b'0'..=b'7'));
                    let literal = decode_numeric_literal(&self.src[octal_start..self.pos]);
                    let number = i64::from_str_radix(&literal, 8)
                        .map_err(|_| format!("Invalid octal literal at position {}", start))?;
                    return Ok(Token::Integer(number));
                }
                _ => {}
            }
        }
        self.consume_numeric_digits(|byte| byte.is_ascii_digit());
        let mut is_float = false;
        // PHP permits an empty fractional part, so both `1.5` and `1.` are
        // floating-point literals.
        if self.pos < self.src.len() && self.src[self.pos] == b'.' {
            is_float = true;
            self.pos += 1; // skip '.'
            self.consume_numeric_digits(|byte| byte.is_ascii_digit());
        }
        // Scientific notation is valid with or without a decimal point.
        if self.pos < self.src.len() && matches!(self.src[self.pos], b'e' | b'E') {
            let mut exponent = self.pos + 1;
            if matches!(self.src.get(exponent), Some(b'+' | b'-')) {
                exponent += 1;
            }
            if self.src.get(exponent).is_some_and(u8::is_ascii_digit) {
                is_float = true;
                self.pos = exponent + 1;
                self.consume_numeric_digits(|byte| byte.is_ascii_digit());
            }
        }
        if is_float {
            let s = decode_numeric_literal(&self.src[start..self.pos]);
            let f: f64 = s
                .parse()
                .map_err(|_| format!("Invalid float literal at position {}", start))?;
            Ok(Token::Float(f))
        } else {
            let s = decode_numeric_literal(&self.src[start..self.pos]);
            let n: i64 = s
                .parse()
                .map_err(|_| format!("Integer literal too large at position {}", start))?;
            Ok(Token::Integer(n))
        }
    }

    fn consume_numeric_digits(&mut self, valid: impl Fn(u8) -> bool) {
        while self.pos < self.src.len() {
            if valid(self.src[self.pos]) {
                self.pos += 1;
            } else if self.src[self.pos] == b'_'
                && self.pos > 0
                && valid(self.src[self.pos - 1])
                && self.src.get(self.pos + 1).is_some_and(|byte| valid(*byte))
            {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn read_identifier(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.src.len()
            && (self.src[self.pos].is_ascii_alphanumeric()
                || self.src[self.pos] == b'_'
                || self.src[self.pos] >= b'\x80')
        {
            self.pos += 1;
        }
        decode_php_source(&self.src[start..self.pos])
    }

    fn is_value_token(tok: Option<&Token>) -> bool {
        matches!(
            tok,
            Some(
                Token::Integer(_)
                    | Token::Float(_)
                    | Token::Variable(_, _)
                    | Token::This(_)
                    | Token::StringLiteral(_)
                    | Token::RParen
                    | Token::RBracket
                    | Token::Identifier(_, _)
                    | Token::MagicConstant { .. }
                    | Token::True
                    | Token::False
                    | Token::Null
            )
        )
    }

    /// A sign is part of the historical break/continue operand grammar, not
    /// part of its integer literal. Preserve it for the parser even though
    /// ordinary negative literals use the lexer's compact signed token.
    fn is_loop_control_operand_sign(tokens: &[Token]) -> bool {
        matches!(
            tokens
                .iter()
                .rev()
                .skip_while(|token| matches!(token, Token::LParen(_)))
                .next(),
            Some(Token::Break { .. } | Token::Continue { .. })
        )
    }

    /// Close the current PHP segment, emit intervening inline HTML as an echo,
    /// and resume lexing at a later long opening tag. A closing tag also ends
    /// the current statement, so supply the semicolon when source omitted it.
    /// PHP consumes the first line break immediately after `?>`.
    fn finish_php_segment(&mut self, tokens: &mut Vec<Token>) -> Result<(), String> {
        if !matches!(
            tokens.last(),
            Some(Token::Semicolon | Token::LBrace | Token::RBrace)
        ) {
            tokens.push(Token::Semicolon);
        }

        let mut inline_start = self.pos;
        if self.src.get(inline_start..inline_start + 2) == Some(b"\r\n") {
            inline_start += 2;
        } else if self.src.get(inline_start) == Some(&b'\n') {
            inline_start += 1;
        }

        let next_open = self.src[inline_start..]
            .windows(5)
            .position(|window| window == b"<?php")
            .map(|offset| inline_start + offset);
        let inline_end = next_open.unwrap_or(self.src.len());
        if inline_end > inline_start {
            let inline = std::str::from_utf8(&self.src[inline_start..inline_end])
                .map_err(|_| "Inline HTML is not valid UTF-8".to_string())?;
            let line = 1 + self.src[..inline_start]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count();
            tokens.push(Token::Echo { line });
            tokens.push(Token::StringLiteral(inline.to_string()));
            tokens.push(Token::Semicolon);
        }
        self.pos = match next_open {
            Some(open) => open + 5,
            None => self.src.len(),
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_85_pipe_is_distinct_from_bitwise_or_and_logical_or() {
        let tokens = Lexer::new("<?php $value |> trim(...) | 1 || false;")
            .tokenize()
            .unwrap();
        assert!(matches!(tokens[2], Token::PipeGreater(1)));
        assert!(tokens.iter().any(|token| *token == Token::Pipe));
        assert!(tokens.iter().any(|token| *token == Token::PipePipe));
    }

    fn echo(line: usize) -> Token {
        Token::Echo { line }
    }

    #[test]
    fn test_echo_42() {
        let tokens = Lexer::new("<?php echo 42;").tokenize().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(1),
                Token::Integer(42),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn closing_tag_ends_the_php_segment() {
        let tokens = Lexer::new("<?php echo 42 ?>").tokenize().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(1),
                Token::Integer(42),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn closing_tag_emits_inline_html_and_resumes_php() {
        let tokens = Lexer::new("<?php echo 1; ?>\nplain<?php echo 2; ?>")
            .tokenize()
            .unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(1),
                Token::Integer(1),
                Token::Semicolon,
                echo(2),
                Token::StringLiteral("plain".into()),
                Token::Semicolon,
                echo(2),
                Token::Integer(2),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_assign_echo() {
        let tokens = Lexer::new("<?php $a = 42; echo $a;").tokenize().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                Token::Variable("a".into(), 1),
                Token::Assign,
                Token::Integer(42),
                Token::Semicolon,
                echo(1),
                Token::Variable("a".into(), 1),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn null_coalescing_assignment_is_one_token() {
        let tokens = Lexer::new("<?php $value ??= 42;").tokenize().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                Token::Variable("value".into(), 1),
                Token::QuestionQuestionAssign,
                Token::Integer(42),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn error_control_operator_is_tokenized() {
        let tokens = Lexer::new("<?php @trigger_error('hidden');")
            .tokenize()
            .unwrap();
        assert_eq!(tokens[1], Token::At);
        assert_eq!(tokens[2], Token::Identifier("trigger_error".into(), 1));
    }

    #[test]
    fn spread_operator_preserves_its_own_source_line() {
        let tokens =
            Lexer::new("<?php\n$items = [\n    ...[1],\n];\nfunction collect(...$values) {}")
                .tokenize()
                .unwrap();
        let lines: Vec<_> = tokens
            .iter()
            .filter_map(|token| match token {
                Token::DotDotDot(line) => Some(*line),
                _ => None,
            })
            .collect();

        assert_eq!(lines, vec![3, 5]);
    }

    #[test]
    fn throwable_keywords_preserve_their_independent_source_lines() {
        let tokens = Lexer::new("<?php\n$stored = new Exception();\nthrow $stored;")
            .tokenize()
            .unwrap();
        let locations: Vec<_> = tokens
            .iter()
            .filter_map(|token| match token {
                Token::New(line) => Some(("new", *line)),
                Token::Throw(line) => Some(("throw", *line)),
                _ => None,
            })
            .collect();

        assert_eq!(locations, vec![("new", 2), ("throw", 3)]);
    }

    #[test]
    fn test_function_call() {
        let tokens = Lexer::new("<?php echo my_double(21);").tokenize().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(1),
                Token::Identifier("my_double".into(), 1),
                Token::LParen(1),
                Token::Integer(21),
                Token::RParen,
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn magic_constants_are_case_insensitive_but_member_names_remain_identifiers() {
        let tokens = Lexer::new(
            "<?php\necho __line__, __property__; echo \\__FILE__; echo Example::__CLASS__;",
        )
        .tokenize()
        .unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(2),
                Token::MagicConstant {
                    name: "__line__".into(),
                    line: 2,
                },
                Token::Comma,
                Token::MagicConstant {
                    name: "__property__".into(),
                    line: 2,
                },
                Token::Semicolon,
                echo(2),
                Token::Backslash,
                Token::Identifier("__FILE__".into(), 2),
                Token::Semicolon,
                echo(2),
                Token::Identifier("Example".into(), 2),
                Token::DoubleColon,
                Token::Identifier("__CLASS__".into(), 2),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn goto_records_its_source_line_but_member_names_remain_identifiers() {
        let tokens = Lexer::new("<?php\nGOTO finish; $object->goto(); finish:")
            .tokenize()
            .unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                Token::Goto {
                    name: "GOTO".into(),
                    line: 2,
                },
                Token::Identifier("finish".into(), 2),
                Token::Semicolon,
                Token::Variable("object".into(), 2),
                Token::Arrow,
                Token::Identifier("goto".into(), 2),
                Token::LParen(2),
                Token::RParen,
                Token::Semicolon,
                Token::Identifier("finish".into(), 2),
                Token::Colon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_negative_literal() {
        let tokens = Lexer::new("<?php echo -1;").tokenize().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(1),
                Token::Integer(-1),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn float_literal_may_have_an_empty_fractional_part() {
        let tokens = Lexer::new("<?php echo 10.; echo -0.;").tokenize().unwrap();

        assert!(tokens.contains(&Token::Float(10.0)));
        assert!(tokens.contains(&Token::Float(-0.0)));
    }

    #[test]
    fn test_if_while_tokens() {
        let tokens = Lexer::new("<?php if ($x <= 10) { echo $x; }")
            .tokenize()
            .unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                Token::If,
                Token::LParen(1),
                Token::Variable("x".into(), 1),
                Token::LessEqual,
                Token::Integer(10),
                Token::RParen,
                Token::LBrace,
                echo(1),
                Token::Variable("x".into(), 1),
                Token::Semicolon,
                Token::RBrace,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn nowdoc_preserves_variables_and_escape_sequences() {
        let tokens = Lexer::new("<?php echo <<<'DOC'\n$name\\n\nDOC;")
            .tokenize()
            .unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(1),
                Token::StringLiteral("$name\\n".into()),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn heredoc_uses_shared_string_interpolation() {
        let tokens = Lexer::new("<?php $name = 'PHP'; echo <<<DOC\nHello $name!\nDOC;")
            .tokenize()
            .unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                Token::Variable("name".into(), 1),
                Token::Assign,
                Token::StringLiteral("PHP".into()),
                Token::Semicolon,
                echo(1),
                Token::LParen(0),
                Token::StringLiteral("Hello ".into()),
                Token::Dot,
                Token::Variable("name".into(), 2),
                Token::Dot,
                Token::StringLiteral("!".into()),
                Token::RParen,
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn simple_string_interpolation_retains_the_variable_source_line() {
        let tokens = Lexer::new("<?php\necho \"$value\";").tokenize().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(2),
                Token::LParen(0),
                Token::StringLiteral(String::new()),
                Token::Dot,
                Token::Variable("value".into(), 2),
                Token::RParen,
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn complex_string_interpolation_emits_a_method_call_expression() {
        let tokens = Lexer::new("<?php echo <<<DOC\nvalue={$this->render('x', 2)}\nDOC;")
            .tokenize()
            .unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(1),
                Token::LParen(0),
                Token::StringLiteral("value=".into()),
                Token::Dot,
                Token::LParen(0),
                Token::This(2),
                Token::Arrow,
                Token::Identifier("render".into(), 2),
                Token::LParen(2),
                Token::StringLiteral("x".into()),
                Token::Comma,
                Token::Integer(2),
                Token::RParen,
                Token::RParen,
                Token::RParen,
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn string_interpolation_preserves_nullsafe_property_form_and_source_line() {
        let tokens = Lexer::new(
            "<?php\n$nullable = null;\necho \"$nullable?->property()|{$nullable?->method()}\";",
        )
        .tokenize()
        .unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                Token::Variable("nullable".into(), 2),
                Token::Assign,
                Token::Null,
                Token::Semicolon,
                echo(3),
                Token::LParen(0),
                Token::Variable("nullable".into(), 3),
                Token::NullSafe,
                Token::Identifier("property".into(), 3),
                Token::Dot,
                Token::StringLiteral("()|".into()),
                Token::Dot,
                Token::LParen(0),
                Token::Variable("nullable".into(), 3),
                Token::NullSafe,
                Token::Identifier("method".into(), 3),
                Token::LParen(3),
                Token::RParen,
                Token::RParen,
                Token::RParen,
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn flexible_heredoc_removes_closing_indentation() {
        let tokens = Lexer::new("<?php echo <<<DOC\n    first\n      second\n    DOC;")
            .tokenize()
            .unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(1),
                Token::StringLiteral("first\n  second".into()),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn heredoc_rejects_shallow_non_empty_body_indentation() {
        let tokens = Lexer::new("<?php echo <<<DOC\n  first\n    DOC;")
            .tokenize()
            .unwrap();
        assert!(tokens.contains(&Token::ParseError(
            "Invalid body indentation level (expecting an indentation level of at least 4)".into(),
            2,
        )));
    }

    #[test]
    fn binary_document_prefixes_do_not_emit_an_identifier() {
        let tokens = Lexer::new("<?php echo b<<<DOC\nfirst\nDOC; echo B<<<'RAW'\nsecond\nRAW;")
            .tokenize()
            .unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(1),
                Token::StringLiteral("first".into()),
                Token::Semicolon,
                echo(3),
                Token::StringLiteral("second".into()),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn document_indentation_errors_retain_the_first_source_line() {
        let tokens = Lexer::new("<?php\necho <<<DOC\n a\n\tb\n DOC;")
            .tokenize()
            .unwrap();
        assert!(tokens.contains(&Token::ParseError(
            "Invalid indentation - tabs and spaces cannot be mixed".into(),
            4,
        )));

        let tokens = Lexer::new("<?php\necho <<<DOC\n a\nb\n DOC;")
            .tokenize()
            .unwrap();
        assert!(tokens.contains(&Token::ParseError(
            "Invalid body indentation level (expecting an indentation level of at least 1)".into(),
            4,
        )));
    }

    #[test]
    fn unterminated_documents_distinguish_empty_and_started_bodies() {
        let empty = Lexer::new("<?php\necho <<<DOC\n").tokenize().unwrap();
        assert!(empty.contains(&Token::ParseError(
            "syntax error, unexpected end of file".into(),
            3,
        )));

        let started = Lexer::new("<?php\necho <<<'DOC'\n\n").tokenize().unwrap();
        assert!(started.contains(&Token::ParseError(
            "syntax error, unexpected end of file, expecting variable or heredoc end or \"${\" or \"{$\""
                .into(),
            4,
        )));
    }

    #[test]
    fn outer_document_marker_is_not_taken_from_an_interpolated_expression() {
        let tokens = Lexer::new(
            "<?php echo <<<DOC\n    outer\n    ${<<<DOC\n        inner\n        DOC}\n    tail\n    DOC;",
        )
        .tokenize()
        .unwrap();

        assert!(tokens.iter().any(|token| matches!(
            token,
            Token::CompileDeprecation(message, 2)
                if message.starts_with("Using ${expr} (variable variables)")
        )));
        assert_eq!(tokens.last(), Some(&Token::Eof));
    }

    #[test]
    fn nowdoc_marker_scanning_does_not_treat_dollar_braces_as_interpolation() {
        let tokens = Lexer::new("<?php echo <<<'DOC'\n${\nDOC;")
            .tokenize()
            .unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(1),
                Token::StringLiteral("${".into()),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn document_strings_accept_all_php_line_endings() {
        let tokens = Lexer::new("<?php echo <<<DOC\r  first\r\r  second\r  DOC;")
            .tokenize()
            .unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                echo(1),
                Token::StringLiteral("first\r\rsecond".into()),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn quoted_simple_interpolation_offsets_are_parse_errors() {
        let tokens = Lexer::new("<?php\necho <<<DOC\n$items['name']\nDOC;")
            .tokenize()
            .unwrap();

        assert!(tokens.contains(&Token::ParseError(
            "syntax error, unexpected string content \"\", expecting \"-\" or identifier or variable or number"
                .into(),
            3,
        )));

        let tokens = Lexer::new("<?php\necho \"$items['name']\";")
            .tokenize()
            .unwrap();
        assert!(tokens.contains(&Token::ParseError(
            "syntax error, unexpected string content \"\", expecting \"-\" or identifier or variable or number"
                .into(),
            2,
        )));
    }

    #[test]
    fn adjacent_values_preserve_the_document_start_parse_error() {
        let tokens = Lexer::new("<?php\n$value = factory<<<\"DOC\"\nbody\nDOC;")
            .tokenize()
            .unwrap();

        assert!(tokens.contains(&Token::ParseError(
            "syntax error, unexpected heredoc start \"<<<\"DOC\"".into(),
            2,
        )));
    }

    #[test]
    fn overflowing_octal_escapes_precede_interpolation_deprecations() {
        let tokens = Lexer::new("<?php\necho <<<DOC\n\\400\n${\"\\400\"}\nDOC;")
            .tokenize()
            .unwrap();
        let diagnostics: Vec<_> = tokens
            .iter()
            .filter(|token| {
                matches!(
                    token,
                    Token::CompileWarning(..) | Token::CompileDeprecation(..)
                )
            })
            .cloned()
            .collect();

        assert_eq!(
            diagnostics,
            vec![
                Token::CompileWarning(
                    "Octal escape sequence overflow \\400 is greater than \\377".into(),
                    3,
                ),
                Token::CompileWarning(
                    "Octal escape sequence overflow \\400 is greater than \\377".into(),
                    4,
                ),
                Token::CompileDeprecation(
                    "Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead"
                        .into(),
                    3,
                ),
            ]
        );
    }

    #[test]
    fn unicode_codepoint_escapes_preserve_valid_and_legacy_boundaries() {
        let tokens = Lexer::new(r#"<?php echo "\u{10FFFF}|\u1234";"#)
            .tokenize()
            .unwrap();

        assert!(tokens.contains(&Token::StringLiteral(format!(
            "{}|\\u1234",
            char::from_u32(0x10ffff).unwrap()
        ))));
    }

    #[test]
    fn invalid_unicode_codepoint_escapes_use_php_diagnostics_and_source_lines() {
        let generic = "Invalid UTF-8 codepoint escape sequence";
        let too_large = "Invalid UTF-8 codepoint escape sequence: Codepoint too large";
        let cases = [
            (r#"<?php echo "\u{}";"#, generic, 1),
            (r#"<?php echo "\u{+41}";"#, generic, 1),
            (r#"<?php echo "\u{41 }";"#, generic, 1),
            (r#"<?php echo "\u{110000}";"#, too_large, 1),
            (r#"<?php echo "\u{FFFFFFFFFFFFFFFF}";"#, too_large, 1),
            ("<?php\necho \"prefix\n\\u{-41}\";", generic, 3),
            (
                "<?php\n$value = <<<TEXT\nprefix\n\\u{110000}\nTEXT;",
                too_large,
                4,
            ),
        ];

        for (source, message, line) in cases {
            let tokens = Lexer::new(source).tokenize().unwrap();
            assert!(
                tokens.contains(&Token::ParseError(message.into(), line)),
                "missing {message:?} on line {line} in {tokens:?}"
            );
        }
    }

    #[test]
    fn legacy_source_bytes_are_consistent_in_identifiers_and_strings() {
        let source = decode_php_source(b"<?php class \xa9 {} echo '\xa9';");
        let tokens = Lexer::new(&source).tokenize().unwrap();

        assert!(tokens.contains(&Token::Identifier("©".into(), 1)));
        assert!(tokens.contains(&Token::StringLiteral("©".into())));
    }

    #[test]
    fn explicit_octal_integer_literal() {
        let tokens = Lexer::new("<?php echo 0o777; echo 0O20;")
            .tokenize()
            .unwrap();

        assert!(tokens.contains(&Token::Integer(511)));
        assert!(tokens.contains(&Token::Integer(16)));
    }

    #[test]
    fn malformed_prefixed_integers_leave_the_prefix_as_an_identifier() {
        let tokens = Lexer::new("<?php 0x_1; 0b2; 0o8;").tokenize().unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                Token::Integer(0),
                Token::Identifier("x_1".into(), 1),
                Token::Semicolon,
                Token::Integer(0),
                Token::Identifier("b2".into(), 1),
                Token::Semicolon,
                Token::Integer(0),
                Token::Identifier("o8".into(), 1),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn long_array_constructor_is_ascii_case_insensitive() {
        let tokens = Lexer::new("<?php ArRaY (true); $object->ArRaY();")
            .tokenize()
            .unwrap();

        assert!(tokens.contains(&Token::ArrayKw));
        assert!(tokens.contains(&Token::True));
        assert!(tokens.contains(&Token::Identifier("ArRaY".into(), 1)));
    }

    #[test]
    fn first_class_callable_in_attribute_survives_as_compile_error() {
        let tokens = Lexer::new("<?php\n#[Example(...)]\nclass Subject {}")
            .tokenize()
            .unwrap();

        assert!(tokens.contains(&Token::AttributeStart(2)));
        assert!(
            tokens
                .iter()
                .any(|token| matches!(token, Token::DotDotDot(2)))
        );
    }
}
