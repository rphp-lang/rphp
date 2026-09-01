//! Baseline filesystem builtins and their private path/glob helpers.
//!
//! Feature-complete stream-backed file operations remain in `file_contents`.
//! This module keeps the smaller default-build handlers together while their
//! bounded PHP-visible contracts expand independently of the opt-in surfaces.

use std::borrow::Cow;
#[cfg(not(feature = "file-write"))]
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::{
    php_byte_result, typed_internal_bool_argument, typed_internal_int_argument,
    typed_internal_string_value_argument_expected,
};

pub(crate) const GLOB_ERR: i64 = 0x0004;
pub(crate) const GLOB_MARK: i64 = 0x0008;
pub(crate) const GLOB_NOCHECK: i64 = 0x0010;
pub(crate) const GLOB_NOSORT: i64 = 0x0020;
pub(crate) const GLOB_BRACE: i64 = 0x0080;
pub(crate) const GLOB_NOESCAPE: i64 = 0x1000;
pub(crate) const GLOB_ONLYDIR: i64 = 1 << 30;
pub(crate) const GLOB_AVAILABLE_FLAGS: i64 =
    GLOB_ERR | GLOB_MARK | GLOB_NOCHECK | GLOB_NOSORT | GLOB_BRACE | GLOB_NOESCAPE | GLOB_ONLYDIR;

const STAT_FIELD_NAMES: [&str; 13] = [
    "dev", "ino", "mode", "nlink", "uid", "gid", "rdev", "size", "atime", "mtime", "ctime",
    "blksize", "blocks",
];
const STAT_CACHE: &str = "\0rphp-filesystem-stat-cache";
const LSTAT_CACHE: &str = "\0rphp-filesystem-lstat-cache";

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
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    if let Some(data) = decode_data_uri(path.as_ref()) {
        match data {
            Ok(bytes) => ret!(rv, php_byte_result(bytes, false)),
            Err(error) => {
                report_data_uri_error(ed, eg, path.as_ref(), error)?;
                if eg.exception.is_some() {
                    return Ok(());
                }
                ret!(rv, Value::bool(false));
            }
        }
    }
    match std::fs::File::open(path.as_ref()) {
        Ok(mut file) => {
            clear_filesystem_stat_cache(eg);
            let mut bytes = Vec::new();
            match std::io::Read::read_to_end(&mut file, &mut bytes) {
                Ok(_) => ret!(rv, php_byte_result(bytes, false)),
                Err(_) => ret!(rv, Value::bool(false)),
            }
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DataUriError {
    IllegalParameter,
    UnableToDecode,
    MissingComma,
}

impl DataUriError {
    fn reason(self) -> &'static str {
        match self {
            Self::IllegalParameter => "rfc2397: illegal parameter",
            Self::UnableToDecode => "rfc2397: unable to decode",
            Self::MissingComma => "rfc2397: no comma in URL",
        }
    }
}

/// Decode the RFC 2397 subset exposed by PHP's lowercase `data:` wrapper.
/// The wrapper percent-decodes ordinary payloads but passes base64 payloads
/// directly to the strict decoder. Invalid percent escapes remain literal.
pub(super) fn decode_data_uri(uri: &str) -> Option<Result<Vec<u8>, DataUriError>> {
    let encoded = uri.strip_prefix("data:")?;
    let encoded = encoded.strip_prefix("//").unwrap_or(encoded);
    let Some((metadata, payload)) = encoded.split_once(',') else {
        return Some(Err(DataUriError::MissingComma));
    };

    let mut fields = metadata.split(';');
    let _media_type = fields.next();
    let parameters: Vec<_> = fields.collect();
    let mut base64 = false;
    for (index, parameter) in parameters.iter().enumerate() {
        if *parameter == "base64" && index + 1 == parameters.len() {
            base64 = true;
        } else if !parameter.contains('=') {
            return Some(Err(DataUriError::IllegalParameter));
        }
    }

    let bytes = php_string_to_bytes(payload);
    if base64 {
        return Some(crate::base64::decode(&bytes, true).ok_or(DataUriError::UnableToDecode));
    }

    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(index + 1).and_then(|byte| hex_value(*byte)),
                bytes.get(index + 2).and_then(|byte| hex_value(*byte)),
            )
        {
            decoded.push((high << 4) | low);
            index += 3;
        } else if bytes[index] == b'+' {
            decoded.push(b' ');
            index += 1;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Some(Ok(decoded))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(super) fn report_data_uri_error(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    uri: &str,
    error: DataUriError,
) -> Result<(), VmError> {
    super::report_internal_diagnostic(
        eg,
        execute_data,
        2,
        "Warning",
        &format!(
            "file_get_contents({uri}): Failed to open stream: {}",
            error.reason()
        ),
    )?;
    Ok(())
}

#[cfg(test)]
mod data_uri_tests {
    use super::{DataUriError, decode_data_uri};

    #[test]
    fn decodes_plain_percent_and_base64_payloads() {
        assert_eq!(
            decode_data_uri("data://text/plain,hello%20world%00%2B+"),
            Some(Ok(b"hello world\0+ ".to_vec()))
        );
        assert_eq!(
            decode_data_uri("data:,plain%2Ctext"),
            Some(Ok(b"plain,text".to_vec()))
        );
        assert_eq!(
            decode_data_uri("data:text/plain;base64,AAECYWJj"),
            Some(Ok(b"\0\x01\x02abc".to_vec()))
        );
        assert_eq!(
            decode_data_uri("data:text/plain,a%GGb%2"),
            Some(Ok(b"a%GGb%2".to_vec()))
        );
    }

    #[test]
    fn rejects_php_rfc2397_parameter_and_base64_boundaries() {
        assert_eq!(
            decode_data_uri("data:text/plain;BASE64,YQ=="),
            Some(Err(DataUriError::IllegalParameter))
        );
        assert_eq!(
            decode_data_uri("data:text/plain;foo,bar"),
            Some(Err(DataUriError::IllegalParameter))
        );
        assert_eq!(
            decode_data_uri("data:text/plain;base64,YQ==="),
            Some(Err(DataUriError::UnableToDecode))
        );
        assert_eq!(
            decode_data_uri("data:text/plain"),
            Some(Err(DataUriError::MissingComma))
        );
        assert_eq!(decode_data_uri("DATA:text/plain,upper"), None);
    }
}

/// Default-build file_put_contents($filename, $data, $flags = 0): int|false.
/// Writes using Latin-1 byte mapping to preserve binary data round-trip and
/// supports the ordinary regular-file append/exclusive-lock flag pair.
#[cfg(not(feature = "file-write"))]
pub(super) fn fn_file_put_contents(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
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
        match options.open(path.as_ref()) {
            Ok(mut file) => {
                clear_filesystem_stat_cache(eg);
                (|| -> std::io::Result<()> {
                    if locked {
                        file.lock()?;
                        if !append {
                            file.set_len(0)?;
                        }
                    }
                    file.write_all(&raw_bytes)
                })()
            }
            Err(error) => Err(error),
        }
    } else {
        match std::fs::File::create(path.as_ref()) {
            Ok(mut file) => {
                clear_filesystem_stat_cache(eg);
                file.write_all(&raw_bytes)
            }
            Err(error) => Err(error),
        }
    };
    match result {
        Ok(()) => ret!(rv, Value::long(raw_bytes.len() as i64)),
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

#[derive(Clone, Copy)]
enum FilesystemStatQuery {
    Quiet,
    Report,
    LinkReport,
    LinkQuiet,
}

impl FilesystemStatQuery {
    fn cache_key(self) -> &'static str {
        match self {
            Self::Quiet | Self::Report => STAT_CACHE,
            Self::LinkReport | Self::LinkQuiet => LSTAT_CACHE,
        }
    }

    #[cfg(feature = "stream-registry")]
    fn wrapper_flags(self) -> i64 {
        match self {
            Self::Quiet => 6,
            Self::Report => 4,
            Self::LinkReport => 5,
            Self::LinkQuiet => 7,
        }
    }
}

pub(super) fn metadata_stat_fields(metadata: &std::fs::Metadata) -> [i64; 13] {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        [
            metadata.dev() as i64,
            metadata.ino() as i64,
            metadata.mode() as i64,
            metadata.nlink() as i64,
            metadata.uid() as i64,
            metadata.gid() as i64,
            metadata.rdev() as i64,
            i64::try_from(metadata.size()).unwrap_or(i64::MAX),
            metadata.atime(),
            metadata.mtime(),
            metadata.ctime(),
            metadata.blksize() as i64,
            metadata.blocks() as i64,
        ]
    }
    #[cfg(not(unix))]
    {
        let file_type = metadata.file_type();
        let mut mode = if file_type.is_symlink() {
            0o120000
        } else if file_type.is_dir() {
            0o040000
        } else if file_type.is_file() {
            0o100000
        } else {
            0
        };
        mode |= 0o444;
        if !metadata.permissions().readonly() {
            mode |= 0o222;
        }
        let timestamp = |value: std::io::Result<std::time::SystemTime>| {
            value.map_or(0, |value| {
                match value.duration_since(std::time::UNIX_EPOCH) {
                    Ok(duration) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
                    Err(error) => -i64::try_from(error.duration().as_secs()).unwrap_or(i64::MAX),
                }
            })
        };
        [
            0,
            0,
            mode,
            1,
            0,
            0,
            0,
            i64::try_from(metadata.len()).unwrap_or(i64::MAX),
            timestamp(metadata.accessed()),
            timestamp(metadata.modified()),
            timestamp(metadata.created()),
            0,
            0,
        ]
    }
}

pub(super) fn stat_array_value(fields: [i64; 13]) -> Value {
    let mut result = PhpArray::with_hash_capacity(fields.len() * 2);
    for value in fields {
        result.push(Value::long(value));
    }
    for (name, value) in STAT_FIELD_NAMES.into_iter().zip(fields) {
        result.set_str(name, Value::long(value));
    }
    Value::array(result)
}

#[cfg(feature = "stream-registry")]
pub(super) fn normalize_wrapper_stat(value: Value) -> Value {
    let Some(array) = value.as_array() else {
        return Value::bool(false);
    };
    let mut fields = [0; 13];
    for (index, name) in STAT_FIELD_NAMES.into_iter().enumerate() {
        fields[index] = array.get_str(name).map(Value::to_long_val).unwrap_or(0);
    }
    stat_array_value(fields)
}

fn cached_stat_value(
    eg: &ExecutorGlobals,
    query: FilesystemStatQuery,
    path: &str,
) -> Option<Value> {
    eg.static_vars
        .get(query.cache_key())
        .and_then(|cache| cache.get(path))
        .cloned()
}

fn cache_stat_value(
    eg: &mut ExecutorGlobals,
    query: FilesystemStatQuery,
    path: &str,
    value: &Value,
) {
    let key = query.cache_key();
    if let Some(cache) = eg.static_vars.get_mut(key) {
        cache.clear();
        cache.insert(path.to_string(), value.clone());
        return;
    }
    eg.static_vars.insert(
        key.to_string(),
        [(path.to_string(), value.clone())].into_iter().collect(),
    );
}

pub(super) fn clear_filesystem_stat_cache(eg: &mut ExecutorGlobals) {
    if eg.static_vars.is_empty() {
        return;
    }
    for key in [STAT_CACHE, LSTAT_CACHE] {
        if let Some(cache) = eg.static_vars.get_mut(key) {
            cache.clear();
        }
    }
}

#[inline]
pub(super) fn filesystem_stat_cache_is_populated(eg: &ExecutorGlobals) -> bool {
    if eg.static_vars.is_empty() {
        return false;
    }
    [STAT_CACHE, LSTAT_CACHE].into_iter().any(|key| {
        eg.static_vars
            .get(key)
            .is_some_and(|cache| !cache.is_empty())
    })
}

fn local_filesystem_path(path: &str) -> Option<&str> {
    if path.len() >= 7 && path.as_bytes()[..7].eq_ignore_ascii_case(b"file://") {
        let local = path.get(7..)?;
        if local.len() >= 10 && local.as_bytes()[..10].eq_ignore_ascii_case(b"localhost/") {
            return local.get(9..);
        }
        if local.is_empty() || local.starts_with('/') {
            return Some(local);
        }
        #[cfg(windows)]
        if local.as_bytes().get(1) == Some(&b':') {
            return Some(local);
        }
        return None;
    }

    #[cfg(windows)]
    if path.as_bytes().get(1) == Some(&b':') {
        return Some(path);
    }

    if let Some(separator) = path.find("://") {
        let protocol = &path[..separator];
        if protocol.len() > 1
            && protocol
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
        {
            return None;
        }
    }

    Some(path)
}

fn filesystem_stat_value(
    eg: &mut ExecutorGlobals,
    path: &str,
    query: FilesystemStatQuery,
) -> Result<Value, VmError> {
    if let Some(value) = cached_stat_value(eg, query, path) {
        return Ok(value);
    }

    #[cfg(feature = "stream-registry")]
    if let Some(value) = super::user_wrapper::url_stat_value(eg, path, query.wrapper_flags())? {
        if eg.exception.is_some() {
            return Ok(Value::bool(false));
        }
        let value = normalize_wrapper_stat(value);
        if value.value_type() == ValueType::Array {
            cache_stat_value(eg, query, path, &value);
            if matches!(
                query,
                FilesystemStatQuery::LinkReport | FilesystemStatQuery::LinkQuiet
            ) && stat_mode(&value).is_some_and(|mode| mode & 0o170000 != 0o120000)
            {
                cache_stat_value(eg, FilesystemStatQuery::Report, path, &value);
            }
        }
        return Ok(value);
    }

    let Some(local) = local_filesystem_path(path) else {
        return Ok(Value::bool(false));
    };
    let metadata = match query {
        FilesystemStatQuery::Quiet | FilesystemStatQuery::Report => std::fs::metadata(local),
        FilesystemStatQuery::LinkReport | FilesystemStatQuery::LinkQuiet => {
            std::fs::symlink_metadata(local)
        }
    };
    let value = metadata
        .map(|metadata| stat_array_value(metadata_stat_fields(&metadata)))
        .unwrap_or_else(|_| Value::bool(false));
    if value.value_type() == ValueType::Array {
        cache_stat_value(eg, query, path, &value);
        if matches!(
            query,
            FilesystemStatQuery::LinkReport | FilesystemStatQuery::LinkQuiet
        ) && stat_mode(&value).is_some_and(|mode| mode & 0o170000 != 0o120000)
        {
            cache_stat_value(eg, FilesystemStatQuery::Report, path, &value);
        }
    }
    Ok(value)
}

fn stat_mode(value: &Value) -> Option<i64> {
    value
        .as_array()
        .and_then(|stat| stat.get_str("mode"))
        .and_then(Value::as_long)
}

fn stat_long_field(value: &Value, field: &str) -> Option<i64> {
    value
        .as_array()
        .and_then(|stat| stat.get_str(field))
        .and_then(Value::as_long)
}

#[derive(Clone, Copy)]
enum FileAccessKind {
    Read,
    Write,
    Execute,
}

#[cfg(target_os = "linux")]
fn linux_process_identity() -> &'static (u32, Vec<u32>) {
    static IDENTITY: std::sync::OnceLock<(u32, Vec<u32>)> = std::sync::OnceLock::new();
    IDENTITY.get_or_init(|| {
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        let uid = status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .and_then(|line| line.split_ascii_whitespace().next())
            .and_then(|value| value.parse().ok())
            .unwrap_or(u32::MAX);
        let gid = status
            .lines()
            .find_map(|line| line.strip_prefix("Gid:"))
            .and_then(|line| line.split_ascii_whitespace().next())
            .and_then(|value| value.parse().ok())
            .unwrap_or(u32::MAX);
        let mut groups = status
            .lines()
            .find_map(|line| line.strip_prefix("Groups:"))
            .into_iter()
            .flat_map(str::split_ascii_whitespace)
            .filter_map(|value| value.parse().ok())
            .collect::<Vec<_>>();
        if gid != u32::MAX && !groups.contains(&gid) {
            groups.push(gid);
        }
        (uid, groups)
    })
}

fn stat_access_allowed_fields(
    mode: i64,
    owner: Option<i64>,
    group: Option<i64>,
    access: FileAccessKind,
    plain_local: bool,
) -> bool {
    let bit = match access {
        FileAccessKind::Read => 0o4,
        FileAccessKind::Write => 0o2,
        FileAccessKind::Execute => 0o1,
    };

    #[cfg(target_os = "linux")]
    {
        let (uid, groups) = linux_process_identity();
        if plain_local && *uid == 0 {
            return !matches!(access, FileAccessKind::Execute) || mode & 0o111 != 0;
        }
        let owner = owner.unwrap_or(-1);
        let group = group.unwrap_or(-1);
        let shift = if owner >= 0 && owner as u32 == *uid {
            6
        } else if group >= 0 && groups.contains(&(group as u32)) {
            3
        } else {
            0
        };
        mode & (bit << shift) != 0
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (owner, group, plain_local);
        mode & (bit | bit << 3 | bit << 6) != 0
    }
}

fn stat_access_allowed(value: &Value, access: FileAccessKind) -> bool {
    let Some(mode) = stat_mode(value) else {
        return false;
    };
    stat_access_allowed_fields(
        mode,
        stat_long_field(value, "uid"),
        stat_long_field(value, "gid"),
        access,
        false,
    )
}

fn report_stat_failure(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    operation: &str,
    path: &str,
) -> Result<(), VmError> {
    super::report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!("{function}(): {operation} failed for {path}"),
    )?;
    Ok(())
}

enum StatFilenameArgument {
    Path(Value),
    Invalid,
}

fn stat_filename_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    quiet: bool,
) -> Result<Option<StatFilenameArgument>, VmError> {
    let Some(value) =
        typed_internal_string_value_argument_expected(ed, eg, function, 0, "filename", "string")?
    else {
        return Ok(None);
    };
    let path = value.as_str().unwrap_or_default();
    if path.is_empty() {
        return Ok(Some(StatFilenameArgument::Invalid));
    }
    if path.as_bytes().contains(&0) {
        if !quiet {
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("{function}(): Filename contains null byte"),
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
        }
        return Ok(Some(StatFilenameArgument::Invalid));
    }
    Ok(Some(StatFilenameArgument::Path(value)))
}

fn stat_result(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    query: FilesystemStatQuery,
) -> Result<(), VmError> {
    let Some(path) = stat_filename_argument(ed, eg, function, false)? else {
        return Ok(());
    };
    let StatFilenameArgument::Path(path) = path else {
        ret!(rv, Value::bool(false));
    };
    let path = path.as_str().unwrap_or_default();
    let value = filesystem_stat_value(eg, path, query)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    if value.value_type() == ValueType::False {
        report_stat_failure(
            ed,
            eg,
            function,
            if matches!(query, FilesystemStatQuery::LinkReport) {
                "Lstat"
            } else {
                "stat"
            },
            path,
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    ret!(rv, value);
}

fn stat_field_result(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    field: &str,
) -> Result<(), VmError> {
    let Some(path) = stat_filename_argument(ed, eg, function, false)? else {
        return Ok(());
    };
    let StatFilenameArgument::Path(path) = path else {
        ret!(rv, Value::bool(false));
    };
    let path = path.as_str().unwrap_or_default();
    let value = filesystem_stat_value(eg, path, FilesystemStatQuery::Report)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    if let Some(value) = value
        .as_array()
        .and_then(|stat| stat.get_str(field))
        .and_then(Value::as_long)
    {
        ret!(rv, Value::long(value));
    }
    report_stat_failure(ed, eg, function, "stat", path)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, Value::bool(false));
}

/// clearstatcache($clear_realpath_cache = false, $filename = ''): void
pub(super) fn fn_clearstatcache(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if arg_opt!(ed, 0).is_some()
        && typed_internal_bool_argument(ed, eg, "clearstatcache", 0, "clear_realpath_cache")?
            .is_none()
    {
        return Ok(());
    }
    if arg_opt!(ed, 1).is_some()
        && filesystem_string_value_argument(ed, eg, "clearstatcache", 1, "filename")?.is_none()
    {
        return Ok(());
    }
    clear_filesystem_stat_cache(eg);
    ret!(rv, Value::null());
}

fn disk_space_result(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    available: bool,
) -> Result<(), VmError> {
    let Some(path) = filesystem_string_value_argument(ed, eg, function, 0, "directory")? else {
        return Ok(());
    };
    let path = path.as_str().unwrap_or_default();
    if path.is_empty() {
        ret!(rv, Value::bool(false));
    }
    let result = if available {
        fs2::available_space(path)
    } else {
        fs2::total_space(path)
    };
    match result {
        Ok(space) => ret!(rv, Value::double(space as f64)),
        Err(error) => {
            let mut message = error.to_string();
            if let Some(suffix) = message.rfind(" (os error ")
                && message.ends_with(')')
            {
                message.truncate(suffix);
            }
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("{function}(): {message}"),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ret!(rv, Value::bool(false));
        }
    }
}

pub(super) fn fn_disk_free_space(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    disk_space_result(ed, rv, eg, "disk_free_space", true)
}

pub(super) fn fn_diskfreespace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    disk_space_result(ed, rv, eg, "diskfreespace", true)
}

pub(super) fn fn_disk_total_space(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    disk_space_result(ed, rv, eg, "disk_total_space", false)
}

/// file_exists($filename): bool
pub(super) fn fn_file_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = stat_filename_argument(ed, eg, "file_exists", true)? else {
        return Ok(());
    };
    let StatFilenameArgument::Path(path) = path else {
        ret!(rv, Value::bool(false));
    };
    let path = path.as_str().unwrap_or_default();
    #[cfg(feature = "stream-registry")]
    let is_user_wrapper = super::user_wrapper::definition_for_url(eg, path).is_some();
    #[cfg(not(feature = "stream-registry"))]
    let is_user_wrapper = false;
    if !is_user_wrapper {
        let Some(local) = local_filesystem_path(path) else {
            ret!(rv, Value::bool(false));
        };
        ret!(rv, Value::bool(std::fs::metadata(local).is_ok()));
    }
    let value = filesystem_stat_value(eg, path, FilesystemStatQuery::Quiet)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, Value::bool(value.value_type() == ValueType::Array));
}

/// stat($filename): array|false
pub(super) fn fn_stat(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    stat_result(ed, rv, eg, "stat", FilesystemStatQuery::Report)
}

/// lstat($filename): array|false
pub(super) fn fn_lstat(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    stat_result(ed, rv, eg, "lstat", FilesystemStatQuery::LinkReport)
}

pub(super) fn fn_filesize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    stat_field_result(ed, rv, eg, "filesize", "size")
}

pub(super) fn fn_fileatime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    stat_field_result(ed, rv, eg, "fileatime", "atime")
}

pub(super) fn fn_filemtime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    stat_field_result(ed, rv, eg, "filemtime", "mtime")
}

pub(super) fn fn_filectime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    stat_field_result(ed, rv, eg, "filectime", "ctime")
}

#[cold]
pub(super) fn fn_fileowner(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    stat_field_result(ed, rv, eg, "fileowner", "uid")
}

#[cold]
pub(super) fn fn_filegroup(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    stat_field_result(ed, rv, eg, "filegroup", "gid")
}

#[cold]
pub(super) fn fn_fileinode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    stat_field_result(ed, rv, eg, "fileinode", "ino")
}

fn file_type_name(mode: i64) -> Option<&'static str> {
    match mode & 0o170000 {
        0o010000 => Some("fifo"),
        0o020000 => Some("char"),
        0o040000 => Some("dir"),
        0o060000 => Some("block"),
        0o100000 => Some("file"),
        0o120000 => Some("link"),
        0o140000 => Some("socket"),
        _ => None,
    }
}

#[cold]
pub(super) fn fn_filetype(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = stat_filename_argument(ed, eg, "filetype", false)? else {
        return Ok(());
    };
    let StatFilenameArgument::Path(path) = path else {
        ret!(rv, Value::bool(false));
    };
    let path = path.as_str().unwrap_or_default();
    let value = filesystem_stat_value(eg, path, FilesystemStatQuery::LinkReport)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let Some(mode) = stat_mode(&value) else {
        report_stat_failure(ed, eg, "filetype", "Lstat", path)?;
        if eg.exception.is_some() {
            return Ok(());
        }
        ret!(rv, Value::bool(false));
    };
    let Some(kind) = file_type_name(mode) else {
        super::report_internal_diagnostic(
            eg,
            ed,
            8,
            "Notice",
            &format!("filetype(): Unknown file type ({})", mode & 0o170000),
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        ret!(rv, Value::string("unknown"));
    };
    ret!(rv, Value::string(kind));
}

#[cold]
pub(super) fn fn_linkinfo(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = filesystem_string_argument(ed, eg, "linkinfo", 0, "path")? else {
        return Ok(());
    };
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => ret!(rv, Value::long(metadata_stat_fields(&metadata)[0])),
        Err(error) => {
            let message = error
                .to_string()
                .split_once(" (os error ")
                .map_or_else(|| error.to_string(), |(message, _)| message.to_string());
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("linkinfo(): {message}"),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ret!(rv, Value::long(-1));
        }
    }
}

fn file_mode_predicate(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    query: FilesystemStatQuery,
    predicate: impl FnOnce(i64) -> bool,
) -> Result<(), VmError> {
    let Some(path) = stat_filename_argument(ed, eg, function, true)? else {
        return Ok(());
    };
    let StatFilenameArgument::Path(path) = path else {
        ret!(rv, Value::bool(false));
    };
    let value = filesystem_stat_value(eg, path.as_str().unwrap_or_default(), query)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, Value::bool(stat_mode(&value).is_some_and(predicate)));
}

fn file_access_predicate(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    access: FileAccessKind,
) -> Result<(), VmError> {
    let Some(path) = stat_filename_argument(ed, eg, function, true)? else {
        return Ok(());
    };
    let StatFilenameArgument::Path(path) = path else {
        ret!(rv, Value::bool(false));
    };
    let path = path.as_str().unwrap_or_default();
    #[cfg(feature = "stream-registry")]
    let is_user_wrapper = super::user_wrapper::definition_for_url(eg, path).is_some();
    #[cfg(not(feature = "stream-registry"))]
    let is_user_wrapper = false;
    let local = local_filesystem_path(path);
    if !is_user_wrapper {
        let Some(local) = local else {
            ret!(rv, Value::bool(false));
        };
        let allowed = std::fs::metadata(local).is_ok_and(|metadata| {
            let fields = metadata_stat_fields(&metadata);
            stat_access_allowed_fields(fields[2], Some(fields[4]), Some(fields[5]), access, true)
        });
        ret!(rv, Value::bool(allowed));
    }
    let value = filesystem_stat_value(eg, path, FilesystemStatQuery::Quiet)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, Value::bool(stat_access_allowed(&value, access)));
}

/// is_file($filename): bool
pub(super) fn fn_is_file(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    file_mode_predicate(ed, rv, eg, "is_file", FilesystemStatQuery::Quiet, |mode| {
        mode & 0o170000 == 0o100000
    })
}

/// is_dir($filename): bool
pub(super) fn fn_is_dir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    file_mode_predicate(ed, rv, eg, "is_dir", FilesystemStatQuery::Quiet, |mode| {
        mode & 0o170000 == 0o040000
    })
}

pub(super) fn fn_is_executable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    file_access_predicate(ed, rv, eg, "is_executable", FileAccessKind::Execute)
}

/// is_link($filename): bool — lstat semantics also recognize broken links.
pub(super) fn fn_is_link(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    file_mode_predicate(
        ed,
        rv,
        eg,
        "is_link",
        FilesystemStatQuery::LinkQuiet,
        |mode| mode & 0o170000 == 0o120000,
    )
}

pub(super) fn fn_chmod(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
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
    if result {
        clear_filesystem_stat_cache(eg);
    }
    ret!(rv, Value::bool(result));
}

pub(super) fn fn_fileperms(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    stat_field_result(ed, rv, eg, "fileperms", "mode")
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
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = filesystem_string_value_argument(ed, eg, "realpath", 0, "path")? else {
        return Ok(());
    };
    let path = path.as_str().unwrap_or_default();
    let path = if path.is_empty() { "." } else { path };
    match std::fs::canonicalize(path) {
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
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    match std::fs::File::open(path) {
        Ok(mut file) => {
            clear_filesystem_stat_cache(eg);
            let mut bytes = Vec::new();
            if std::io::Read::read_to_end(&mut file, &mut bytes).is_err() {
                ret!(rv, Value::bool(false));
            }
            let mut arr = PhpArray::new();
            let mut start = 0;
            while start < bytes.len() {
                match bytes[start..].iter().position(|byte| *byte == b'\n') {
                    Some(pos) => {
                        let end = start + pos + 1;
                        arr.push(php_byte_result(bytes[start..end].to_vec(), false));
                        start = end;
                    }
                    None => {
                        arr.push(php_byte_result(bytes[start..].to_vec(), false));
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
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    return_default_file_lines(path.as_ref(), rv, eg)
}

fn filesystem_string_value_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<Value>, VmError> {
    let Some(value) = typed_internal_string_value_argument_expected(
        ed, eg, function, index, parameter, "string",
    )?
    else {
        return Ok(None);
    };
    let path = value.as_str().unwrap_or_default();
    if path.contains('\0') {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            &format!(
                "{function}(): Argument #{} (${parameter}) must not contain any null bytes",
                index + 1
            ),
        ));
        return Ok(None);
    }
    Ok(Some(value))
}

fn filesystem_string_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<String>, VmError> {
    Ok(
        filesystem_string_value_argument(ed, eg, function, index, parameter)?
            .map(|value| value.as_str().unwrap_or_default().to_string()),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OptionalStreamContext {
    Valid,
    InvalidResource,
}

fn classify_optional_stream_context(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
) -> Option<OptionalStreamContext> {
    let Some(context) = arg_opt!(ed, index) else {
        return Some(OptionalStreamContext::Valid);
    };
    let context = context.dereferenced();
    if context.value_type() == ValueType::Null {
        return Some(OptionalStreamContext::Valid);
    }
    let Some(resource) = context.as_resource_id() else {
        super::typed_internal_argument_error(
            eg,
            function,
            context,
            index as usize + 1,
            "context",
            "resource or null",
        );
        return None;
    };

    #[cfg(feature = "stream-context")]
    {
        if super::streams::context::context_snapshot(eg, resource).is_none() {
            super::streams::context::invalid_context_error(eg, function);
            return Some(OptionalStreamContext::InvalidResource);
        }
        Some(OptionalStreamContext::Valid)
    }
    #[cfg(not(feature = "stream-context"))]
    {
        let _ = resource;
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("{function}(): supplied resource is not a valid Stream-Context resource"),
        ));
        Some(OptionalStreamContext::InvalidResource)
    }
}

fn filesystem_error_reason(error: &std::io::Error) -> String {
    let rendered = error.to_string();
    rendered
        .split_once(" (os error ")
        .map_or(rendered.as_str(), |(reason, _)| reason)
        .to_string()
}

fn report_filesystem_diagnostic(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    level: i64,
    label: &str,
    message: &str,
) -> Result<(), VmError> {
    super::report_internal_diagnostic(eg, ed, level, label, message)?;
    Ok(())
}

/// mkdir($directory, $permissions = 0777, $recursive = false, $context = null): bool
pub(super) fn fn_mkdir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = filesystem_string_argument(ed, eg, "mkdir", 0, "directory")? else {
        return Ok(());
    };
    let permissions = if arg_opt!(ed, 1).is_some() {
        let Some(permissions) = typed_internal_int_argument(ed, eg, "mkdir", 1, "permissions")?
        else {
            return Ok(());
        };
        permissions
    } else {
        0o777
    };
    let recursive = if arg_opt!(ed, 2).is_some() {
        let Some(recursive) = typed_internal_bool_argument(ed, eg, "mkdir", 2, "recursive")? else {
            return Ok(());
        };
        recursive
    } else {
        false
    };
    let Some(context) = classify_optional_stream_context(ed, eg, "mkdir", 3) else {
        return Ok(());
    };

    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(recursive);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(permissions as u32);
    }
    #[cfg(not(unix))]
    let _ = permissions;
    let result = if recursive && std::fs::symlink_metadata(&path).is_ok() {
        // `create_dir_all` treats an existing final directory as success, while
        // PHP reports EEXIST for the requested final component.
        std::fs::create_dir(&path)
    } else {
        builder.create(&path)
    };
    if context == OptionalStreamContext::InvalidResource {
        return Ok(());
    }
    match result {
        Ok(()) => {
            clear_filesystem_stat_cache(eg);
            ret!(rv, Value::bool(true));
        }
        Err(error) => {
            report_filesystem_diagnostic(
                ed,
                eg,
                2,
                "Warning",
                &format!("mkdir(): {}", filesystem_error_reason(&error)),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ret!(rv, Value::bool(false));
        }
    }
}

/// rmdir($directory, $context = null): bool
pub(super) fn fn_rmdir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = filesystem_string_argument(ed, eg, "rmdir", 0, "directory")? else {
        return Ok(());
    };
    let Some(context) = classify_optional_stream_context(ed, eg, "rmdir", 1) else {
        return Ok(());
    };
    let result = std::fs::remove_dir(&path);
    if context == OptionalStreamContext::InvalidResource {
        return Ok(());
    }
    match result {
        Ok(()) => {
            clear_filesystem_stat_cache(eg);
            ret!(rv, Value::bool(true));
        }
        Err(error) => {
            report_filesystem_diagnostic(
                ed,
                eg,
                2,
                "Warning",
                &format!("rmdir({path}): {}", filesystem_error_reason(&error)),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ret!(rv, Value::bool(false));
        }
    }
}

/// unlink($filename, $context = null): bool
pub(super) fn fn_unlink(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = filesystem_string_argument(ed, eg, "unlink", 0, "filename")? else {
        return Ok(());
    };
    let Some(context) = classify_optional_stream_context(ed, eg, "unlink", 1) else {
        return Ok(());
    };
    let result = std::fs::remove_file(&path);
    if context == OptionalStreamContext::InvalidResource {
        return Ok(());
    }
    match result {
        Ok(()) => {
            clear_filesystem_stat_cache(eg);
            ret!(rv, Value::bool(true));
        }
        Err(error) => {
            report_filesystem_diagnostic(
                ed,
                eg,
                2,
                "Warning",
                &format!("unlink({path}): {}", filesystem_error_reason(&error)),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ret!(rv, Value::bool(false));
        }
    }
}

/// rename($from, $to, $context = null): bool
pub(super) fn fn_rename(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(from) = filesystem_string_argument(ed, eg, "rename", 0, "from")? else {
        return Ok(());
    };
    let Some(to) = filesystem_string_argument(ed, eg, "rename", 1, "to")? else {
        return Ok(());
    };
    let Some(context) = classify_optional_stream_context(ed, eg, "rename", 2) else {
        return Ok(());
    };
    let result = std::fs::rename(&from, &to);
    if context == OptionalStreamContext::InvalidResource {
        return Ok(());
    }
    match result {
        Ok(()) => {
            clear_filesystem_stat_cache(eg);
            ret!(rv, Value::bool(true));
        }
        Err(error) => {
            report_filesystem_diagnostic(
                ed,
                eg,
                2,
                "Warning",
                &format!("rename({from},{to}): {}", filesystem_error_reason(&error)),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ret!(rv, Value::bool(false));
        }
    }
}

/// copy($from, $to, $context = null): bool
pub(super) fn fn_copy(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(from) = filesystem_string_argument(ed, eg, "copy", 0, "from")? else {
        return Ok(());
    };
    let Some(to) = filesystem_string_argument(ed, eg, "copy", 1, "to")? else {
        return Ok(());
    };
    if from.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "Path must not be empty",
        ));
        return Ok(());
    }
    let Some(context) = classify_optional_stream_context(ed, eg, "copy", 2) else {
        return Ok(());
    };
    if Path::new(&from).is_dir() {
        if context == OptionalStreamContext::InvalidResource {
            return Ok(());
        }
        report_filesystem_diagnostic(
            ed,
            eg,
            2,
            "Warning",
            "copy(): The first argument to copy() function cannot be a directory",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        ret!(rv, Value::bool(false));
    }
    let source_exists = std::fs::metadata(&from).is_ok();
    if source_exists && to.is_empty() {
        if context == OptionalStreamContext::InvalidResource {
            return Ok(());
        }
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "Path must not be empty",
        ));
        return Ok(());
    }
    if source_exists && Path::new(&to).is_dir() {
        if context == OptionalStreamContext::InvalidResource {
            return Ok(());
        }
        report_filesystem_diagnostic(
            ed,
            eg,
            2,
            "Warning",
            "copy(): The second argument to copy() function cannot be a directory",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        ret!(rv, Value::bool(false));
    }
    if paths_refer_to_same_file(Path::new(&from), Path::new(&to)) {
        if context == OptionalStreamContext::InvalidResource {
            return Ok(());
        }
        ret!(rv, Value::bool(false));
    }
    let result = copy_file_contents(Path::new(&from), Path::new(&to), eg);
    if context == OptionalStreamContext::InvalidResource {
        return Ok(());
    }
    match result {
        Ok(copied) => {
            if copied {
                clear_filesystem_stat_cache(eg);
            }
            ret!(rv, Value::bool(copied));
        }
        Err(error) => {
            report_filesystem_diagnostic(
                ed,
                eg,
                2,
                "Warning",
                &format!(
                    "copy({from}): Failed to open stream: {}",
                    filesystem_error_reason(&error)
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ret!(rv, Value::bool(false));
        }
    }
}

#[cfg(unix)]
fn copy_file_contents(
    from: &Path,
    to: &Path,
    eg: &mut ExecutorGlobals,
) -> Result<bool, std::io::Error> {
    use std::os::unix::fs::MetadataExt;

    let mut source = std::fs::File::open(from)?;
    clear_filesystem_stat_cache(eg);
    let mut destination = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(to)?;
    clear_filesystem_stat_cache(eg);

    let source_metadata = source.metadata()?;
    let destination_metadata = destination.metadata()?;
    if source_metadata.dev() == destination_metadata.dev()
        && source_metadata.ino() == destination_metadata.ino()
    {
        return Ok(false);
    }

    destination.set_len(0)?;
    std::io::copy(&mut source, &mut destination)?;
    Ok(true)
}

#[cfg(not(unix))]
fn copy_file_contents(
    from: &Path,
    to: &Path,
    eg: &mut ExecutorGlobals,
) -> Result<bool, std::io::Error> {
    use std::io::{Read, Write};

    // Stable std has no portable hard-link identity. Buffer before opening an
    // existing destination so aliases cannot truncate the source.
    let mut source = std::fs::File::open(from)?;
    clear_filesystem_stat_cache(eg);
    let mut contents = Vec::new();
    source.read_to_end(&mut contents)?;
    let mut destination = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(to)?;
    clear_filesystem_stat_cache(eg);
    destination.write_all(&contents)?;
    destination.set_len(contents.len() as u64)?;
    Ok(true)
}

fn paths_refer_to_same_file(from: &Path, to: &Path) -> bool {
    let (Ok(from_metadata), Ok(to_metadata)) = (std::fs::metadata(from), std::fs::metadata(to))
    else {
        return false;
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        from_metadata.dev() == to_metadata.dev() && from_metadata.ino() == to_metadata.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (from_metadata, to_metadata);
        std::fs::canonicalize(from)
            .ok()
            .zip(std::fs::canonicalize(to).ok())
            .is_some_and(|(from, to)| from == to)
    }
}

/// tempnam($directory, $prefix): string|false
pub(super) fn fn_tempnam(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(directory) = filesystem_string_argument(ed, eg, "tempnam", 0, "directory")? else {
        return Ok(());
    };
    let Some(prefix) = filesystem_string_argument(ed, eg, "tempnam", 1, "prefix")? else {
        return Ok(());
    };
    let prefix = tempnam_prefix(&prefix);
    let system_temp = absolute_directory(&std::env::temp_dir());
    let (directory, already_fell_back) = if directory.is_empty() {
        (system_temp.clone(), true)
    } else if Path::new(&directory).is_dir() {
        (absolute_directory(Path::new(&directory)), false)
    } else {
        report_tempnam_fallback(ed, eg)?;
        if eg.exception.is_some() {
            return Ok(());
        }
        (system_temp.clone(), true)
    };

    match create_tempnam_file(&directory, &prefix, eg) {
        Ok(path) => ret!(rv, Value::string(path.to_string_lossy().into_owned())),
        Err(_) if !already_fell_back && directory != system_temp => {
            report_tempnam_fallback(ed, eg)?;
            if eg.exception.is_some() {
                return Ok(());
            }
            match create_tempnam_file(&system_temp, &prefix, eg) {
                Ok(path) => ret!(rv, Value::string(path.to_string_lossy().into_owned())),
                Err(_) => ret!(rv, Value::bool(false)),
            }
        }
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

fn report_tempnam_fallback(ed: *mut ExecuteData, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    report_filesystem_diagnostic(
        ed,
        eg,
        8,
        "Notice",
        "tempnam(): file created in the system's temporary directory",
    )
}

fn absolute_directory(directory: &Path) -> PathBuf {
    std::fs::canonicalize(directory).unwrap_or_else(|_| {
        if directory.is_absolute() {
            directory.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|current| current.join(directory))
                .unwrap_or_else(|_| directory.to_path_buf())
        }
    })
}

fn tempnam_prefix(prefix: &str) -> String {
    #[cfg(windows)]
    let basename = prefix
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    #[cfg(not(windows))]
    let basename = prefix
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();

    let mut end = basename.len().min(63);
    while !basename.is_char_boundary(end) {
        end -= 1;
    }
    basename[..end].to_string()
}

fn create_tempnam_file(
    directory: &Path,
    prefix: &str,
    eg: &mut ExecutorGlobals,
) -> Result<PathBuf, std::io::Error> {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let mut last_collision = None;
    for _ in 0..128 {
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let nonce = timestamp ^ ((std::process::id() as u128) << 48) ^ sequence as u128;
        let name = format!("{prefix}{:019x}", nonce & ((1_u128 << 76) - 1));
        let path = directory.join(name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(_) => {
                clear_filesystem_stat_cache(eg);
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                last_collision = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_collision.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not create a unique temporary file",
        )
    }))
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
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    file_access_predicate(ed, rv, eg, "is_readable", FileAccessKind::Read)
}

/// is_writable($filename): bool / is_writeable()
pub(super) fn fn_is_writable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    file_access_predicate(ed, rv, eg, "is_writable", FileAccessKind::Write)
}

pub(super) fn fn_is_writeable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    file_access_predicate(ed, rv, eg, "is_writeable", FileAccessKind::Write)
}

/// glob($pattern, $flags = 0): array|false
pub(super) fn fn_glob(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(pattern) = filesystem_string_argument(ed, eg, "glob", 0, "pattern")? else {
        return Ok(());
    };
    let flags = if arg_opt!(ed, 1).is_some() {
        let Some(flags) = typed_internal_int_argument(ed, eg, "glob", 1, "flags")? else {
            return Ok(());
        };
        flags
    } else {
        0
    };
    if flags & !GLOB_AVAILABLE_FLAGS != 0 {
        super::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "glob(): At least one of the passed flags is invalid or not supported on this platform",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        ret!(rv, Value::bool(false));
    }

    let options = GlobOptions {
        mark: flags & GLOB_MARK != 0,
        no_check: flags & GLOB_NOCHECK != 0,
        no_sort: flags & GLOB_NOSORT != 0,
        no_escape: flags & GLOB_NOESCAPE != 0,
        only_dir: flags & GLOB_ONLYDIR != 0,
        abort_on_error: flags & GLOB_ERR != 0,
    };
    let expanded = if flags & GLOB_BRACE != 0 {
        expand_glob_braces(&pattern, options.no_escape)
    } else {
        vec![pattern.clone()]
    };
    let mut results = Vec::new();
    for expanded_pattern in expanded {
        let mut pattern_results = Vec::new();
        if !collect_glob_pattern(&expanded_pattern, options, &mut pattern_results) {
            ret!(rv, Value::bool(false));
        }
        if options.no_check && !options.only_dir && pattern_results.is_empty() {
            pattern_results.push(expanded_pattern);
        }
        if !options.no_sort {
            pattern_results.sort();
        }
        results.extend(pattern_results);
    }

    let mut arr = PhpArray::new();
    for result in results {
        arr.push(Value::string(result));
    }
    ret!(rv, Value::array(arr));
}

#[derive(Clone, Copy)]
struct GlobOptions {
    mark: bool,
    no_check: bool,
    no_sort: bool,
    no_escape: bool,
    only_dir: bool,
    abort_on_error: bool,
}

fn collect_glob_pattern(pattern: &str, options: GlobOptions, results: &mut Vec<String>) -> bool {
    if pattern.is_empty() {
        return true;
    }
    let leading_slashes = pattern
        .chars()
        .take_while(|character| *character == '/')
        .count();
    let absolute = leading_slashes != 0;
    let components: Vec<&str> = pattern.split('/').collect();
    let index = leading_slashes;
    let filesystem_prefix = if absolute {
        PathBuf::from("/")
    } else {
        PathBuf::from(".")
    };
    let display_prefix = if absolute {
        &pattern[..leading_slashes]
    } else {
        ""
    };
    collect_glob_components(
        &filesystem_prefix,
        display_prefix,
        &components,
        index,
        options,
        results,
    )
}

fn collect_glob_components(
    filesystem_prefix: &Path,
    display_prefix: &str,
    components: &[&str],
    index: usize,
    options: GlobOptions,
    results: &mut Vec<String>,
) -> bool {
    if index == components.len() {
        return push_glob_result(filesystem_prefix, display_prefix, options, results);
    }

    let component = components[index];
    if component.is_empty() {
        if index + 1 == components.len() {
            let marked = if display_prefix.ends_with('/') {
                display_prefix.to_string()
            } else {
                format!("{display_prefix}/")
            };
            return push_glob_result(filesystem_prefix, &marked, options, results);
        }
        let repeated_separator = if display_prefix.ends_with('/') {
            format!("{display_prefix}/")
        } else {
            format!("{display_prefix}//")
        };
        return collect_glob_components(
            filesystem_prefix,
            &repeated_separator,
            components,
            index + 1,
            options,
            results,
        );
    }

    if !component_has_glob_magic(component, options.no_escape) {
        let literal = unescape_glob_literal(component, options.no_escape);
        let filesystem_path = filesystem_prefix.join(&literal);
        let display_path = join_glob_display(display_prefix, &literal);
        if index + 1 < components.len() && !filesystem_path.is_dir() {
            return true;
        }
        return collect_glob_components(
            &filesystem_path,
            &display_path,
            components,
            index + 1,
            options,
            results,
        );
    }

    let entries = match std::fs::read_dir(filesystem_prefix) {
        Ok(entries) => entries,
        Err(error) => {
            return !options.abort_on_error || error.kind() == std::io::ErrorKind::NotFound;
        }
    };
    let tokens = compile_glob_tokens(component, options.no_escape);
    let final_component = index + 1 == components.len();
    for name in [".", ".."] {
        if !glob_tokens_match(&tokens, name) {
            continue;
        }
        let display_path = join_glob_display(display_prefix, name);
        if final_component {
            let mut display_path = display_path;
            if options.mark && !display_path.ends_with('/') {
                display_path.push('/');
            }
            results.push(display_path);
            continue;
        }
        let filesystem_path = filesystem_prefix.join(name);
        if !collect_glob_components(
            &filesystem_path,
            &display_path,
            components,
            index + 1,
            options,
            results,
        ) {
            return false;
        }
    }
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) if options.abort_on_error => return false,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !glob_tokens_match(&tokens, &name) {
            continue;
        }
        let display_path = join_glob_display(display_prefix, &name);
        if final_component {
            if options.only_dir || options.mark {
                let is_directory = entry.path().is_dir();
                if options.only_dir && !is_directory {
                    continue;
                }
                let mut display_path = display_path;
                if options.mark && is_directory && !display_path.ends_with('/') {
                    display_path.push('/');
                }
                results.push(display_path);
            } else {
                results.push(display_path);
            }
            continue;
        }
        let filesystem_path = entry.path();
        if !filesystem_path.is_dir() {
            continue;
        }
        if !collect_glob_components(
            &filesystem_path,
            &display_path,
            components,
            index + 1,
            options,
            results,
        ) {
            return false;
        }
    }
    true
}

fn push_glob_result(
    filesystem_path: &Path,
    display_path: &str,
    options: GlobOptions,
    results: &mut Vec<String>,
) -> bool {
    let link_metadata = match std::fs::symlink_metadata(filesystem_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return !options.abort_on_error || error.kind() == std::io::ErrorKind::NotFound;
        }
    };
    let is_directory = if link_metadata.file_type().is_symlink() {
        std::fs::metadata(filesystem_path)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
    } else {
        link_metadata.is_dir()
    };
    if options.only_dir && !is_directory {
        return true;
    }
    let mut display_path = display_path.to_string();
    if options.mark && is_directory && !display_path.ends_with('/') {
        display_path.push('/');
    }
    results.push(display_path);
    true
}

fn join_glob_display(prefix: &str, component: &str) -> String {
    if prefix.is_empty() {
        component.to_string()
    } else if prefix.ends_with('/') {
        format!("{prefix}{component}")
    } else {
        format!("{prefix}/{component}")
    }
}

fn component_has_glob_magic(pattern: &str, no_escape: bool) -> bool {
    let mut escaped = false;
    for character in pattern.chars() {
        if !no_escape && !escaped && character == '\\' {
            escaped = true;
            continue;
        }
        if !escaped && matches!(character, '*' | '?' | '[') {
            return true;
        }
        escaped = false;
    }
    false
}

fn unescape_glob_literal(pattern: &str, no_escape: bool) -> String {
    if no_escape {
        return pattern.to_string();
    }
    let mut literal = String::with_capacity(pattern.len());
    let mut characters = pattern.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            if let Some(escaped) = characters.next() {
                literal.push(escaped);
            } else {
                literal.push(character);
            }
        } else {
            literal.push(character);
        }
    }
    literal
}

fn expand_glob_braces(pattern: &str, no_escape: bool) -> Vec<String> {
    let mut expanded = Vec::new();
    expand_glob_braces_inner(pattern, no_escape, &mut expanded);
    expanded
}

fn expand_glob_braces_inner(pattern: &str, no_escape: bool, expanded: &mut Vec<String>) {
    if expanded.len() >= 1024 {
        return;
    }
    let characters: Vec<char> = pattern.chars().collect();
    let mut escaped = false;
    let mut opening = None;
    let mut depth = 0usize;
    for (index, character) in characters.iter().copied().enumerate() {
        if !no_escape && !escaped && character == '\\' {
            escaped = true;
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if character == '{' {
            if depth == 0 {
                opening = Some(index);
            }
            depth += 1;
            continue;
        }
        if character != '}' || depth == 0 {
            continue;
        }
        depth -= 1;
        if depth != 0 {
            continue;
        }
        let start = opening.expect("brace depth has an opening delimiter");
        let alternatives = split_brace_alternatives(&characters[start + 1..index], no_escape);
        let prefix: String = characters[..start].iter().collect();
        let suffix: String = characters[index + 1..].iter().collect();
        for alternative in alternatives {
            expand_glob_braces_inner(
                &format!("{prefix}{alternative}{suffix}"),
                no_escape,
                expanded,
            );
        }
        return;
    }
    expanded.push(pattern.to_string());
}

fn split_brace_alternatives(characters: &[char], no_escape: bool) -> Vec<String> {
    let mut alternatives = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut escaped = false;
    for (index, character) in characters.iter().copied().enumerate() {
        if !no_escape && !escaped && character == '\\' {
            escaped = true;
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                alternatives.push(characters[start..index].iter().collect());
                start = index + 1;
            }
            _ => {}
        }
    }
    alternatives.push(characters[start..].iter().collect());
    alternatives
}

#[derive(Clone)]
enum GlobToken {
    Star,
    Any,
    Literal(char),
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
        posix: Vec<PosixGlobClass>,
    },
}

#[derive(Clone, Copy)]
enum PosixGlobClass {
    Alnum,
    Alpha,
    Blank,
    Cntrl,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Space,
    Upper,
    Xdigit,
}

impl PosixGlobClass {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "alnum" => Self::Alnum,
            "alpha" => Self::Alpha,
            "blank" => Self::Blank,
            "cntrl" => Self::Cntrl,
            "digit" => Self::Digit,
            "graph" => Self::Graph,
            "lower" => Self::Lower,
            "print" => Self::Print,
            "punct" => Self::Punct,
            "space" => Self::Space,
            "upper" => Self::Upper,
            "xdigit" => Self::Xdigit,
            _ => return None,
        })
    }

    fn matches(self, character: char) -> bool {
        match self {
            Self::Alnum => character.is_ascii_alphanumeric(),
            Self::Alpha => character.is_ascii_alphabetic(),
            Self::Blank => matches!(character, ' ' | '\t'),
            Self::Cntrl => character.is_ascii_control(),
            Self::Digit => character.is_ascii_digit(),
            Self::Graph => character.is_ascii_graphic(),
            Self::Lower => character.is_ascii_lowercase(),
            Self::Print => character == ' ' || character.is_ascii_graphic(),
            Self::Punct => character.is_ascii_punctuation(),
            Self::Space => character.is_ascii_whitespace(),
            Self::Upper => character.is_ascii_uppercase(),
            Self::Xdigit => character.is_ascii_hexdigit(),
        }
    }
}

impl GlobToken {
    fn matches(&self, character: char) -> bool {
        match self {
            Self::Any => true,
            Self::Literal(expected) => *expected == character,
            Self::Class {
                negated,
                ranges,
                posix,
            } => {
                let contains = ranges
                    .iter()
                    .any(|(start, end)| *start <= character && character <= *end)
                    || posix.iter().any(|class| class.matches(character));
                contains != *negated
            }
            Self::Star => false,
        }
    }
}

#[cfg(test)]
fn glob_match(pattern: &str, text: &str, no_escape: bool) -> bool {
    let tokens = compile_glob_tokens(pattern, no_escape);
    glob_tokens_match(&tokens, text)
}

fn glob_tokens_match(tokens: &[GlobToken], text: &str) -> bool {
    if text.starts_with('.')
        && !matches!(tokens.first(), Some(GlobToken::Literal(character)) if *character == '.')
    {
        return false;
    }
    let text: Vec<char> = text.chars().collect();
    let mut pattern_index = 0usize;
    let mut text_index = 0usize;
    let mut star = None;

    while text_index < text.len() {
        if matches!(tokens.get(pattern_index), Some(GlobToken::Star)) {
            pattern_index += 1;
            star = Some((pattern_index, text_index));
        } else if tokens
            .get(pattern_index)
            .is_some_and(|token| token.matches(text[text_index]))
        {
            pattern_index += 1;
            text_index += 1;
        } else if let Some((after_star, matched)) = star {
            let next = matched + 1;
            if next > text.len() {
                return false;
            }
            star = Some((after_star, next));
            pattern_index = after_star;
            text_index = next;
        } else {
            return false;
        }
    }
    while matches!(tokens.get(pattern_index), Some(GlobToken::Star)) {
        pattern_index += 1;
    }
    pattern_index == tokens.len()
}

fn compile_glob_tokens(pattern: &str, no_escape: bool) -> Vec<GlobToken> {
    let characters: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < characters.len() {
        match characters[index] {
            '\\' if !no_escape && index + 1 < characters.len() => {
                tokens.push(GlobToken::Literal(characters[index + 1]));
                index += 2;
            }
            '*' => {
                if !matches!(tokens.last(), Some(GlobToken::Star)) {
                    tokens.push(GlobToken::Star);
                }
                index += 1;
            }
            '?' => {
                tokens.push(GlobToken::Any);
                index += 1;
            }
            '[' => {
                if let Some((class, next)) = compile_glob_class(&characters, index, no_escape) {
                    tokens.push(class);
                    index = next;
                } else {
                    tokens.push(GlobToken::Literal('['));
                    index += 1;
                }
            }
            character => {
                tokens.push(GlobToken::Literal(character));
                index += 1;
            }
        }
    }
    tokens
}

fn compile_glob_class(
    characters: &[char],
    opening: usize,
    no_escape: bool,
) -> Option<(GlobToken, usize)> {
    let mut index = opening + 1;
    let negated = matches!(characters.get(index), Some('!' | '^'));
    if negated {
        index += 1;
    }
    let mut members = Vec::new();
    let mut posix = Vec::new();
    if characters.get(index) == Some(&']') {
        members.push((']', false));
        index += 1;
    }
    while index < characters.len() && characters[index] != ']' {
        if characters[index] == '[' && characters.get(index + 1) == Some(&':') {
            let name_start = index + 2;
            let mut closing = name_start;
            while closing + 1 < characters.len()
                && !(characters[closing] == ':' && characters[closing + 1] == ']')
            {
                closing += 1;
            }
            if closing + 1 < characters.len() {
                let name: String = characters[name_start..closing].iter().collect();
                if let Some(class) = PosixGlobClass::from_name(&name) {
                    posix.push(class);
                    index = closing + 2;
                    continue;
                }
            }
        }
        let escaped = !no_escape && characters[index] == '\\' && index + 1 < characters.len();
        if escaped {
            index += 1;
        }
        members.push((characters[index], escaped));
        index += 1;
    }
    if index == characters.len() || (members.is_empty() && posix.is_empty()) {
        return None;
    }

    let mut ranges = Vec::new();
    let mut member = 0usize;
    while member < members.len() {
        if member + 2 < members.len() && members[member + 1].0 == '-' && !members[member + 1].1 {
            ranges.push((members[member].0, members[member + 2].0));
            member += 3;
        } else {
            ranges.push((members[member].0, members[member].0));
            member += 1;
        }
    }
    Some((
        GlobToken::Class {
            negated,
            ranges,
            posix,
        },
        index + 1,
    ))
}

#[cfg(test)]
mod tests {
    use super::{bytes_to_php_string, expand_glob_braces, glob_match, php_string_to_bytes};

    #[test]
    fn byte_string_helpers_round_trip_every_byte() {
        let bytes: Vec<u8> = (u8::MIN..=u8::MAX).collect();
        assert_eq!(php_string_to_bytes(&bytes_to_php_string(&bytes)), bytes);
    }

    #[test]
    fn glob_matcher_preserves_supported_wildcard_contract() {
        assert!(glob_match("*.php", "index.php", false));
        assert!(glob_match("file-?.txt", "file-a.txt", false));
        assert!(glob_match("file-[a-c].txt", "file-b.txt", false));
        assert!(glob_match("file-[a\\-c].txt", "file--.txt", false));
        assert!(!glob_match("file-[a\\-c].txt", "file-b.txt", false));
        assert!(glob_match("literal\\*", "literal*", false));
        assert!(glob_match("*", "", false));
        assert!(!glob_match("*", ".hidden", false));
        assert!(!glob_match("*.php", "index.phpt", false));
        assert!(!glob_match("file-[!a-c].txt", "file-b.txt", false));
        assert!(!glob_match("file-?.txt", "file-long.txt", false));
        assert_eq!(
            expand_glob_braces("config/{dev,{prod,test}}/*.php", false),
            ["config/dev/*.php", "config/prod/*.php", "config/test/*.php",]
        );
    }
}
