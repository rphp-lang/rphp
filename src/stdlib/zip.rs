//! Composer-facing `ZipArchive` extraction backed by a Rust ZIP reader.
//!
//! Composer needs an in-process archive path when no external unzip process is
//! available.  Keep the open archive as request-owned metadata and reopen the
//! immutable file for inspection/extraction, avoiding host PHP or subprocess
//! delegation while retaining PHP's object and error-code surface.

use std::fs::{self, File};
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::*;
use crate::compiler::compile::{ClassConstantDefinition, ClassDef, PropertyDefinition};
use crate::parser::Visibility;
use crate::value::{NativeObjectState, ObjectLayout};
use crate::vm::function::InternalFunctionHandler;

const ZIP_ARCHIVE: &str = "ZipArchive";

const ER_OK: i64 = 0;
const ER_SEEK: i64 = 4;
const ER_READ: i64 = 5;
const ER_WRITE: i64 = 6;
const ER_CRC: i64 = 7;
const ER_NOENT: i64 = 9;
const ER_EXISTS: i64 = 10;
const ER_OPEN: i64 = 11;
const ER_MEMORY: i64 = 14;
const ER_INVAL: i64 = 18;
const ER_NOZIP: i64 = 19;
const ER_INCONS: i64 = 21;

fn return_value(rv: *mut Value, value: Value) -> Result<(), VmError> {
    write_return_value(rv, value);
    Ok(())
}

#[derive(Clone, Default)]
struct State {
    path: Option<PathBuf>,
    count: usize,
    status: i64,
}

impl NativeObjectState for State {
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

struct Method {
    name: &'static str,
    handler: InternalFunctionHandler,
    required: u32,
    parameters: &'static [&'static str],
    hints: Vec<ParamTypeHint>,
    result: ParamTypeHint,
    defaults: Vec<Option<Value>>,
    default_spellings: Vec<Option<&'static str>>,
}

fn class_constant(name: &str, value: Value) -> ClassConstantDefinition {
    ClassConstantDefinition {
        attributes: Vec::new(),
        name: name.to_string(),
        value,
        source_file: String::new(),
        source_expression: None,
        callable_factory: None,
        evaluation_scope: None,
        value_is_deferred: false,
        evaluation_error: None,
        visibility: Visibility::Public,
        declaring_class: ZIP_ARCHIVE.to_string(),
        type_hint: ParamTypeHint::None,
        is_final: false,
    }
}

fn property(name: &str, default: Value, hint: ParamTypeHint) -> PropertyDefinition {
    PropertyDefinition::declared(
        name.to_string(),
        Some(default),
        Visibility::Public,
        ZIP_ARCHIVE.to_string(),
        hint,
        true,
        false,
    )
}

#[cold]
pub(super) fn register(eg: &mut ExecutorGlobals, functions: &mut Vec<Box<InternalFunction>>) {
    let constants = [
        ("CREATE", 1),
        ("EXCL", 2),
        ("CHECKCONS", 4),
        ("OVERWRITE", 8),
        ("RDONLY", 16),
        ("OPSYS_UNIX", 3),
        ("ER_OK", ER_OK),
        ("ER_SEEK", ER_SEEK),
        ("ER_READ", ER_READ),
        ("ER_WRITE", ER_WRITE),
        ("ER_CRC", ER_CRC),
        ("ER_NOENT", ER_NOENT),
        ("ER_EXISTS", ER_EXISTS),
        ("ER_OPEN", ER_OPEN),
        ("ER_MEMORY", ER_MEMORY),
        ("ER_INVAL", ER_INVAL),
        ("ER_NOZIP", ER_NOZIP),
        ("ER_INCONS", ER_INCONS),
    ]
    .into_iter()
    .map(|(name, value)| class_constant(name, Value::long(value)))
    .collect();
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: ZIP_ARCHIVE.to_string(),
        source_file: None,
        declaration_line: 0,
        end_line: 0,
        doc_comment: None,
        parent: None,
        implements: vec!["Countable".to_string()],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        is_trait: false,
        is_enum: false,
        uses: Vec::new(),
        trait_aliases: Vec::new(),
        trait_precedences: Vec::new(),
        properties: vec![
            property("lastId", Value::long(-1), ParamTypeHint::Int),
            property("status", Value::long(0), ParamTypeHint::Int),
            property("statusSys", Value::long(0), ParamTypeHint::Int),
            property("numFiles", Value::long(0), ParamTypeHint::Int),
            property("filename", Value::string(""), ParamTypeHint::String),
            property("comment", Value::string(""), ParamTypeHint::String),
        ],
        static_properties: Vec::new(),
        constants,
        property_layout: Rc::new(ObjectLayout::empty()),
        property_defaults: Rc::from([]),
        readonly_props: vec![
            "lastId".to_string(),
            "status".to_string(),
            "statusSys".to_string(),
            "numFiles".to_string(),
            "filename".to_string(),
            "comment".to_string(),
        ],
        methods: Vec::new(),
        abstract_methods: Vec::new(),
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .expect("ZipArchive registers once per request");

    let false_type = || ParamTypeHint::ClassName("false".into());
    let int_or_false = || ParamTypeHint::Union(vec![ParamTypeHint::Int, false_type()].into());
    let array_or_false = || ParamTypeHint::Union(vec![ParamTypeHint::Array, false_type()].into());
    let string_or_false = || ParamTypeHint::Union(vec![ParamTypeHint::String, false_type()].into());
    let methods = [
        Method {
            name: "open",
            handler: method_open,
            required: 1,
            parameters: &["filename", "flags"],
            hints: vec![ParamTypeHint::String, ParamTypeHint::Int],
            result: ParamTypeHint::Union(vec![ParamTypeHint::Bool, ParamTypeHint::Int].into()),
            defaults: vec![None, Some(Value::long(0))],
            default_spellings: vec![None, Some("0")],
        },
        Method {
            name: "close",
            handler: method_close,
            required: 0,
            parameters: &[],
            hints: Vec::new(),
            result: ParamTypeHint::Bool,
            defaults: Vec::new(),
            default_spellings: Vec::new(),
        },
        Method {
            name: "count",
            handler: method_count,
            required: 0,
            parameters: &[],
            hints: Vec::new(),
            result: ParamTypeHint::Int,
            defaults: Vec::new(),
            default_spellings: Vec::new(),
        },
        Method {
            name: "statIndex",
            handler: method_stat_index,
            required: 1,
            parameters: &["index", "flags"],
            hints: vec![ParamTypeHint::Int, ParamTypeHint::Int],
            result: array_or_false(),
            defaults: vec![None, Some(Value::long(0))],
            default_spellings: vec![None, Some("0")],
        },
        Method {
            name: "extractTo",
            handler: method_extract_to,
            required: 1,
            parameters: &["pathto", "files"],
            hints: vec![
                ParamTypeHint::String,
                ParamTypeHint::Nullable(std::rc::Rc::new(ParamTypeHint::Union(
                    vec![ParamTypeHint::Array, ParamTypeHint::String].into(),
                ))),
            ],
            result: ParamTypeHint::Bool,
            defaults: vec![None, Some(Value::null())],
            default_spellings: vec![None, Some("null")],
        },
        Method {
            name: "locateName",
            handler: method_locate_name,
            required: 1,
            parameters: &["name", "flags"],
            hints: vec![ParamTypeHint::String, ParamTypeHint::Int],
            result: int_or_false(),
            defaults: vec![None, Some(Value::long(0))],
            default_spellings: vec![None, Some("0")],
        },
        Method {
            name: "getNameIndex",
            handler: method_get_name_index,
            required: 1,
            parameters: &["index", "flags"],
            hints: vec![ParamTypeHint::Int, ParamTypeHint::Int],
            result: string_or_false(),
            defaults: vec![None, Some(Value::long(0))],
            default_spellings: vec![None, Some("0")],
        },
        Method {
            name: "getFromIndex",
            handler: method_get_from_index,
            required: 1,
            parameters: &["index", "len", "flags"],
            hints: vec![ParamTypeHint::Int, ParamTypeHint::Int, ParamTypeHint::Int],
            result: string_or_false(),
            defaults: vec![None, Some(Value::long(0)), Some(Value::long(0))],
            default_spellings: vec![None, Some("0"), Some("0")],
        },
    ];

    for method in methods {
        eg.register_internal_method_contract(
            ZIP_ARCHIVE,
            method.name,
            false,
            method.required,
            method.parameters,
            method.hints.clone(),
            method.result.clone(),
            &method.default_spellings,
            false,
        );
        let mut function = Box::new(
            make_internal_method(
                method.handler,
                method.parameters.len() as u32 + 1,
                method.required,
                Vec::new(),
            )
            .with_static_parameter_names(method.parameters),
        );
        function.common.sig.param_type_hints = method.hints;
        function.common.sig.return_type_hint = method.result;
        let pointer = &function.common as *const FunctionCommon;
        eg.insert_function_entry(
            super::builtin_classes::internal_method_lookup_name(ZIP_ARCHIVE, method.name),
            pointer,
        );
        eg.method_declaring_class
            .insert(pointer, ZIP_ARCHIVE.into());
        eg.register_internal_function_display_name(
            pointer,
            super::builtin_classes::internal_method_display_name(ZIP_ARCHIVE, method.name),
        );
        eg.register_internal_function_reflection_metadata(pointer, method.defaults, "zip");
        functions.push(function);
    }
}

fn receiver(ed: *mut ExecuteData) -> Value {
    super::owned_argument(ed, 0)
}

fn with_state<T>(ed: *mut ExecuteData, operation: impl FnOnce(&State) -> T) -> Option<T> {
    let value = receiver(ed);
    let object = value.as_object()?;
    Some(operation(object.native_object_state::<State>()?))
}

fn open_archive(path: &Path) -> Result<::zip::ZipArchive<File>, i64> {
    let file = File::open(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => ER_NOENT,
        io::ErrorKind::PermissionDenied => ER_OPEN,
        _ => ER_READ,
    })?;
    ::zip::ZipArchive::new(file).map_err(|error| match error {
        ::zip::result::ZipError::InvalidArchive(_)
        | ::zip::result::ZipError::UnsupportedArchive(_) => ER_NOZIP,
        ::zip::result::ZipError::Io(error) => match error.kind() {
            io::ErrorKind::UnexpectedEof => ER_INCONS,
            _ => ER_READ,
        },
        _ => ER_INVAL,
    })
}

fn update_open_properties(object: &mut PhpObject, path: &Path, count: usize, status: i64) {
    object.set_property("status", Value::long(status));
    object.set_property("statusSys", Value::long(0));
    object.set_property("numFiles", Value::long(count as i64));
    object.set_property(
        "filename",
        Value::string(path.to_string_lossy().into_owned()),
    );
}

fn method_open(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = PathBuf::from(
        String::from_utf8_lossy(
            arg!(ed, 1)
                .php_string_bytes()
                .expect("validated ZIP filename")
                .as_ref(),
        )
        .into_owned(),
    );
    let value = receiver(ed);
    let Some(mut object) = value.as_object_mut() else {
        return return_value(rv, Value::long(ER_INVAL));
    };
    match open_archive(&path) {
        Ok(archive) => {
            let count = archive.len();
            *object.native_object_state_mut::<State>() = State {
                path: Some(path.clone()),
                count,
                status: ER_OK,
            };
            update_open_properties(&mut object, &path, count, ER_OK);
            return_value(rv, Value::bool(true))
        }
        Err(code) => {
            *object.native_object_state_mut::<State>() = State {
                path: None,
                count: 0,
                status: code,
            };
            update_open_properties(&mut object, &path, 0, code);
            return_value(rv, Value::long(code))
        }
    }
}

fn method_close(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = receiver(ed);
    let Some(mut object) = value.as_object_mut() else {
        return return_value(rv, Value::bool(false));
    };
    let state = object.native_object_state_mut::<State>();
    let was_open = state.path.take().is_some();
    state.count = 0;
    state.status = ER_OK;
    object.set_property("status", Value::long(ER_OK));
    return_value(rv, Value::bool(was_open))
}

fn method_count(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::long(with_state(ed, |state| state.count as i64).unwrap_or_default()),
    )
}

fn archive_path(ed: *mut ExecuteData) -> Option<PathBuf> {
    with_state(ed, |state| state.path.clone()).flatten()
}

fn method_stat_index(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = archive_path(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let Ok(mut archive) = open_archive(&path) else {
        return return_value(rv, Value::bool(false));
    };
    let index = arg!(ed, 1).to_long_val();
    let Ok(index) = usize::try_from(index) else {
        return return_value(rv, Value::bool(false));
    };
    let Ok(file) = archive.by_index(index) else {
        return return_value(rv, Value::bool(false));
    };
    let mut result = PhpArray::with_hash_capacity(8);
    result.set_str("name", Value::string(file.name()));
    result.set_str("index", Value::long(index as i64));
    result.set_str("crc", Value::long(i64::from(file.crc32())));
    result.set_str("size", Value::long(file.size() as i64));
    result.set_str("mtime", Value::long(0));
    result.set_str("comp_size", Value::long(file.compressed_size() as i64));
    let compression = file.compression();
    let compression = if compression == ::zip::CompressionMethod::STORE {
        0
    } else if compression == ::zip::CompressionMethod::DEFLATE {
        8
    } else {
        -1
    };
    result.set_str("comp_method", Value::long(compression));
    result.set_str("encryption_method", Value::long(0));
    return_value(rv, Value::array(result))
}

fn selected_names(value: &Value) -> Option<Vec<Vec<u8>>> {
    match value.dereferenced().value_type() {
        ValueType::Null | ValueType::Undef => None,
        ValueType::String => value.php_string_bytes().map(|name| vec![name.into_owned()]),
        ValueType::Array => value.as_array().map(|array| {
            array
                .values()
                .filter_map(|name| name.php_string_bytes().map(|name| name.into_owned()))
                .collect()
        }),
        _ => Some(Vec::new()),
    }
}

fn extract_entry<R: io::Read + io::Seek>(
    archive: &mut ::zip::ZipArchive<R>,
    index: usize,
    destination: &Path,
) -> io::Result<()> {
    let mut entry = archive.by_index(index).map_err(io::Error::other)?;
    let Some(relative) = entry.enclosed_name() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ZIP entry escapes extraction directory",
        ));
    };
    let target = destination.join(relative);
    if entry.is_dir() {
        fs::create_dir_all(&target)?;
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = File::create(&target)?;
    io::copy(&mut entry, &mut output)?;
    #[cfg(unix)]
    if let Some(mode) = entry.unix_mode() {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o777))?;
    }
    Ok(())
}

fn method_extract_to(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = archive_path(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let destination = PathBuf::from(
        String::from_utf8_lossy(
            arg!(ed, 1)
                .php_string_bytes()
                .expect("validated extraction path")
                .as_ref(),
        )
        .into_owned(),
    );
    let selected = arg_opt!(ed, 2).and_then(selected_names);
    let Ok(mut archive) = open_archive(&path) else {
        return return_value(rv, Value::bool(false));
    };
    if fs::create_dir_all(&destination).is_err() {
        return return_value(rv, Value::bool(false));
    }
    for index in 0..archive.len() {
        let include = if let Some(selected) = &selected {
            let Ok(entry) = archive.by_index(index) else {
                return return_value(rv, Value::bool(false));
            };
            let include = selected
                .iter()
                .any(|name| name.as_slice() == entry.name_raw());
            drop(entry);
            include
        } else {
            true
        };
        if include && extract_entry(&mut archive, index, &destination).is_err() {
            return return_value(rv, Value::bool(false));
        }
    }
    return_value(rv, Value::bool(true))
}

fn method_locate_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = archive_path(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let wanted = arg!(ed, 1).php_string_bytes().unwrap_or_default();
    let Ok(mut archive) = open_archive(&path) else {
        return return_value(rv, Value::bool(false));
    };
    for index in 0..archive.len() {
        let Ok(file) = archive.by_index(index) else {
            continue;
        };
        if file.name_raw() == wanted.as_ref() {
            return return_value(rv, Value::long(index as i64));
        }
    }
    return_value(rv, Value::bool(false))
}

fn method_get_name_index(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = archive_path(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let Ok(mut archive) = open_archive(&path) else {
        return return_value(rv, Value::bool(false));
    };
    let Ok(index) = usize::try_from(arg!(ed, 1).to_long_val()) else {
        return return_value(rv, Value::bool(false));
    };
    let Ok(file) = archive.by_index(index) else {
        return return_value(rv, Value::bool(false));
    };
    return_value(rv, super::php_byte_result(file.name_raw().to_vec(), false))
}

fn method_get_from_index(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(path) = archive_path(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let Ok(mut archive) = open_archive(&path) else {
        return return_value(rv, Value::bool(false));
    };
    let Ok(index) = usize::try_from(arg!(ed, 1).to_long_val()) else {
        return return_value(rv, Value::bool(false));
    };
    let Ok(mut file) = archive.by_index(index) else {
        return return_value(rv, Value::bool(false));
    };
    let limit = arg_opt!(ed, 2).map(Value::to_long_val).unwrap_or_default();
    let mut bytes = Vec::new();
    if limit > 0 {
        io::Read::take(&mut file, limit as u64)
            .read_to_end(&mut bytes)
            .ok();
    } else {
        file.read_to_end(&mut bytes).ok();
    }
    return_value(rv, super::php_byte_result(bytes, false))
}
