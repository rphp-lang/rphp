#[cfg(not(target_vendor = "apple"))]
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CsvState {
    FieldStart,
    Unquoted,
    Quoted,
    AfterQuote,
}

/// Incremental byte-oriented CSV parser used by `PhpStream`.
///
/// Physical reads stay in the stream owner so cursor and EOF handling remain
/// shared with `fgets()`. This parser only retains the current record and can
/// therefore continue a quoted field across any number of physical lines.
pub(super) struct CsvParser {
    separator: u8,
    enclosure: u8,
    escape: Option<u8>,
    state: CsvState,
    field: Vec<u8>,
    fields: Vec<Option<Vec<u8>>>,
    pending_escape: bool,
    saw_non_eol_byte: bool,
    record_done: bool,
    string_input: bool,
}

impl CsvParser {
    #[cfg_attr(target_vendor = "apple", cold)]
    #[cfg_attr(target_vendor = "apple", inline(never))]
    #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
    pub(super) fn new(separator: u8, enclosure: u8, escape: Option<u8>) -> Self {
        Self {
            separator,
            enclosure,
            // PHP accepts every single byte at the call boundary but its CSV
            // scanner only treats the ASCII range as an active escape.
            escape: escape.filter(u8::is_ascii),
            state: CsvState::FieldStart,
            field: Vec::new(),
            fields: Vec::new(),
            pending_escape: false,
            saw_non_eol_byte: false,
            record_done: false,
            string_input: false,
        }
    }

    /// Build a parser for `str_getcsv()`. Unlike stream records, embedded
    /// line endings belong to the supplied string; only its final line ending
    /// terminates an unquoted record.
    #[cfg_attr(target_vendor = "apple", cold)]
    #[cfg_attr(target_vendor = "apple", inline(never))]
    #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
    pub(super) fn new_string(separator: u8, enclosure: u8, escape: Option<u8>) -> Self {
        let mut parser = Self::new(separator, enclosure, escape);
        parser.string_input = true;
        parser
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
    pub(super) fn push_segment(&mut self, segment: &[u8]) -> io::Result<()> {
        debug_assert!(!self.record_done);
        let mut index = 0;
        while index < segment.len() {
            let byte = segment[index];
            let final_string_line_feed = self.string_input
                && byte == b'\n'
                && index + 1 == segment.len()
                && self.state != CsvState::Quoted;
            if byte != b'\r' && byte != b'\n' {
                self.saw_non_eol_byte = true;
            }

            if self.pending_escape {
                self.push_byte(byte)?;
                self.pending_escape = false;
                index += 1;
                continue;
            }

            match self.state {
                CsvState::FieldStart => {
                    if (byte == b'\n' && !self.string_input) || final_string_line_feed {
                        self.finish_record_at_line_end()?;
                    } else if byte == self.enclosure {
                        // PHP accepts an enclosure after leading horizontal
                        // whitespace and does not retain that whitespace.
                        self.field.clear();
                        self.state = CsvState::Quoted;
                    } else if byte == self.separator {
                        self.finish_field(false)?;
                    } else {
                        self.push_byte(byte)?;
                        if !matches!(byte, b' ' | b'\t') {
                            self.state = CsvState::Unquoted;
                        }
                    }
                }
                CsvState::Unquoted => {
                    if (byte == b'\n' && !self.string_input) || final_string_line_feed {
                        self.finish_record_at_line_end()?;
                    } else if byte == self.separator {
                        self.finish_field(false)?;
                    } else {
                        self.push_byte(byte)?;
                    }
                }
                CsvState::Quoted => {
                    if byte == self.enclosure {
                        if segment.get(index + 1).copied() == Some(self.enclosure) {
                            self.push_byte(self.enclosure)?;
                            index += 1;
                        } else {
                            self.state = CsvState::AfterQuote;
                        }
                    } else if self.escape == Some(byte) {
                        self.push_byte(byte)?;
                        if let Some(next) = segment.get(index + 1).copied() {
                            self.push_byte(next)?;
                            index += 1;
                        } else {
                            self.pending_escape = true;
                        }
                    } else {
                        self.push_byte(byte)?;
                    }
                }
                CsvState::AfterQuote => {
                    if (byte == b'\n' && !self.string_input) || final_string_line_feed {
                        self.finish_record_at_line_end()?;
                    } else if byte == self.separator {
                        self.finish_field(false)?;
                    } else {
                        // PHP appends non-delimiter bytes after a closing
                        // enclosure (including whitespace and later quotes).
                        self.push_byte(byte)?;
                    }
                }
            }
            index += 1;
            if self.record_done {
                debug_assert_eq!(index, segment.len());
                break;
            }
        }
        Ok(())
    }

    #[cfg_attr(target_vendor = "apple", cold)]
    #[cfg_attr(target_vendor = "apple", inline(never))]
    #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
    pub(super) fn needs_continuation(&self) -> bool {
        !self.record_done && self.state == CsvState::Quoted
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
    pub(super) fn finish(
        mut self,
        strip_final_carriage_return: bool,
    ) -> io::Result<Vec<Option<Vec<u8>>>> {
        if !self.record_done {
            self.finish_record(strip_final_carriage_return)?;
        }
        Ok(self.fields)
    }

    #[cfg_attr(target_vendor = "apple", cold)]
    #[cfg_attr(target_vendor = "apple", inline(never))]
    #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
    fn push_byte(&mut self, byte: u8) -> io::Result<()> {
        if self.field.len() == self.field.capacity() {
            self.field.try_reserve(1).map_err(allocation_error)?;
        }
        self.field.push(byte);
        Ok(())
    }

    #[cfg_attr(target_vendor = "apple", cold)]
    #[cfg_attr(target_vendor = "apple", inline(never))]
    #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
    fn push_field(&mut self, field: Option<Vec<u8>>) -> io::Result<()> {
        if self.fields.len() == self.fields.capacity() {
            self.fields.try_reserve(1).map_err(allocation_error)?;
        }
        self.fields.push(field);
        Ok(())
    }

    #[cfg_attr(target_vendor = "apple", cold)]
    #[cfg_attr(target_vendor = "apple", inline(never))]
    #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
    fn finish_field(&mut self, strip_carriage_return: bool) -> io::Result<()> {
        if strip_carriage_return && self.field.last() == Some(&b'\r') {
            self.field.pop();
        }
        // PHP 8.5 exposes its NUL escape sentinel for an otherwise empty,
        // unterminated enclosed field. Keep the quirk bounded to that exact
        // state; closed empty fields and every other escape remain empty.
        if self.field.is_empty() && self.state == CsvState::Quoted && self.escape == Some(b'\0') {
            self.push_byte(b'\0')?;
        }
        let field = std::mem::take(&mut self.field);
        self.push_field(Some(field))?;
        self.state = CsvState::FieldStart;
        self.pending_escape = false;
        Ok(())
    }

    #[cfg_attr(target_vendor = "apple", cold)]
    #[cfg_attr(target_vendor = "apple", inline(never))]
    #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
    fn finish_record_at_line_end(&mut self) -> io::Result<()> {
        self.finish_record(true)
    }

    #[cfg_attr(target_vendor = "apple", cold)]
    #[cfg_attr(target_vendor = "apple", inline(never))]
    #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
    fn finish_record(&mut self, strip_carriage_return: bool) -> io::Result<()> {
        if !self.saw_non_eol_byte && self.fields.is_empty() {
            self.field.clear();
            self.push_field(None)?;
        } else {
            self.finish_field(strip_carriage_return)?;
        }
        self.record_done = true;
        Ok(())
    }
}

#[cfg_attr(target_vendor = "apple", cold)]
#[cfg_attr(target_vendor = "apple", inline(never))]
#[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
fn allocation_error(_: std::collections::TryReserveError) -> io::Error {
    io::Error::new(io::ErrorKind::OutOfMemory, "CSV record allocation failed")
}

#[cfg(test)]
mod csv_tests {
    use super::*;

    fn parse(parts: &[&[u8]], escape: Option<u8>) -> Vec<Option<Vec<u8>>> {
        let mut parser = CsvParser::new(b',', b'"', escape);
        for part in parts {
            parser.push_segment(part).unwrap();
        }
        parser.finish(true).unwrap()
    }

    fn parse_with(
        input: &[u8],
        separator: u8,
        enclosure: u8,
        escape: Option<u8>,
        string_input: bool,
    ) -> Vec<Option<Vec<u8>>> {
        let mut parser = if string_input {
            CsvParser::new_string(separator, enclosure, escape)
        } else {
            CsvParser::new(separator, enclosure, escape)
        };
        parser.push_segment(input).unwrap();
        parser.finish(true).unwrap()
    }

    #[test]
    fn quoted_fields_continue_across_segments_and_lines() {
        assert_eq!(
            parse(&[b"a,\"long", b"\nfield\",z\r\n"], Some(b'\\')),
            vec![
                Some(b"a".to_vec()),
                Some(b"long\nfield".to_vec()),
                Some(b"z".to_vec())
            ]
        );
    }

    #[test]
    fn doubled_enclosures_and_php_escape_bytes_are_retained() {
        assert_eq!(
            parse(&[b"a,\"b\"\"c\",\"d\\\"e\"\n"], Some(b'\\')),
            vec![
                Some(b"a".to_vec()),
                Some(b"b\"c".to_vec()),
                Some(b"d\\\"e".to_vec())
            ]
        );
    }

    #[test]
    fn blank_record_is_null_but_empty_columns_are_strings() {
        assert_eq!(parse(&[b"\r\n"], None), vec![None]);
        assert_eq!(
            parse(&[b",\n"], None),
            vec![Some(Vec::new()), Some(Vec::new())]
        );
        assert_eq!(parse(&[b"\"\"\n"], None), vec![Some(Vec::new())]);
    }

    #[test]
    fn leading_space_before_enclosure_is_discarded_like_php() {
        assert_eq!(
            parse(&[b"  \"a,b\", c \n"], None),
            vec![Some(b"a,b".to_vec()), Some(b" c ".to_vec())]
        );
    }

    #[test]
    fn enclosure_wins_when_it_is_also_the_escape_byte() {
        assert_eq!(
            parse(&[b"\"a\"\"b\"\n"], Some(b'"')),
            vec![Some(b"a\"b".to_vec())]
        );
    }

    #[test]
    fn newline_remains_a_record_boundary_when_selected_as_separator() {
        let mut parser = CsvParser::new(b'\n', b'"', None);
        parser.push_segment(b"a,b\n").unwrap();
        assert_eq!(parser.finish(false).unwrap(), vec![Some(b"a,b".to_vec())]);
    }

    #[test]
    fn enclosure_precedes_an_identical_separator_at_field_start() {
        assert_eq!(
            parse_with(b".red..blue.\n", b'.', b'.', Some(b'.'), false),
            vec![Some(b"red.blue".to_vec())]
        );
    }

    #[test]
    fn non_ascii_escape_is_accepted_but_not_scanned_as_an_escape() {
        assert_eq!(
            parse_with(b"\"a\xfe\"b\",c\n", b',', b'"', Some(0xfe), false),
            vec![Some(b"a\xfeb\"".to_vec()), Some(b"c".to_vec())]
        );
    }

    #[test]
    fn string_input_keeps_embedded_newlines_and_nul_escape_empty_quirk() {
        assert_eq!(
            parse_with(b"\"a\nb\",c\nd\r\n", b',', b'"', None, true),
            vec![Some(b"a\nb".to_vec()), Some(b"c\nd".to_vec())]
        );
        assert_eq!(
            parse_with(b"\0yy", b'y', b'y', Some(b'\0'), true),
            vec![Some(vec![b'\0']), Some(vec![b'\0'])]
        );
    }
}
