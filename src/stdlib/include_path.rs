//! Request-local PHP include-path policy and filesystem path resolution.

use std::path::{Component, Path, PathBuf};

use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::streams::checked_args::{argument_error, given_type_name};

mod report;

pub(super) use report::fn_stream_resolve_include_path;

const INCLUDE_PATH_STATE: &str = "\0rphp-include-path";
const INCLUDE_PATH_VALUE: &str = "current";
const DEFAULT_INCLUDE_PATH: &str = ".";

#[cold]
pub(super) fn fn_get_include_path(
    _execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(return_pointer, Value::string(current(eg).to_string()))
}

#[cold]
pub(super) fn fn_set_include_path(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = argument(execute_data, 0);
    let path = match path_argument(value, eg, "set_include_path", "include_path") {
        Ok(path) => path,
        Err(()) => return Ok(()),
    };
    if path.is_empty() {
        return return_value(return_pointer, Value::bool(false));
    }

    let previous = current(eg).to_string();
    eg.static_vars
        .entry(INCLUDE_PATH_STATE.to_string())
        .or_default()
        .insert(INCLUDE_PATH_VALUE.to_string(), Value::string(path));
    return_value(return_pointer, Value::string(previous))
}

/// Resolve an existing relative filesystem path through the request's current
/// include path. Explicit relative paths (`./` and `../`), absolute paths and
/// wrapper URLs deliberately bypass the search.
#[cold]
pub(crate) fn resolve_existing(eg: &ExecutorGlobals, requested: &str) -> Option<String> {
    if bypasses_search(requested) {
        return None;
    }

    let include_path = current(eg);
    for entry in include_path.split(path_separator()) {
        let candidate = if entry.is_empty() {
            PathBuf::from(requested)
        } else {
            Path::new(entry).join(requested)
        };
        if candidate.exists() {
            if entry.is_empty() {
                return Some(requested.to_string());
            }
            return Some(
                std::fs::canonicalize(&candidate)
                    .unwrap_or(candidate)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    None
}

#[cold]
pub(crate) fn resolve_for_open(
    eg: &ExecutorGlobals,
    requested: &str,
    use_include_path: bool,
) -> String {
    if use_include_path {
        resolve_existing(eg, requested).unwrap_or_else(|| requested.to_string())
    } else {
        requested.to_string()
    }
}

fn current(eg: &ExecutorGlobals) -> &str {
    eg.static_vars
        .get(INCLUDE_PATH_STATE)
        .and_then(|state| state.get(INCLUDE_PATH_VALUE))
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_INCLUDE_PATH)
}

fn bypasses_search(requested: &str) -> bool {
    let path = Path::new(requested);
    if path.is_absolute() || requested.contains("://") {
        return true;
    }
    matches!(
        path.components().next(),
        Some(Component::CurDir | Component::ParentDir)
    )
}

fn path_separator() -> char {
    if cfg!(windows) { ';' } else { ':' }
}

fn weak_string(value: &Value) -> Option<String> {
    match value.value_type() {
        ValueType::String => value.as_str().map(str::to_string),
        ValueType::True | ValueType::Long | ValueType::Double => Some(value.echo_to_string()),
        ValueType::Null | ValueType::False => Some(String::new()),
        ValueType::Undef
        | ValueType::Array
        | ValueType::Object
        | ValueType::Resource
        | ValueType::Reference
        | ValueType::Closure => None,
    }
}

fn path_argument(
    value: &Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    name: &str,
) -> Result<String, ()> {
    let Some(path) = weak_string(value) else {
        argument_error(
            eg,
            "TypeError",
            format!(
                "{function}(): Argument #1 (${name}) must be of type string, {} given",
                given_type_name(value)
            ),
        );
        return Err(());
    };
    if path.contains('\0') {
        argument_error(
            eg,
            "ValueError",
            format!("{function}(): Argument #1 (${name}) must not contain any null bytes"),
        );
        return Err(());
    }
    Ok(path)
}

fn argument<'a>(execute_data: *mut ExecuteData, index: u32) -> &'a Value {
    let value = unsafe { (*execute_data).cv(index) };
    if value.is_reference() {
        unsafe { &*value.as_ref_ptr() }
    } else {
        value
    }
}

fn return_value(pointer: *mut Value, value: Value) -> Result<(), VmError> {
    if !pointer.is_null() {
        unsafe { pointer.write(value) };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{bypasses_search, resolve_existing, resolve_for_open};
    use crate::runtime::ExecutorGlobals;
    use crate::value::Value;

    #[test]
    fn resolver_uses_order_and_bypasses_explicit_paths() {
        let root =
            std::env::temp_dir().join(format!("rphp-include-path-unit-{}", std::process::id()));
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("probe.txt"), b"first").unwrap();
        std::fs::write(second.join("probe.txt"), b"second").unwrap();

        let mut eg = ExecutorGlobals::new();
        eg.static_vars.insert(
            super::INCLUDE_PATH_STATE.to_string(),
            std::collections::HashMap::from([(
                super::INCLUDE_PATH_VALUE.to_string(),
                Value::string(format!(
                    "{}{}{}",
                    first.to_string_lossy(),
                    super::path_separator(),
                    second.to_string_lossy()
                )),
            )]),
        );
        let expected = std::fs::canonicalize(first.join("probe.txt"))
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            resolve_existing(&eg, "probe.txt").as_deref(),
            Some(expected.as_str())
        );
        assert!(resolve_existing(&eg, "missing.txt").is_none());
        assert_eq!(resolve_for_open(&eg, "missing.txt", true), "missing.txt");
        assert!(bypasses_search("./probe.txt"));
        assert!(bypasses_search("../probe.txt"));
        assert!(bypasses_search("file:///tmp/probe.txt"));

        std::fs::remove_dir_all(root).unwrap();
    }
}
