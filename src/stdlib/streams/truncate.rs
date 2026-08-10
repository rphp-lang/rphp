//! PHP-compatible `ftruncate()` validation and backend dispatch.

use crate::runtime::ExecutorGlobals;
use crate::value::Value;
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::checked_args::{argument_error, given_type_name, weak_long_argument};
use super::{argument, return_value, with_stream};

#[cold]
pub(super) fn fn_ftruncate(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(resource) = stream_argument(execute_data, eg) else {
        return Ok(());
    };

    let size_value = argument(execute_data, 1);
    let Some(size) = weak_long_argument(size_value) else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "ftruncate(): Argument #2 ($size) must be of type int, {} given",
                given_type_name(size_value)
            ),
        );
        return Ok(());
    };
    if size < 0 {
        argument_error(
            eg,
            "ValueError",
            "ftruncate(): Argument #2 ($size) must be greater than or equal to 0".to_string(),
        );
        return Ok(());
    }

    let succeeded = with_stream(eg, resource, |stream| stream.truncate(size as u64))
        .is_some_and(|result| result.is_ok());
    return_value(return_pointer, Value::bool(succeeded))
}

fn stream_argument(execute_data: *mut ExecuteData, eg: &mut ExecutorGlobals) -> Option<i64> {
    let value = argument(execute_data, 0);
    let Some(resource) = value.as_resource_id() else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "ftruncate(): Argument #1 ($stream) must be of type resource, {} given",
                given_type_name(value)
            ),
        );
        return None;
    };
    if with_stream(eg, resource, |_| ()).is_none() {
        argument_error(
            eg,
            "TypeError",
            "ftruncate(): supplied resource is not a valid stream resource".to_string(),
        );
        return None;
    }
    Some(resource)
}
