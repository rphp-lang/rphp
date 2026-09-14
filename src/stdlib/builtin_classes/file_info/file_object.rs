//! A lazy physical line reader and an independent logical iterator cursor.
//! Native I/O never calls PHP; wrapper I/O snapshots the resource before a
//! callback, so no borrow of the file object survives user code.
use super::*;
use crate::stdlib::stream::PhpStream;
use std::cell::RefCell;

mod csv_records;
mod mutation;
mod read_position;

const DROP_NEW_LINE: u32 = 1;
const READ_AHEAD: u32 = 2;
const SKIP_EMPTY: u32 = 4;
const READ_CSV: u32 = 8;

#[derive(Clone)]
enum Backend {
    Native(Rc<RefCell<PhpStream>>),
    #[cfg(feature = "stream-registry")]
    Wrapper(Value),
}

#[derive(Clone)]
pub(super) struct FileState {
    backend: Backend,
    cache: Option<Value>,
    index: i64,
    maximum: usize,
    flags: u32,
    eof: bool,
    override_line: bool,
    pub(super) temporary: bool,
    csv: csv_records::Controls,
    csv_read_failed: bool,
    csv_line: Option<Vec<u8>>,
}

impl FileState {
    #[cold]
    pub(super) fn append_debug_properties(
        &self,
        eg: &mut ExecutorGlobals,
        properties: &mut PhpArray,
    ) {
        let mode = if self.temporary {
            Value::string("wb")
        } else {
            match &self.backend {
                Backend::Native(stream) => Value::string(stream.borrow().metadata().mode),
                #[cfg(feature = "stream-registry")]
                Backend::Wrapper(value) => crate::stdlib::streams::user_wrapper::file_object_mode(
                    eg,
                    value.as_resource_id().unwrap(),
                ),
            }
        };
        #[cfg(not(feature = "stream-registry"))]
        let _ = eg;
        properties.set_str("\0SplFileObject\0openMode", mode);
        properties.set_str(
            "\0SplFileObject\0delimiter",
            php_byte_result(vec![self.csv.separator], false),
        );
        properties.set_str(
            "\0SplFileObject\0enclosure",
            php_byte_result(vec![self.csv.enclosure], false),
        );
    }

    pub(super) fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        if let Some(value) = &self.cache {
            visit(value);
        }
        #[cfg(feature = "stream-registry")]
        if let Backend::Wrapper(value) = &self.backend {
            visit(value);
        }
    }

    pub(super) fn into_values(self, pending: &mut Vec<Value>) {
        #[cfg(feature = "stream-registry")]
        if let Backend::Wrapper(value) = self.backend {
            pending.push(value);
        }
        if let Some(value) = self.cache {
            pending.push(value);
        }
    }
}

fn initialized(receiver: &Value, eg: &mut ExecutorGlobals) -> bool {
    let ready = receiver
        .as_object()
        .unwrap()
        .native_file_info()
        .is_some_and(|state| state.file.is_some());
    if !ready {
        error(
            eg,
            "Error",
            "The parent constructor was not called: the object is in an invalid state",
        );
    }
    ready
}

fn read<T>(receiver: &Value, project: impl FnOnce(&FileState) -> T) -> T {
    let object = receiver.as_object().unwrap();
    project(object.native_file_info().unwrap().file.as_ref().unwrap())
}

fn write<T>(receiver: &Value, change: impl FnOnce(&mut FileState) -> T) -> T {
    let mut object = receiver.as_object_mut().unwrap();
    change(object.native_file_info_mut().file.as_mut().unwrap())
}

#[cold]
fn construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let method = "SplFileObject::__construct";
    let Some(path) = string_argument(ed, eg, method, "filename")? else {
        return Ok(());
    };
    let mode = if arg_opt!(ed, 2).is_some() {
        let argument = owned_argument(ed, 2);
        let Some(value) = typed_internal_string_value_expected(
            ed, eg, &argument, method, 1, "mode", "string", "string",
        )?
        else {
            return Ok(());
        };
        value.echo_to_string()
    } else {
        "r".into()
    };
    let use_include_path = if arg_opt!(ed, 3).is_some() {
        let argument = owned_argument(ed, 3);
        let Some(value) =
            typed_internal_bool_value_argument(ed, eg, &argument, method, 2, "useIncludePath")?
        else {
            return Ok(());
        };
        value
    } else {
        false
    };
    if let Some(context) = arg_opt!(ed, 4)
        && context.value_type() != ValueType::Null
        && context.as_resource_id().is_none()
    {
        typed_internal_argument_error(eg, method, context, 4, "context", "resource or null");
        return Ok(());
    }
    let receiver = owned_argument(ed, 0);
    if receiver
        .as_object()
        .unwrap()
        .native_file_info()
        .is_some_and(|state| state.file.is_some())
    {
        error(eg, "Error", "Cannot call constructor twice");
        return Ok(());
    }
    if path.is_empty() {
        error(eg, "ValueError", "Path must not be empty");
        return Ok(());
    }
    if path.contains(&0) {
        error(
            eg,
            "ValueError",
            "SplFileObject::__construct(): Argument #1 ($filename) must not contain any null bytes",
        );
        return Ok(());
    }
    let requested = bytes_to_php_string(&path);
    #[cfg(feature = "include-path")]
    let resolved =
        crate::stdlib::include_path::resolve_for_open_from(eg, &requested, use_include_path, ed);
    #[cfg(not(feature = "include-path"))]
    let resolved = {
        let _ = use_include_path;
        requested.clone()
    };
    #[cfg(feature = "stream-registry")]
    let wrapper = crate::stdlib::streams::user_wrapper::open_file_object(eg, &resolved, &mode)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    #[cfg(feature = "stream-registry")]
    let backend = if let Some(value) = wrapper {
        Some(Backend::Wrapper(value))
    } else {
        open_native(eg, &resolved, &path, &mode)
    };
    #[cfg(not(feature = "stream-registry"))]
    let backend = open_native(eg, &resolved, &path, &mode);
    let Some(backend) = backend else {
        return Ok(());
    };
    initialize(&receiver, eg, path, backend, false);
    ret!(rv, Value::null());
}

#[cold]
fn initialize(
    receiver: &Value,
    eg: &ExecutorGlobals,
    path: Vec<u8>,
    backend: Backend,
    temporary: bool,
) {
    let override_line = resolve_object_public_method(eg, receiver, "getCurrentLine")
        .is_some_and(|method| method.common().fn_type == FunctionType::User);
    let mut object = receiver.as_object_mut().unwrap();
    let state = object.native_file_info_mut();
    state.path = Some(path);
    state.file = Some(Box::new(FileState {
        backend,
        cache: None,
        index: 0,
        maximum: 0,
        flags: 0,
        eof: false,
        override_line,
        temporary,
        csv: csv_records::Controls::default(),
        csv_read_failed: false,
        csv_line: None,
    }));
}

#[cold]
fn open_native(
    eg: &mut ExecutorGlobals,
    resolved: &str,
    path: &[u8],
    mode: &str,
) -> Option<Backend> {
    match PhpStream::open(resolved, mode) {
        Ok(stream) => {
            if stream.is_plain_file() {
                filesystem::clear_filesystem_stat_cache(eg);
            }
            Some(Backend::Native(Rc::new(RefCell::new(stream))))
        }
        Err(failure) => {
            let reason = if !matches!(
                mode.as_bytes().first(),
                Some(b'r' | b'w' | b'a' | b'x' | b'c')
            ) {
                format!("`{mode}' is not a valid mode for fopen")
            } else {
                filesystem::filesystem_error_reason(&failure).into()
            };
            path_error(
                eg,
                "SplFileObject::__construct(",
                path,
                &format!("): Failed to open stream: {reason}"),
            );
            None
        }
    }
}

#[cold]
fn physical_line(
    receiver: &Value,
    eg: &mut ExecutorGlobals,
    require_line: bool,
) -> Result<Option<Value>, VmError> {
    let (backend, maximum, flags, eof) = read(receiver, |state| {
        (state.backend.clone(), state.maximum, state.flags, state.eof)
    });
    if eof {
        if require_line {
            cannot_read(receiver, eg);
        }
        return Ok(None);
    }
    let (bytes, eof) = match backend {
        Backend::Native(stream) => {
            let mut stream = stream.borrow_mut();
            let mut bytes = Vec::new();
            let limit = (maximum != 0).then(|| maximum.saturating_add(1));
            let result = stream.read_line(&mut bytes, limit);
            if stream.take_plain_file_io() {
                filesystem::clear_filesystem_stat_cache(eg);
            }
            if let Err(failure) = result {
                error(
                    eg,
                    "RuntimeException",
                    &format!("Cannot read from file {}", failure),
                );
                return Ok(None);
            }
            (bytes, stream.is_eof())
        }
        #[cfg(feature = "stream-registry")]
        Backend::Wrapper(value) => {
            let id = value.as_resource_id().unwrap();
            let Some((bytes, eof)) = crate::stdlib::streams::user_wrapper::read_file_object_line(
                eg,
                id,
                maximum,
                require_line,
            )?
            else {
                return Ok(None);
            };
            (bytes, eof)
        }
    };
    if eg.exception.is_some() {
        return Ok(None);
    }
    write(receiver, |state| state.eof = eof);
    let mut bytes = bytes;
    if flags & DROP_NEW_LINE != 0 && bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    Ok(Some(php_byte_result(bytes, false)))
}

#[cold]
fn cannot_read(receiver: &Value, eg: &mut ExecutorGlobals) {
    if let Some(path) = path(receiver, eg) {
        path_error(eg, "Cannot read from file ", &path, "");
    }
}

#[cold]
fn fetch_line(
    ed: *mut ExecuteData,
    receiver: &Value,
    eg: &mut ExecutorGlobals,
    operation: &str,
) -> Result<(), VmError> {
    if read(receiver, |state| state.cache.is_some() || state.eof) {
        return Ok(());
    }
    if read(receiver, |state| state.flags & READ_CSV != 0) {
        let controls = read(receiver, |state| state.csv);
        let _ = csv_records::fetch(ed, receiver, eg, controls, false, operation)?;
        return Ok(());
    }
    loop {
        let value = if read(receiver, |state| state.override_line) {
            let Some(method) = resolve_object_public_method(eg, receiver, "getCurrentLine") else {
                return Ok(());
            };
            let value = call_resolved_with_values_from_internal(ed, eg, &method, &[], true)?;
            if eg.exception.is_some() {
                return Ok(());
            }
            if value.as_str().is_none() {
                let class = receiver.as_object().unwrap().class_name.to_string();
                error(
                    eg,
                    "TypeError",
                    &format!(
                        "{class}::getCurrentLine(): Return value must be of type string, {} returned",
                        value.diagnostic_type_name()
                    ),
                );
                iterator_delegate::discard(value, eg)?;
                return Ok(());
            }
            write(receiver, |state| state.index = state.index.wrapping_add(1));
            Some(value)
        } else {
            physical_line(receiver, eg, false)?
        };
        if eg.exception.is_some() {
            return Ok(());
        }
        let skip = value
            .as_ref()
            .is_some_and(|value| value.as_str() == Some(""))
            && read(receiver, |state| state.flags & SKIP_EMPTY != 0);
        write(receiver, |state| {
            state.cache = if skip { None } else { value }
        });
        if !skip || read(receiver, |state| state.eof) {
            break;
        }
    }
    Ok(())
}

#[cold]
fn current(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    fetch_line(ed, &receiver, eg, "SplFileObject::current")?;
    ret!(
        rv,
        read(&receiver, |state| state
            .cache
            .clone()
            .unwrap_or_else(|| Value::bool(false)))
    );
}

#[cold]
fn to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    fetch_line(ed, &receiver, eg, "SplFileObject::__toString")?;
    if read(&receiver, |state| state.cache.is_none()) && eg.exception.is_none() {
        cannot_read(&receiver, eg);
        return Ok(());
    }
    ret!(
        rv,
        read(&receiver, |state| {
            if state
                .cache
                .as_ref()
                .is_some_and(|value| value.as_array().is_some())
            {
                php_byte_result(state.csv_line.clone().unwrap_or_default(), false)
            } else {
                state.cache.clone().unwrap_or_else(|| Value::string(""))
            }
        })
    );
}

#[cold]
fn get_current_line(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let value = physical_line(&receiver, eg, true)?.unwrap_or_else(|| Value::string(""));
    if eg.exception.is_some() {
        return Ok(());
    }
    write(&receiver, |state| {
        state.cache = Some(value.clone());
        state.csv_line = None;
        state.index = state.index.wrapping_add(1);
    });
    ret!(rv, value);
}

#[cold]
fn next(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let ahead = write(&receiver, |state| {
        state.cache = None;
        state.csv_line = None;
        state.index = state.index.wrapping_add(1);
        state.flags & READ_AHEAD != 0
    });
    if ahead {
        fetch_line(ed, &receiver, eg, "SplFileObject::next")?;
    }
    ret!(rv, Value::null());
}

#[cold]
fn rewind_cursor(
    ed: *mut ExecuteData,
    receiver: &Value,
    eg: &mut ExecutorGlobals,
    method: &str,
) -> Result<bool, VmError> {
    #[cfg(not(feature = "stream-registry"))]
    let _ = (ed, method);
    let backend = read(receiver, |state| state.backend.clone());
    let ok = match backend {
        Backend::Native(stream) => stream.borrow_mut().rewind(),
        #[cfg(feature = "stream-registry")]
        Backend::Wrapper(value) => crate::stdlib::streams::user_wrapper::rewind_file_object(
            eg,
            ed,
            value.as_resource_id().unwrap(),
            method,
        )?,
    };
    if ok {
        write(receiver, |state| {
            state.cache = None;
            state.csv_line = None;
            state.index = 0;
            state.eof = false;
            state.csv_read_failed = false;
        });
    } else {
        if let Some(path) = path(receiver, eg) {
            path_error(eg, "Cannot rewind file ", &path, "");
        }
    }
    Ok(ok)
}

#[cold]
fn rewind(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    if rewind_cursor(ed, &receiver, eg, "SplFileObject::rewind")?
        && read(&receiver, |state| state.flags & READ_AHEAD != 0)
    {
        fetch_line(ed, &receiver, eg, "SplFileObject::rewind")?;
    }
    ret!(rv, Value::null());
}

#[cold]
fn valid(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let ahead = read(&receiver, |state| state.flags & READ_AHEAD != 0);
    if ahead {
        fetch_line(ed, &receiver, eg, "SplFileObject::valid")?;
    } else {
        refresh_wrapper_eof(&receiver, eg)?;
    }
    ret!(
        rv,
        Value::bool(read(&receiver, |state| if ahead {
            state.cache.is_some()
        } else {
            !state.eof
        }))
    );
}

macro_rules! projection {
    ($name:ident, $body:expr) => {
        #[cold]
        fn $name(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            let receiver = owned_argument(ed, 0);
            if !initialized(&receiver, eg) {
                return Ok(());
            }
            ret!(rv, read(&receiver, $body));
        }
    };
}
projection!(key, |state| Value::long(state.index));

#[cold]
fn refresh_wrapper_eof(receiver: &Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    #[cfg(feature = "stream-registry")]
    if let Some((resource, failed)) = read(receiver, |state| match &state.backend {
        Backend::Wrapper(resource) => Some((resource.clone(), state.csv_read_failed)),
        Backend::Native(_) => None,
    }) {
        if let Some(eof) = crate::stdlib::streams::user_wrapper::file_object_eof(
            eg,
            resource.as_resource_id().unwrap(),
            failed,
        )? {
            if eg.exception.is_none() {
                write(receiver, |state| state.eof = eof);
            }
        }
    }
    #[cfg(not(feature = "stream-registry"))]
    let _ = (receiver, eg);
    Ok(())
}

#[cold]
fn eof(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    refresh_wrapper_eof(&receiver, eg)?;
    ret!(rv, Value::bool(read(&receiver, |state| state.eof)));
}
projection!(get_flags, |state| Value::long(state.flags as i64));
projection!(get_max_line_len, |state| Value::long(state.maximum as i64));
projection!(get_children, |_state| Value::null());
projection!(has_children, |_state| Value::bool(false));

fn int_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
    name: &str,
) -> Result<Option<i64>, VmError> {
    let argument = owned_argument(ed, 1);
    typed_internal_int_value_argument_expected(ed, eg, &argument, method, 0, name, "int")
}

#[cold]
fn seek(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(line) = int_argument(ed, eg, "SplFileObject::seek", "line")? else {
        return Ok(());
    };
    if line < 0 {
        error(
            eg,
            "ValueError",
            "SplFileObject::seek(): Argument #1 ($line) must be greater than or equal to 0",
        );
        return Ok(());
    }
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) || !rewind_cursor(ed, &receiver, eg, "SplFileObject::seek")? {
        return Ok(());
    }
    let ahead = read(&receiver, |state| state.flags & READ_AHEAD != 0);
    while read(&receiver, |state| state.index < line) {
        fetch_line(ed, &receiver, eg, "SplFileObject::seek")?;
        if eg.exception.is_some() {
            return Ok(());
        }
        if read(&receiver, |state| {
            state.eof && (ahead || state.index + 1 < line)
        }) {
            write(&receiver, |state| {
                state.cache = None;
                state.csv_line = None;
            });
            break;
        }
        write(&receiver, |state| {
            state.index = state.index.wrapping_add(1);
            state.cache = None;
            state.csv_line = None;
        });
    }
    if ahead {
        fetch_line(ed, &receiver, eg, "SplFileObject::seek")?;
    }
    ret!(rv, Value::null());
}

#[cold]
fn set_flags(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(flags) = int_argument(ed, eg, "SplFileObject::setFlags", "flags")? else {
        return Ok(());
    };
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    write(&receiver, |state| state.flags = flags as u32 & 15);
    ret!(rv, Value::null());
}

#[cold]
fn set_max_line_len(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(maximum) = int_argument(ed, eg, "SplFileObject::setMaxLineLen", "maxLength")? else {
        return Ok(());
    };
    if maximum < 0 {
        error(
            eg,
            "ValueError",
            "SplFileObject::setMaxLineLen(): Argument #1 ($maxLength) must be greater than or equal to 0",
        );
        return Ok(());
    }
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    write(&receiver, |state| state.maximum = maximum as usize);
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
pub(crate) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Array, Bool, ClassName, Int, String, Union, Void};
    let mut functions = Vec::with_capacity(30);
    eg.reserve_internal_method_contracts("SplFileObject", 30);
    let mut method = |name,
                      handler,
                      names: &[&str],
                      hints: Vec<ParamTypeHint>,
                      defaults: &[Option<&str>],
                      result: ParamTypeHint| {
        let required = defaults.iter().filter(|value| value.is_none()).count() as u32;
        eg.register_internal_method_contract(
            "SplFileObject",
            name,
            false,
            required,
            names,
            hints.clone(),
            result,
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
        if name == "flock" {
            function.common.sig.ref_args = 1 << 1;
            eg.register_internal_method_reference_arguments("SplFileObject", name, 1 << 1);
        }
        if name == "__toString" {
            function.common.sig.return_type_hint = String;
        }
        let pointer = &function.common as *const FunctionCommon;
        eg.function_table
            .insert(internal_method_lookup_name("SplFileObject", name), pointer);
        eg.method_declaring_class
            .insert(pointer, "SplFileObject".into());
        eg.register_internal_function_display_name(
            pointer,
            internal_method_display_name("SplFileObject", name),
        );
        eg.register_internal_function_reflection_metadata(
            pointer,
            defaults
                .iter()
                .map(|value| {
                    value.map(|value| match value {
                        "null" => Value::null(),
                        "false" => Value::bool(false),
                        "'\\\\'" => Value::string("\\"),
                        "\"\\n\"" => Value::string("\n"),
                        _ => Value::string(value.trim_matches('\'')),
                    })
                })
                .collect(),
            "SPL",
        );
        functions.push(function);
    };
    method(
        "__construct",
        construct as InternalFunctionHandler,
        &["filename", "mode", "useIncludePath", "context"],
        vec![String, String, Bool, ParamTypeHint::None],
        &[None, Some("'r'"), Some("false"), Some("null")],
        ParamTypeHint::None,
    );
    for (name, handler, result) in [
        ("__toString", to_string as InternalFunctionHandler, String),
        (
            "current",
            current as InternalFunctionHandler,
            Union(vec![Array, String, ClassName("false".into())]),
        ),
        ("next", next, Void),
        ("rewind", rewind, Void),
        ("valid", valid, Bool),
        ("key", key, Int),
        ("eof", eof, Bool),
        ("getFlags", get_flags, Int),
        ("getMaxLineLen", get_max_line_len, Int),
        ("getCurrentLine", get_current_line, String),
        (
            "getChildren",
            get_children,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::None)),
        ),
        ("hasChildren", has_children, ClassName("false".into())),
    ] {
        method(name, handler, &[], vec![], &[], result);
    }
    method("seek", seek, &["line"], vec![Int], &[None], Void);
    method("setFlags", set_flags, &["flags"], vec![Int], &[None], Void);
    method(
        "setMaxLineLen",
        set_max_line_len,
        &["maxLength"],
        vec![Int],
        &[None],
        Void,
    );
    method(
        "getCsvControl",
        csv_records::get_controls,
        &[],
        vec![],
        &[],
        Array,
    );
    method(
        "setCsvControl",
        csv_records::set_controls,
        &["separator", "enclosure", "escape"],
        vec![String, String, String],
        &[Some("','"), Some("'\"'"), Some("'\\\\'")],
        Void,
    );
    method(
        "fgetcsv",
        csv_records::get_record,
        &["separator", "enclosure", "escape"],
        vec![String, String, String],
        &[Some("','"), Some("'\"'"), Some("'\\\\'")],
        Union(vec![Array, ClassName("false".into())]),
    );
    method(
        "fputcsv",
        csv_records::put_record,
        &["fields", "separator", "enclosure", "escape", "eol"],
        vec![Array, String, String, String, String],
        &[
            None,
            Some("','"),
            Some("'\"'"),
            Some("'\\\\'"),
            Some("\"\\n\""),
        ],
        Union(vec![Int, ClassName("false".into())]),
    );
    method(
        "ftell",
        read_position::tell,
        &[],
        vec![],
        &[],
        Union(vec![Int, ClassName("false".into())]),
    );
    method("fgets", read_position::line, &[], vec![], &[], String);
    method(
        "fgetc",
        read_position::byte,
        &[],
        vec![],
        &[],
        Union(vec![String, ClassName("false".into())]),
    );
    method(
        "fread",
        read_position::bytes,
        &["length"],
        vec![Int],
        &[None],
        Union(vec![String, ClassName("false".into())]),
    );
    method("fflush", mutation::flush, &[], vec![], &[], Bool);
    method("fstat", mutation::stat, &[], vec![], &[], Array);
    method(
        "ftruncate",
        mutation::truncate,
        &["size"],
        vec![Int],
        &[None],
        Bool,
    );
    method(
        "fwrite",
        mutation::bytes,
        &["data", "length"],
        vec![String, ParamTypeHint::Nullable(Box::new(Int))],
        &[None, Some("null")],
        Union(vec![Int, ClassName("false".into())]),
    );
    method(
        "flock",
        mutation::lock,
        &["operation", "wouldBlock"],
        vec![Int, ParamTypeHint::None],
        &[None, Some("null")],
        Bool,
    );
    functions
}

#[cold]
pub(crate) fn register_temporary(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut functions = Vec::with_capacity(1);
    eg.reserve_internal_method_contracts("SplTempFileObject", 1);
    recursive_iterator::register_method(
        eg,
        &mut functions,
        "SplTempFileObject",
        "__construct",
        mutation::construct_temporary,
        &["maxMemory"],
        vec![ParamTypeHint::Int],
        &[Some("2097152")],
        ParamTypeHint::None,
    );
    eg.register_internal_function_reflection_metadata(
        &functions[0].common,
        vec![Some(Value::long(2097152))],
        "SPL",
    );
    functions
}
