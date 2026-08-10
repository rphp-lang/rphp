//! Canonical reporting policy for `stream_resolve_include_path()`.

use std::path::{Component, Path};

use crate::runtime::ExecutorGlobals;
use crate::value::Value;
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::{argument, current, path_argument, path_separator, return_value};

#[cold]
pub(in crate::stdlib) fn fn_stream_resolve_include_path(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = argument(execute_data, 0);
    let filename = match path_argument(value, eg, "stream_resolve_include_path", "filename") {
        Ok(filename) => filename,
        Err(()) => return Ok(()),
    };
    let value = resolve_reported_path(eg, &filename)
        .map(Value::string)
        .unwrap_or_else(|| Value::bool(false));
    return_value(return_pointer, value)
}

/// Return PHP's canonical report for an existing include-path target. Unlike
/// open resolution, this API canonicalizes direct paths and `file://` URIs,
/// resolves symlinks and skips empty include-path entries.
#[cold]
fn resolve_reported_path(eg: &ExecutorGlobals, requested: &str) -> Option<String> {
    if let Some(prefix) = requested.get(..7)
        && prefix.eq_ignore_ascii_case("file://")
    {
        return local_file_uri_path(requested).and_then(canonical_path);
    }
    if requested.contains("://") {
        return None;
    }

    let path = Path::new(requested);
    if path.is_absolute()
        || matches!(
            path.components().next(),
            Some(Component::CurDir | Component::ParentDir)
        )
    {
        return canonical_path(path);
    }

    for entry in current(eg).split(path_separator()) {
        if entry.is_empty() {
            continue;
        }
        if let Some(path) = canonical_path(Path::new(entry).join(path)) {
            return Some(path);
        }
    }
    None
}

fn local_file_uri_path(uri: &str) -> Option<&Path> {
    let remainder = uri.get(7..)?;
    if remainder.starts_with('/') {
        return Some(Path::new(remainder));
    }
    let localhost = remainder.get(..9)?;
    let path = remainder.get(9..)?;
    if localhost.eq_ignore_ascii_case("localhost") && path.starts_with('/') {
        Some(Path::new(path))
    } else {
        None
    }
}

fn canonical_path(path: impl AsRef<Path>) -> Option<String> {
    std::fs::canonicalize(path)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{local_file_uri_path, resolve_reported_path};
    use crate::runtime::ExecutorGlobals;
    use crate::value::Value;

    #[test]
    fn reporting_canonicalizes_candidates_and_accepts_only_local_file_uris() {
        let root = std::env::temp_dir().join(format!("rphp-include-report-{}", std::process::id()));
        let first = root.join("first");
        std::fs::create_dir_all(&first).unwrap();

        let mut eg = ExecutorGlobals::new();
        eg.static_vars.insert(
            super::super::INCLUDE_PATH_STATE.to_string(),
            std::collections::HashMap::from([(
                super::super::INCLUDE_PATH_VALUE.to_string(),
                Value::string(first.to_string_lossy().into_owned()),
            )]),
        );
        let expected = std::fs::canonicalize(&first)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            resolve_reported_path(&eg, "").as_deref(),
            Some(expected.as_str())
        );
        assert!(resolve_reported_path(&eg, "missing.txt").is_none());
        assert_eq!(
            local_file_uri_path("file:///tmp/probe.txt"),
            Some(Path::new("/tmp/probe.txt"))
        );
        assert_eq!(
            local_file_uri_path("file://localhost/tmp/probe.txt"),
            Some(Path::new("/tmp/probe.txt"))
        );
        assert!(local_file_uri_path("file://remote/tmp/probe.txt").is_none());

        std::fs::remove_dir_all(root).unwrap();
    }
}
