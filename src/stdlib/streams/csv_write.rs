use std::borrow::Cow;
use std::io;

use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

#[cold]
pub(super) fn fn_fputcsv(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = super::checked_args::stream_argument(execute_data, eg, "fputcsv") else {
        return Ok(());
    };
    let Some(fields) = super::argument(execute_data, 1).as_array() else {
        let value = super::argument(execute_data, 1);
        super::checked_args::argument_error(
            eg,
            "TypeError",
            format!(
                "fputcsv(): Argument #2 ($fields) must be of type array, {} given",
                value.type_name()
            ),
        );
        return Ok(());
    };
    let Some(separator) = super::csv_errors::csv_character_argument(
        execute_data,
        eg,
        2,
        b',',
        "fputcsv",
        3,
        "separator",
    ) else {
        return Ok(());
    };
    let Some(enclosure) = super::csv_errors::csv_character_argument(
        execute_data,
        eg,
        3,
        b'"',
        "fputcsv",
        4,
        "enclosure",
    ) else {
        return Ok(());
    };
    let Some(escape) =
        super::csv_errors::csv_escape_argument(execute_data, eg, 4, Some(b'\\'), "fputcsv", 5)
    else {
        return Ok(());
    };
    let eol = match super::optional_argument(execute_data, 5) {
        Some(value) if value.value_type() == ValueType::Null => Cow::Borrowed("\n"),
        Some(_) => {
            let value = super::argument(execute_data, 5);
            if matches!(
                value.value_type(),
                ValueType::Array | ValueType::Object | ValueType::Resource | ValueType::Closure
            ) {
                super::checked_args::argument_error(
                    eg,
                    "TypeError",
                    format!(
                        "fputcsv(): Argument #6 ($eol) must be of type ?string, {} given",
                        value.type_name()
                    ),
                );
                return Ok(());
            }
            super::argument_string(execute_data, 5)
        }
        None => Cow::Borrowed("\n"),
    };
    let eol = super::super::php_string_to_bytes(eol.as_ref());

    if super::optional_argument(execute_data, 4).is_none() {
        super::super::report_internal_deprecation(
            eg,
            execute_data,
            "fputcsv(): the $escape parameter must be provided as its default value will change",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }

    let mut encoder = CsvEncoder::new(separator, enclosure, escape);
    for field in fields.values() {
        let field = if field.is_reference() {
            unsafe { &*field.as_ref_ptr() }
        } else {
            field
        };
        if field.value_type() == ValueType::Array {
            super::super::report_internal_diagnostic(
                eg,
                execute_data,
                2,
                "Warning",
                "Array to string conversion",
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        let string = match field.as_str() {
            Some(string) => Cow::Borrowed(string),
            None => Cow::Owned(field.echo_to_string()),
        };
        let bytes = super::super::php_string_to_bytes(string.as_ref());
        if encoder.push_field(&bytes).is_err() {
            return super::return_value(return_pointer, Value::bool(false));
        }
    }
    let Ok(record) = encoder.finish(&eol) else {
        return super::return_value(return_pointer, Value::bool(false));
    };

    let record_length = record.len();
    let result = super::with_stream_io(eg, resource, |stream| {
        let mut remaining = record.as_slice();
        while !remaining.is_empty() {
            let written = stream.write(remaining)?;
            if written == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write complete CSV record",
                ));
            }
            remaining = &remaining[written..];
        }
        Ok(record_length)
    });
    match result {
        Some(Ok(written)) if written <= i64::MAX as usize => {
            super::return_value(return_pointer, Value::long(written as i64))
        }
        _ => super::return_value(return_pointer, Value::bool(false)),
    }
}

/// Incremental encoder for one `fputcsv()` record.
struct CsvEncoder {
    separator: u8,
    enclosure: u8,
    escape: Option<u8>,
    bytes: Vec<u8>,
    first_field: bool,
}

impl CsvEncoder {
    fn new(separator: u8, enclosure: u8, escape: Option<u8>) -> Self {
        Self {
            separator,
            enclosure,
            escape,
            bytes: Vec::new(),
            first_field: true,
        }
    }

    fn push_field(&mut self, field: &[u8]) -> io::Result<()> {
        let quote = field.iter().copied().any(|byte| {
            byte == self.separator
                || byte == self.enclosure
                || self.escape == Some(byte)
                || matches!(byte, b'\n' | b'\r' | b'\t' | b' ')
        });
        let separator_bytes = usize::from(!self.first_field);
        let quote_bytes = if quote {
            field.len().saturating_add(2)
        } else {
            0
        };
        self.bytes
            .try_reserve(
                separator_bytes
                    .saturating_add(field.len())
                    .saturating_add(quote_bytes),
            )
            .map_err(allocation_error)?;

        if !self.first_field {
            self.bytes.push(self.separator);
        }
        self.first_field = false;
        if !quote {
            self.bytes.extend_from_slice(field);
            return Ok(());
        }

        self.bytes.push(self.enclosure);
        let mut escaped = false;
        for &byte in field {
            if self.escape == Some(byte) {
                escaped = true;
            } else if byte == self.enclosure && !escaped {
                self.bytes.push(self.enclosure);
            } else {
                escaped = false;
            }
            self.bytes.push(byte);
        }
        self.bytes.push(self.enclosure);
        Ok(())
    }

    fn finish(mut self, eol: &[u8]) -> io::Result<Vec<u8>> {
        self.bytes
            .try_reserve(eol.len())
            .map_err(allocation_error)?;
        self.bytes.extend_from_slice(eol);
        Ok(self.bytes)
    }
}

fn allocation_error(_: std::collections::TryReserveError) -> io::Error {
    io::Error::new(io::ErrorKind::OutOfMemory, "CSV record allocation failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_php_whitespace_and_special_bytes() {
        let mut encoder = CsvEncoder::new(b',', b'"', Some(b'\\'));
        for field in [
            b"plain".as_slice(),
            b"a b",
            b"a\tb",
            b"a,b",
            b"a\"b",
            b"a\\b",
            b"a\nb",
            b"",
        ] {
            encoder.push_field(field).unwrap();
        }
        assert_eq!(
            encoder.finish(b"\n").unwrap(),
            b"plain,\"a b\",\"a\tb\",\"a,b\",\"a\"\"b\",\"a\\b\",\"a\nb\",\n"
        );
    }

    #[test]
    fn preserves_php_escape_and_custom_eol_rules() {
        let mut escaped = CsvEncoder::new(b',', b'"', Some(b'\\'));
        escaped.push_field(b"a\\\"b").unwrap();
        escaped.push_field(b"a\\\\b").unwrap();
        escaped.push_field(b"a\"b").unwrap();
        assert_eq!(
            escaped.finish(b"<EOL>").unwrap(),
            b"\"a\\\"b\",\"a\\\\b\",\"a\"\"b\"<EOL>"
        );

        let mut no_escape = CsvEncoder::new(b',', b'"', None);
        no_escape.push_field(b"a\\\"b").unwrap();
        no_escape.push_field(b"a\\\\b").unwrap();
        assert_eq!(no_escape.finish(b"").unwrap(), b"\"a\\\"\"b\",a\\\\b");
    }

    #[test]
    fn allows_an_empty_record_and_empty_eol() {
        let encoder = CsvEncoder::new(b';', b'~', None);
        assert_eq!(encoder.finish(b"").unwrap(), Vec::<u8>::new());
    }
}
