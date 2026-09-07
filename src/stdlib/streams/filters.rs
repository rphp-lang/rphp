//! Request-local user filters. Native stream operations do not consult the
//! registration table. Callback state and bucket ownership are allocated only
//! for streams which actually acquire a filter.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use crate::compiler::compile::{ClassDef, PropertyDefinition};
use crate::compiler::{make_internal_function, make_internal_method};
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, PhpObject, Value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{
    FunctionCommon, InternalFunction, InternalFunctionHandler, ParamTypeHint,
};

const REGISTRY: &str = "\0rphp-user-stream-filter-registry";
const ORDER: &str = "\0order";
const PASS_ON: i64 = 2;
const SOURCE: &str = "\0rphp-filter-diagnostic-source";

#[derive(Clone, Copy)]
enum BuiltinFilter {
    Uppercase,
}

// Inventory and collision checks share the factories that actually execute.
const BUILTIN_FILTERS: &[(&str, BuiltinFilter)] = &[("string.toupper", BuiltinFilter::Uppercase)];

fn builtin_filter(name: &str) -> Option<BuiltinFilter> {
    BUILTIN_FILTERS
        .iter()
        .find_map(|(registered, kind)| (*registered == name).then_some(*kind))
}

mod lifecycle;
pub(in crate::stdlib) mod uri;
#[cfg(feature = "stream-contents")]
pub(super) use lifecycle::read_contents;
#[cfg(feature = "stream-line")]
pub(super) use lifecycle::read_record;
pub(in crate::stdlib) use lifecycle::shutdown;
pub(super) use lifecycle::{
    close, eof, flush, position, read, read_csv, read_line, seek, unread_bytes, with_backend, write,
};

/// Diagnostic origin is data, not an executable VM frame. A fast internal
/// handler's temporary frame must never replace the active user frame.
#[inline]
pub(in crate::stdlib) fn with_source<T>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    operation: impl FnOnce(&mut ExecutorGlobals) -> Result<T, VmError>,
) -> Result<T, VmError> {
    let (file, line) = crate::stdlib::internal_call_source(frame);
    let function = crate::vm::execute::displayed_frame_function_name(eg, frame);
    with_origin(eg, file, line, function, operation)
}

#[cold]
pub(in crate::stdlib) fn with_origin<T>(
    eg: &mut ExecutorGlobals,
    file: String,
    line: usize,
    function: String,
    operation: impl FnOnce(&mut ExecutorGlobals) -> Result<T, VmError>,
) -> Result<T, VmError> {
    let current = [
        ("file".to_string(), Value::string(file)),
        ("line".to_string(), Value::long(line as i64)),
        ("function".to_string(), Value::string(function)),
    ]
    .into_iter()
    .collect();
    let previous = eg.static_vars.insert(SOURCE.into(), current);
    let result = operation(eg);
    if let Some(previous) = previous {
        eg.static_vars.insert(SOURCE.into(), previous);
    } else {
        eg.static_vars.remove(SOURCE);
    }
    result
}

pub(super) fn diagnostic_source(eg: &ExecutorGlobals) -> (String, usize) {
    let Some(source) = eg.static_vars.get(SOURCE) else {
        return (String::new(), 0);
    };
    (
        source
            .get("file")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        source
            .get("line")
            .and_then(Value::as_long)
            .unwrap_or(0)
            .max(0) as usize,
    )
}

fn diagnostic_function(eg: &ExecutorGlobals) -> &str {
    eg.static_vars
        .get(SOURCE)
        .and_then(|source| source.get("function"))
        .and_then(Value::as_str)
        .unwrap_or("Unknown")
}

type SharedBucket = Rc<RefCell<Bucket>>;
type SharedBrigade = Rc<RefCell<Brigade>>;

struct Bucket {
    bytes: Vec<u8>,
    owner: Weak<RefCell<Brigade>>,
}

#[derive(Default)]
struct Brigade {
    buckets: VecDeque<Value>,
}

#[cold]
pub(super) fn register_functions(
    eg: &mut ExecutorGlobals,
    functions: &mut Vec<Box<InternalFunction>>,
) {
    lifecycle::register_functions(eg, functions);
    for (name, handler, names, hints, result) in [
        (
            "stream_filter_register",
            fn_register as InternalFunctionHandler,
            vec!["filter_name", "class"],
            vec![ParamTypeHint::String, ParamTypeHint::String],
            ParamTypeHint::Bool,
        ),
        (
            "stream_bucket_new",
            fn_bucket_new,
            vec!["stream", "buffer"],
            vec![ParamTypeHint::None, ParamTypeHint::String],
            ParamTypeHint::ClassName("StreamBucket".into()),
        ),
        (
            "stream_bucket_make_writeable",
            fn_bucket_make_writeable,
            vec!["brigade"],
            vec![ParamTypeHint::None],
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName("StreamBucket".into()))),
        ),
        (
            "stream_bucket_append",
            fn_bucket_append,
            vec!["brigade", "bucket"],
            vec![
                ParamTypeHint::None,
                ParamTypeHint::ClassName("StreamBucket".into()),
            ],
            ParamTypeHint::Void,
        ),
        (
            "stream_bucket_prepend",
            fn_bucket_prepend,
            vec!["brigade", "bucket"],
            vec![
                ParamTypeHint::None,
                ParamTypeHint::ClassName("StreamBucket".into()),
            ],
            ParamTypeHint::Void,
        ),
    ] {
        let count = names.len() as u32;
        let mut function = Box::new(make_internal_function(
            handler,
            count,
            count,
            names.into_iter().map(str::to_string).collect(),
        ));
        function.common.sig.param_type_hints = hints;
        function.common.sig.return_type_hint = result;
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(name, pointer).unwrap();
        eg.register_internal_function_reflection_metadata(
            pointer,
            vec![None; count as usize],
            "standard",
        );
        functions.push(function);
    }
}

fn internal_class(name: &str, final_class: bool) -> ClassDef {
    ClassDef {
        attributes: vec![],
        name: name.into(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: final_class,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    }
}

#[cold]
pub(in crate::stdlib) fn register_classes(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut base = internal_class("php_user_filter", false);
    for (name, hint, default) in [
        ("filtername", ParamTypeHint::String, Value::string("")),
        ("params", ParamTypeHint::Mixed, Value::string("")),
        ("stream", ParamTypeHint::None, Value::null()),
    ] {
        base.properties.push(PropertyDefinition::declared(
            name.into(),
            Some(default),
            Visibility::Public,
            base.name.clone(),
            hint,
            false,
            false,
        ));
    }
    eg.register_class(base).unwrap();
    let mut bucket = internal_class("StreamBucket", true);
    for (name, hint, default) in [
        ("bucket", ParamTypeHint::None, Some(Value::null())),
        ("data", ParamTypeHint::String, None),
        ("datalen", ParamTypeHint::Int, None),
        ("dataLength", ParamTypeHint::Int, None),
    ] {
        bucket.properties.push(PropertyDefinition::declared(
            name.into(),
            default,
            Visibility::Public,
            bucket.name.clone(),
            hint,
            false,
            false,
        ));
    }
    eg.register_class(bucket).unwrap();

    let mut functions = Vec::new();
    for (name, handler, names, hints, result, ref_args) in [
        (
            "filter",
            fn_default_filter as InternalFunctionHandler,
            vec!["in", "out", "consumed", "closing"],
            vec![
                ParamTypeHint::None,
                ParamTypeHint::None,
                ParamTypeHint::None,
                ParamTypeHint::Bool,
            ],
            ParamTypeHint::Int,
            0b100,
        ),
        (
            "onCreate",
            fn_default_create,
            vec![],
            vec![],
            ParamTypeHint::Bool,
            0,
        ),
        (
            "onClose",
            fn_default_close,
            vec![],
            vec![],
            ParamTypeHint::Void,
            0,
        ),
    ] {
        let count = names.len() as u32;
        eg.register_internal_method_contract(
            "php_user_filter",
            name,
            false,
            count,
            &names,
            hints.clone(),
            result.clone(),
            &vec![None; count as usize],
            true,
        );
        eg.register_internal_method_reference_arguments("php_user_filter", name, ref_args);
        let mut function = Box::new(make_internal_method(
            handler,
            count + 1,
            count,
            names.into_iter().map(str::to_string).collect(),
        ));
        function.common.sig.param_type_hints = hints;
        // Internal method masks use public argument positions, not hidden $this.
        function.common.sig.ref_args = ref_args;
        if ref_args != 0 {
            function.common.plan.call = crate::vm::function::CallStrategy::Full;
        }
        function.common.sig.return_type_hint = result;
        let pointer = &function.common as *const FunctionCommon;
        eg.function_table.insert(
            format!("php_user_filter::{name}").to_ascii_lowercase(),
            pointer,
        );
        eg.method_declaring_class
            .insert(pointer, "php_user_filter".into());
        functions.push(function);
    }
    functions
}

fn fn_default_filter(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    super::return_value(rv, Value::long(0))
}

fn fn_default_create(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    super::return_value(rv, Value::bool(true))
}

fn fn_default_close(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    super::return_value(rv, Value::null())
}

pub(super) fn registered_names(eg: &ExecutorGlobals) -> Value {
    let mut names = PhpArray::new();
    for (name, _) in BUILTIN_FILTERS {
        names.push(Value::string(*name));
    }
    if let Some(registered) = eg
        .static_vars
        .get(REGISTRY)
        .and_then(|state| state.get(ORDER))
        .and_then(Value::as_array)
    {
        for (_, name) in registered.iter() {
            names.push(name.clone());
        }
    }
    Value::array(names)
}

#[cold]
fn fn_register(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(name) = super::super::typed_internal_string_argument(
        ed,
        eg,
        "stream_filter_register",
        0,
        "filter_name",
    )?
    else {
        return Ok(());
    };
    let Some(class) =
        super::super::typed_internal_string_argument(ed, eg, "stream_filter_register", 1, "class")?
    else {
        return Ok(());
    };
    for (index, label, value) in [(1, "filter_name", &name), (2, "class", &class)] {
        if value.is_empty() {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                &format!(
                    "stream_filter_register(): Argument #{index} (${label}) must be a non-empty string"
                ),
            ));
            return Ok(());
        }
    }
    if builtin_filter(&name).is_some() {
        return super::return_value(rv, Value::bool(false));
    }
    let key = format!("filter:{name}");
    let state = eg.static_vars.entry(REGISTRY.into()).or_default();
    if state.contains_key(&key) {
        return super::return_value(rv, Value::bool(false));
    }
    state.insert(key, Value::string(class));
    let mut order = state
        .get(ORDER)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(PhpArray::new);
    order.push(Value::string(name));
    state.insert(ORDER.into(), Value::array(order));
    super::return_value(rv, Value::bool(true))
}

fn insert_resource<T: 'static>(eg: &mut ExecutorGlobals, kind: &'static str, value: T) -> Value {
    #[cfg(feature = "resource-lifetime")]
    {
        super::super::resource::insert_value_for_request(eg, kind, value)
    }
    #[cfg(not(feature = "resource-lifetime"))]
    {
        Value::resource(super::super::resource::insert_for_request(eg, kind, value))
    }
}

fn shared_payload<T: Clone + 'static>(eg: &mut ExecutorGlobals, value: &Value) -> Option<T> {
    super::super::resource::with_request_payload_mut::<T, _>(
        eg,
        value.as_resource_id()?,
        |payload| payload.clone(),
    )
}

fn bucket_object(eg: &ExecutorGlobals, handle: Value, bytes: &[u8]) -> Value {
    let class = eg
        .find_class("StreamBucket")
        .expect("stream classes registered before bucket creation");
    let mut object = PhpObject::with_layout_from_defaults(
        class.class_id,
        class.property_layout.clone(),
        class.property_defaults.as_ref(),
    );
    object.set_property("bucket", handle);
    object.set_property("data", super::super::php_byte_result(bytes.to_vec(), false));
    object.set_property("datalen", Value::long(bytes.len() as i64));
    object.set_property("dataLength", Value::long(bytes.len() as i64));
    Value::object(object)
}

fn valid_resource(
    eg: &mut ExecutorGlobals,
    value: &Value,
    function: &str,
    index: usize,
    name: &str,
    kind: &str,
) -> bool {
    if let Some(id) = value.as_resource_id() {
        if super::super::resource::is_open_for_request(eg, id)
            && super::super::resource::type_for_request(eg, id) == kind
        {
            return true;
        }
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "{function}(): supplied resource is not a valid {} resource",
                if kind == "stream" {
                    "stream"
                } else {
                    "Stream Bucket Brigade"
                }
            ),
        ));
    } else {
        super::super::typed_internal_argument_error(eg, function, value, index, name, "resource");
    }
    false
}

#[cold]
fn fn_bucket_new(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let stream = super::argument(ed, 0).clone();
    if !valid_resource(eg, &stream, "stream_bucket_new", 1, "stream", "stream") {
        return Ok(());
    }
    let Some(buffer) = super::super::typed_internal_string_value_argument_expected(
        ed,
        eg,
        "stream_bucket_new",
        1,
        "buffer",
        "string",
    )?
    else {
        return Ok(());
    };
    let bytes = super::super::php_bytes_after_weak_string_coercion(&buffer)
        .0
        .into_owned();
    let handle = insert_resource(
        eg,
        "stream bucket",
        Rc::new(RefCell::new(Bucket {
            bytes: bytes.clone(),
            owner: Weak::new(),
        })),
    );
    super::return_value(rv, bucket_object(eg, handle, &bytes))
}

#[cold]
fn fn_bucket_make_writeable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = super::argument(ed, 0).clone();
    let Some(brigade) = shared_payload::<SharedBrigade>(eg, &value) else {
        valid_resource(
            eg,
            &value,
            "stream_bucket_make_writeable",
            1,
            "brigade",
            "stream bucket brigade",
        );
        return Ok(());
    };
    let Some(handle) = brigade.borrow_mut().buckets.pop_front() else {
        return super::return_value(rv, Value::null());
    };
    let bucket = shared_payload::<SharedBucket>(eg, &handle).expect("brigade owns live bucket");
    bucket.borrow_mut().owner = Weak::new();
    let bytes = bucket.borrow().bytes.clone();
    super::return_value(rv, bucket_object(eg, handle, &bytes))
}

fn move_bucket(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    prepend: bool,
) -> Result<(), VmError> {
    let function = if prepend {
        "stream_bucket_prepend"
    } else {
        "stream_bucket_append"
    };
    let value = super::argument(ed, 0).clone();
    let Some(brigade) = shared_payload::<SharedBrigade>(eg, &value) else {
        valid_resource(eg, &value, function, 1, "brigade", "stream bucket brigade");
        return Ok(());
    };
    let object_value = super::argument(ed, 1).clone();
    let Some(object) = object_value
        .as_object()
        .filter(|object| object.class_name.eq_ignore_ascii_case("StreamBucket"))
    else {
        super::super::typed_internal_argument_error(
            eg,
            function,
            &object_value,
            2,
            "bucket",
            "StreamBucket",
        );
        return Ok(());
    };
    let handle = object
        .get_property("bucket")
        .cloned()
        .unwrap_or_else(Value::null);
    let bytes = object
        .get_property("data")
        .map(|value| {
            super::super::php_bytes_after_weak_string_coercion(value)
                .0
                .into_owned()
        })
        .unwrap_or_default();
    drop(object);
    let Some(bucket) = shared_payload::<SharedBucket>(eg, &handle) else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("{function}(): Invalid bucket"),
        ));
        return Ok(());
    };
    if let Some(previous) = bucket.borrow().owner.upgrade() {
        previous
            .borrow_mut()
            .buckets
            .retain(|value| value.as_resource_id() != handle.as_resource_id());
    }
    {
        let mut state = bucket.borrow_mut();
        state.bytes = bytes;
        state.owner = Rc::downgrade(&brigade);
    }
    if prepend {
        brigade.borrow_mut().buckets.push_front(handle);
    } else {
        brigade.borrow_mut().buckets.push_back(handle);
    }
    super::return_value(rv, Value::null())
}

#[cold]
fn fn_bucket_append(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    move_bucket(ed, rv, eg, false)
}

#[cold]
fn fn_bucket_prepend(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    move_bucket(ed, rv, eg, true)
}
