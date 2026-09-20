//! Read-only `phar://` archives.
//!
//! The manifest parser, the request-local archive registry behind the
//! `phar://` wrapper and the `Phar` class surface a self-contained
//! distribution such as `phpstan.phar` needs: `Phar::mapPhar()` from the
//! stub, `phar://` includes, reads, stats and directory listings of a mapped
//! archive, and `Phar::running()` inside it. Archives are never written and
//! compressed members are not served.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::compiler::compile::{ClassConstantDefinition, ClassDef};
use crate::compiler::make_internal_method;
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::{ObjectLayout, PhpArray, Value, ValueType, make_error_value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction, ParamTypeHint};

const PHAR: &str = "Phar";
const PHAR_EXCEPTION: &str = "PharException";
const HALT_COMPILER: &[u8] = b"__HALT_COMPILER();";
const MANIFEST_SIGNED: u32 = 0x0001_0000;
const ENTRY_COMPRESSED: u32 = 0x0000_F000;
/// Suffix of the parse error for a file without a `__HALT_COMPILER();` token.
const HALT_MISSING: &str = "(__HALT_COMPILER(); not found)";
/// PHP scans for the token in 1024-byte reads and reports a final read
/// shorter than the token as truncation rather than absence.
const HALT_TRUNCATED: &str = "(truncated entry)";
const HALT_SCAN_CHUNK: usize = 1024;

/// The diagnostic PHP gives a file that never mentions `__HALT_COMPILER();`.
fn halt_missing_reason(path: &str, data: &[u8]) -> String {
    let final_read = data.len() % HALT_SCAN_CHUNK;
    let suffix = if final_read < "__HALT_COMPILER();".len() {
        HALT_TRUNCATED
    } else {
        HALT_MISSING
    };
    format!("internal corruption of phar \"{path}\" {suffix}")
}

const SIGNATURE_MD5: u32 = 1;
const SIGNATURE_SHA1: u32 = 2;
const SIGNATURE_SHA256: u32 = 3;
const SIGNATURE_SHA512: u32 = 4;
const SIGNATURE_OPENSSL: u32 = 0x10;
const SIGNATURE_OPENSSL_SHA256: u32 = 0x11;
const SIGNATURE_OPENSSL_SHA512: u32 = 0x12;

/// One regular archive member: its bytes live in the archive buffer.
struct PharEntry {
    offset: usize,
    size: usize,
    mtime: i64,
}

/// A parsed, signature-checked archive kept in memory for the request.
pub(crate) struct PharArchive {
    /// Canonical filesystem path of the archive file.
    pub(crate) path: String,
    data: Vec<u8>,
    entries: HashMap<String, PharEntry>,
    directories: HashSet<String>,
    file_mtime: i64,
    alias: Option<String>,
}

/// Request-local archive registry: parsed archives by canonical path and the
/// aliases `Phar::mapPhar()`/`loadPhar()` or manifests publish for them.
#[derive(Default)]
pub struct PharRuntime {
    archives: HashMap<String, Rc<PharArchive>>,
    aliases: HashMap<String, String>,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| "truncated manifest".to_string())?;
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u16(&mut self) -> Result<u16, String> {
        let bytes = self.bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }
}

/// Offset of the manifest: the byte after the stub's `__HALT_COMPILER();`,
/// an optional `?>` and one line break.
fn manifest_start(data: &[u8]) -> Option<usize> {
    let index = memchr::memmem::find(data, HALT_COMPILER).or_else(|| {
        data.windows(HALT_COMPILER.len())
            .position(|window| window.eq_ignore_ascii_case(HALT_COMPILER))
    })?;
    let mut position = index + HALT_COMPILER.len();
    while data
        .get(position)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        position += 1;
    }
    if data[position..].starts_with(b"?>") {
        position += 2;
    }
    if data[position..].starts_with(b"\r\n") {
        position += 2;
    } else if data[position..].starts_with(b"\n") {
        position += 1;
    }
    Some(position)
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02X}")).collect()
}

/// Verify the trailing signature block: `hash | [OpenSSL length] | flags |
/// "GBMB"`. OpenSSL signatures need the public key PHP keeps beside the
/// archive; they are accepted unverified.
fn verify_signature(data: &[u8], path: &str) -> Result<(&'static str, String), String> {
    let broken = || format!("phar \"{path}\" has a broken signature");
    if data.len() < 8 || &data[data.len() - 4..] != b"GBMB" {
        return Err(broken());
    }
    let mismatch = |algorithm: &str| {
        format!("phar \"{path}\" {algorithm} signature could not be verified: broken signature")
    };
    let flags_at = data.len() - 8;
    let flags = u32::from_le_bytes([
        data[flags_at],
        data[flags_at + 1],
        data[flags_at + 2],
        data[flags_at + 3],
    ]);
    let (name, hash_len) = match flags {
        SIGNATURE_MD5 => ("MD5", 16),
        SIGNATURE_SHA1 => ("SHA-1", 20),
        SIGNATURE_SHA256 => ("SHA-256", 32),
        SIGNATURE_SHA512 => ("SHA-512", 64),
        SIGNATURE_OPENSSL | SIGNATURE_OPENSSL_SHA256 | SIGNATURE_OPENSSL_SHA512 => {
            if flags_at < 4 {
                return Err(broken());
            }
            let len_at = flags_at - 4;
            let len = u32::from_le_bytes([
                data[len_at],
                data[len_at + 1],
                data[len_at + 2],
                data[len_at + 3],
            ]) as usize;
            let start = len_at.checked_sub(len).ok_or_else(broken)?;
            return Ok(("OpenSSL", hex(&data[start..len_at])));
        }
        _ => return Err(broken()),
    };
    let signed_end = flags_at.checked_sub(hash_len).ok_or_else(broken)?;
    let stored = &data[signed_end..flags_at];
    let computed: Vec<u8> = match flags {
        SIGNATURE_MD5 => super::md5_digest(&data[..signed_end]).to_vec(),
        SIGNATURE_SHA1 => super::sha1_digest(&data[..signed_end]).to_vec(),
        SIGNATURE_SHA256 => super::sha256::sha256_digest(&data[..signed_end]).to_vec(),
        _ => super::sha512::sha512_digest(&data[..signed_end]).to_vec(),
    };
    if computed != stored {
        return Err(mismatch(name.replace('-', "").as_str()));
    }
    Ok((name, hex(stored)))
}

impl PharArchive {
    fn parse(path: String, data: Vec<u8>, file_mtime: i64) -> Result<Self, String> {
        let manifest_offset =
            manifest_start(&data).ok_or_else(|| halt_missing_reason(&path, &data))?;
        let mut reader = Reader {
            data: &data,
            pos: manifest_offset,
        };
        let corrupt = |what: &str| format!("internal corruption of phar \"{path}\" ({what})");
        let manifest_len = reader.u32().map_err(|reason| corrupt(&reason))? as usize;
        let entry_count = reader.u32().map_err(|reason| corrupt(&reason))? as usize;
        let _api_version = reader.u16().map_err(|reason| corrupt(&reason))?;
        let flags = reader.u32().map_err(|reason| corrupt(&reason))?;
        let alias_len = reader.u32().map_err(|reason| corrupt(&reason))? as usize;
        let alias = reader.bytes(alias_len).map_err(|reason| corrupt(&reason))?;
        let alias = (!alias.is_empty()).then(|| String::from_utf8_lossy(alias).into_owned());
        let metadata_len = reader.u32().map_err(|reason| corrupt(&reason))? as usize;
        reader
            .bytes(metadata_len)
            .map_err(|reason| corrupt(&reason))?;

        let data_start = manifest_offset
            .checked_add(4)
            .and_then(|start| start.checked_add(manifest_len))
            .filter(|start| *start <= data.len())
            .ok_or_else(|| corrupt("manifest length exceeds the file"))?;
        let mut entries = HashMap::with_capacity(entry_count);
        let mut directories = HashSet::new();
        let mut offset = data_start;
        for _ in 0..entry_count {
            let name_len = reader.u32().map_err(|reason| corrupt(&reason))? as usize;
            let name =
                String::from_utf8_lossy(reader.bytes(name_len).map_err(|reason| corrupt(&reason))?)
                    .into_owned();
            let size = reader.u32().map_err(|reason| corrupt(&reason))? as usize;
            let mtime = i64::from(reader.u32().map_err(|reason| corrupt(&reason))?);
            let stored = reader.u32().map_err(|reason| corrupt(&reason))? as usize;
            let _crc = reader.u32().map_err(|reason| corrupt(&reason))?;
            let entry_flags = reader.u32().map_err(|reason| corrupt(&reason))?;
            let entry_metadata = reader.u32().map_err(|reason| corrupt(&reason))? as usize;
            reader
                .bytes(entry_metadata)
                .map_err(|reason| corrupt(&reason))?;
            if entry_flags & ENTRY_COMPRESSED != 0 {
                return Err(format!(
                    "phar \"{path}\" contains compressed entries, which are not supported"
                ));
            }
            if offset
                .checked_add(stored)
                .is_none_or(|end| end > data.len())
            {
                return Err(corrupt("entry data exceeds the file"));
            }
            let key = name.trim_end_matches('/').to_string();
            let mut parent = key.as_str();
            while let Some((directory, _)) = parent.rsplit_once('/') {
                directories.insert(directory.to_string());
                parent = directory;
            }
            if name.ends_with('/') {
                directories.insert(key);
            } else {
                entries.insert(
                    key,
                    PharEntry {
                        offset,
                        size,
                        mtime,
                    },
                );
            }
            offset += stored;
        }
        if reader.pos != data_start {
            return Err(corrupt("manifest length mismatch"));
        }
        if flags & MANIFEST_SIGNED != 0 {
            verify_signature(&data, &path)?;
        }
        Ok(Self {
            path,
            data,
            entries,
            directories,
            file_mtime,
            alias,
        })
    }

    fn entry_bytes(&self, entry: &PharEntry) -> &[u8] {
        &self.data[entry.offset..entry.offset + entry.size]
    }

    fn is_directory(&self, internal: &str) -> bool {
        internal.is_empty() || self.directories.contains(internal)
    }

    /// Immediate children of `directory`, sorted and without dot entries,
    /// as PHP lists phar directories.
    fn children(&self, directory: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .entries
            .keys()
            .chain(self.directories.iter())
            .filter_map(|key| {
                let remainder = if directory.is_empty() {
                    key.as_str()
                } else {
                    key.strip_prefix(directory)?.strip_prefix('/')?
                };
                (!remainder.is_empty() && !remainder.contains('/')).then(|| remainder.to_string())
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

/// Whether `path` names a member of a phar archive.
pub(crate) fn is_phar_url(path: &str) -> bool {
    path.len() > 7 && path.as_bytes()[..7].eq_ignore_ascii_case(b"phar://")
}

fn normalize_internal(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn load_archive(eg: &mut ExecutorGlobals, file: &str) -> Result<Rc<PharArchive>, String> {
    let canonical = std::fs::canonicalize(file)
        .map_err(|_| format!("unable to open phar for reading \"{file}\""))?
        .to_string_lossy()
        .into_owned();
    if let Some(archive) = eg.phar_runtime.archives.get(&canonical) {
        return Ok(Rc::clone(archive));
    }
    let data = std::fs::read(&canonical)
        .map_err(|error| format!("unable to open phar \"{canonical}\": {error}"))?;
    let file_mtime = std::fs::metadata(&canonical)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let archive = Rc::new(PharArchive::parse(canonical.clone(), data, file_mtime)?);
    if let Some(alias) = &archive.alias {
        eg.phar_runtime
            .aliases
            .entry(alias.clone())
            .or_insert_with(|| canonical.clone());
    }
    eg.phar_runtime
        .archives
        .insert(canonical, Rc::clone(&archive));
    Ok(archive)
}

/// A resolved `phar://` URL.
pub(crate) enum PharTarget {
    /// Not a `phar://` URL at all.
    NotPhar,
    /// The archive part could not be opened; carries PHP's reason.
    Invalid(String),
    Found {
        archive: Rc<PharArchive>,
        /// Normalized member path, empty for the archive root.
        internal: String,
    },
}

/// Resolve `phar://<alias-or-file>/<member>`: an alias registered by the
/// stub or a manifest wins, otherwise the first path prefix that is an
/// existing file is opened as the archive.
pub(crate) fn resolve(eg: &mut ExecutorGlobals, url: &str) -> PharTarget {
    if !is_phar_url(url) {
        return PharTarget::NotPhar;
    }
    let rest = &url[7..];
    // PHP only recognizes an alias when a member path follows it, so a bare
    // `phar://alias` is not a URL of the archive root.
    let alias = rest
        .split_once('/')
        .and_then(|(head, tail)| Some((eg.phar_runtime.aliases.get(head).cloned()?, tail)));
    let (archive, internal_raw) = if let Some((path, tail)) = alias {
        (load_archive(eg, &path), tail)
    } else {
        let mut found = None;
        let mut index = 0;
        loop {
            let end = rest[index..]
                .find('/')
                .map_or(rest.len(), |position| index + position);
            let candidate = &rest[..end];
            if !candidate.is_empty() && std::path::Path::new(candidate).is_file() {
                found = Some((candidate, &rest[end..]));
                break;
            }
            if end == rest.len() {
                break;
            }
            index = end + 1;
        }
        match found {
            Some((file, tail)) => (load_archive(eg, file), tail),
            None => {
                return PharTarget::Invalid(format!("invalid url or non-existent phar \"{url}\""));
            }
        }
    };
    match archive {
        Ok(archive) => PharTarget::Found {
            archive,
            internal: normalize_internal(internal_raw),
        },
        Err(reason) => PharTarget::Invalid(reason),
    }
}

fn canonical_url(archive: &PharArchive, internal: &str) -> String {
    if internal.is_empty() {
        format!("phar://{}", archive.path)
    } else {
        format!("phar://{}/{internal}", archive.path)
    }
}

/// Bytes and canonical URL of a `phar://` file, or PHP's failure reason.
/// `None` when `url` is not a phar URL.
pub(crate) fn read_url(
    eg: &mut ExecutorGlobals,
    url: &str,
) -> Option<Result<(Vec<u8>, String), String>> {
    match resolve(eg, url) {
        PharTarget::NotPhar => None,
        PharTarget::Invalid(reason) => Some(Err(format!("phar error: {reason}"))),
        PharTarget::Found {
            archive, internal, ..
        } => Some(match archive.entries.get(&internal) {
            Some(entry) => Ok((
                archive.entry_bytes(entry).to_vec(),
                canonical_url(&archive, &internal),
            )),
            None => Err(format!(
                "phar error: \"{internal}\" is not a file in phar \"{}\"",
                archive.path
            )),
        }),
    }
}

/// Open a `phar://` file as a read-only memory stream for `fopen()`.
pub(crate) fn open_stream(
    eg: &mut ExecutorGlobals,
    url: &str,
    mode: &str,
) -> Option<std::io::Result<super::stream::PhpStream>> {
    let result = read_url(eg, url)?;
    Some(match result {
        Ok((bytes, canonical)) => {
            if mode.contains(['w', 'a', 'x', 'c', '+']) {
                Err(std::io::Error::other(
                    "phar error: write operations disabled by the php.ini setting phar.readonly",
                ))
            } else {
                Ok(super::stream::PhpStream::readonly_bytes(
                    bytes, &canonical, mode,
                ))
            }
        }
        Err(reason) => Err(std::io::Error::other(reason)),
    })
}

/// Open `path` as a phar member when it is a `phar://` URL, otherwise as a
/// native stream.
pub(crate) fn open_or_native(
    eg: &mut ExecutorGlobals,
    path: &str,
    mode: &str,
) -> std::io::Result<super::stream::PhpStream> {
    match open_stream(eg, path, mode) {
        Some(result) => result,
        None => super::stream::PhpStream::open(path, mode),
    }
}

/// `stat()` fields of a `phar://` URL: `false` for a missing member, `None`
/// when `url` is not a phar URL. Members of a read-only archive report
/// `0444`/`0555` permissions like PHP.
pub(crate) fn stat_url(eg: &mut ExecutorGlobals, url: &str) -> Option<Value> {
    let (archive, internal) = match resolve(eg, url) {
        PharTarget::NotPhar => return None,
        PharTarget::Invalid(_) => return Some(Value::bool(false)),
        PharTarget::Found { archive, internal } => (archive, internal),
    };
    let (mode, size, mtime) = if let Some(entry) = archive.entries.get(&internal) {
        (0o100444, entry.size as i64, entry.mtime)
    } else if archive.is_directory(&internal) {
        (0o040555, 0, archive.file_mtime)
    } else {
        return Some(Value::bool(false));
    };
    Some(super::filesystem::stat_array_value([
        0, 0, mode, 1, 0, 0, 0, size, mtime, mtime, mtime, -1, -1,
    ]))
}

/// Sorted member names directly below a `phar://` directory, or PHP's
/// failure reason; `None` when `url` is not a phar URL.
pub(crate) fn directory_listing(
    eg: &mut ExecutorGlobals,
    url: &str,
) -> Option<Result<Vec<String>, String>> {
    Some(match resolve(eg, url) {
        PharTarget::NotPhar => return None,
        PharTarget::Invalid(reason) => Err(format!(
            "phar error: {reason}\nphar url \"{url}\" is unknown"
        )),
        PharTarget::Found {
            archive, internal, ..
        } => {
            if archive.is_directory(&internal) {
                Ok(archive.children(&internal))
            } else {
                // PHP's directory wrapper logs nothing for a missing or
                // non-directory member, so the caller reports the generic
                // stream failure.
                Err("operation failed".to_string())
            }
        }
    })
}

/// Source bytes and canonical URL for `include 'phar://...'`; `None` when
/// the path is not a phar URL, `Err` with PHP's reason otherwise.
pub(crate) fn include_source(
    eg: &mut ExecutorGlobals,
    path: &str,
) -> Option<Result<(Vec<u8>, String), String>> {
    read_url(eg, path)
}

fn argument(ed: *mut ExecuteData, index: u32) -> Value {
    super::owned_argument(ed, index)
}

fn optional_argument(ed: *mut ExecuteData, index: u32) -> Option<Value> {
    let value = argument(ed, index);
    (!value.is_undef()).then_some(value)
}

fn write(rv: *mut Value, value: Value) {
    super::write_return_value(rv, value);
}

fn throw_phar_exception(eg: &mut ExecutorGlobals, message: &str) {
    eg.exception = Some(make_error_value(PHAR_EXCEPTION, message));
}

/// Source file of the user code that invoked the current internal handler.
fn caller_source_file(ed: *mut ExecuteData) -> Option<String> {
    let (file, _) = super::internal_call_source(ed);
    (!file.is_empty()).then_some(file)
}

/// Archive path for a `phar://` source file: the registered archive whose
/// path prefixes the URL, else the first file prefix that opens.
fn archive_path_of_source(eg: &mut ExecutorGlobals, file: &str) -> Option<String> {
    let rest = file.get(7..)?;
    let registered = eg.phar_runtime.archives.keys().find(|path| {
        rest.strip_prefix(path.as_str())
            .is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
    });
    if let Some(path) = registered {
        return Some(path.clone());
    }
    match resolve(eg, file) {
        PharTarget::Found { archive, .. } => Some(archive.path.clone()),
        _ => None,
    }
}

fn phar_map_phar(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let alias = optional_argument(ed, 1)
        .filter(|value| value.value_type() != ValueType::Null)
        .map(|value| value.echo_to_string());
    let Some(file) = caller_source_file(ed) else {
        throw_phar_exception(eg, "Phar::mapPhar() must be called from within a phar stub");
        return Ok(());
    };
    let archive = if is_phar_url(&file) {
        match archive_path_of_source(eg, &file) {
            Some(path) => load_archive(eg, &path),
            None => Err(format!("invalid url or non-existent phar \"{file}\"")),
        }
    } else {
        load_archive(eg, &file)
    };
    let archive = match archive {
        Ok(archive) => archive,
        Err(reason) if reason.ends_with(HALT_MISSING) || reason.ends_with(HALT_TRUNCATED) => {
            throw_phar_exception(eg, "__HALT_COMPILER(); must be declared in a phar");
            return Ok(());
        }
        Err(reason) => {
            throw_phar_exception(eg, &reason);
            return Ok(());
        }
    };
    if let Some(alias) = alias.or_else(|| archive.alias.clone()) {
        if eg
            .phar_runtime
            .aliases
            .get(&alias)
            .is_some_and(|existing| *existing != archive.path)
        {
            throw_phar_exception(
                eg,
                &format!("alias \"{alias}\" is already used for a different archive"),
            );
            return Ok(());
        }
        eg.phar_runtime.aliases.insert(alias, archive.path.clone());
    }
    write(rv, Value::bool(true));
    Ok(())
}

fn phar_running(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let return_phar = optional_argument(ed, 1).is_none_or(|value| value.is_truthy());
    let path = caller_source_file(ed)
        .filter(|file| is_phar_url(file))
        .and_then(|file| archive_path_of_source(eg, &file));
    write(
        rv,
        Value::string(match path {
            Some(path) if return_phar => format!("phar://{path}"),
            Some(path) => path,
            None => String::new(),
        }),
    );
    Ok(())
}

fn phar_load_phar(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let file = argument(ed, 1).echo_to_string();
    let alias = optional_argument(ed, 2)
        .filter(|value| value.value_type() != ValueType::Null)
        .map(|value| value.echo_to_string());
    match load_archive(eg, &file) {
        Ok(archive) => {
            if let Some(alias) = alias.or_else(|| archive.alias.clone()) {
                eg.phar_runtime.aliases.insert(alias, archive.path.clone());
            }
            write(rv, Value::bool(true));
        }
        Err(reason) => throw_phar_exception(eg, &reason),
    }
    Ok(())
}

/// PHP's `Phar::isValidPharFilename()`: the basename needs an extension
/// after a non-empty stem, executable names carry `.phar` inside that
/// extension while data archives must not, and the path must either be an
/// existing regular file or a new name inside an existing directory.
fn is_valid_phar_filename(filename: &str, executable: bool) -> bool {
    let trimmed = filename.trim_end_matches('/');
    let (directory, basename) = match trimmed.rsplit_once('/') {
        Some((directory, basename)) => {
            (if directory.is_empty() { "/" } else { directory }, basename)
        }
        None => (".", trimmed),
    };
    let Some(dot) = basename[1.min(basename.len())..].find('.') else {
        return false;
    };
    let has_phar = basename[dot + 1..].contains(".phar");
    if has_phar != executable {
        return false;
    }
    match std::fs::metadata(trimmed) {
        Ok(metadata) => metadata.is_file(),
        Err(_) => std::path::Path::new(directory).is_dir(),
    }
}

fn phar_is_valid_phar_filename(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let filename = argument(ed, 1).echo_to_string();
    let executable = optional_argument(ed, 2).is_none_or(|value| value.is_truthy());
    write(
        rv,
        Value::bool(is_valid_phar_filename(&filename, executable)),
    );
    Ok(())
}

fn phar_false(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write(rv, Value::bool(false));
    Ok(())
}

fn phar_null(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write(rv, Value::null());
    Ok(())
}

fn phar_supported_signatures(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let mut names = PhpArray::with_packed_capacity(4);
    for name in ["MD5", "SHA-1", "SHA-256", "SHA-512"] {
        names.push(Value::string(name));
    }
    write(rv, Value::array(names));
    Ok(())
}

fn phar_supported_compression(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write(rv, Value::array(PhpArray::new()));
    Ok(())
}

struct MethodDeclaration {
    name: &'static str,
    handler: crate::vm::function::InternalFunctionHandler,
    parameters: &'static [&'static str],
    required: u32,
    hints: fn() -> Vec<ParamTypeHint>,
    return_hint: fn() -> ParamTypeHint,
    defaults: fn() -> Vec<Option<Value>>,
    default_spellings: &'static [Option<&'static str>],
}

fn empty_class(
    name: &str,
    parent: Option<&str>,
    constants: Vec<ClassConstantDefinition>,
) -> ClassDef {
    ClassDef {
        attributes: Vec::new(),
        name: name.to_string(),
        source_file: None,
        declaration_line: 0,
        end_line: 0,
        doc_comment: None,
        parent: parent.map(str::to_string),
        implements: Vec::new(),
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: Vec::new(),
        trait_aliases: Vec::new(),
        trait_precedences: Vec::new(),
        properties: Vec::new(),
        static_properties: Vec::new(),
        constants,
        property_layout: std::rc::Rc::new(ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: Vec::new(),
        methods: Vec::new(),
        abstract_methods: Vec::new(),
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    }
}

/// Register `PharException` and the static `Phar` surface.
pub(super) fn register_classes(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    eg.register_class(empty_class(PHAR_EXCEPTION, Some("Exception"), Vec::new()))
        .expect("PharException registers once per request");
    let constants = [
        ("BZ2", 8192),
        ("GZ", 4096),
        ("NONE", 0),
        ("PHAR", 1),
        ("TAR", 2),
        ("ZIP", 3),
        ("COMPRESSED", 61440),
        ("PHP", 0),
        ("PHPS", 1),
        ("MD5", 1),
        ("OPENSSL", 16),
        ("OPENSSL_SHA256", 17),
        ("OPENSSL_SHA512", 18),
        ("SHA1", 2),
        ("SHA256", 3),
        ("SHA512", 4),
    ]
    .into_iter()
    .map(|(constant, value)| ClassConstantDefinition {
        attributes: Vec::new(),
        name: constant.to_string(),
        value: Value::long(value),
        source_file: String::new(),
        evaluation_error: None,
        source_expression: None,
        callable_factory: None,
        evaluation_scope: None,
        value_is_deferred: false,
        visibility: Visibility::Public,
        declaring_class: PHAR.to_string(),
        type_hint: ParamTypeHint::Int,
        is_final: false,
    })
    .collect();
    eg.register_class(empty_class(PHAR, None, constants))
        .expect("Phar registers once per request");

    let declarations = [
        MethodDeclaration {
            name: "mapPhar",
            handler: phar_map_phar,
            parameters: &["alias", "offset"],
            required: 0,
            hints: || vec![nullable_string_hint(), ParamTypeHint::Int],
            return_hint: || ParamTypeHint::Bool,
            defaults: || vec![Some(Value::null()), Some(Value::long(0))],
            default_spellings: &[Some("null"), Some("0")],
        },
        MethodDeclaration {
            name: "running",
            handler: phar_running,
            parameters: &["returnPhar"],
            required: 0,
            hints: || vec![ParamTypeHint::Bool],
            return_hint: || ParamTypeHint::String,
            defaults: || vec![Some(Value::bool(true))],
            default_spellings: &[Some("true")],
        },
        MethodDeclaration {
            name: "loadPhar",
            handler: phar_load_phar,
            parameters: &["filename", "alias"],
            required: 1,
            hints: || vec![ParamTypeHint::String, nullable_string_hint()],
            return_hint: || ParamTypeHint::Bool,
            defaults: || vec![None, Some(Value::null())],
            default_spellings: &[None, Some("null")],
        },
        MethodDeclaration {
            name: "isValidPharFilename",
            handler: phar_is_valid_phar_filename,
            parameters: &["filename", "executable"],
            required: 1,
            hints: || vec![ParamTypeHint::String, ParamTypeHint::Bool],
            return_hint: || ParamTypeHint::Bool,
            defaults: || vec![None, Some(Value::bool(true))],
            default_spellings: &[None, Some("true")],
        },
        MethodDeclaration {
            name: "canWrite",
            handler: phar_false,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            return_hint: || ParamTypeHint::Bool,
            defaults: Vec::new,
            default_spellings: &[],
        },
        MethodDeclaration {
            name: "canCompress",
            handler: phar_false,
            parameters: &["compression"],
            required: 0,
            hints: || vec![ParamTypeHint::Int],
            return_hint: || ParamTypeHint::Bool,
            defaults: || vec![Some(Value::long(0))],
            default_spellings: &[Some("0")],
        },
        MethodDeclaration {
            name: "getSupportedSignatures",
            handler: phar_supported_signatures,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            return_hint: || ParamTypeHint::Array,
            defaults: Vec::new,
            default_spellings: &[],
        },
        MethodDeclaration {
            name: "getSupportedCompression",
            handler: phar_supported_compression,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            return_hint: || ParamTypeHint::Array,
            defaults: Vec::new,
            default_spellings: &[],
        },
        MethodDeclaration {
            name: "interceptFileFuncs",
            handler: phar_null,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            return_hint: || ParamTypeHint::ClassName("void".into()),
            defaults: Vec::new,
            default_spellings: &[],
        },
        MethodDeclaration {
            name: "mungServer",
            handler: phar_null,
            parameters: &["variables"],
            required: 1,
            hints: || vec![ParamTypeHint::Array],
            return_hint: || ParamTypeHint::ClassName("void".into()),
            defaults: || vec![None],
            default_spellings: &[None],
        },
    ];

    let mut functions = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let hints = (declaration.hints)();
        eg.register_internal_method_contract(
            PHAR,
            declaration.name,
            true,
            declaration.required,
            declaration.parameters,
            hints.clone(),
            (declaration.return_hint)(),
            declaration.default_spellings,
            false,
        );
        let mut function = Box::new(
            make_internal_method(
                declaration.handler,
                declaration.parameters.len() as u32 + 1,
                declaration.required,
                Vec::new(),
            )
            .with_static_parameter_names(declaration.parameters),
        );
        function.handler_validates_types = true;
        function.common.sig.param_type_hints = hints;
        function.common.sig.return_type_hint = (declaration.return_hint)();
        let pointer = &function.common as *const FunctionCommon;
        eg.function_table.insert(
            super::builtin_classes::internal_method_lookup_name(PHAR, declaration.name),
            pointer,
        );
        eg.method_declaring_class.insert(pointer, PHAR.into());
        eg.register_internal_static_method(pointer);
        eg.register_internal_function_display_name(
            pointer,
            super::builtin_classes::internal_method_display_name(PHAR, declaration.name),
        );
        eg.register_internal_function_reflection_metadata(
            pointer,
            (declaration.defaults)(),
            "Phar",
        );
        functions.push(function);
    }
    functions
}

fn nullable_string_hint() -> ParamTypeHint {
    ParamTypeHint::Nullable(Box::new(ParamTypeHint::String))
}
