//! Introspection for the stream wrappers implemented by RPHP.

use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::checked_args::{argument_error, given_type_name};
use super::{argument, return_value, with_stream};

const WRAPPERS: &[&str] = &["php", "file"];

#[cold]
pub(super) fn fn_stream_get_wrappers(
    _execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(return_pointer, string_registry(WRAPPERS))
}

#[cold]
pub(super) fn fn_stream_get_transports(
    _execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // Coroutine descriptors are intentionally not PHP Stream resources.
    return_value(return_pointer, Value::array(PhpArray::new()))
}

#[cold]
pub(super) fn fn_stream_get_filters(
    _execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(return_pointer, Value::array(PhpArray::new()))
}

#[cold]
pub(super) fn fn_stream_is_local(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = argument(execute_data, 0);
    if let Some(resource) = value.as_resource_id() {
        if with_stream(eg, resource, |_| ()).is_some() {
            return return_value(return_pointer, Value::bool(true));
        }
        argument_error(
            eg,
            "TypeError",
            "stream_is_local(): supplied resource is not a valid stream resource".to_string(),
        );
        return Ok(());
    }

    let url = match value.value_type() {
        ValueType::Object => {
            let Some(value) = crate::vm::execute::call_object_string_conversion(eg, value)? else {
                object_conversion_error(eg, &given_type_name(value));
                return Ok(());
            };
            value.echo_to_string()
        }
        ValueType::Closure => {
            object_conversion_error(eg, "Closure");
            return Ok(());
        }
        _ => value.echo_to_string(),
    };
    return_value(return_pointer, Value::bool(is_local_url(&url)))
}

fn string_registry(values: &[&str]) -> Value {
    let mut registry = PhpArray::with_packed_capacity(values.len());
    for value in values {
        registry.push(Value::string(*value));
    }
    Value::array(registry)
}

fn is_local_url(url: &str) -> bool {
    let Some(separator) = url.find("://") else {
        return true;
    };
    let scheme = &url[..separator];
    let remainder = &url[separator + 3..];
    if scheme.eq_ignore_ascii_case("file") {
        return remainder.starts_with('/')
            || remainder
                .get(..10)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("localhost/"));
    }
    !["http", "https", "ftp", "ftps", "data"]
        .iter()
        .any(|remote| scheme.eq_ignore_ascii_case(remote))
}

fn object_conversion_error(eg: &mut ExecutorGlobals, class: &str) {
    argument_error(
        eg,
        "Error",
        format!("Object of class {class} could not be converted to string"),
    );
}

#[cfg(test)]
mod tests {
    use super::is_local_url;

    #[test]
    fn locality_distinguishes_remote_wrappers_and_file_hosts() {
        for local in [
            "",
            "relative.php",
            "/tmp/file.php",
            "php://memory",
            "glob://*.php",
            "unknown://target",
            "file:///tmp/file.php",
            "file://localhost/tmp/file.php",
        ] {
            assert!(is_local_url(local), "{local}");
        }
        for remote in [
            "http://example.com",
            "HTTPS://example.com",
            "ftp://example.com/file",
            "data://text/plain,value",
            "file://remote/tmp/file.php",
            "file://localhost",
        ] {
            assert!(!is_local_url(remote), "{remote}");
        }
    }
}
