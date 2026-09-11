//! URI parsing is paid once at open. Thereafter filtered and ordinary streams
//! use the same resource, byte-I/O and exception-safe close contracts.

use super::{ExecutorGlobals, Value, VmError, lifecycle};
use crate::stdlib::stream::PhpStream;
use crate::stdlib::{self, resource, streams};
use crate::vm::frame::ExecuteData;
use std::io::SeekFrom;

// Keep the short length/prefix rejection in each opener. Outlining this
// predicate spills the opener's live owners even for ordinary native streams.
#[inline(always)]
pub(in crate::stdlib) fn recognizes(path: &str) -> bool {
    path.as_bytes()
        .get(..13)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"php://filter/"))
}

#[cfg(test)]
mod prefix_tests {
    #[test]
    fn filter_prefix_is_ascii_insensitive_and_requires_its_full_delimiter() {
        for path in ["php://filter/", "PHP://FiLtEr/resource=php://memory"] {
            assert!(super::recognizes(path));
        }
        for path in [
            "",
            "php://memory",
            "php://filter",
            "php://filters/",
            "php://filterX",
            "éphp://filter/",
        ] {
            assert!(!super::recognizes(path));
        }
        let prefix = "php://filter/";
        for end in 0..prefix.len() {
            assert!(!super::recognizes(&prefix[..end]));
        }
    }
}

#[cold]
pub(in crate::stdlib) fn open_internal(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    path: &str,
    mode: &str,
) -> Result<Value, VmError> {
    super::with_source(eg, frame, |eg| {
        Ok(open(eg, frame, path, mode)?.unwrap_or_else(|| Value::bool(false)))
    })
}

#[cold]
fn open(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    path: &str,
    mode: &str,
) -> Result<Option<Value>, VmError> {
    // Nested filter wrappers are peeled iteratively, avoiding recursive open
    // frames. Inner filters attach first, as if each wrapper opened its input.
    let mut underlying = path;
    let mut layers = Vec::new();
    while recognizes(underlying) {
        let Some(resource_at) = underlying[12..].find("/resource=").map(|i| i + 12) else {
            eg.exception = Some(crate::value::make_error_value(
                "Error",
                "No URL resource specified",
            ));
            return Ok(None);
        };
        let filter_end = if resource_at == 12 {
            underlying.len()
        } else {
            resource_at
        };
        layers.push(&underlying[13..filter_end]);
        underlying = &underlying[resource_at + 10..];
    }
    if underlying.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "Path must not be empty",
        ));
        return Ok(None);
    }
    let function = super::diagnostic_function(eg).to_string();
    if layers.is_empty() {
        if !stdlib::filesystem::url_open_allowed(frame, eg, underlying, &function)? {
            return Ok(None);
        }
    } else if underlying.starts_with("data:") && !stdlib::filesystem::url_fopen_enabled(eg) {
        lifecycle::warn(
            eg,
            &format!("{function}({path}): Failed to open stream: operation failed"),
        )?;
        return Ok(None);
    }
    let backend = match PhpStream::open(underlying, mode) {
        Ok(backend) => backend,
        Err(error) => {
            let reason = if !layers.is_empty() && underlying.starts_with("data:") {
                "operation failed".to_string()
            } else {
                match error.kind() {
                    std::io::ErrorKind::NotFound => "No such file or directory".into(),
                    std::io::ErrorKind::PermissionDenied => "Permission denied".into(),
                    _ => error.to_string(),
                }
            };
            lifecycle::warn(
                eg,
                &format!(
                    "{}({path}): Failed to open stream: {reason}",
                    super::diagnostic_function(eg)
                ),
            )?;
            return Ok(None);
        }
    };
    if backend.is_plain_file() {
        stdlib::filesystem::clear_filesystem_stat_cache(eg);
    }
    let default_mode = i64::from(backend.is_readable()) | (i64::from(backend.is_writable()) << 1);
    let owner = streams::insert_stream(eg, backend);
    let result = (|| {
        let function = super::diagnostic_function(eg).to_string();
        for layer in layers.into_iter().rev() {
            for component in layer.split('/').filter(|part| !part.is_empty()) {
                let (names, direction) = if component
                    .get(..5)
                    .is_some_and(|s| s.eq_ignore_ascii_case("read="))
                {
                    (&component[5..], 1)
                } else if component
                    .get(..6)
                    .is_some_and(|s| s.eq_ignore_ascii_case("write="))
                {
                    (&component[6..], 2)
                } else {
                    (component, default_mode)
                };
                for encoded in names.split('|') {
                    let bytes = stdlib::percent_decode_php_bytes(encoded.as_bytes(), true);
                    let name = stdlib::bytes_to_php_string(&bytes);
                    let attached = lifecycle::attach_value(
                        eg,
                        frame,
                        &owner,
                        &name,
                        direction,
                        Value::null(),
                        false,
                        &function,
                        false,
                    )?;
                    if attached.value_type() == crate::value::ValueType::False {
                        lifecycle::warn(
                            eg,
                            &format!("{function}(): Unable to create filter ({name})"),
                        )?;
                    }
                    if eg.exception.is_some() {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    })();
    match result {
        Ok(true) => Ok(Some(owner)),
        other => {
            let cleanup = close(eg, owner.as_resource_id().expect("opened stream"));
            other?;
            cleanup?;
            Ok(None)
        }
    }
}

#[cold]
pub(in crate::stdlib) fn close(eg: &mut ExecutorGlobals, id: i64) -> Result<(), VmError> {
    if lifecycle::close(eg, id, false)?.is_none() {
        resource::close_for_request::<PhpStream>(eg, id);
    }
    Ok(())
}

#[cold]
fn read(eg: &mut ExecutorGlobals, id: i64, size: usize) -> Result<Option<Vec<u8>>, VmError> {
    let mut bytes = vec![0; size];
    if let Some(result) = streams::with_stream_io(eg, id, |backend| backend.read(&mut bytes)) {
        return Ok(result.ok().map(|read| {
            bytes.truncate(read);
            bytes
        }));
    }
    lifecycle::read(eg, id, size)
}

/// `None` means stream failure; an empty successful read is distinct. Always
/// retire the descriptor, even if PHP throws while creating/reading/closing.
#[cold]
#[cfg(feature = "file-contents")]
fn consume(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    path: &str,
    offset: i64,
    length: Option<usize>,
    output: bool,
) -> Result<Option<(Vec<u8>, usize)>, VmError> {
    let Some(owner) = open(eg, frame, path, "rb")? else {
        return Ok(None);
    };
    let id = owner.as_resource_id().expect("opened stream");
    let result = consume_opened(eg, id, offset, length, output);
    let cleanup = close(eg, id);
    let value = result?;
    cleanup?;
    Ok(value)
}

#[cold]
fn consume_opened(
    eg: &mut ExecutorGlobals,
    id: i64,
    offset: i64,
    length: Option<usize>,
    output: bool,
) -> Result<Option<(Vec<u8>, usize)>, VmError> {
    if offset != 0 {
        let position = if offset < 0 {
            SeekFrom::End(offset)
        } else {
            SeekFrom::Start(offset as u64)
        };
        let success = match streams::with_stream_io(eg, id, |s| s.seek(position)) {
            Some(result) => result.is_ok(),
            None => lifecycle::seek(eg, id, position)? == Some(true),
        };
        if !success || eg.exception.is_some() {
            return Ok(None);
        }
    }
    let mut bytes = Vec::new();
    let mut count = 0;
    let limit = length.unwrap_or(usize::MAX);
    while count < limit {
        let Some(chunk) = read(eg, id, (limit - count).min(8192))? else {
            return Ok(None);
        };
        if eg.exception.is_some() {
            return Ok(None);
        }
        if chunk.is_empty() {
            break;
        }
        count += chunk.len();
        if output {
            eg.write_output(&chunk);
            if eg.exception.is_some() {
                return Ok(None);
            }
        } else {
            bytes.extend_from_slice(&chunk)
        }
    }
    Ok(Some((bytes, count)))
}

#[cold]
#[cfg(feature = "file-contents")]
pub(in crate::stdlib) fn contents(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    path: &str,
    offset: i64,
    length: Option<usize>,
) -> Result<Value, VmError> {
    super::with_source(eg, frame, |eg| {
        Ok(
            consume(eg, frame, path, offset, length, false)?.map_or_else(
                || Value::bool(false),
                |(bytes, _)| stdlib::php_byte_result(bytes, false),
            ),
        )
    })
}

#[cold]
#[cfg(feature = "file-contents")]
pub(in crate::stdlib) fn output(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    path: &str,
) -> Result<Value, VmError> {
    super::with_source(eg, frame, |eg| {
        Ok(consume(eg, frame, path, 0, None, true)?.map_or_else(
            || Value::bool(false),
            |(_, count)| Value::long(count as i64),
        ))
    })
}

#[cold]
pub(in crate::stdlib) fn include(
    eg: &mut ExecutorGlobals,
    path: &str,
    origin: (String, usize),
    operation: &str,
    factory: &str,
) -> Result<streams::user_wrapper::IncludeOpenResult, VmError> {
    let frame = eg.current_execute_data.get();
    let mut canonical = path;
    while recognizes(canonical) {
        let Some(index) = canonical[12..].find("/resource=").map(|i| i + 12) else {
            break;
        };
        canonical = &canonical[index + 10..];
    }
    if canonical.starts_with("data:")
        && !stdlib::ini_default(eg, "allow_url_include")
            .is_some_and(|value| stdlib::ini_boolean(&value))
    {
        super::with_origin(eg, origin.0, origin.1, operation.into(), |eg| {
            lifecycle::warn(
                eg,
                &format!("{operation}({path}): Failed to open stream: operation failed"),
            )
        })?;
        return Ok(streams::user_wrapper::IncludeOpenResult::ReadFailed {
            stream: Value::null(),
        });
    }
    let canonical = std::fs::canonicalize(canonical)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| canonical.into());
    let opened = super::with_origin(eg, origin.0.clone(), origin.1, factory.into(), |eg| {
        open(eg, frame, path, "rb")
    })?;
    let Some(owner) = opened else {
        return Ok(streams::user_wrapper::IncludeOpenResult::ReadFailed {
            stream: Value::null(),
        });
    };
    let id = owner.as_resource_id().expect("opened include stream");
    super::with_origin(eg, origin.0, origin.1, operation.into(), |eg| {
        let read = consume_opened(eg, id, 0, None, false);
        if matches!(read, Ok(None)) && eg.exception.is_none() {
            // The include caller must report failure before onClose. A require
            // error (or warning-handler exception) suppresses that callback.
            return Ok(streams::user_wrapper::IncludeOpenResult::ReadFailed { stream: owner });
        }
        let cleanup = close(eg, id);
        let source = read?.map(|(bytes, _)| bytes).unwrap_or_default();
        cleanup?;
        Ok(streams::user_wrapper::IncludeOpenResult::Opened { source, canonical })
    })
}
