//! Baseline filesystem builtins and their private path/glob helpers.
//!
//! Feature-complete stream-backed file operations remain in `file_contents`.
//! This module keeps the smaller default-build handlers together while their
//! bounded PHP-visible contracts expand independently of the opt-in surfaces.

use std::borrow::Cow;
#[cfg(not(feature = "file-write"))]
use std::io::Write;

use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::{
    php_byte_result, typed_internal_int_argument, typed_internal_string_value_argument_expected,
};

// ============================================================================
// Filesystem functions
// ============================================================================

/// file_get_contents($filename): string|false
/// PHP strings are byte strings. We use Latin-1 (byte→char 1:1) to preserve raw bytes
/// losslessly inside Rust String, pending a proper byte-string Value backend.
#[cfg(not(feature = "file-contents"))]
pub(super) fn fn_file_get_contents(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    match std::fs::read(path.as_ref()) {
        Ok(bytes) => ret!(rv, Value::string(bytes_to_php_string(&bytes))),
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

/// Convert raw bytes to a Rust String preserving every byte losslessly.
/// Uses Latin-1 encoding: each byte 0x00-0xFF maps to the same Unicode codepoint.
/// This is the standard way to round-trip PHP byte strings through Rust Strings.
pub(super) fn bytes_to_php_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Convert a PHP-style string back to raw bytes (inverse of bytes_to_php_string).
pub(super) fn php_string_to_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u8).collect()
}

/// Default-build file_put_contents($filename, $data, $flags = 0): int|false.
/// Writes using Latin-1 byte mapping to preserve binary data round-trip and
/// supports the ordinary regular-file append/exclusive-lock flag pair.
#[cfg(not(feature = "file-write"))]
pub(super) fn fn_file_put_contents(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let data = arg_str!(ed, 1);
    let flags = arg_opt!(ed, 2).map(Value::to_long_val).unwrap_or(0);
    let raw_bytes = php_string_to_bytes(data.as_ref());
    let append = flags & 8 != 0;
    let locked = flags & 2 != 0;
    let result = if append || locked {
        let mut options = std::fs::OpenOptions::new();
        options.create(true).write(true);
        if append {
            options.append(true);
        } else {
            options.truncate(false);
        }
        options.open(path.as_ref()).and_then(|mut file| {
            if locked {
                file.lock()?;
                if !append {
                    file.set_len(0)?;
                }
            }
            file.write_all(&raw_bytes)
        })
    } else {
        std::fs::write(path.as_ref(), &raw_bytes)
    };
    match result {
        Ok(()) => ret!(rv, Value::long(raw_bytes.len() as i64)),
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

/// file_exists($filename): bool
pub(super) fn fn_file_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    ret!(
        rv,
        Value::bool(std::path::Path::new(path.as_ref()).exists())
    );
}

/// filemtime($filename): int|false
pub(super) fn fn_filemtime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let modified = std::fs::metadata(path.as_ref()).and_then(|metadata| metadata.modified());
    match modified {
        Ok(timestamp) => {
            let seconds = match timestamp.duration_since(std::time::UNIX_EPOCH) {
                Ok(duration) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
                Err(error) => -i64::try_from(error.duration().as_secs()).unwrap_or(i64::MAX),
            };
            ret!(rv, Value::long(seconds));
        }
        Err(_) => {
            eg.write_output(
                format!("Warning: filemtime(): stat failed for {}\n", path.as_ref()).as_bytes(),
            );
            ret!(rv, Value::bool(false));
        }
    }
}

/// is_file($filename): bool
pub(super) fn fn_is_file(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    ret!(
        rv,
        Value::bool(std::path::Path::new(path.as_ref()).is_file())
    );
}

/// is_dir($filename): bool
pub(super) fn fn_is_dir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    ret!(
        rv,
        Value::bool(std::path::Path::new(path.as_ref()).is_dir())
    );
}

/// is_link($filename): bool — lstat semantics also recognize broken links.
pub(super) fn fn_is_link(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let is_link = std::fs::symlink_metadata(path.as_ref())
        .is_ok_and(|metadata| metadata.file_type().is_symlink());
    ret!(rv, Value::bool(is_link));
}

pub(super) fn fn_chmod(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let mode = arg_long!(ed, 1) as u32;
    #[cfg(unix)]
    let result = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path.as_ref(), std::fs::Permissions::from_mode(mode)).is_ok()
    };
    #[cfg(not(unix))]
    let result = false;
    ret!(rv, Value::bool(result));
}

pub(super) fn fn_fileperms(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path.as_ref()) {
            Ok(metadata) => ret!(rv, Value::long(i64::from(metadata.permissions().mode()))),
            Err(_) => ret!(rv, Value::bool(false)),
        }
    }
    #[cfg(not(unix))]
    ret!(rv, Value::bool(false));
}

pub(super) fn fn_umask(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn umask(mask: u32) -> u32;
        }
        let supplied = arg_opt!(ed, 0).map(|value| value.to_long_val() as u32);
        // SAFETY: this declaration matches POSIX `umask(mode_t) -> mode_t`;
        // every u32 bit pattern is valid input, and the read-only path restores
        // the process mask before returning.
        let previous = unsafe {
            let previous = umask(supplied.unwrap_or(0));
            if supplied.is_none() {
                umask(previous);
            }
            previous
        };
        ret!(rv, Value::long(i64::from(previous)));
    }
    #[cfg(not(unix))]
    ret!(rv, Value::long(0));
}

/// dirname($path, $levels = 1): string
pub(super) fn fn_dirname(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_path = arg!(ed, 0);
    let exact_levels = arg_opt!(ed, 1);
    if exact_path.value_type() == ValueType::String
        && exact_levels.is_none_or(|levels| levels.value_type() == ValueType::Long)
    {
        let levels = exact_levels.map_or(1, |levels| levels.as_long().unwrap_or(1));
        if levels < 1 {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "dirname(): Argument #2 ($levels) must be greater than or equal to 1",
            ));
            return Ok(());
        }
        let binary = exact_path.is_binary_string();
        let path = exact_path.php_string_bytes().unwrap_or_default();
        ret!(
            rv,
            php_byte_result(
                crate::path_decomposition::dirname(&path, levels as u64),
                binary
            )
        );
    }

    let Some(path) =
        typed_internal_string_value_argument_expected(ed, eg, "dirname", 0, "path", "string")?
    else {
        return Ok(());
    };
    let levels = if arg_opt!(ed, 1).is_some() {
        let Some(levels) = typed_internal_int_argument(ed, eg, "dirname", 1, "levels")? else {
            return Ok(());
        };
        levels
    } else {
        1
    };
    if levels < 1 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "dirname(): Argument #2 ($levels) must be greater than or equal to 1",
        ));
        return Ok(());
    }
    let binary = path.is_binary_string();
    let path = path.php_string_bytes().unwrap_or_default();
    ret!(
        rv,
        php_byte_result(
            crate::path_decomposition::dirname(&path, levels as u64),
            binary
        )
    );
}

/// basename($path, $suffix = ""): string
pub(super) fn fn_basename(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_path = arg!(ed, 0);
    let exact_suffix = arg_opt!(ed, 1);
    if exact_path.value_type() == ValueType::String
        && exact_suffix.is_none_or(|suffix| suffix.value_type() == ValueType::String)
    {
        let path = exact_path.php_string_bytes().unwrap_or_default();
        let suffix = exact_suffix
            .and_then(Value::php_string_bytes)
            .unwrap_or_default();
        ret!(
            rv,
            php_byte_result(
                crate::path_decomposition::basename(&path, &suffix),
                exact_path.is_binary_string()
            )
        );
    }

    let Some(path) =
        typed_internal_string_value_argument_expected(ed, eg, "basename", 0, "path", "string")?
    else {
        return Ok(());
    };
    let suffix = if arg_opt!(ed, 1).is_some() {
        let Some(suffix) = typed_internal_string_value_argument_expected(
            ed, eg, "basename", 1, "suffix", "string",
        )?
        else {
            return Ok(());
        };
        suffix.php_string_bytes().unwrap_or_default().into_owned()
    } else {
        Vec::new()
    };
    let binary = path.is_binary_string();
    let path = path.php_string_bytes().unwrap_or_default();
    ret!(
        rv,
        php_byte_result(crate::path_decomposition::basename(&path, &suffix), binary)
    );
}

/// realpath($path): string|false
pub(super) fn fn_realpath(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    match std::fs::canonicalize(path.as_ref()) {
        Ok(p) => ret!(rv, Value::string(p.to_string_lossy().into_owned())),
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

/// getcwd(): string|false
pub(super) fn fn_getcwd(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    match std::env::current_dir() {
        Ok(p) => ret!(rv, Value::string(p.to_string_lossy().into_owned())),
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

/// file($filename): array|false — read file into array of lines
/// Uses Latin-1 mapping to preserve binary content losslessly.
pub(in crate::stdlib) fn return_default_file_lines(
    path: &str,
    rv: *mut Value,
) -> Result<(), VmError> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let contents = bytes_to_php_string(&bytes);
            let mut arr = PhpArray::new();
            let mut start = 0;
            while start < contents.len() {
                match contents[start..].find('\n') {
                    Some(pos) => {
                        arr.push(Value::string(contents[start..start + pos + 1].to_string()));
                        start += pos + 1;
                    }
                    None => {
                        arr.push(Value::string(contents[start..].to_string()));
                        break;
                    }
                }
            }
            ret!(rv, Value::array(arr));
        }
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

#[cfg(not(feature = "file-lines"))]
pub(super) fn fn_file(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    return_default_file_lines(path.as_ref(), rv)
}

/// mkdir($pathname, $mode = 0777, $recursive = false): bool
pub(super) fn fn_mkdir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let recursive = arg_opt!(ed, 2).map(|v| v.is_truthy()).unwrap_or(false);
    let result = if recursive {
        std::fs::create_dir_all(path.as_ref())
    } else {
        std::fs::create_dir(path.as_ref())
    };
    ret!(rv, Value::bool(result.is_ok()));
}

/// rmdir($dirname): bool
pub(super) fn fn_rmdir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    ret!(rv, Value::bool(std::fs::remove_dir(path.as_ref()).is_ok()));
}

/// unlink($filename): bool
pub(super) fn fn_unlink(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    ret!(rv, Value::bool(std::fs::remove_file(path.as_ref()).is_ok()));
}

/// rename($old, $new): bool
pub(super) fn fn_rename(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let old = arg_str!(ed, 0);
    let new = arg_str!(ed, 1);
    ret!(
        rv,
        Value::bool(std::fs::rename(old.as_ref(), new.as_ref()).is_ok())
    );
}

/// copy($source, $dest): bool
pub(super) fn fn_copy(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let src = arg_str!(ed, 0);
    let dst = arg_str!(ed, 1);
    ret!(
        rv,
        Value::bool(std::fs::copy(src.as_ref(), dst.as_ref()).is_ok())
    );
}

/// tempnam($dir, $prefix): string|false
pub(super) fn fn_tempnam(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let dir = arg_str!(ed, 0);
    let prefix = arg_str!(ed, 1);
    let dir_path = std::path::Path::new(dir.as_ref());
    if !dir_path.is_dir() {
        ret!(rv, Value::bool(false));
    }
    // Generate a unique filename: prefix + pid + atomic counter
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("{}{}{}", prefix, std::process::id(), seq);
    let path = dir_path.join(&name);
    match std::fs::File::create(&path) {
        Ok(_) => ret!(rv, Value::string(path.to_string_lossy().into_owned())),
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

/// sys_get_temp_dir(): string
pub(super) fn fn_sys_get_temp_dir(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::string(std::env::temp_dir().to_string_lossy().into_owned())
    );
}

/// pathinfo($path, $flags = PATHINFO_ALL): array|string
pub(super) fn fn_pathinfo(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_path = arg!(ed, 0);
    let exact_flags = arg_opt!(ed, 1);
    if exact_path.value_type() == ValueType::String
        && exact_flags.is_none_or(|flags| flags.value_type() == ValueType::Long)
    {
        let flags = exact_flags.map_or(crate::path_decomposition::PATHINFO_ALL, |flags| {
            flags
                .as_long()
                .unwrap_or(crate::path_decomposition::PATHINFO_ALL)
        });
        let binary = exact_path.is_binary_string();
        let path = exact_path.php_string_bytes().unwrap_or_default();
        ret!(rv, pathinfo_value(&path, flags, binary));
    }

    let Some(path) =
        typed_internal_string_value_argument_expected(ed, eg, "pathinfo", 0, "path", "string")?
    else {
        return Ok(());
    };
    let flags = if arg_opt!(ed, 1).is_some() {
        let Some(flags) = typed_internal_int_argument(ed, eg, "pathinfo", 1, "flags")? else {
            return Ok(());
        };
        flags
    } else {
        crate::path_decomposition::PATHINFO_ALL
    };
    let binary = path.is_binary_string();
    let path = path.php_string_bytes().unwrap_or_default();
    ret!(rv, pathinfo_value(&path, flags, binary));
}

fn pathinfo_value(path: &[u8], flags: i64, binary: bool) -> Value {
    let info = crate::path_decomposition::pathinfo(&path);

    if flags != crate::path_decomposition::PATHINFO_ALL {
        let selected = if flags & crate::path_decomposition::PATHINFO_DIRNAME != 0
            && !info.dirname.is_empty()
        {
            info.dirname
        } else if flags & crate::path_decomposition::PATHINFO_BASENAME != 0 {
            info.basename
        } else if flags & crate::path_decomposition::PATHINFO_EXTENSION != 0
            && info.extension.is_some()
        {
            info.extension.unwrap_or_default()
        } else if flags & crate::path_decomposition::PATHINFO_FILENAME != 0 {
            info.filename
        } else {
            Vec::new()
        };
        return php_byte_result(selected, binary);
    }

    let mut arr = PhpArray::new();
    if !info.dirname.is_empty() {
        arr.set_str("dirname", php_byte_result(info.dirname, binary));
    }
    arr.set_str("basename", php_byte_result(info.basename, binary));
    if let Some(extension) = info.extension {
        arr.set_str("extension", php_byte_result(extension, binary));
    }
    arr.set_str("filename", php_byte_result(info.filename, binary));
    Value::array(arr)
}

/// is_readable($filename): bool
pub(super) fn fn_is_readable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let p = std::path::Path::new(path.as_ref());
    // Simple check: file exists and we can open it for reading
    ret!(rv, Value::bool(std::fs::File::open(p).is_ok()));
}

/// is_writable($filename): bool / is_writeable()
pub(super) fn fn_is_writable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let p = std::path::Path::new(path.as_ref());
    let writable = if p.is_dir() {
        // For directories: try creating a temp file inside
        let probe = p.join(format!(".rphp_writable_probe_{}", std::process::id()));
        match std::fs::File::create(&probe) {
            Ok(_) => {
                let _ = std::fs::remove_file(&probe);
                true
            }
            Err(_) => false,
        }
    } else {
        // For files: try opening for append (non-destructive)
        std::fs::OpenOptions::new().append(true).open(p).is_ok()
    };
    ret!(rv, Value::bool(writable));
}

/// glob($pattern): array|false
pub(super) fn fn_glob(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let pattern = arg_str!(ed, 0);
    let pat = pattern.as_ref();
    let mut arr = PhpArray::new();

    // Split pattern into directory and filename parts
    let (dir, file_pat) = match pat.rfind('/') {
        Some(pos) => (&pat[..pos], &pat[pos + 1..]),
        None => (".", pat),
    };
    let dir = if dir.is_empty() { "/" } else { dir };

    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut results: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if glob_match(file_pat, &name) {
                // Return full path (dir + name) when pattern had directory component
                if pat.contains('/') {
                    results.push(format!("{}/{}", dir, name));
                } else {
                    results.push(name);
                }
            }
        }
        results.sort(); // PHP glob returns sorted results
        for r in results {
            arr.push(Value::string(r));
        }
    }
    ret!(rv, Value::array(arr));
}

/// Simple glob matcher for *, ? patterns (no full POSIX glob)
fn glob_match(pattern: &str, text: &str) -> bool {
    let pi: Vec<char> = pattern.chars().collect();
    let ti: Vec<char> = text.chars().collect();
    glob_match_inner(&pi, 0, &ti, 0)
}

fn glob_match_inner(pat: &[char], pi: usize, txt: &[char], ti: usize) -> bool {
    if pi == pat.len() && ti == txt.len() {
        return true;
    }
    if pi == pat.len() {
        return false;
    }
    match pat[pi] {
        '*' => {
            // Match zero or more characters
            for skip in 0..=(txt.len() - ti) {
                if glob_match_inner(pat, pi + 1, txt, ti + skip) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if ti < txt.len() {
                glob_match_inner(pat, pi + 1, txt, ti + 1)
            } else {
                false
            }
        }
        c => {
            if ti < txt.len() && txt[ti] == c {
                glob_match_inner(pat, pi + 1, txt, ti + 1)
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{bytes_to_php_string, glob_match, php_string_to_bytes};

    #[test]
    fn byte_string_helpers_round_trip_every_byte() {
        let bytes: Vec<u8> = (u8::MIN..=u8::MAX).collect();
        assert_eq!(php_string_to_bytes(&bytes_to_php_string(&bytes)), bytes);
    }

    #[test]
    fn glob_matcher_preserves_supported_wildcard_contract() {
        assert!(glob_match("*.php", "index.php"));
        assert!(glob_match("file-?.txt", "file-a.txt"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("*.php", "index.phpt"));
        assert!(!glob_match("file-?.txt", "file-long.txt"));
    }
}
