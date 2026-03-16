/// Minimal PHP lexer — just enough tokens for the vertical slice.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    OpenTag,        // <?php
    // Keywords
    Echo,           // echo
    Function,       // function
    Return,         // return
    If,             // if
    Else,           // else
    While,          // while
    For,            // for
    ElseIf,         // elseif
    Do,             // do
    Break,          // break
    Continue,       // continue
    Switch,         // switch
    Case,           // case
    Default,        // default
    Null,           // null
    True,           // true
    False,          // false
    ArrayKw,        // array
    Foreach,        // foreach
    As,             // as
    Isset,          // isset
    Empty,          // empty
    Unset,          // unset
    Match,          // match
    Try,            // try
    Catch,          // catch
    Finally,        // finally
    Throw,          // throw
    Class,          // class
    New,            // new
    Public,         // public
    Protected,      // protected
    Private,        // private
    This,           // $this (handled as special variable)
    Extends,        // extends
    Static,         // static
    Instanceof,     // instanceof
    Const,          // const
    // Literals
    Integer(i64),   // 42, -1
    Float(f64),     // 3.14, 1.5e10
    StringLiteral(String), // "hello", 'world'
    Variable(String), // $a, $foo
    Identifier(String), // my_double, strlen
    // Operators
    Assign,         // =
    Plus,           // +
    Minus,          // -
    Star,           // *
    Slash,          // /
    Percent,        // %
    Dot,            // . (concat)
    PlusPlus,       // ++
    MinusMinus,     // --
    EqualEqual,     // ==
    IdenticalEqual, // ===
    NotEqual,       // !=
    NotIdentical,   // !==
    Less,           // <
    LessEqual,      // <=
    Greater,        // >
    GreaterEqual,   // >=
    AmpAmp,         // &&
    PipePipe,       // ||
    Bang,           // !
    PlusAssign,     // +=
    MinusAssign,    // -=
    StarAssign,     // *=
    SlashAssign,    // /=
    PercentAssign,  // %=
    DotAssign,      // .=
    Question,       // ?
    QuestionQuestion, // ??
    Colon,          // :
    // Punctuation
    Semicolon,      // ;
    LParen,         // (
    RParen,         // )
    LBrace,         // {
    RBrace,         // }
    Comma,          // ,
    LBracket,       // [
    RBracket,       // ]
    DoubleArrow,    // =>
    Arrow,          // ->
    DoubleColon,    // ::
    Fn,             // fn (arrow functions)
    Use,            // use (closure use)
    Pipe,           // | (bitwise or, multi-catch separator)
    Ampersand,      // & (bitwise and, reference)
    DotDotDot,      // ... (variadic / spread)
    Eof,
}

enum StringPart {
    Literal(String),
    Variable(String),
    ArrayAccess(String, String), // var_name, index (string or integer literal)
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            src: source.as_bytes(),
            pos: 0,
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
                tokens.push(Token::Eof);
                break;
            }

            let ch = self.src[self.pos];

            match ch {
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
                    if self.peek_next() == Some(b'=') {
                        tokens.push(Token::LessEqual);
                        self.pos += 2;
                    } else {
                        tokens.push(Token::Less);
                        self.pos += 1;
                    }
                }
                b'>' => {
                    if self.peek_next() == Some(b'=') {
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
                    if self.peek_next() == Some(b'=') {
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
                        tokens.push(Token::DotDotDot);
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
                    } else {
                        // Single & — bitwise and / reference
                        tokens.push(Token::Ampersand);
                        self.pos += 1;
                    }
                }
                b'|' => {
                    if self.peek_next() == Some(b'|') {
                        tokens.push(Token::PipePipe);
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
                b'"' => {
                    let parts = self.read_double_quoted_string()?;
                    if parts.len() == 1 {
                        match &parts[0] {
                            StringPart::Literal(s) => {
                                tokens.push(Token::StringLiteral(s.clone()));
                            }
                            StringPart::Variable(name) => {
                                // "$var" → ("" . $var) to force string
                                tokens.push(Token::LParen);
                                tokens.push(Token::StringLiteral(String::new()));
                                tokens.push(Token::Dot);
                                tokens.push(Token::Variable(name.clone()));
                                tokens.push(Token::RParen);
                            }
                            StringPart::ArrayAccess(name, idx) => {
                                tokens.push(Token::LParen);
                                tokens.push(Token::StringLiteral(String::new()));
                                tokens.push(Token::Dot);
                                Self::emit_array_access_tokens(&mut tokens, name, idx);
                                tokens.push(Token::RParen);
                            }
                        }
                    } else {
                        tokens.push(Token::LParen);
                        let mut first = true;
                        for part in &parts {
                            if !first {
                                tokens.push(Token::Dot);
                            }
                            first = false;
                            match part {
                                StringPart::Literal(s) => tokens.push(Token::StringLiteral(s.clone())),
                                StringPart::Variable(name) => tokens.push(Token::Variable(name.clone())),
                                StringPart::ArrayAccess(name, idx) => {
                                    Self::emit_array_access_tokens(&mut tokens, name, idx);
                                }
                            }
                        }
                        tokens.push(Token::RParen);
                    }
                }
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
                    tokens.push(Token::LParen);
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
                    tokens.push(Token::LBracket);
                    self.pos += 1;
                }
                b']' => {
                    tokens.push(Token::RBracket);
                    self.pos += 1;
                }
                b'?' => {
                    if self.peek_next() == Some(b'?') {
                        tokens.push(Token::QuestionQuestion);
                        self.pos += 2;
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
                    self.pos += 1;
                    let name = self.read_identifier();
                    if name.is_empty() {
                        return Err("Expected variable name after $".into());
                    }
                    tokens.push(Token::Variable(name));
                }
                b'0'..=b'9' => {
                    tokens.push(self.read_number()?);
                }
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                    let ident = self.read_identifier();
                    match ident.as_str() {
                        "echo" => tokens.push(Token::Echo),
                        "function" => tokens.push(Token::Function),
                        "return" => tokens.push(Token::Return),
                        "if" => tokens.push(Token::If),
                        "else" => tokens.push(Token::Else),
                        "elseif" => tokens.push(Token::ElseIf),
                        "while" => tokens.push(Token::While),
                        "for" => tokens.push(Token::For),
                        "do" => tokens.push(Token::Do),
                        "break" => tokens.push(Token::Break),
                        "continue" => tokens.push(Token::Continue),
                        "switch" => tokens.push(Token::Switch),
                        "case" => tokens.push(Token::Case),
                        "default" => tokens.push(Token::Default),
                        "null" | "NULL" => tokens.push(Token::Null),
                        "true" | "TRUE" => tokens.push(Token::True),
                        "false" | "FALSE" => tokens.push(Token::False),
                        "array" => tokens.push(Token::ArrayKw),
                        "foreach" => tokens.push(Token::Foreach),
                        "as" => tokens.push(Token::As),
                        "isset" => tokens.push(Token::Isset),
                        "empty" => tokens.push(Token::Empty),
                        "unset" => tokens.push(Token::Unset),
                        "match" => tokens.push(Token::Match),
                        "try" => tokens.push(Token::Try),
                        "catch" => tokens.push(Token::Catch),
                        "finally" => tokens.push(Token::Finally),
                        "throw" => tokens.push(Token::Throw),
                        "class" => tokens.push(Token::Class),
                        "new" => tokens.push(Token::New),
                        "public" => tokens.push(Token::Public),
                        "protected" => tokens.push(Token::Protected),
                        "private" => tokens.push(Token::Private),
                        "extends" => tokens.push(Token::Extends),
                        "static" => tokens.push(Token::Static),
                        "instanceof" => tokens.push(Token::Instanceof),
                        "const" => tokens.push(Token::Const),
                        "fn" => tokens.push(Token::Fn),
                        "use" => tokens.push(Token::Use),
                        _ => tokens.push(Token::Identifier(ident)),
                    }
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
                        return Err(format!("Unterminated comment starting at position {}", start));
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
        while self.pos < self.src.len() && self.src[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        // Check for decimal point followed by digit → float
        if self.pos < self.src.len() && self.src[self.pos] == b'.'
            && self.src.get(self.pos + 1).map_or(false, |c| c.is_ascii_digit())
        {
            self.pos += 1; // skip '.'
            while self.pos < self.src.len() && self.src[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            // Scientific notation
            if self.pos < self.src.len() && (self.src[self.pos] == b'e' || self.src[self.pos] == b'E') {
                self.pos += 1;
                if self.pos < self.src.len() && (self.src[self.pos] == b'+' || self.src[self.pos] == b'-') {
                    self.pos += 1;
                }
                while self.pos < self.src.len() && self.src[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
            }
            let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
            let f: f64 = s.parse().map_err(|_| format!("Invalid float literal at position {}", start))?;
            Ok(Token::Float(f))
        } else {
            let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
            let n: i64 = s.parse().map_err(|_| format!("Integer literal too large at position {}", start))?;
            Ok(Token::Integer(n))
        }
    }

    fn read_identifier(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.src.len()
            && (self.src[self.pos].is_ascii_alphanumeric() || self.src[self.pos] == b'_')
        {
            self.pos += 1;
        }
        String::from_utf8(self.src[start..self.pos].to_vec()).unwrap()
    }

    fn read_string(&mut self, quote: u8) -> Result<String, String> {
        self.pos += 1; // skip opening quote
        let mut result = String::new();
        while self.pos < self.src.len() && self.src[self.pos] != quote {
            if self.src[self.pos] == b'\\' && self.pos + 1 < self.src.len() {
                self.pos += 1; // skip backslash
                let escaped = self.src[self.pos];
                if quote == b'"' {
                    // Double-quoted: interpret escape sequences
                    match escaped {
                        b'n' => result.push('\n'),
                        b'r' => result.push('\r'),
                        b't' => result.push('\t'),
                        b'\\' => result.push('\\'),
                        b'$' => result.push('$'),
                        b'"' => result.push('"'),
                        _ => {
                            // Unknown escape — keep backslash + char
                            result.push('\\');
                            result.push(escaped as char);
                        }
                    }
                } else {
                    // Single-quoted: only \\ and \' are special
                    match escaped {
                        b'\\' => result.push('\\'),
                        b'\'' => result.push('\''),
                        _ => {
                            result.push('\\');
                            result.push(escaped as char);
                        }
                    }
                }
                self.pos += 1;
            } else {
                // Decode UTF-8 character (handles multi-byte correctly)
                let rest = &self.src[self.pos..];
                match std::str::from_utf8(rest) {
                    Ok(s) => {
                        let ch = s.chars().next().unwrap();
                        result.push(ch);
                        self.pos += ch.len_utf8();
                    }
                    Err(e) => {
                        // Try partial valid UTF-8
                        let valid_up_to = e.valid_up_to();
                        if valid_up_to > 0 {
                            let s = std::str::from_utf8(&rest[..valid_up_to]).unwrap();
                            let ch = s.chars().next().unwrap();
                            result.push(ch);
                            self.pos += ch.len_utf8();
                        } else {
                            return Err(format!(
                                "Invalid UTF-8 byte 0x{:02x} in string at position {}",
                                self.src[self.pos], self.pos
                            ));
                        }
                    }
                }
            }
        }
        if self.pos >= self.src.len() {
            return Err("Unterminated string literal".into());
        }
        self.pos += 1; // skip closing quote
        Ok(result)
    }

    fn read_double_quoted_string(&mut self) -> Result<Vec<StringPart>, String> {
        self.pos += 1; // skip opening "
        let mut parts = Vec::new();
        let mut current = String::new();

        while self.pos < self.src.len() && self.src[self.pos] != b'"' {
            if self.src[self.pos] == b'\\' && self.pos + 1 < self.src.len() {
                self.pos += 1; // skip backslash
                match self.src[self.pos] {
                    b'n' => current.push('\n'),
                    b'r' => current.push('\r'),
                    b't' => current.push('\t'),
                    b'\\' => current.push('\\'),
                    b'$' => current.push('$'),
                    b'"' => current.push('"'),
                    _ => {
                        current.push('\\');
                        current.push(self.src[self.pos] as char);
                    }
                }
                self.pos += 1;
            } else if self.src[self.pos] == b'$' {
                let next = self.src.get(self.pos + 1).copied().unwrap_or(0);
                if next == b'_' || next.is_ascii_alphabetic() {
                    // Variable interpolation: $varname
                    if !current.is_empty() {
                        parts.push(StringPart::Literal(std::mem::take(&mut current)));
                    }
                    self.pos += 1; // skip $
                    let name = self.read_identifier();
                    parts.push(StringPart::Variable(name));
                } else {
                    current.push('$');
                    self.pos += 1;
                }
            } else if self.src[self.pos] == b'{' && self.src.get(self.pos + 1) == Some(&b'$') {
                // {$var} or {$var[idx]} syntax
                if !current.is_empty() {
                    parts.push(StringPart::Literal(std::mem::take(&mut current)));
                }
                self.pos += 2; // skip {$
                let name = self.read_identifier();
                if self.pos < self.src.len() && self.src[self.pos] == b'[' {
                    // {$var[idx]} — read the index
                    self.pos += 1; // skip [
                    let mut idx_str = String::new();
                    while self.pos < self.src.len() && self.src[self.pos] != b']' {
                        idx_str.push(self.src[self.pos] as char);
                        self.pos += 1;
                    }
                    if self.pos < self.src.len() && self.src[self.pos] == b']' {
                        self.pos += 1; // skip ]
                    }
                    if self.pos < self.src.len() && self.src[self.pos] == b'}' {
                        self.pos += 1; // skip }
                    }
                    parts.push(StringPart::ArrayAccess(name, idx_str));
                } else {
                    if self.pos < self.src.len() && self.src[self.pos] == b'}' {
                        self.pos += 1; // skip }
                    }
                    parts.push(StringPart::Variable(name));
                }
            } else {
                // Regular character — handle UTF-8
                let rest = &self.src[self.pos..];
                match std::str::from_utf8(rest) {
                    Ok(s) => {
                        let ch = s.chars().next().unwrap();
                        current.push(ch);
                        self.pos += ch.len_utf8();
                    }
                    Err(e) => {
                        let valid_up_to = e.valid_up_to();
                        if valid_up_to > 0 {
                            let s = std::str::from_utf8(&rest[..valid_up_to]).unwrap();
                            let ch = s.chars().next().unwrap();
                            current.push(ch);
                            self.pos += ch.len_utf8();
                        } else {
                            return Err(format!(
                                "Invalid UTF-8 byte 0x{:02x} in string at position {}",
                                self.src[self.pos], self.pos
                            ));
                        }
                    }
                }
            }
        }

        if self.pos >= self.src.len() {
            return Err("Unterminated string literal".into());
        }
        self.pos += 1; // skip closing "

        if !current.is_empty() || parts.is_empty() {
            parts.push(StringPart::Literal(current));
        }

        Ok(parts)
    }

    /// Emit tokens for $var[idx] in interpolated strings
    fn emit_array_access_tokens(tokens: &mut Vec<Token>, name: &str, idx: &str) {
        tokens.push(Token::Variable(name.to_string()));
        tokens.push(Token::LBracket);
        // Try to parse index as integer, otherwise treat as string key
        if let Ok(n) = idx.parse::<i64>() {
            tokens.push(Token::Integer(n));
        } else {
            // Strip quotes if present (e.g. 'key' or "key")
            let key = if (idx.starts_with('\'') && idx.ends_with('\''))
                || (idx.starts_with('"') && idx.ends_with('"'))
            {
                &idx[1..idx.len() - 1]
            } else {
                idx
            };
            tokens.push(Token::StringLiteral(key.to_string()));
        }
        tokens.push(Token::RBracket);
    }

    fn is_value_token(tok: Option<&Token>) -> bool {
        matches!(
            tok,
            Some(Token::Integer(_) | Token::Float(_) | Token::Variable(_) | Token::StringLiteral(_) | Token::RParen | Token::RBracket | Token::Identifier(_) | Token::True | Token::False | Token::Null)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_echo_42() {
        let tokens = Lexer::new("<?php echo 42;").tokenize().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                Token::Echo,
                Token::Integer(42),
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
                Token::Variable("a".into()),
                Token::Assign,
                Token::Integer(42),
                Token::Semicolon,
                Token::Echo,
                Token::Variable("a".into()),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_function_call() {
        let tokens = Lexer::new("<?php echo my_double(21);").tokenize().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::OpenTag,
                Token::Echo,
                Token::Identifier("my_double".into()),
                Token::LParen,
                Token::Integer(21),
                Token::RParen,
                Token::Semicolon,
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
                Token::Echo,
                Token::Integer(-1),
                Token::Semicolon,
                Token::Eof,
            ]
        );
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
                Token::LParen,
                Token::Variable("x".into()),
                Token::LessEqual,
                Token::Integer(10),
                Token::RParen,
                Token::LBrace,
                Token::Echo,
                Token::Variable("x".into()),
                Token::Semicolon,
                Token::RBrace,
                Token::Eof,
            ]
        );
    }
}
