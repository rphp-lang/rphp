//! CSV records share the stream byte parser/encoder, but retain the file
//! object's distinct cached-record, logical-key and control provenance rules.
use super::*;
use crate::stdlib::stream::{CsvEncoder, CsvParser};

#[derive(Clone, Copy)]
pub(super) struct Controls {
    pub(super) separator: u8,
    pub(super) enclosure: u8,
    escape: Option<u8>,
    explicit_escape: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            separator: b',',
            enclosure: b'"',
            escape: Some(b'\\'),
            explicit_escape: false,
        }
    }
}

#[cold]
fn control_arguments(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
    first: u32,
) -> Result<Option<[Option<Value>; 3]>, VmError> {
    let mut values = [None, None, None];
    for (offset, name) in ["separator", "enclosure", "escape"].into_iter().enumerate() {
        let slot = first + offset as u32;
        if arg_opt!(ed, slot).is_some() {
            let argument = owned_argument(ed, slot);
            let Some(value) = typed_internal_string_value_expected(
                ed,
                eg,
                &argument,
                method,
                slot - 1,
                name,
                "string",
                "string",
            )?
            else {
                return Ok(None);
            };
            values[offset] = Some(value);
        }
    }
    Ok(Some(values))
}

#[cold]
fn validate_controls(
    eg: &mut ExecutorGlobals,
    method: &str,
    first: u32,
    defaults: Controls,
    values: [Option<Value>; 3],
) -> Option<Controls> {
    let mut result = defaults;
    for (offset, value) in values.iter().enumerate() {
        let Some(value) = value else {
            continue;
        };
        let (bytes, _) = php_bytes_after_weak_string_coercion(value);
        if bytes.len() != 1 && !(offset == 2 && bytes.is_empty()) {
            let name = ["separator", "enclosure", "escape"][offset];
            let bound = if offset == 2 {
                "empty or a single character"
            } else {
                "a single character"
            };
            error(
                eg,
                "ValueError",
                &format!(
                    "{method}(): Argument #{} (${name}) must be {bound}",
                    first + offset as u32
                ),
            );
            return None;
        }
        match offset {
            0 => result.separator = bytes[0],
            1 => result.enclosure = bytes[0],
            _ => {
                result.escape = bytes.first().copied();
                result.explicit_escape = true;
            }
        }
    }
    Some(result)
}

#[cold]
fn escape_diagnostic(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
    controls: Controls,
) -> Result<(), VmError> {
    if !controls.explicit_escape {
        let message = if method == "SplFileObject::setCsvControl" {
            format!(
                "{method}(): the $escape parameter must be provided as its default value will change"
            )
        } else {
            format!(
                "{method}(): the $escape parameter must be provided, as its default value will change, either explicitly or via SplFileObject::setCsvControl()"
            )
        };
        report_internal_deprecation(eg, ed, &message)?;
    }
    Ok(())
}

#[cold]
pub(super) fn get_controls(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let controls = read(&receiver, |state| state.csv);
    let mut values = PhpArray::with_packed_capacity(3);
    values.push(php_byte_result(vec![controls.separator], false));
    values.push(php_byte_result(vec![controls.enclosure], false));
    values.push(php_byte_result(
        controls.escape.into_iter().collect(),
        false,
    ));
    ret!(rv, Value::array(values));
}

#[cold]
pub(super) fn set_controls(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let method = "SplFileObject::setCsvControl";
    let Some(arguments) = control_arguments(ed, eg, method, 1)? else {
        return Ok(());
    };
    let Some(controls) = validate_controls(eg, method, 1, Controls::default(), arguments) else {
        return Ok(());
    };
    escape_diagnostic(ed, eg, method, controls)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    write(&receiver, |state| state.csv = controls);
    ret!(rv, Value::null());
}

/// Read raw physical bytes. CSV retains embedded line endings; DROP_NEW_LINE
/// is only an empty-line selection rule, never a mutation of quoted content.
#[cold]
pub(super) fn segment(
    ed: *mut ExecuteData,
    receiver: &Value,
    eg: &mut ExecutorGlobals,
    maximum: usize,
    operation: &str,
    require_line: bool,
) -> Result<Option<(Vec<u8>, bool)>, VmError> {
    #[cfg(not(feature = "stream-registry"))]
    let _ = require_line;
    let backend = read(receiver, |state| state.backend.clone());
    let result = match backend {
        Backend::Native(stream) => {
            let mut bytes = Vec::new();
            let (status, eof) = {
                let mut stream = stream.borrow_mut();
                let status = stream.read_line(
                    &mut bytes,
                    (maximum != 0).then(|| maximum.saturating_add(1)),
                );
                if stream.take_plain_file_io() {
                    filesystem::clear_filesystem_stat_cache(eg);
                }
                (status, stream.is_eof())
            };
            if let Err(failure) = status {
                let errno = failure.raw_os_error().unwrap_or(9);
                let reason = if errno == 9 {
                    "Bad file descriptor".to_string()
                } else {
                    filesystem::filesystem_error_reason(&failure).to_string()
                };
                report_internal_diagnostic(
                    eg,
                    ed,
                    8,
                    "Notice",
                    &format!(
                        "{operation}(): Read of 8192 bytes failed with errno={errno} {reason}"
                    ),
                )?;
            }
            Some((bytes, eof))
        }
        #[cfg(feature = "stream-registry")]
        Backend::Wrapper(resource) => crate::stdlib::streams::user_wrapper::read_file_object_line(
            eg,
            resource.as_resource_id().unwrap(),
            maximum,
            require_line,
        )?,
    };
    if let Some((_, eof)) = result.as_ref() {
        write(receiver, |state| state.eof = *eof);
    }
    if eg.exception.is_some() {
        return Ok(None);
    }
    Ok(result)
}

#[cold]
pub(super) fn fetch(
    ed: *mut ExecuteData,
    receiver: &Value,
    eg: &mut ExecutorGlobals,
    controls: Controls,
    advance: bool,
    operation: &str,
) -> Result<Option<Value>, VmError> {
    if read(receiver, |state| state.eof) {
        write(receiver, |state| {
            state.cache = None;
            state.csv_line = None;
        });
        return Ok(None);
    }
    write(receiver, |state| {
        if advance && state.cache.is_some() {
            state.index = state.index.wrapping_add(1);
        }
        state.cache = None;
        state.csv_line = None;
    });
    loop {
        let (maximum, flags) = read(receiver, |state| (state.maximum, state.flags));
        let Some((bytes, mut eof)) = segment(ed, receiver, eg, maximum, operation, false)? else {
            write(receiver, |state| {
                state.csv_read_failed = true;
                state.cache = Some(Value::bool(false));
            });
            return Ok(None);
        };
        let empty = bytes.is_empty()
            || (flags & DROP_NEW_LINE != 0 && matches!(bytes.as_slice(), b"\n" | b"\r\n"));
        if flags & SKIP_EMPTY != 0 && empty {
            if eof {
                return Ok(None);
            }
            write(receiver, |state| state.index = state.index.wrapping_add(1));
            continue;
        }
        let mut parser = CsvParser::new(controls.separator, controls.enclosure, controls.escape);
        if parser.push_segment(&bytes).is_err() {
            return Ok(None);
        }
        // The string projection is the first physical line, not a re-encoded
        // CSV record. Move this buffer into the cache after parsing continues.
        let mut first_line = bytes;
        while parser.needs_continuation() && !eof {
            let Some((bytes, next_eof)) = segment(ed, receiver, eg, 0, operation, false)? else {
                return Ok(None);
            };
            eof = next_eof;
            if bytes.is_empty() {
                break;
            }
            if parser.push_segment(&bytes).is_err() {
                return Ok(None);
            }
        }
        let Ok(fields) = parser.finish(eof) else {
            return Ok(None);
        };
        let mut array = PhpArray::with_packed_capacity(fields.len());
        for field in fields {
            array.push(field.map_or_else(Value::null, |bytes| php_byte_result(bytes, false)));
        }
        let value = Value::array(array);
        if flags & DROP_NEW_LINE != 0 && first_line.last() == Some(&b'\n') {
            first_line.pop();
            if first_line.last() == Some(&b'\r') {
                first_line.pop();
            }
        }
        write(receiver, |state| {
            state.cache = Some(value.clone());
            state.csv_line = Some(first_line);
            state.csv_read_failed = false;
        });
        return Ok(Some(value));
    }
}

#[cold]
pub(super) fn get_record(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let defaults = read(&receiver, |state| state.csv);
    let method = "SplFileObject::fgetcsv";
    let Some(arguments) = control_arguments(ed, eg, method, 1)? else {
        return Ok(());
    };
    let Some(controls) = validate_controls(eg, method, 1, defaults, arguments) else {
        return Ok(());
    };
    escape_diagnostic(ed, eg, method, controls)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let value =
        fetch(ed, &receiver, eg, controls, true, method)?.unwrap_or_else(|| Value::bool(false));
    ret!(rv, value);
}

#[cold]
pub(super) fn put_record(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let method = "SplFileObject::fputcsv";
    let fields = owned_argument(ed, 1);
    let Some(array) = fields.as_array() else {
        typed_internal_argument_error(eg, method, &fields, 1, "fields", "array");
        return Ok(());
    };
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let defaults = read(&receiver, |state| state.csv);
    let Some(arguments) = control_arguments(ed, eg, method, 2)? else {
        return Ok(());
    };
    let eol = if arg_opt!(ed, 5).is_some() {
        let argument = owned_argument(ed, 5);
        let Some(value) = typed_internal_string_value_expected(
            ed, eg, &argument, method, 4, "eol", "string", "string",
        )?
        else {
            return Ok(());
        };
        value
    } else {
        Value::string("\n")
    };
    let Some(controls) = validate_controls(eg, method, 2, defaults, arguments) else {
        return Ok(());
    };
    escape_diagnostic(ed, eg, method, controls)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let mut encoder = CsvEncoder::new(controls.separator, controls.enclosure, controls.escape);
    for field in array.values() {
        let Some(value) = internal_value_to_string_value(ed, eg, field)? else {
            return Ok(());
        };
        let (bytes, _) = php_bytes_after_weak_string_coercion(&value);
        if encoder.push_field(&bytes).is_err() {
            ret!(rv, Value::bool(false));
        }
    }
    let (eol, _) = php_bytes_after_weak_string_coercion(&eol);
    let Ok(bytes) = encoder.finish(&eol) else {
        ret!(rv, Value::bool(false));
    };
    let backend = read(&receiver, |state| state.backend.clone());
    let count = match backend {
        Backend::Native(stream) => {
            let mut written = 0;
            loop {
                if written == bytes.len() {
                    break Some(written);
                }
                let result = {
                    let mut stream = stream.borrow_mut();
                    let result = stream.write(&bytes[written..]);
                    if stream.take_plain_file_io() {
                        filesystem::clear_filesystem_stat_cache(eg);
                    }
                    result
                };
                match result {
                    Ok(0) => break Some(written),
                    Ok(count) => written += count,
                    Err(failure)
                        if failure.kind() == std::io::ErrorKind::PermissionDenied
                            && stream.borrow().metadata().stream_type != "STDIO" =>
                    {
                        break (written != 0).then_some(written);
                    }
                    Err(failure) => {
                        let errno = failure.raw_os_error().unwrap_or(9);
                        let reason = if errno == 9 {
                            "Bad file descriptor".to_string()
                        } else {
                            filesystem::filesystem_error_reason(&failure).to_string()
                        };
                        report_internal_diagnostic(
                            eg,
                            ed,
                            8,
                            "Notice",
                            &format!(
                                "{method}(): Write of {} bytes failed with errno={errno} {reason}",
                                bytes.len() - written
                            ),
                        )?;
                        break (written != 0).then_some(written);
                    }
                }
            }
        }
        #[cfg(feature = "stream-registry")]
        Backend::Wrapper(resource) => {
            crate::stdlib::streams::user_wrapper::write_file_object_record(
                eg,
                ed,
                resource.as_resource_id().unwrap(),
                &bytes,
                method,
            )?
        }
    };
    ret!(
        rv,
        count.map_or_else(|| Value::bool(false), |count| Value::long(count as i64))
    );
}
