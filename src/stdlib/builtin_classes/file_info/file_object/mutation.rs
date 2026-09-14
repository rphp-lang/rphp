//! Stream mutations preserve the cached logical line and iterator EOF. The
//! underlying physical cursor owns read-after-write and truncation behavior.
use super::*;

#[cold]
pub(super) fn construct_temporary(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let supplied = arg_opt!(ed, 1).is_some();
    let maximum = if supplied {
        let Some(value) = int_argument(ed, eg, "SplTempFileObject::__construct", "maxMemory")?
        else {
            return Ok(());
        };
        value
    } else {
        2 * 1024 * 1024
    };
    let receiver = owned_argument(ed, 0);
    if receiver
        .as_object()
        .unwrap()
        .native_file_info()
        .is_some_and(|s| s.file.is_some())
    {
        error(eg, "Error", "Cannot call constructor twice");
        return Ok(());
    }
    let path = if maximum < 0 {
        "php://memory".to_string()
    } else if !supplied {
        "php://temp".to_string()
    } else {
        format!("php://temp/maxmemory:{maximum}")
    };
    let Some(backend) = open_native(eg, &path, path.as_bytes(), "w+b") else {
        return Ok(());
    };
    initialize(&receiver, eg, path.into_bytes(), backend, true);
    ret!(rv, Value::null());
}

#[cold]
pub(super) fn bytes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let method = "SplFileObject::fwrite";
    let argument = owned_argument(ed, 1);
    let Some(converted) = typed_internal_string_value_expected(
        ed, eg, &argument, method, 0, "data", "string", "string",
    )?
    else {
        return Ok(());
    };
    // Retain the converted PHP value across callbacks; ordinary strings can
    // lend their bytes directly instead of allocating a second write buffer.
    let (data, _) = php_bytes_after_weak_string_coercion(&converted);
    let length = if let Some(value) = arg_opt!(ed, 2)
        && value.value_type() != ValueType::Null
    {
        let argument = owned_argument(ed, 2);
        let Some(length) = typed_internal_int_value_argument_expected(
            ed, eg, &argument, method, 1, "length", "?int",
        )?
        else {
            return Ok(());
        };
        (length.max(0) as usize).min(data.len())
    } else {
        data.len()
    };
    if length == 0 {
        ret!(rv, Value::long(0));
    }
    let backend = read(&receiver, |state| state.backend.clone());
    let written = match backend {
        Backend::Native(stream) => {
            let result = {
                let mut stream = stream.borrow_mut();
                let result = stream.write(&data[..length]);
                if stream.take_plain_file_io() {
                    filesystem::clear_filesystem_stat_cache(eg);
                }
                result
            };
            match result {
                Ok(count) => Some(count),
                Err(failure) => {
                    let errno = failure.raw_os_error().unwrap_or_else(|| {
                        if failure.kind() == std::io::ErrorKind::PermissionDenied {
                            9
                        } else {
                            0
                        }
                    });
                    let reason = if errno == 9 {
                        "Bad file descriptor".to_string()
                    } else {
                        filesystem::filesystem_error_reason(&failure)
                    };
                    report_internal_diagnostic(
                        eg,
                        ed,
                        8,
                        "Notice",
                        &format!(
                            "{method}(): Write of {length} bytes failed with errno={errno} {reason}"
                        ),
                    )?;
                    None
                }
            }
        }
        #[cfg(feature = "stream-registry")]
        Backend::Wrapper(value) => crate::stdlib::streams::user_wrapper::write_file_object_record(
            eg,
            ed,
            value.as_resource_id().unwrap(),
            &data[..length],
            method,
        )?,
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(
        rv,
        written.map_or_else(|| Value::bool(false), |n| Value::long(n as i64))
    );
}

#[cold]
pub(super) fn flush(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let backend = read(&receiver, |state| state.backend.clone());
    let success = match backend {
        Backend::Native(stream) => {
            let mut stream = stream.borrow_mut();
            let success = stream.flush().is_ok();
            if stream.take_plain_file_io() {
                filesystem::clear_filesystem_stat_cache(eg);
            }
            success
        }
        #[cfg(feature = "stream-registry")]
        Backend::Wrapper(value) => {
            crate::stdlib::streams::user_wrapper::flush(eg, value.as_resource_id().unwrap())?
                .unwrap_or(false)
        }
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, Value::bool(success));
}

#[cold]
pub(super) fn truncate(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let Some(length) = int_argument(ed, eg, "SplFileObject::ftruncate", "size")? else {
        return Ok(());
    };
    if length < 0 {
        error(
            eg,
            "ValueError",
            "SplFileObject::ftruncate(): Argument #1 ($size) must be greater than or equal to 0",
        );
        return Ok(());
    }
    let backend = read(&receiver, |state| state.backend.clone());
    let success = match backend {
        Backend::Native(stream) => stream.borrow_mut().truncate(length as u64).is_ok(),
        #[cfg(feature = "stream-registry")]
        Backend::Wrapper(value) => {
            match crate::stdlib::streams::user_wrapper::file_object_truncate(
                eg,
                value.as_resource_id().unwrap(),
                length,
            )? {
                Some(success) => success,
                None => {
                    if eg.exception.is_none() {
                        let path = super::super::path(&receiver, eg).unwrap_or_default();
                        error(
                            eg,
                            "LogicException",
                            &format!("Can't truncate file {}", bytes_to_php_string(&path)),
                        );
                    }
                    return Ok(());
                }
            }
        }
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, Value::bool(success));
}

#[cold]
pub(super) fn stat(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let backend = read(&receiver, |state| state.backend.clone());
    let value = match backend {
        Backend::Native(stream) => match stream.borrow().stat() {
            Ok(Some(crate::stdlib::stream::StreamStat::Filesystem(metadata))) => {
                filesystem::stat_array_value(filesystem::metadata_stat_fields(&metadata))
            }
            Ok(Some(crate::stdlib::stream::StreamStat::Memory { size, writable })) => {
                filesystem::stat_array_value(crate::stdlib::streams::virtual_stream_stat_fields(
                    size, writable,
                ))
            }
            Ok(None) | Err(_) => Value::bool(false),
        },
        #[cfg(feature = "stream-registry")]
        Backend::Wrapper(value) => crate::stdlib::streams::user_wrapper::stat_for(
            eg,
            ed,
            value.as_resource_id().unwrap(),
            "SplFileObject::fstat",
        )?
        .map_or_else(|| Value::bool(false), filesystem::normalize_wrapper_stat),
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, value);
}

#[cold]
pub(super) fn lock(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let Some(operation) = int_argument(ed, eg, "SplFileObject::flock", "operation")? else {
        return Ok(());
    };
    if operation & 3 == 0 {
        error(
            eg,
            "ValueError",
            "SplFileObject::flock(): Argument #1 ($operation) must be one of LOCK_SH, LOCK_EX, or LOCK_UN",
        );
        return Ok(());
    }
    let by_ref = arg_opt!(ed, 2).is_some();
    if by_ref {
        crate::stdlib::streams::set_argument(ed, 2, Value::long(0));
    }
    let backend = read(&receiver, |state| state.backend.clone());
    let success = match backend {
        Backend::Native(stream) => match stream.borrow().lock(operation) {
            Ok(()) => true,
            Err(failure) => {
                if by_ref && failure.kind() == std::io::ErrorKind::WouldBlock {
                    crate::stdlib::streams::set_argument(ed, 2, Value::long(1));
                }
                false
            }
        },
        #[cfg(feature = "stream-registry")]
        Backend::Wrapper(value) => crate::stdlib::streams::user_wrapper::file_object_lock(
            eg,
            ed,
            value.as_resource_id().unwrap(),
            operation & 7,
        )?,
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, Value::bool(success));
}
