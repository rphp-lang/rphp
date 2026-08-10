use std::io;

use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::super::stream::PhpStream;
use super::super::streams::checked_args::{argument_error, given_type_name, weak_long_argument};
use super::{argument, optional_argument, return_value, string_argument};

const FILE_APPEND: i64 = 8;
const LOCK_EX: i64 = 2;
const WRITE_CHUNK_SIZE: usize = 8 * 1024;

#[cold]
pub(in crate::stdlib) fn fn_file_put_contents(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let filename_value = argument(execute_data, 0);
    let Some(filename) = string_argument(filename_value) else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "file_put_contents(): Argument #1 ($filename) must be of type string, {} given",
                given_type_name(filename_value)
            ),
        );
        return Ok(());
    };
    let data = argument(execute_data, 1);

    let flags = match optional_argument(execute_data, 2) {
        Some(value) => {
            let Some(flags) = weak_long_argument(value) else {
                argument_error(
                    eg,
                    "TypeError",
                    format!(
                        "file_put_contents(): Argument #3 ($flags) must be of type int, {} given",
                        given_type_name(value)
                    ),
                );
                return Ok(());
            };
            flags
        }
        None => 0,
    };

    if let Some(context) = optional_argument(execute_data, 3)
        && context.value_type() != ValueType::Null
    {
        if context.value_type() == ValueType::Resource {
            argument_error(
                eg,
                "TypeError",
                "file_put_contents(): supplied resource is not a valid Stream-Context resource"
                    .to_string(),
            );
        } else {
            argument_error(
                eg,
                "TypeError",
                format!(
                    "file_put_contents(): Argument #4 ($context) must be of type resource or null, {} given",
                    given_type_name(context)
                ),
            );
        }
        return Ok(());
    }

    let source_resource = if data.value_type() == ValueType::Resource {
        let Some(resource) = data.as_resource_id().filter(|resource| {
            super::super::resource::is_open_for_request(eg, *resource)
                && super::super::resource::type_for_request(eg, *resource) == "stream"
        }) else {
            argument_error(
                eg,
                "TypeError",
                "file_put_contents(): supplied resource is not a valid stream resource".to_string(),
            );
            return Ok(());
        };
        Some(resource)
    } else {
        None
    };

    if filename.is_empty() {
        argument_error(eg, "ValueError", "Path must not be empty".to_string());
        return Ok(());
    }

    let append = flags & FILE_APPEND != 0;
    let locked = flags & LOCK_EX != 0;
    let mode = if append {
        "a"
    } else if locked {
        "c"
    } else {
        "w"
    };
    let mut destination = match PhpStream::open(&filename, mode) {
        Ok(stream) => stream,
        Err(_) => return return_value(return_pointer, Value::bool(false)),
    };
    if locked
        && (destination.lock_exclusive().is_err()
            || (!append && destination.truncate_file().is_err()))
    {
        return return_value(return_pointer, Value::bool(false));
    }

    let written = if let Some(resource) = source_resource {
        copy_stream_data(eg, resource, &mut destination)
    } else if let Some(array) = data.as_array() {
        write_array_data(&mut destination, array.values())
    } else if matches!(data.value_type(), ValueType::Object | ValueType::Closure) {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "value cannot be converted to file data",
        ))
    } else {
        write_value_data(&mut destination, data)
    };
    let Ok(written) = written else {
        return return_value(return_pointer, Value::bool(false));
    };
    match i64::try_from(written) {
        Ok(written) => return_value(return_pointer, Value::long(written)),
        Err(_) => return_value(return_pointer, Value::bool(false)),
    }
}

fn copy_stream_data(
    eg: &mut ExecutorGlobals,
    resource: i64,
    destination: &mut PhpStream,
) -> io::Result<usize> {
    let mut chunk = [0u8; WRITE_CHUNK_SIZE];
    let mut total = 0usize;
    loop {
        let read =
            super::super::streams::with_stream(eg, resource, |source| source.read(&mut chunk))
                .transpose()?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "stream closed"))?;
        if read == 0 {
            return Ok(total);
        }
        total = total
            .checked_add(write_bytes(destination, &chunk[..read])?)
            .ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "write length overflow"))?;
    }
}

fn write_array_data<'a>(
    destination: &mut PhpStream,
    values: impl Iterator<Item = &'a Value>,
) -> io::Result<usize> {
    let mut total = 0usize;
    for value in values {
        let written = write_value_data(destination, value)?;
        total = total
            .checked_add(written)
            .ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "write length overflow"))?;
    }
    Ok(total)
}

fn write_value_data(destination: &mut PhpStream, value: &Value) -> io::Result<usize> {
    let value = if value.is_reference() {
        unsafe { &*value.as_ref_ptr() }
    } else {
        value
    };
    match value.value_type() {
        ValueType::String => write_php_string(destination, value.as_str().unwrap_or_default()),
        ValueType::Object | ValueType::Closure => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "array element cannot be converted to file data",
        )),
        _ => write_php_string(destination, &value.echo_to_string()),
    }
}

fn write_php_string(destination: &mut PhpStream, value: &str) -> io::Result<usize> {
    if value.is_ascii() {
        return write_bytes(destination, value.as_bytes());
    }
    let mut chunk = [0u8; WRITE_CHUNK_SIZE];
    let mut used = 0usize;
    let mut total = 0usize;
    for character in value.chars() {
        chunk[used] = character as u8;
        used += 1;
        if used == chunk.len() {
            total = total
                .checked_add(write_bytes(destination, &chunk)?)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::OutOfMemory, "write length overflow")
                })?;
            used = 0;
        }
    }
    if used != 0 {
        total = total
            .checked_add(write_bytes(destination, &chunk[..used])?)
            .ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "write length overflow"))?;
    }
    Ok(total)
}

fn write_bytes(destination: &mut PhpStream, bytes: &[u8]) -> io::Result<usize> {
    let mut written = 0usize;
    while written < bytes.len() {
        match destination.write(&bytes[written..])? {
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "zero-length write",
                ));
            }
            count => written += count,
        }
    }
    Ok(written)
}
