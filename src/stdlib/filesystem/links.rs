//! Local link operations do not dispatch stream wrappers or invalidate the
//! stat cache. A symlink preserves its target bytes but expands its destination;
//! hard links and readlink use native path resolution directly.

use std::path::{Path, PathBuf};

use crate::runtime::ExecutorGlobals;
use crate::value::Value;
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::{filesystem_error_reason, filesystem_string_value_argument};

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn path_argument(
    frame: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<PathBuf>, VmError> {
    let Some(value) = filesystem_string_value_argument(frame, eg, function, index, parameter)?
    else {
        return Ok(None);
    };
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = value.php_string_bytes().expect("validated string argument");
        Ok(Some(PathBuf::from(std::ffi::OsStr::from_bytes(&bytes))))
    }
    #[cfg(not(unix))]
    Ok(Some(PathBuf::from(value.as_str().unwrap_or_default())))
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn report_error(
    frame: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    error: &std::io::Error,
) -> Result<(), VmError> {
    super::report_filesystem_diagnostic(
        frame,
        eg,
        2,
        "Warning",
        &format!("{function}(): {}", filesystem_error_reason(error)),
    )
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(crate) fn fn_link(
    frame: *mut ExecuteData,
    result: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    create_link(frame, result, eg, false)
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(crate) fn fn_symlink(
    frame: *mut ExecuteData,
    result: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    create_link(frame, result, eg, true)
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn create_link(
    frame: *mut ExecuteData,
    result: *mut Value,
    eg: &mut ExecutorGlobals,
    symbolic: bool,
) -> Result<(), VmError> {
    let function = if symbolic { "symlink" } else { "link" };
    let Some(target) = path_argument(frame, eg, function, 0, "target")? else {
        return Ok(());
    };
    let Some(destination) = path_argument(frame, eg, function, 1, "link")? else {
        return Ok(());
    };
    let operation = if symbolic {
        create_symbolic_link(&target, &destination)
    } else {
        std::fs::hard_link(&target, &destination)
    };
    if let Err(error) = &operation {
        report_error(frame, eg, function, error)?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    ret!(result, Value::bool(operation.is_ok()));
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(crate) fn fn_readlink(
    frame: *mut ExecuteData,
    result: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = path_argument(frame, eg, "readlink", 0, "path")? else {
        return Ok(());
    };
    match std::fs::read_link(path) {
        Ok(target) => {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                ret!(result, Value::binary_string(target.as_os_str().as_bytes()));
            }
            #[cfg(not(unix))]
            ret!(result, Value::string(target.to_string_lossy().as_ref()));
        }
        Err(error) => {
            report_error(frame, eg, "readlink", &error)?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ret!(result, Value::bool(false));
        }
    }
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn create_symbolic_link(target: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, expanded_destination(destination)?)
    }
    #[cfg(not(unix))]
    {
        let _ = (target, destination);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Symbolic links are not supported",
        ))
    }
}

#[cfg(unix)]
#[cold]
#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn expanded_destination(path: &Path) -> std::io::Result<PathBuf> {
    use std::collections::VecDeque;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Component;

    if path.as_os_str().is_empty() {
        return Err(std::io::Error::from_raw_os_error(2));
    }
    let mut pending: VecDeque<_> = path
        .components()
        .map(|part| part.as_os_str().to_os_string())
        .collect();
    let mut resolved = std::env::current_dir()?;
    let mut followed = 0;
    while let Some(part) = pending.pop_front() {
        match Path::new(&part).components().next() {
            Some(Component::RootDir) => resolved = PathBuf::from("/"),
            Some(Component::CurDir) => {}
            Some(Component::ParentDir) => {
                resolved.pop();
            }
            Some(Component::Normal(_)) => {
                resolved.push(part);
                if let Ok(target) = std::fs::read_link(&resolved) {
                    followed += 1;
                    if followed > 32 {
                        return Err(std::io::Error::from_raw_os_error(2));
                    }
                    resolved.pop();
                    for part in target.components().rev() {
                        pending.push_front(part.as_os_str().to_os_string());
                    }
                }
            }
            _ => {}
        }
    }
    if path.as_os_str().as_bytes().ends_with(b"/") {
        resolved.push("");
    }
    Ok(resolved)
}
