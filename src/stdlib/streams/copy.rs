use std::io::SeekFrom;

use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::checked_args::{
    argument_error, given_type_name, stream_argument_at, weak_long_argument,
};

const COPY_CHUNK_SIZE: usize = 8 * 1024;

#[cold]
pub(super) fn fn_stream_copy_to_stream(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(source) = stream_argument_at(execute_data, eg, "stream_copy_to_stream", 0, "from")
    else {
        return Ok(());
    };
    let Some(destination) = stream_argument_at(execute_data, eg, "stream_copy_to_stream", 1, "to")
    else {
        return Ok(());
    };

    let length = match super::optional_argument(execute_data, 2) {
        Some(value) if value.value_type() == ValueType::Null => None,
        Some(value) => {
            let Some(length) = weak_long_argument(value) else {
                argument_error(
                    eg,
                    "TypeError",
                    format!(
                        "stream_copy_to_stream(): Argument #3 ($length) must be of type ?int, {} given",
                        given_type_name(value)
                    ),
                );
                return Ok(());
            };
            if length < 0 {
                None
            } else {
                let Ok(length) = usize::try_from(length) else {
                    return super::return_value(return_pointer, Value::bool(false));
                };
                Some(length)
            }
        }
        None => None,
    };

    let offset = match super::optional_argument(execute_data, 3) {
        Some(value) => {
            let Some(offset) = weak_long_argument(value) else {
                argument_error(
                    eg,
                    "TypeError",
                    format!(
                        "stream_copy_to_stream(): Argument #4 ($offset) must be of type int, {} given",
                        given_type_name(value)
                    ),
                );
                return Ok(());
            };
            (offset > 0).then_some(offset as u64)
        }
        None => None,
    };

    if let Some(offset) = offset {
        let seek = super::with_stream(eg, source, |stream| stream.seek(SeekFrom::Start(offset)));
        if !matches!(seek, Some(Ok(_))) {
            return super::return_value(return_pointer, Value::bool(false));
        }
    }

    let mut chunk = [0u8; COPY_CHUNK_SIZE];
    let mut copied = 0usize;
    loop {
        let requested = length
            .map(|length| length.saturating_sub(copied))
            .unwrap_or(chunk.len())
            .min(chunk.len());
        if requested == 0 {
            break;
        }

        let read = super::with_stream(eg, source, |stream| stream.read(&mut chunk[..requested]));
        let read = match read {
            Some(Ok(read)) => read,
            _ => return super::return_value(return_pointer, Value::bool(false)),
        };
        if read == 0 {
            break;
        }

        let mut written = 0;
        while written < read {
            let write = super::with_stream(eg, destination, |stream| {
                stream.write(&chunk[written..read])
            });
            match write {
                Some(Ok(0)) | Some(Err(_)) | None => {
                    return super::return_value(return_pointer, Value::bool(false));
                }
                Some(Ok(count)) => written += count,
            }
        }
        copied = match copied.checked_add(read) {
            Some(copied) => copied,
            None => return super::return_value(return_pointer, Value::bool(false)),
        };
    }

    match i64::try_from(copied) {
        Ok(copied) => super::return_value(return_pointer, Value::long(copied)),
        Err(_) => super::return_value(return_pointer, Value::bool(false)),
    }
}
