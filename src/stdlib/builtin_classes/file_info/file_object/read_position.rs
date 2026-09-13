//! Physical stream operations keep the existing unread buffer and file owner.
//! Only character/line reads change iterator cache/key; block reads do not.
use super::*;

#[cold]
fn clear_line(receiver: &Value) {
    write(receiver, |state| {
        state.cache = None;
        state.csv_line = None;
    });
}

#[cold]
fn read_notice(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
    failure: &std::io::Error,
) -> Result<(), VmError> {
    if let Some(errno) = failure.raw_os_error() {
        let reason = filesystem::filesystem_error_reason(failure);
        report_internal_diagnostic(
            eg,
            ed,
            8,
            "Notice",
            &format!("{method}(): Read of 8192 bytes failed with errno={errno} {reason}"),
        )?;
    }
    Ok(())
}

#[cold]
pub(super) fn tell(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    // Neither position projection can call PHP, so no owner snapshot is needed.
    let position = read(&receiver, |state| match &state.backend {
        Backend::Native(stream) => stream
            .borrow_mut()
            .position()
            .ok()
            .and_then(|n| i64::try_from(n).ok()),
        #[cfg(feature = "stream-registry")]
        Backend::Wrapper(value) => {
            crate::stdlib::streams::user_wrapper::position(eg, value.as_resource_id().unwrap())
        }
    });
    ret!(rv, position.map_or_else(|| Value::bool(false), Value::long));
}

#[cold]
pub(super) fn line(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    clear_line(&receiver);
    let (eof, maximum) = read(&receiver, |state| (state.eof, state.maximum));
    if eof {
        cannot_read(&receiver, eg);
        return Ok(());
    }
    let result = csv_records::segment(ed, &receiver, eg, maximum, "SplFileObject::fgets", true)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let mut bytes = result.map_or_else(Vec::new, |(bytes, _)| bytes);
    let flags = read(&receiver, |state| state.flags);
    if flags & DROP_NEW_LINE != 0 && bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    let value = php_byte_result(bytes, false);
    write(&receiver, |state| {
        state.cache = Some(value.clone());
        state.index = state.index.wrapping_add(1);
    });
    ret!(rv, value);
}

#[cold]
pub(super) fn byte(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    clear_line(&receiver);
    let backend = read(&receiver, |state| state.backend.clone());
    let byte = match backend {
        Backend::Native(stream) => {
            let mut buffer = [0u8; 1];
            let (result, eof) = {
                let mut stream = stream.borrow_mut();
                let result = stream.read(&mut buffer);
                if stream.take_plain_file_io() {
                    filesystem::clear_filesystem_stat_cache(eg);
                }
                (result, stream.is_eof())
            };
            write(&receiver, |state| state.eof = eof);
            match result {
                Ok(0) => None,
                Ok(_) => Some(buffer[0]),
                Err(failure) => {
                    read_notice(ed, eg, "SplFileObject::fgetc", &failure)?;
                    None
                }
            }
        }
        #[cfg(feature = "stream-registry")]
        Backend::Wrapper(value) => {
            let result = crate::stdlib::streams::user_wrapper::read_file_object_bytes(
                eg,
                value.as_resource_id().unwrap(),
                1,
            )?;
            result.and_then(|(bytes, eof)| {
                write(&receiver, |state| state.eof = eof);
                bytes.first().copied()
            })
        }
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    if byte == Some(b'\n') {
        write(&receiver, |state| state.index = state.index.wrapping_add(1));
    }
    ret!(
        rv,
        byte.map_or_else(|| Value::bool(false), |b| php_byte_result(vec![b], false))
    );
}

#[cold]
pub(super) fn bytes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let method = "SplFileObject::fread";
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let Some(length) = int_argument(ed, eg, method, "length")? else {
        return Ok(());
    };
    if length <= 0 {
        error(
            eg,
            "ValueError",
            "SplFileObject::fread(): Argument #1 ($length) must be greater than 0",
        );
        return Ok(());
    }
    let Ok(length) = usize::try_from(length) else {
        ret!(rv, Value::bool(false));
    };
    let backend = read(&receiver, |state| state.backend.clone());
    let bytes = match backend {
        Backend::Native(stream) => {
            let mut bytes = Vec::new();
            if bytes.try_reserve_exact(length).is_err() {
                ret!(rv, Value::bool(false));
            }
            let (result, eof) = {
                let mut stream = stream.borrow_mut();
                let result = stream.read_into_vec(&mut bytes, length);
                if stream.take_plain_file_io() {
                    filesystem::clear_filesystem_stat_cache(eg);
                }
                (result, stream.is_eof())
            };
            write(&receiver, |state| state.eof = eof);
            match result {
                Ok(_) => Some(bytes),
                Err(failure) => {
                    read_notice(ed, eg, method, &failure)?;
                    None
                }
            }
        }
        #[cfg(feature = "stream-registry")]
        Backend::Wrapper(value) => crate::stdlib::streams::user_wrapper::read_file_object_bytes(
            eg,
            value.as_resource_id().unwrap(),
            length,
        )?
        .map(|(bytes, eof)| {
            write(&receiver, |state| state.eof = eof);
            bytes
        }),
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(
        rv,
        bytes.map_or_else(|| Value::bool(false), |bytes| php_byte_result(bytes, false))
    );
}
