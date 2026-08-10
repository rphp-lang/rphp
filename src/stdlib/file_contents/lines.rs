use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::super::stream::PhpStream;
use super::super::streams::checked_args::{argument_error, given_type_name, weak_long_argument};
use super::{argument, optional_argument, return_value, string_argument};

const FILE_USE_INCLUDE_PATH: i64 = 1;
const FILE_IGNORE_NEW_LINES: i64 = 2;
const FILE_SKIP_EMPTY_LINES: i64 = 4;
const VALID_FLAGS: i64 = FILE_USE_INCLUDE_PATH | FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES;

#[cold]
pub(in crate::stdlib) fn fn_file(
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
                "file(): Argument #1 ($filename) must be of type string, {} given",
                given_type_name(filename_value)
            ),
        );
        return Ok(());
    };

    let flags = match optional_argument(execute_data, 1) {
        Some(value) => {
            let Some(flags) = weak_long_argument(value) else {
                argument_error(
                    eg,
                    "TypeError",
                    format!(
                        "file(): Argument #2 ($flags) must be of type int, {} given",
                        given_type_name(value)
                    ),
                );
                return Ok(());
            };
            flags
        }
        None => 0,
    };

    #[cfg(not(feature = "stream-context"))]
    let context_resource = match optional_argument(execute_data, 2) {
        Some(context) if context.value_type() == ValueType::Null => false,
        Some(context) if context.value_type() == ValueType::Resource => true,
        Some(context) => {
            argument_error(
                eg,
                "TypeError",
                format!(
                    "file(): Argument #3 ($context) must be of type resource or null, {} given",
                    given_type_name(context)
                ),
            );
            return Ok(());
        }
        None => false,
    };
    #[cfg(feature = "stream-context")]
    let context_resource = match super::super::streams::context::optional_context_resource(
        execute_data,
        2,
        eg,
        "file",
        3,
    ) {
        Ok(context) => context,
        Err(()) => return Ok(()),
    };

    if flags & !VALID_FLAGS != 0 {
        argument_error(
            eg,
            "ValueError",
            "file(): Argument #2 ($flags) must be a valid flag value".to_string(),
        );
        return Ok(());
    }
    #[cfg(not(feature = "stream-context"))]
    if context_resource {
        argument_error(
            eg,
            "TypeError",
            "file(): supplied resource is not a valid Stream-Context resource".to_string(),
        );
        return Ok(());
    }
    #[cfg(feature = "stream-context")]
    if let Some(context) = context_resource
        && super::super::streams::context::context_snapshot(eg, context).is_none()
    {
        super::super::streams::context::invalid_context_error(eg, "file");
        return Ok(());
    }
    if filename.is_empty() {
        argument_error(eg, "ValueError", "Path must not be empty".to_string());
        return Ok(());
    }

    #[cfg(feature = "include-path")]
    let filename = super::super::include_path::resolve_for_open(
        eg,
        &filename,
        flags & FILE_USE_INCLUDE_PATH != 0,
    );
    let mut stream = match PhpStream::open(&filename, "r") {
        Ok(stream) => stream,
        Err(_) => return return_value(return_pointer, Value::bool(false)),
    };
    let ignore_new_lines = flags & FILE_IGNORE_NEW_LINES != 0;
    let skip_empty_lines = flags & FILE_SKIP_EMPTY_LINES != 0;
    let mut result = PhpArray::new();
    let mut line = Vec::new();
    loop {
        match stream.read_line(&mut line, None) {
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => return return_value(return_pointer, Value::bool(false)),
        }
        if ignore_new_lines {
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
        }
        if skip_empty_lines && line.is_empty() {
            continue;
        }
        result.push(Value::string(super::super::bytes_to_php_string(&line)));
    }
    return_value(return_pointer, Value::array(result))
}
