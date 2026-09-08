//! Timestamp changes do not require opening an existing file for content I/O.

use std::path::{Path, PathBuf};

use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::{
    clear_filesystem_stat_cache, filesystem_error_reason, filesystem_string_value_argument,
    local_filesystem_path, report_filesystem_diagnostic,
};

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code; section placement does not change its ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn time_argument(
    frame: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    index: u32,
    parameter: &str,
) -> Result<Option<Option<i64>>, VmError> {
    if arg_opt!(frame, index)
        .is_none_or(|value| value.dereferenced().value_type() == ValueType::Null)
    {
        return Ok(Some(None));
    }
    Ok(super::super::typed_internal_int_argument_expected(
        frame, eg, "touch", index, parameter, "?int",
    )?
    .map(Some))
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code; section placement does not change its ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(crate) fn fn_touch(
    frame: *mut ExecuteData,
    result: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(filename) = filesystem_string_value_argument(frame, eg, "touch", 0, "filename")?
    else {
        return Ok(());
    };
    let Some(mtime) = time_argument(frame, eg, 1, "mtime")? else {
        return Ok(());
    };
    let Some(atime) = time_argument(frame, eg, 2, "atime")? else {
        return Ok(());
    };
    let display_path = filename.as_str().unwrap_or_default();
    if display_path.is_empty() {
        ret!(result, Value::bool(false));
    }
    if mtime.is_none() && atime.is_some() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "touch(): Argument #2 ($mtime) cannot be null when argument #3 ($atime) is an integer",
        ));
        return Ok(());
    }
    let Some(path) = local_path(frame, eg, &filename)? else {
        return Ok(());
    };
    if path.as_os_str().is_empty() {
        ret!(result, Value::bool(false));
    }
    let file_uri =
        display_path.len() >= 7 && display_path.as_bytes()[..7].eq_ignore_ascii_case(b"file://");
    let display_path = if file_uri {
        &display_path[7..]
    } else {
        display_path
    };
    // F_OK-style admission follows symbolic links, including a dangling
    // link whose target must be created. Never truncate an existing file.
    if std::fs::metadata(&path).is_err()
        && let Err(error) = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
    {
        let caller = if file_uri {
            format!("touch({display_path})")
        } else {
            "touch()".to_string()
        };
        report_filesystem_diagnostic(
            frame,
            eg,
            2,
            "Warning",
            &format!(
                "{caller}: Unable to create file {display_path} because {}",
                filesystem_error_reason(&error)
            ),
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        ret!(result, Value::bool(false));
    }
    let changed = set_times(
        &path,
        mtime.map(|modified| (atime.unwrap_or(modified), modified)),
    );
    if let Err(error) = &changed {
        report_filesystem_diagnostic(
            frame,
            eg,
            2,
            "Warning",
            &format!("touch(): Utime failed: {}", filesystem_error_reason(error)),
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    } else {
        clear_filesystem_stat_cache(eg);
    }
    ret!(result, Value::bool(changed.is_ok()));
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code; section placement does not change its ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn local_path(
    frame: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<PathBuf>, VmError> {
    let text = value.as_str().unwrap_or_default();
    let local = if local_filesystem_path(text).is_some() {
        // touch admits the standard file wrapper, but strips only file://,
        // preserving an admitted localhost authority as native path text.
        if text.len() >= 7 && text.as_bytes()[..7].eq_ignore_ascii_case(b"file://") {
            &text[7..]
        } else {
            text
        }
    } else {
        let protocol = text.split_once("://").map_or("", |(scheme, _)| scheme);
        let recognized = matches!(
            protocol.to_ascii_lowercase().as_str(),
            "file" | "php" | "http" | "https" | "ftp" | "ftps" | "data" | "glob" | "phar"
        );
        #[cfg(feature = "stream-registry")]
        let recognized = recognized
            || super::super::user_wrapper::definition_for_protocol(eg, protocol).is_some();
        if recognized {
            report_filesystem_diagnostic(
                frame,
                eg,
                2,
                "Warning",
                "touch(): Cannot call touch() for a non-standard stream",
            )?;
            // An empty path denotes a non-throwing unsupported backend;
            // None means a callback interrupted the operation.
            return Ok(eg.exception.is_none().then(PathBuf::new));
        }
        report_filesystem_diagnostic(
            frame,
            eg,
            2,
            "Warning",
            &format!(
                "touch(): Unable to find the wrapper \"{protocol}\" - did you forget to enable it when you configured PHP?"
            ),
        )?;
        if eg.exception.is_some() {
            return Ok(None);
        }
        text
    };
    // The only removed prefix is ASCII file://; its byte offset
    // is also valid in the lossless PHP-byte view of a binary path.
    let offset = text.len() - local.len();
    let bytes = value.php_string_bytes().expect("validated string path");
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(Some(PathBuf::from(std::ffi::OsStr::from_bytes(
            &bytes[offset..],
        ))))
    }
    #[cfg(not(unix))]
    Ok(Some(PathBuf::from(local)))
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
#[cold]
#[inline(never)]
// SAFETY: compiler-generated code; section placement does not change its ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn set_times(path: &Path, times: Option<(i64, i64)>) -> std::io::Result<()> {
    use std::ffi::{CString, c_char, c_int, c_long};
    use std::os::unix::ffi::OsStrExt;

    #[repr(C)]
    struct NativeTimes {
        accessed: c_long,
        modified: c_long,
    }
    const _: () = assert!(size_of::<NativeTimes>() == 16 && align_of::<NativeTimes>() == 8);
    unsafe extern "C" {
        fn utime(path: *const c_char, times: *const NativeTimes) -> c_int;
    }
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let times = times.map(|(accessed, modified)| NativeTimes { accessed, modified });
    // SAFETY: Linux-64 utimbuf contains two ordered time_t/c_long fields
    // (verified size 16, alignment 8, offsets 0/8). The owned NUL-terminated
    // path and optional stack struct remain live for this synchronous call;
    // null times asks the OS for current time. libc retains neither pointer.
    let status = unsafe {
        utime(
            path.as_ptr(),
            times.as_ref().map_or(std::ptr::null(), |t| t),
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
fn set_times(path: &Path, times: Option<(i64, i64)>) -> std::io::Result<()> {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    let convert = |seconds: i64| {
        if seconds >= 0 {
            UNIX_EPOCH.checked_add(Duration::from_secs(seconds as u64))
        } else {
            UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs()))
        }
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))
    };
    let (accessed, modified) = match times {
        Some((a, m)) => (convert(a)?, convert(m)?),
        None => {
            let now = SystemTime::now();
            (now, now)
        }
    };
    std::fs::File::open(path)?.set_times(
        std::fs::FileTimes::new()
            .set_accessed(accessed)
            .set_modified(modified),
    )
}
