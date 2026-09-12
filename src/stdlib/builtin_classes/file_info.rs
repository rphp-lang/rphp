//! File metadata is a cold native object capability. Path bytes are retained
//! without resolution; stat/cache and local link I/O use filesystem helpers.
use super::*;
use crate::stdlib::filesystem;
use crate::vm::function::InternalFunctionHandler;
use std::rc::Rc;

mod directory;
pub(crate) use directory::prepare_clone;
pub(super) use directory::register as register_directory_iterators;

/// Only native bytes and request-local class IDs: no PHP values or GC edges.
#[derive(Clone, Default)]
struct NativeFileInfo {
    path: Option<Vec<u8>>,
    info_class_id: u32,
    file_class_id: u32,
    directory: Option<Box<directory::DirectoryState>>,
}

impl crate::value::NativeObjectState for NativeFileInfo {
    #[cold]
    #[inline(never)]
    fn clone_state(&self) -> Box<dyn crate::value::NativeObjectState> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

trait FileInfoObject {
    fn native_file_info(&self) -> Option<&NativeFileInfo>;
    fn native_file_info_mut(&mut self) -> &mut NativeFileInfo;
}

impl FileInfoObject for crate::value::PhpObject {
    #[cold]
    fn native_file_info(&self) -> Option<&NativeFileInfo> {
        self.native_object_state()
    }

    #[cold]
    fn native_file_info_mut(&mut self) -> &mut NativeFileInfo {
        self.native_object_state_mut()
    }
}

fn error(eg: &mut ExecutorGlobals, kind: &str, message: &str) {
    eg.exception = Some(make_error_value(kind, message));
}

#[cold]
fn path_error(eg: &mut ExecutorGlobals, prefix: &str, path: &[u8], suffix: &str) {
    let mut message = Vec::with_capacity(prefix.len() + path.len() + suffix.len());
    message.extend_from_slice(prefix.as_bytes());
    message.extend_from_slice(path);
    message.extend_from_slice(suffix.as_bytes());
    let exception = make_error_value("RuntimeException", "");
    exception
        .as_object_mut()
        .expect("exception object")
        .set_property("message", php_byte_result(message, false));
    eg.exception = Some(exception);
}

fn path(receiver: &Value, eg: &mut ExecutorGlobals) -> Option<Vec<u8>> {
    let mut object = receiver.as_object_mut().expect("file-info receiver");
    let state = object.native_file_info();
    let directory = state
        .and_then(|state| state.directory.as_deref())
        .filter(|directory| directory.is_projected());
    let path = if let Some(directory) = directory
        && directory.filename.is_empty()
    {
        Some(if directory.is_open() {
            directory.base().to_vec()
        } else {
            directory.pathname()
        })
    } else {
        state.and_then(|state| state.path.clone())
    };
    if directory.is_some() {
        object
            .native_file_info_mut()
            .directory
            .as_mut()
            .unwrap()
            .stat_path_materialized = true;
    }
    if path.is_none() {
        error(eg, "Error", "Object not initialized");
    }
    path
}

fn string_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
    parameter: &str,
) -> Result<Option<Vec<u8>>, VmError> {
    let argument = owned_argument(ed, 1);
    let value = typed_internal_string_value_expected(
        ed, eg, &argument, method, 0, parameter, "string", "string",
    )?;
    Ok(value.and_then(|value| value.php_string_bytes().map(|bytes| bytes.into_owned())))
}

#[cold]
fn construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(mut path) = string_argument(ed, eg, "SplFileInfo::__construct", "filename")? else {
        return Ok(());
    };
    if path.contains(&0) {
        error(
            eg,
            "ValueError",
            "SplFileInfo::__construct(): Argument #1 ($filename) must not contain any null bytes",
        );
        return Ok(());
    }
    while path.len() > 1 && path.last() == Some(&b'/') {
        path.pop();
    }
    let mut object = arg!(ed, 0).as_object_mut().expect("file-info receiver");
    let state = object.native_file_info_mut();
    if let Some(directory) = &mut state.directory
        && directory.is_projected()
    {
        let separator = path.iter().rposition(|byte| *byte == b'/').unwrap_or(0);
        directory.base = Some(path[..separator].to_vec());
    }
    state.path = Some(path);
    ret!(rv, Value::null());
}

#[derive(Clone, Copy)]
enum Projection {
    Path,
    Filename,
    Extension,
    Basename,
    Pathname,
}

#[cold]
fn lexical(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    projection: Projection,
) -> Result<(), VmError> {
    let suffix = if matches!(projection, Projection::Basename) && arg_opt!(ed, 1).is_some() {
        let Some(suffix) = string_argument(ed, eg, "SplFileInfo::getBasename", "suffix")? else {
            return Ok(());
        };
        suffix
    } else {
        Vec::new()
    };
    let object = arg!(ed, 0).as_object().expect("file-info receiver");
    let state = object.native_file_info();
    let path = state.and_then(|state| state.path.as_deref());
    if path.is_none() && !matches!(projection, Projection::Path | Projection::Pathname) {
        error(eg, "Error", "Object not initialized");
        return Ok(());
    }
    let path = path.unwrap_or_default();
    let separator = path.iter().rposition(|byte| *byte == b'/');
    let directory = state
        .and_then(|state| state.directory.as_deref())
        .filter(|directory| directory.is_projected());
    let filename = if let Some(directory) = directory {
        &directory.filename
    } else if path == b"/" {
        path
    } else {
        &path[separator.map_or(0, |position| position + 1)..]
    };
    let result = match projection {
        Projection::Path => directory.map_or_else(
            || path[..separator.unwrap_or(0)].to_vec(),
            |directory| directory.base().to_vec(),
        ),
        Projection::Pathname => path.to_vec(),
        Projection::Filename => filename.to_vec(),
        Projection::Extension => filename
            .iter()
            .rposition(|byte| *byte == b'.')
            .map_or_else(Vec::new, |dot| filename[dot + 1..].to_vec()),
        Projection::Basename => crate::path_decomposition::basename(
            if directory.is_some() { filename } else { path },
            &suffix,
        ),
    };
    ret!(rv, php_byte_result(result, false));
}

macro_rules! lexical_method {
    ($name:ident, $projection:ident) => {
        #[cold]
        fn $name(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            lexical(ed, rv, eg, Projection::$projection)
        }
    };
}
lexical_method!(get_path, Path);
lexical_method!(get_filename, Filename);
lexical_method!(get_extension, Extension);
lexical_method!(get_basename, Basename);
lexical_method!(get_pathname, Pathname);

#[cold]
fn stat_field(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    method: &str,
    field: &str,
) -> Result<(), VmError> {
    let Some(path) = path(arg!(ed, 0), eg) else {
        return Ok(());
    };
    let value = filesystem::file_info_stat(eg, &path, false, false)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    if let Some(value) = filesystem::stat_long_field(&value, field) {
        ret!(rv, Value::long(value));
    }
    path_error(
        eg,
        &format!("SplFileInfo::{method}(): stat failed for "),
        &path,
        "",
    );
    Ok(())
}
macro_rules! stat_method {
    ($name:ident, $public:literal, $field:literal) => {
        #[cold]
        fn $name(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            stat_field(ed, rv, eg, $public, $field)
        }
    };
}
stat_method!(get_perms, "getPerms", "mode");
stat_method!(get_inode, "getInode", "ino");
stat_method!(get_size, "getSize", "size");
stat_method!(get_owner, "getOwner", "uid");
stat_method!(get_group, "getGroup", "gid");
stat_method!(get_atime, "getATime", "atime");
stat_method!(get_mtime, "getMTime", "mtime");
stat_method!(get_ctime, "getCTime", "ctime");

#[cold]
fn mode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    kind: i64,
) -> Result<(), VmError> {
    let Some(path) = path(arg!(ed, 0), eg) else {
        return Ok(());
    };
    let value = filesystem::file_info_stat(eg, &path, kind == 0 || kind == 0o120000, kind != 0)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let mode = filesystem::stat_mode(&value);
    if kind != 0 {
        ret!(
            rv,
            Value::bool(mode.is_some_and(|mode| mode & 0o170000 == kind))
        );
    }
    if let Some(name) = mode.and_then(filesystem::file_type_name) {
        ret!(rv, Value::string(name));
    }
    path_error(eg, "SplFileInfo::getType(): Lstat failed for ", &path, "");
    Ok(())
}
macro_rules! mode_method {
    ($name:ident, $kind:expr) => {
        #[cold]
        fn $name(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            mode(ed, rv, eg, $kind)
        }
    };
}
mode_method!(get_type, 0);
mode_method!(is_file, 0o100000);
mode_method!(is_dir, 0o040000);
mode_method!(is_link, 0o120000);

#[cold]
fn access(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    mode: u8,
) -> Result<(), VmError> {
    let Some(path) = path(arg!(ed, 0), eg) else {
        return Ok(());
    };
    let allowed = filesystem::file_info_access(eg, &path, mode)?;
    ret!(rv, Value::bool(allowed));
}
macro_rules! access_method {
    ($name:ident, $mode:expr) => {
        #[cold]
        fn $name(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            access(ed, rv, eg, $mode)
        }
    };
}
access_method!(is_readable, 0);
access_method!(is_writable, 1);
access_method!(is_executable, 2);

fn path_value(path: &std::path::Path) -> Value {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Value::binary_string(path.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    Value::string(path.to_string_lossy().as_ref())
}

#[cold]
fn get_link_target(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = path(arg!(ed, 0), eg) else {
        return Ok(());
    };
    match std::fs::read_link(filesystem::file_info_native_path(&path)) {
        Ok(target) => {
            ret!(rv, path_value(&target));
        }
        Err(failure) => path_error(
            eg,
            "Unable to read link ",
            &path,
            &format!(", error: {}", filesystem::filesystem_error_reason(&failure)),
        ),
    }
    Ok(())
}

#[cold]
fn get_real_path(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let object = arg!(ed, 0).as_object().expect("file-info receiver");
    let Some(path) = object
        .native_file_info()
        .and_then(|state| {
            if let Some(directory) = &state.directory
                && directory.is_projected()
                && directory.filename.is_empty()
            {
                return Some(if directory.is_open() {
                    std::borrow::Cow::Borrowed(directory.base())
                } else {
                    std::borrow::Cow::Owned(directory.pathname())
                });
            }
            state.path.as_deref().map(std::borrow::Cow::Borrowed)
        })
        .filter(|path| !path.is_empty())
    else {
        ret!(rv, Value::bool(false));
    };
    ret!(
        rv,
        std::fs::canonicalize(filesystem::file_info_native_path(&path))
            .map_or_else(|_| Value::bool(false), |path| path_value(&path))
    );
}

#[cold]
fn debug_info(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let mut properties = array_object::member_properties(arg!(ed, 0), eg);
    let object = arg!(ed, 0).as_object().expect("file-info receiver");
    let state = object.native_file_info();
    let path = state.and_then(|state| state.path.as_deref());
    properties.set_str(
        "\0SplFileInfo\0pathName",
        php_byte_result(path.unwrap_or_default().to_vec(), false),
    );
    if let Some(path) = path {
        let directory = state
            .and_then(|state| state.directory.as_deref())
            .filter(|directory| directory.is_projected());
        let filename = if let Some(directory) = directory {
            &directory.filename
        } else if path == b"/" {
            path
        } else {
            &path[path
                .iter()
                .rposition(|byte| *byte == b'/')
                .map_or(0, |position| position + 1)..]
        };
        if directory.is_none_or(|directory| {
            !directory.filename.is_empty() || directory.stat_path_materialized
        }) {
            properties.set_str(
                "\0SplFileInfo\0fileName",
                php_byte_result(filename.to_vec(), false),
            );
        }
        if directory.is_some() {
            properties.set_str("\0DirectoryIterator\0glob", Value::bool(false));
            properties.set_str(
                "\0RecursiveDirectoryIterator\0subPathName",
                Value::string(""),
            );
        }
    }
    ret!(rv, Value::array(properties));
}

#[cold]
fn bad_state(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    report_internal_deprecation(
        eg,
        ed,
        "Method SplFileInfo::_bad_state_ex() is deprecated since 8.2",
    )?;
    if eg.exception.is_none() {
        error(
            eg,
            "Error",
            "The parent constructor was not called: the object is in an invalid state",
        );
    }
    Ok(())
}

fn class_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
    base: &str,
    nullable: bool,
) -> Result<Option<u32>, VmError> {
    let Some(argument) = arg_opt!(ed, 1) else {
        return Ok(Some(0));
    };
    if nullable && argument.value_type() == ValueType::Null {
        return Ok(Some(0));
    }
    let argument = argument.clone();
    let name = if argument.value_type() == ValueType::Object {
        match crate::vm::execute::call_object_string_conversion(eg, &argument)? {
            Some(value) => value.echo_to_string(),
            None => {
                if eg.exception.is_none() {
                    error(
                        eg,
                        "Error",
                        &format!(
                            "Object of class {} could not be converted to string",
                            argument.as_object().unwrap().class_name
                        ),
                    );
                }
                return Ok(None);
            }
        }
    } else {
        if argument.value_type() == ValueType::Array {
            report_internal_diagnostic(eg, ed, 2, "Warning", "Array to string conversion")?;
        }
        argument.echo_to_string_with_precision(eg.precision)
    };
    if eg.exception.is_some() {
        return Ok(None);
    }
    // The default file class is an accepted selection even though this slice
    // deliberately does not implement its stream-consuming object factory.
    if base == "SplFileObject" && name.eq_ignore_ascii_case(base) {
        return Ok(Some(0));
    }
    crate::stdlib::autoload::ensure_symbol_loaded(eg, &name)?;
    if eg.exception.is_some() {
        return Ok(None);
    }
    if eg.class_is_a(&name, base)
        && let Some(class) = eg.find_class(&name)
    {
        return Ok(Some(class.class_id));
    }
    error(
        eg,
        "TypeError",
        &format!(
            "SplFileInfo::{method}(): Argument #1 ($class) must be a class name derived from {base}{}, {name} given",
            if nullable { " or null" } else { "" }
        ),
    );
    Ok(None)
}

#[cold]
fn set_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    file: bool,
) -> Result<(), VmError> {
    let (method, base) = if file {
        ("setFileClass", "SplFileObject")
    } else {
        ("setInfoClass", "SplFileInfo")
    };
    let Some(class_id) = class_argument(ed, eg, method, base, false)? else {
        return Ok(());
    };
    let mut object = arg!(ed, 0).as_object_mut().expect("file-info receiver");
    let state = object.native_file_info_mut();
    if file {
        state.file_class_id = class_id;
    } else {
        state.info_class_id = class_id;
    }
    ret!(rv, Value::null());
}
#[cold]
fn set_info_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    set_class(ed, rv, eg, false)
}
#[cold]
fn set_file_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    set_class(ed, rv, eg, true)
}

#[cold]
fn factory(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    parent: bool,
) -> Result<(), VmError> {
    factory_projection(ed, rv, eg, parent, true)
}

#[cold]
fn factory_projection(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    parent: bool,
    accepts_class: bool,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    let (path, selected, directory) = {
        let object = receiver.as_object().expect("file-info receiver");
        let state = object.native_file_info();
        (
            state.and_then(|state| state.path.clone()),
            state.map_or(0, |state| state.info_class_id),
            state
                .and_then(|state| state.directory.as_deref())
                .is_some_and(|directory| directory.is_projected()),
        )
    };
    let method = if parent { "getPathInfo" } else { "getFileInfo" };
    let base = eg
        .class_by_id(selected)
        .map_or("SplFileInfo", |class| class.name.as_str())
        .to_owned();
    let class_id = if accepts_class {
        let Some(class_id) = class_argument(ed, eg, method, &base, true)? else {
            return Ok(());
        };
        class_id
    } else {
        0
    };
    let path = if parent {
        match path.filter(|path| !path.is_empty()) {
            Some(path) => crate::path_decomposition::dirname(&path, 1),
            None => {
                ret!(rv, Value::null());
            }
        }
    } else {
        match path {
            Some(path) if directory && path.is_empty() => {
                error(eg, "RuntimeException", "Could not open file");
                return Ok(());
            }
            Some(path) => path,
            None => {
                error(eg, "Error", "Object not initialized");
                return Ok(());
            }
        }
    };
    let class = eg
        .class_by_id(if class_id == 0 { selected } else { class_id })
        .or_else(|| eg.find_class("SplFileInfo"))
        .expect("registered file-info class");
    // Native info factories intentionally bypass ordinary `new`'s abstract
    // and constructor-visibility restrictions, but still invoke the selected
    // constructor exactly once. A user constructor may leave it uninitialized.
    let class_id = class.class_id;
    let constructor = eg
        .find_function(&format!("{}::__construct", class.name))
        .or_else(|| {
            find_method_in_class_hierarchy(eg, &class.name, "__construct")
                .map(|(_, _, function, _)| function)
        });
    let object = Value::object(PhpObject::with_layout_from_defaults(
        class.class_id,
        Rc::clone(&class.property_layout),
        &class.property_defaults,
    ));
    if let Some(func_ptr) = constructor {
        let constructor = ResolvedCallback {
            func_ptr,
            prepend_args: vec![object.clone()],
            use_vars: vec![],
            called_scope_class_id: class_id,
            closure_scope_class_id: None,
            bound_this: None,
            closure_static_vars: None,
            is_magic_call: false,
        };
        let result = call_resolved_with_values_from_internal(
            ed,
            eg,
            &constructor,
            &[php_byte_result(path, false)],
            true,
        )?;
        iterator_delegate::discard(result, eg)?;
    }
    if eg.exception.is_some() {
        iterator_delegate::discard(object, eg)?;
        return Ok(());
    }
    ret!(rv, object);
}
#[cold]
fn get_file_info(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    factory(ed, rv, eg, false)
}
#[cold]
fn get_path_info(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    factory(ed, rv, eg, true)
}

#[cold]
#[inline(never)]
fn register_method(
    eg: &mut ExecutorGlobals,
    functions: &mut Vec<Box<InternalFunction>>,
    name: &'static str,
    handler: InternalFunctionHandler,
    names: &[&str],
    hints: Vec<ParamTypeHint>,
    defaults: &[Option<&str>],
    result: ParamTypeHint,
) {
    let required = defaults.iter().filter(|value| value.is_none()).count() as u32;
    // Tentative returns belong only to the link/Reflection contract. Do not
    // clone their nested hints just to discard the unused original afterward.
    let enforced_return = matches!(name, "__toString" | "_bad_state_ex").then(|| result.clone());
    eg.register_internal_method_contract(
        "SplFileInfo",
        name,
        false,
        required,
        names,
        hints.clone(),
        result,
        defaults,
        !matches!(name, "__construct" | "__toString" | "_bad_state_ex"),
    );
    let mut function = Box::new(make_internal_method(
        handler,
        names.len() as u32 + 1,
        required,
        names.iter().map(|name| name.to_string()).collect(),
    ));
    function.handler_validates_types = true;
    function.common.sig.param_type_hints = hints;
    if let Some(result) = enforced_return {
        function.common.sig.return_type_hint = result;
    }
    let pointer = &function.common as *const FunctionCommon;
    eg.function_table
        .insert(internal_method_lookup_name("SplFileInfo", name), pointer);
    eg.method_declaring_class
        .insert(pointer, "SplFileInfo".into());
    eg.register_internal_function_display_name(
        pointer,
        internal_method_display_name("SplFileInfo", name),
    );
    eg.register_internal_function_reflection_metadata(
        pointer,
        defaults
            .iter()
            .map(|value| {
                value.map(|value| {
                    if value == "null" {
                        Value::null()
                    } else {
                        Value::string(value.trim_matches('\''))
                    }
                })
            })
            .collect(),
        "SPL",
    );
    functions.push(function);
}

#[cold]
#[inline(never)]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Array, Bool, ClassName, Int, Nullable, String, Union, Void};
    let mut functions = Vec::with_capacity(30);
    eg.reserve_internal_method_contracts("SplFileInfo", 30);
    macro_rules! method {
        ($name:literal, $handler:ident, $result:expr) => {
            register_method(
                eg,
                &mut functions,
                $name,
                $handler,
                &[],
                vec![],
                &[],
                $result,
            );
        };
    }
    register_method(
        eg,
        &mut functions,
        "__construct",
        construct,
        &["filename"],
        vec![String],
        &[None],
        ParamTypeHint::None,
    );
    method!("getPath", get_path, String);
    method!("getFilename", get_filename, String);
    method!("getExtension", get_extension, String);
    register_method(
        eg,
        &mut functions,
        "getBasename",
        get_basename,
        &["suffix"],
        vec![String],
        &[Some("''")],
        String,
    );
    method!("getPathname", get_pathname, String);
    for (name, handler) in [
        ("getPerms", get_perms as InternalFunctionHandler),
        ("getInode", get_inode),
        ("getSize", get_size),
        ("getOwner", get_owner),
        ("getGroup", get_group),
        ("getATime", get_atime),
        ("getMTime", get_mtime),
        ("getCTime", get_ctime),
    ] {
        register_method(
            eg,
            &mut functions,
            name,
            handler,
            &[],
            vec![],
            &[],
            Union(vec![Int, ClassName("false".into())]),
        );
    }
    method!(
        "getType",
        get_type,
        Union(vec![String, ClassName("false".into())])
    );
    method!("isWritable", is_writable, Bool);
    method!("isReadable", is_readable, Bool);
    method!("isExecutable", is_executable, Bool);
    method!("isFile", is_file, Bool);
    method!("isDir", is_dir, Bool);
    method!("isLink", is_link, Bool);
    method!(
        "getLinkTarget",
        get_link_target,
        Union(vec![String, ClassName("false".into())])
    );
    method!(
        "getRealPath",
        get_real_path,
        Union(vec![String, ClassName("false".into())])
    );
    register_method(
        eg,
        &mut functions,
        "getFileInfo",
        get_file_info,
        &["class"],
        vec![Nullable(Box::new(String))],
        &[Some("null")],
        ClassName("SplFileInfo".into()),
    );
    register_method(
        eg,
        &mut functions,
        "getPathInfo",
        get_path_info,
        &["class"],
        vec![Nullable(Box::new(String))],
        &[Some("null")],
        Nullable(Box::new(ClassName("SplFileInfo".into()))),
    );
    register_method(
        eg,
        &mut functions,
        "setFileClass",
        set_file_class,
        &["class"],
        vec![String],
        &[Some("'SplFileObject'")],
        Void,
    );
    register_method(
        eg,
        &mut functions,
        "setInfoClass",
        set_info_class,
        &["class"],
        vec![String],
        &[Some("'SplFileInfo'")],
        Void,
    );
    method!("__toString", get_pathname, String);
    method!("__debugInfo", debug_info, Array);
    method!("_bad_state_ex", bad_state, Void);
    eg.mark_internal_method_final("SplFileInfo", "_bad_state_ex");
    functions
}
