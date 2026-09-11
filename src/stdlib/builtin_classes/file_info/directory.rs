//! Native directory cursors share the FileInfo projection boundary. Cursor
//! storage contains only native bytes/handles, never hidden PHP ownership.
use super::*;
use crate::stdlib::directory::OwnedDirectoryCursor;
use std::cell::RefCell;

const SKIP_DOTS: u32 = 0x1000;
const FLAGS_MASK: u32 = 0x7ff0;

#[derive(Clone, Default)]
pub(super) struct DirectoryState {
    pub(super) base: Option<Vec<u8>>,
    pub(super) filename: Vec<u8>,
    stream: Option<Rc<RefCell<OwnedDirectoryCursor>>>,
    index: i64,
    flags: u32,
    pub(super) stat_path_materialized: bool,
}

impl DirectoryState {
    pub(super) fn is_open(&self) -> bool {
        self.stream.is_some()
    }

    pub(super) fn is_projected(&self) -> bool {
        self.base.is_some()
    }

    pub(super) fn base(&self) -> &[u8] {
        self.base.as_deref().unwrap_or_default()
    }

    fn pending(mut base: Vec<u8>, flags: u32) -> Self {
        // Directory construction removes one trailing separator, not a run.
        if base.len() > 1 && base.last() == Some(&b'/') {
            base.pop();
        }
        Self {
            base: Some(base),
            flags,
            ..Self::default()
        }
    }

    fn open(base: Vec<u8>, flags: u32) -> std::io::Result<Self> {
        let mut state = Self::pending(base, flags);
        let native = local_path(state.base()).unwrap_or(state.base());
        let stream = OwnedDirectoryCursor::open(&filesystem::file_info_native_path(native))?;
        state.stream = Some(Rc::new(RefCell::new(stream)));
        state.read();
        Ok(state)
    }

    fn is_dot(&self) -> bool {
        self.filename == b"." || self.filename == b".."
    }

    fn read(&mut self) {
        self.stat_path_materialized = false;
        loop {
            self.filename = self
                .stream
                .as_ref()
                .expect("open directory cursor")
                .borrow_mut()
                .next_entry()
                .ok()
                .flatten()
                .unwrap_or_default();
            if self.flags & SKIP_DOTS == 0 || !self.is_dot() {
                break;
            }
        }
    }

    pub(super) fn pathname(&self) -> Vec<u8> {
        let mut path = Vec::with_capacity(self.base().len() + self.filename.len() + 1);
        path.extend_from_slice(self.base());
        path.push(b'/');
        path.extend_from_slice(&self.filename);
        path
    }

    fn rewind(&mut self) {
        self.stream
            .as_ref()
            .expect("open directory cursor")
            .borrow_mut()
            .rewind();
        self.index = 0;
        self.read();
    }

    fn next(&mut self) {
        self.index = self.index.wrapping_add(1);
        self.read();
    }

    fn seek(&mut self, index: i64) -> bool {
        if index < self.index {
            self.rewind();
        }
        while self.index < index {
            if self.filename.is_empty() {
                return false;
            }
            self.next();
        }
        true
    }
}

fn local_path(path: &[u8]) -> Option<&[u8]> {
    if path.len() < 7 || !path[..7].eq_ignore_ascii_case(b"file://") {
        return Some(path);
    }
    let local = &path[7..];
    if local.starts_with(b"/") {
        return Some(local);
    }
    if local.len() >= 10 && local[..10].eq_ignore_ascii_case(b"localhost/") {
        return Some(&local[9..]);
    }
    None
}

fn update_path(state: &mut NativeFileInfo) {
    let directory = state.directory.as_ref().expect("initialized cursor");
    state.path = Some(if directory.filename.is_empty() {
        Vec::new()
    } else {
        directory.pathname()
    });
}

fn publish_pending(receiver: &Value, path: Vec<u8>, flags: u32) {
    let mut object = receiver.as_object_mut().unwrap();
    let state = object.native_file_info_mut();
    state.directory = Some(Box::new(DirectoryState::pending(path, flags)));
    state.path = Some(Vec::new());
}

fn initialized(receiver: &Value, eg: &mut ExecutorGlobals) -> bool {
    if receiver
        .as_object()
        .expect("directory receiver")
        .native_file_info()
        .and_then(|state| state.directory.as_deref())
        .is_some_and(DirectoryState::is_open)
    {
        return true;
    }
    error(eg, "Error", "Object not initialized");
    false
}

fn read<T>(receiver: &Value, project: impl FnOnce(&DirectoryState) -> T) -> T {
    let object = receiver.as_object().expect("directory receiver");
    project(
        object
            .native_file_info()
            .expect("initialized")
            .directory
            .as_ref()
            .expect("directory"),
    )
}

fn change<T>(receiver: &Value, update: impl FnOnce(&mut DirectoryState) -> T) -> T {
    let mut object = receiver.as_object_mut().expect("directory receiver");
    let state = object.native_file_info_mut();
    let result = update(state.directory.as_mut().expect("initialized"));
    update_path(state);
    result
}

fn int_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
    index: u32,
    name: &str,
) -> Result<Option<i64>, VmError> {
    let value = owned_argument(ed, index + 1);
    typed_internal_int_value_argument_expected(ed, eg, &value, method, index, name, "int")
}

#[cold]
fn constructor(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    filesystem_iterator: bool,
) -> Result<(), VmError> {
    let owner = if filesystem_iterator {
        "FilesystemIterator"
    } else {
        "DirectoryIterator"
    };
    let method = format!("{owner}::__construct");
    let Some(path) = string_argument(ed, eg, &method, "directory")? else {
        return Ok(());
    };
    let flags = if filesystem_iterator {
        if arg_opt!(ed, 2).is_some() {
            let Some(flags) = int_argument(ed, eg, &method, 1, "flags")? else {
                return Ok(());
            };
            flags as u32 & FLAGS_MASK
        } else {
            SKIP_DOTS
        }
    } else {
        0
    };
    let receiver = owned_argument(ed, 0);
    if receiver
        .as_object()
        .unwrap()
        .native_file_info()
        .is_some_and(|state| state.path.is_some())
    {
        error(eg, "Error", "Directory object is already initialized");
        return Ok(());
    }
    if path.is_empty() || path.contains(&0) {
        let reason = if path.is_empty() {
            "must not be empty"
        } else {
            "must not contain any null bytes"
        };
        error(
            eg,
            "ValueError",
            &format!("{method}(): Argument #1 ($directory) {reason}"),
        );
        return Ok(());
    }
    let Some(local) = local_path(&path) else {
        publish_pending(&receiver, path.clone(), flags);
        let mut message =
            format!("{method}(): Remote host file access not supported, ").into_bytes();
        message.extend_from_slice(&path);
        byte_exception(eg, "UnexpectedValueException", message);
        return Ok(());
    };
    if let Some(colon) = local.windows(3).position(|window| window == b"://") {
        publish_pending(&receiver, path.clone(), flags);
        error(
            eg,
            "UnexpectedValueException",
            &format!(
                "{method}(): Unable to find the wrapper \"{}\" - did you forget to enable it when you configured PHP?",
                bytes_to_php_string(&local[..colon])
            ),
        );
        return Ok(());
    }
    match DirectoryState::open(path.clone(), flags) {
        Ok(directory) => {
            let mut object = receiver.as_object_mut().unwrap();
            let state = object.native_file_info_mut();
            state.directory = Some(Box::new(directory));
            update_path(state);
        }
        Err(failure) => {
            publish_pending(&receiver, path.clone(), flags);
            let mut message = method.into_bytes();
            message.push(b'(');
            message.extend_from_slice(&path);
            message.extend_from_slice(b"): Failed to open directory: ");
            message.extend_from_slice(filesystem::filesystem_error_reason(&failure).as_bytes());
            byte_exception(eg, "UnexpectedValueException", message);
        }
    }
    ret!(rv, Value::null());
}

fn byte_exception(eg: &mut ExecutorGlobals, kind: &str, message: Vec<u8>) {
    let exception = make_error_value(kind, "");
    exception
        .as_object_mut()
        .expect("exception")
        .set_property("message", php_byte_result(message, false));
    eg.exception = Some(exception);
}

#[cold]
fn construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    constructor(ed, rv, eg, false)
}
#[cold]
fn filesystem_construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    constructor(ed, rv, eg, true)
}

#[cold]
fn valid(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    ret!(
        rv,
        Value::bool(read(&receiver, |state| !state.filename.is_empty()))
    );
}
#[cold]
fn key(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    ret!(rv, Value::long(read(&receiver, |state| state.index)));
}
#[cold]
fn current(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    ret!(rv, receiver);
}
#[cold]
fn next(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    change(&receiver, DirectoryState::next);
    ret!(rv, Value::null());
}
#[cold]
fn rewind(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    change(&receiver, DirectoryState::rewind);
    ret!(rv, Value::null());
}
#[cold]
fn seek(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(index) = int_argument(ed, eg, "DirectoryIterator::seek", 0, "offset")? else {
        return Ok(());
    };
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    if !change(&receiver, |state| state.seek(index)) {
        error(
            eg,
            "OutOfBoundsException",
            &format!("Seek position {index} is out of range"),
        );
    }
    ret!(rv, Value::null());
}
#[cold]
fn is_dot(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    ret!(rv, Value::bool(read(&receiver, DirectoryState::is_dot)));
}
#[cold]
fn filename(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    ret!(
        rv,
        php_byte_result(read(&receiver, |state| state.filename.clone()), false)
    );
}
#[cold]
fn extension(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let extension = read(&receiver, |state| {
        state
            .filename
            .iter()
            .rposition(|byte| *byte == b'.')
            .map_or_else(Vec::new, |dot| state.filename[dot + 1..].to_vec())
    });
    ret!(rv, php_byte_result(extension, false));
}
#[cold]
fn basename(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let suffix = if arg_opt!(ed, 1).is_some() {
        let Some(suffix) = string_argument(ed, eg, "DirectoryIterator::getBasename", "suffix")?
        else {
            return Ok(());
        };
        suffix
    } else {
        Vec::new()
    };
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    ret!(
        rv,
        php_byte_result(
            read(&receiver, |state| crate::path_decomposition::basename(
                &state.filename,
                &suffix
            )),
            false
        )
    );
}
#[cold]
fn filesystem_key(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if flags(&receiver) & 0xf00 == 0x100 {
        let filename = receiver
            .as_object()
            .unwrap()
            .native_file_info()
            .and_then(|state| state.directory.as_ref())
            .map_or_else(Vec::new, |state| state.filename.clone());
        ret!(rv, php_byte_result(filename, false));
    }
    if !receiver
        .as_object()
        .unwrap()
        .native_file_info()
        .and_then(|state| state.directory.as_deref())
        .is_some_and(DirectoryState::is_projected)
    {
        error(eg, "Error", "Object not initialized");
        return Ok(());
    }
    ret!(
        rv,
        php_byte_result(read(&receiver, DirectoryState::pathname), false)
    );
}
#[cold]
fn filesystem_current(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    let mode = flags(&receiver) & 0xf0;
    if mode != 0 && mode != 0x20 {
        ret!(rv, receiver);
    }
    match mode {
        0 => factory_projection(ed, rv, eg, false, false),
        0x20 => {
            if !receiver
                .as_object()
                .unwrap()
                .native_file_info()
                .and_then(|state| state.directory.as_deref())
                .is_some_and(DirectoryState::is_projected)
            {
                error(eg, "Error", "Object not initialized");
                return Ok(());
            }
            ret!(
                rv,
                php_byte_result(read(&receiver, DirectoryState::pathname), false)
            );
        }
        _ => {
            ret!(rv, receiver);
        }
    }
}

#[cold]
fn filesystem_rewind(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if receiver
        .as_object()
        .unwrap()
        .native_file_info()
        .and_then(|state| state.directory.as_deref())
        .is_some_and(DirectoryState::is_open)
    {
        change(&receiver, DirectoryState::rewind);
    }
    ret!(rv, Value::null());
}

fn flags(receiver: &Value) -> u32 {
    receiver
        .as_object()
        .unwrap()
        .native_file_info()
        .and_then(|state| state.directory.as_ref())
        .map_or(0, |state| state.flags)
}

#[cold]
fn get_flags(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    ret!(rv, Value::long(flags(&receiver) as i64));
}
#[cold]
fn set_flags(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(flags) = int_argument(ed, eg, "FilesystemIterator::setFlags", 0, "flags")? else {
        return Ok(());
    };
    let receiver = owned_argument(ed, 0);
    // Changing projection/filter policy does not invalidate the cached pathname.
    receiver
        .as_object_mut()
        .unwrap()
        .native_file_info_mut()
        .directory
        .get_or_insert_with(Box::default)
        .flags = flags as u32 & FLAGS_MASK;
    ret!(rv, Value::null());
}

/// Only the canonical clone path invokes this; copying native auxiliary state
/// elsewhere remains non-observable and does not reopen a descriptor. No borrow
/// survives warning callbacks, and their exception takes priority over open failure.
#[cold]
#[inline(never)]
pub(crate) fn prepare_clone(
    receiver: &Value,
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    op_array: &crate::compiler::OpArray,
) -> Result<(), VmError> {
    let request = {
        let object = receiver.as_object().expect("clone receiver");
        object
            .native_file_info()
            .and_then(|state| state.directory.as_ref())
            .filter(|state| state.is_projected())
            .map(|state| (state.base().to_vec(), state.flags, state.index))
    };
    let Some((base, flags, index)) = request else {
        return Ok(());
    };
    // The stored base has already lost one separator; clone construction uses
    // that same public path as a new native open, independently of the source.
    if base.is_empty() {
        error(
            eg,
            "UnexpectedValueException",
            "Failed to open directory \"\"",
        );
        return Ok(());
    }
    match DirectoryState::open(base.clone(), flags) {
        Ok(mut directory) => {
            directory.seek(index);
            let mut object = receiver.as_object_mut().expect("clone receiver");
            let state = object.native_file_info_mut();
            state.directory = Some(Box::new(directory));
            update_path(state);
        }
        Err(failure) => {
            let function = if op_array.is_main_script() {
                "main".to_string()
            } else {
                crate::vm::execute::displayed_frame_function_name(eg, ed)
            };
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!(
                    "{}({}): Failed to open directory: {}",
                    function,
                    bytes_to_php_string(&base),
                    filesystem::filesystem_error_reason(&failure)
                ),
            )?;
            if eg.exception.is_none() {
                let mut message = b"Failed to open directory \"".to_vec();
                message.extend_from_slice(&base);
                message.push(b'"');
                byte_exception(eg, "UnexpectedValueException", message);
            }
        }
    }
    Ok(())
}

#[cold]
#[inline(never)]
fn method(
    eg: &mut ExecutorGlobals,
    functions: &mut Vec<Box<InternalFunction>>,
    owner: &'static str,
    name: &'static str,
    handler: InternalFunctionHandler,
    names: &[&str],
    hints: Vec<ParamTypeHint>,
    defaults: &[Option<&str>],
    result: ParamTypeHint,
) {
    let required = defaults.iter().filter(|value| value.is_none()).count() as u32;
    eg.register_internal_method_contract(
        owner,
        name,
        false,
        required,
        names,
        hints.clone(),
        result.clone(),
        defaults,
        name != "__construct" && name != "__toString",
    );
    let mut function = Box::new(make_internal_method(
        handler,
        names.len() as u32 + 1,
        required,
        names.iter().map(|name| name.to_string()).collect(),
    ));
    function.handler_validates_types = true;
    function.common.sig.param_type_hints = hints;
    if name == "__toString" {
        function.common.sig.return_type_hint = result;
    }
    let pointer = &function.common as *const FunctionCommon;
    eg.function_table
        .insert(internal_method_lookup_name(owner, name), pointer);
    eg.method_declaring_class.insert(pointer, owner.into());
    eg.register_internal_function_display_name(pointer, format!("{owner}::{name}"));
    eg.register_internal_function_reflection_metadata(
        pointer,
        defaults
            .iter()
            .map(|value| {
                value.map(|value| {
                    value
                        .parse::<i64>()
                        .map_or_else(|_| Value::string(value.trim_matches('\'')), Value::long)
                })
            })
            .collect(),
        "SPL",
    );
    functions.push(function);
}

#[cold]
#[inline(never)]
pub(crate) fn register(
    eg: &mut ExecutorGlobals,
    filesystem_iterator: bool,
) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Bool, ClassName, Int, Mixed, None, String, Union, Void};
    let (owner, count) = if filesystem_iterator {
        ("FilesystemIterator", 6)
    } else {
        ("DirectoryIterator", 12)
    };
    let mut functions = Vec::with_capacity(count);
    eg.reserve_internal_method_contracts(owner, count);
    if !filesystem_iterator {
        let owner = "DirectoryIterator";
        method(
            eg,
            &mut functions,
            owner,
            "__construct",
            construct,
            &["directory"],
            vec![String],
            &[Option::None],
            None,
        );
        for (name, handler, result) in [
            ("getFilename", filename as InternalFunctionHandler, String),
            ("getExtension", extension, String),
            ("isDot", is_dot, Bool),
            ("rewind", rewind, Void),
            ("valid", valid, Bool),
            ("key", key, Mixed),
            ("current", current, Mixed),
            ("next", next, Void),
            ("__toString", filename, String),
        ] {
            method(
                eg,
                &mut functions,
                owner,
                name,
                handler,
                &[],
                vec![],
                &[],
                result,
            );
        }
        method(
            eg,
            &mut functions,
            owner,
            "getBasename",
            basename,
            &["suffix"],
            vec![String],
            &[Some("''")],
            String,
        );
        method(
            eg,
            &mut functions,
            owner,
            "seek",
            seek,
            &["offset"],
            vec![Int],
            &[Option::None],
            Void,
        );
        return functions;
    }
    let owner = "FilesystemIterator";
    method(
        eg,
        &mut functions,
        owner,
        "__construct",
        filesystem_construct,
        &["directory", "flags"],
        vec![String, Int],
        &[Option::None, Some("4096")],
        None,
    );
    for (name, handler, result) in [
        ("rewind", filesystem_rewind as InternalFunctionHandler, Void),
        ("key", filesystem_key, String),
        (
            "current",
            filesystem_current,
            Union(vec![
                ClassName("SplFileInfo".into()),
                ClassName("FilesystemIterator".into()),
                String,
            ]),
        ),
        ("getFlags", get_flags, Int),
    ] {
        method(
            eg,
            &mut functions,
            owner,
            name,
            handler,
            &[],
            vec![],
            &[],
            result,
        );
    }
    method(
        eg,
        &mut functions,
        owner,
        "setFlags",
        set_flags,
        &["flags"],
        vec![Int],
        &[Option::None],
        Void,
    );
    functions
}
