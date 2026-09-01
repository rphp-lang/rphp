//! Request-local user-space stream-wrapper registry and callback dispatch.
//!
//! The ordinary `PhpStream` and filesystem paths never consult this module.
//! State is allocated only after the first successful user registration and
//! live handles retain their wrapper object independently of unregistering the
//! protocol, matching PHP's re-entrant lifecycle boundary.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, PhpObject, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

const REGISTRY_STATE: &str = "\0rphp-user-stream-wrapper-registry";
const ORDER_KEY: &str = "\0order";
const CUSTOM_PREFIX: &str = "custom:";
const DISABLED_PREFIX: &str = "disabled:";
const OPEN_PREFIX: &str = "open:";
const USER_READ_SIZE: i64 = 8192;

const BUILTIN_WRAPPERS: &[&str] = &["php", "file"];

#[derive(Clone)]
pub(crate) struct WrapperDefinition {
    pub(crate) scheme: String,
    pub(crate) class: String,
    pub(crate) flags: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UserStreamKind {
    File,
    Directory,
}

struct UserStreamState {
    object: Value,
    uri: String,
    opened_path: String,
    mode: String,
    kind: UserStreamKind,
    is_url: bool,
    position: usize,
    eof: bool,
    unread: Vec<u8>,
    unread_offset: usize,
    closed: bool,
}

type SharedUserStream = Rc<RefCell<UserStreamState>>;

pub(crate) enum OpenResult {
    NotRegistered,
    Declined { class: String },
    Opened(Value),
}

pub(crate) enum IncludeOpenResult {
    NotRegistered,
    Declined { class: String },
    Opened { source: Vec<u8>, canonical: String },
}

fn custom_key(protocol: &str) -> String {
    format!("{CUSTOM_PREFIX}{protocol}")
}

fn disabled_key(protocol: &str) -> String {
    format!("{DISABLED_PREFIX}{protocol}")
}

fn open_key(resource: i64) -> String {
    format!("{OPEN_PREFIX}{resource}")
}

fn registry(eg: &ExecutorGlobals) -> Option<&std::collections::HashMap<String, Value>> {
    eg.static_vars.get(REGISTRY_STATE)
}

fn definition_value(definition: &WrapperDefinition) -> Value {
    let mut value = PhpArray::with_packed_capacity(3);
    value.push(Value::string(definition.scheme.clone()));
    value.push(Value::string(definition.class.clone()));
    value.push(Value::long(definition.flags));
    Value::array(value)
}

fn definition_from_value(value: &Value) -> Option<WrapperDefinition> {
    let value = value.as_array()?;
    Some(WrapperDefinition {
        scheme: value.get_value_at(0)?.as_str()?.to_string(),
        class: value.get_value_at(1)?.as_str()?.to_string(),
        flags: value.get_value_at(2)?.as_long()?,
    })
}

pub(crate) fn definition_for_protocol(
    eg: &ExecutorGlobals,
    protocol: &str,
) -> Option<WrapperDefinition> {
    registry(eg)
        .and_then(|state| state.get(&custom_key(protocol)))
        .and_then(definition_from_value)
}

pub(crate) fn protocol_from_url(url: &str) -> Option<&str> {
    let separator = url.find("://")?;
    Some(&url[..separator])
}

pub(crate) fn definition_for_url(eg: &ExecutorGlobals, url: &str) -> Option<WrapperDefinition> {
    let first = *url.as_bytes().first()?;
    if !first.is_ascii_alphanumeric() && !matches!(first, b'+' | b'-' | b'.') {
        return None;
    }
    let state = registry(eg)?;
    let protocol = protocol_from_url(url)?;
    state
        .get(&custom_key(protocol))
        .and_then(definition_from_value)
}

fn is_builtin_enabled(eg: &ExecutorGlobals, protocol: &str) -> bool {
    BUILTIN_WRAPPERS.contains(&protocol)
        && registry(eg).is_none_or(|state| !state.contains_key(&disabled_key(protocol)))
}

pub(crate) fn wrappers(eg: &ExecutorGlobals) -> Vec<String> {
    let mut values = BUILTIN_WRAPPERS
        .iter()
        .filter(|protocol| is_builtin_enabled(eg, protocol))
        .map(|protocol| (*protocol).to_string())
        .collect::<Vec<_>>();
    let Some(order) = registry(eg)
        .and_then(|state| state.get(ORDER_KEY))
        .and_then(Value::as_array)
    else {
        return values;
    };
    values.extend(order.values().filter_map(Value::as_str).map(str::to_string));
    values
}

fn append_protocol(eg: &mut ExecutorGlobals, protocol: &str) {
    let previous = registry(eg)
        .and_then(|state| state.get(ORDER_KEY))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(PhpArray::new);
    let mut order = previous;
    order.push(Value::string(protocol));
    eg.static_vars
        .entry(REGISTRY_STATE.to_string())
        .or_default()
        .insert(ORDER_KEY.to_string(), Value::array(order));
}

fn remove_protocol_from_order(eg: &mut ExecutorGlobals, protocol: &str) {
    let retained = registry(eg)
        .and_then(|state| state.get(ORDER_KEY))
        .and_then(Value::as_array)
        .map(|order| {
            order
                .values()
                .filter_map(Value::as_str)
                .filter(|registered| *registered != protocol)
                .map(|registered| Value::string(registered.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut order = PhpArray::with_packed_capacity(retained.len());
    for value in retained {
        order.push(value);
    }
    if let Some(state) = eg.static_vars.get_mut(REGISTRY_STATE) {
        state.insert(ORDER_KEY.to_string(), Value::array(order));
    }
}

fn valid_protocol(protocol: &str) -> bool {
    protocol
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn callback_descriptor(object: &Value, method: &str) -> Value {
    let mut descriptor = PhpArray::with_packed_capacity(2);
    descriptor.push(object.clone());
    descriptor.push(Value::string(method));
    Value::array(descriptor)
}

fn invoke_callback(
    eg: &mut ExecutorGlobals,
    object: &Value,
    method: &str,
    arguments: Vec<Value>,
) -> Result<Option<Value>, VmError> {
    let callback = callback_descriptor(object, method);
    let Some(resolved) = crate::stdlib::resolve_callback_with_cache(&callback, eg, None, None)
    else {
        return Ok(None);
    };
    let num_args = resolved.prepend_args.len() + arguments.len() + resolved.use_vars.len();
    let values = resolved
        .prepend_args
        .iter()
        .cloned()
        .chain(arguments)
        .chain(resolved.use_vars.iter().map(Value::clone_closure_capture));
    crate::stdlib::call_resolved_owned_iter(eg, &resolved, num_args, values).map(Some)
}

fn empty_context(eg: &mut ExecutorGlobals) -> Value {
    #[cfg(feature = "stream-context")]
    {
        let context = super::super::stream::StreamContext {
            options: PhpArray::new(),
            params: PhpArray::new(),
        };
        #[cfg(feature = "resource-lifetime")]
        return super::super::resource::insert_value_for_request(eg, "stream-context", context);
        #[cfg(not(feature = "resource-lifetime"))]
        return Value::resource(super::super::resource::insert_for_request(
            eg,
            "stream-context",
            context,
        ));
    }
    #[cfg(not(feature = "stream-context"))]
    {
        let _ = eg;
        Value::null()
    }
}

fn instantiate_wrapper(
    eg: &mut ExecutorGlobals,
    definition: &WrapperDefinition,
) -> Result<Value, VmError> {
    let Some(class) = eg.find_class(&definition.class) else {
        return Ok(Value::null());
    };
    let class_name = class.name.clone();
    let class_id = class.class_id;
    let property_layout = class.property_layout.clone();
    let property_defaults = class.property_defaults.clone();
    let object = if class.class_id == 0 {
        Value::object(PhpObject::dynamic(
            class_name.clone(),
            0,
            std::collections::HashMap::new(),
        ))
    } else {
        Value::object(PhpObject::with_layout_from_defaults(
            class_id,
            property_layout,
            property_defaults.as_ref(),
        ))
    };
    let context = empty_context(eg);
    if let Some(mut object_state) = object.as_object_mut() {
        object_state.set_property("context", context);
    }
    if let Some((_, is_static, function, _)) =
        crate::stdlib::find_method_in_class_hierarchy(eg, &class_name, "__construct")
        && !is_static
    {
        let resolved = crate::stdlib::ResolvedCallback {
            func_ptr: function,
            prepend_args: vec![object.clone()],
            use_vars: vec![],
            called_scope_class_id: class_id,
            bound_this: Some(object.clone()),
            closure_static_vars: None,
            is_magic_call: false,
        };
        let values = resolved.prepend_args.iter().cloned();
        let _ = crate::stdlib::call_resolved_owned_iter(eg, &resolved, 1, values)?;
    }
    Ok(object)
}

#[cfg(feature = "resource-lifetime")]
fn insert_user_stream(eg: &mut ExecutorGlobals, state: UserStreamState) -> Value {
    super::super::resource::insert_value_for_request(
        eg,
        "stream",
        Rc::new(RefCell::new(state)) as SharedUserStream,
    )
}

#[cfg(not(feature = "resource-lifetime"))]
fn insert_user_stream(eg: &mut ExecutorGlobals, state: UserStreamState) -> Value {
    Value::resource(super::super::resource::insert_for_request(
        eg,
        "stream",
        Rc::new(RefCell::new(state)) as SharedUserStream,
    ))
}

fn retain_open_resource(eg: &mut ExecutorGlobals, resource: i64) {
    // The VM normally skips the shutdown-function phase when its queue was
    // never allocated. A live user wrapper still needs the same request-end
    // close boundary, so publish an empty queue as the existing cold marker.
    eg.shutdown_functions
        .get_or_insert_with(|| Box::new(std::collections::VecDeque::new()));
    eg.static_vars
        .entry(REGISTRY_STATE.to_string())
        .or_default()
        .insert(open_key(resource), Value::long(resource));
}

fn forget_open_resource(eg: &mut ExecutorGlobals, resource: i64) {
    if let Some(state) = eg.static_vars.get_mut(REGISTRY_STATE) {
        state.remove(&open_key(resource));
    }
}

fn shared_stream(eg: &mut ExecutorGlobals, resource: i64) -> Option<SharedUserStream> {
    super::super::resource::with_request_payload_mut::<SharedUserStream, _>(
        eg,
        resource,
        |stream| Rc::clone(stream),
    )
}

pub(crate) fn is_user_stream(eg: &mut ExecutorGlobals, resource: i64) -> bool {
    shared_stream(eg, resource).is_some()
}

pub(crate) fn is_local_stream(eg: &mut ExecutorGlobals, resource: i64) -> Option<bool> {
    let stream = shared_stream(eg, resource)?;
    Some(!stream.borrow().is_url)
}

pub(crate) fn is_local_url(eg: &ExecutorGlobals, url: &str) -> Option<bool> {
    definition_for_url(eg, url).map(|definition| definition.flags & 1 == 0)
}

pub(crate) fn is_user_directory(eg: &mut ExecutorGlobals, resource: i64) -> bool {
    shared_stream(eg, resource)
        .is_some_and(|stream| stream.borrow().kind == UserStreamKind::Directory)
}

fn open(
    eg: &mut ExecutorGlobals,
    path: &str,
    mode: &str,
    options: i64,
    kind: UserStreamKind,
) -> Result<OpenResult, VmError> {
    let Some(definition) = definition_for_url(eg, path) else {
        return Ok(OpenResult::NotRegistered);
    };
    let object = instantiate_wrapper(eg, &definition)?;
    if eg.exception.is_some() || object.value_type() != ValueType::Object {
        return Ok(OpenResult::Declined {
            class: definition.class,
        });
    }
    let opened_path = Value::owned_reference(Value::null());
    let opened_argument = opened_path.clone_owned_reference_alias();
    let method = if kind == UserStreamKind::File {
        "stream_open"
    } else {
        "dir_opendir"
    };
    let arguments = if kind == UserStreamKind::File {
        vec![
            Value::string(path),
            Value::string(mode),
            Value::long(options),
            opened_argument,
        ]
    } else {
        vec![Value::string(path), Value::long(options)]
    };
    let result = invoke_callback(eg, &object, method, arguments)?;
    if eg.exception.is_some() {
        return Ok(OpenResult::Declined {
            class: definition.class,
        });
    }
    if result.as_ref().is_none_or(|value| !value.is_truthy()) {
        return Ok(OpenResult::Declined {
            class: definition.class,
        });
    }
    let canonical = opened_path
        .dereferenced()
        .as_str()
        .filter(|path| !path.is_empty())
        .unwrap_or(path)
        .to_string();
    let value = insert_user_stream(
        eg,
        UserStreamState {
            object,
            uri: path.to_string(),
            opened_path: canonical,
            mode: mode.to_string(),
            kind,
            is_url: definition.flags & 1 != 0,
            position: 0,
            eof: false,
            unread: Vec::new(),
            unread_offset: 0,
            closed: false,
        },
    );
    let resource = value
        .as_resource_id()
        .expect("new user stream must be a resource");
    retain_open_resource(eg, resource);
    Ok(OpenResult::Opened(value))
}

pub(crate) fn open_file(
    eg: &mut ExecutorGlobals,
    path: &str,
    mode: &str,
    options: i64,
) -> Result<OpenResult, VmError> {
    open(eg, path, mode, options, UserStreamKind::File)
}

pub(crate) fn open_directory(
    eg: &mut ExecutorGlobals,
    path: &str,
    options: i64,
) -> Result<OpenResult, VmError> {
    open(eg, path, "", options, UserStreamKind::Directory)
}

fn invoke_on_stream(
    eg: &mut ExecutorGlobals,
    resource: i64,
    method: &str,
    arguments: Vec<Value>,
) -> Result<Option<Value>, VmError> {
    let Some(stream) = shared_stream(eg, resource) else {
        return Ok(None);
    };
    let object = stream.borrow().object.clone();
    invoke_callback(eg, &object, method, arguments)
}

pub(crate) fn read(
    eg: &mut ExecutorGlobals,
    resource: i64,
    requested: usize,
) -> Result<Option<Vec<u8>>, VmError> {
    let Some(stream) = shared_stream(eg, resource) else {
        return Ok(None);
    };
    if stream.borrow().kind != UserStreamKind::File {
        return Ok(None);
    }

    loop {
        let available = {
            let state = stream.borrow();
            state.unread.len().saturating_sub(state.unread_offset)
        };
        if available >= requested || stream.borrow().eof {
            break;
        }
        let object = stream.borrow().object.clone();
        let Some(value) = invoke_callback(
            eg,
            &object,
            "stream_read",
            vec![Value::long(USER_READ_SIZE)],
        )?
        else {
            return Ok(None);
        };
        if eg.exception.is_some() {
            return Ok(None);
        }
        let bytes = crate::stdlib::php_string_to_bytes(&value.echo_to_string());
        let read_empty = bytes.is_empty();
        {
            let mut state = stream.borrow_mut();
            if state.unread_offset == state.unread.len() {
                state.unread.clear();
                state.unread_offset = 0;
            }
            state.unread.extend_from_slice(&bytes);
        }
        let eof = invoke_callback(eg, &object, "stream_eof", vec![])?
            .is_some_and(|value| value.is_truthy());
        stream.borrow_mut().eof = eof;
        if read_empty || eof {
            break;
        }
    }

    let mut state = stream.borrow_mut();
    let available = state.unread.len().saturating_sub(state.unread_offset);
    let length = requested.min(available);
    let start = state.unread_offset;
    let end = start + length;
    let bytes = state.unread[start..end].to_vec();
    state.unread_offset = end;
    state.position = state.position.saturating_add(length);
    if state.unread_offset == state.unread.len() {
        state.unread.clear();
        state.unread_offset = 0;
    }
    Ok(Some(bytes))
}

pub(crate) fn eof(eg: &mut ExecutorGlobals, resource: i64) -> Result<Option<bool>, VmError> {
    let Some(stream) = shared_stream(eg, resource) else {
        return Ok(None);
    };
    let object = stream.borrow().object.clone();
    let eof =
        invoke_callback(eg, &object, "stream_eof", vec![])?.is_some_and(|value| value.is_truthy());
    stream.borrow_mut().eof = eof;
    Ok(Some(eof))
}

pub(crate) fn position(eg: &mut ExecutorGlobals, resource: i64) -> Option<i64> {
    let stream = shared_stream(eg, resource)?;
    i64::try_from(stream.borrow().position).ok()
}

pub(crate) fn cached_eof(eg: &mut ExecutorGlobals, resource: i64) -> Option<bool> {
    let stream = shared_stream(eg, resource)?;
    Some(stream.borrow().eof)
}

pub(crate) fn flush(eg: &mut ExecutorGlobals, resource: i64) -> Result<Option<bool>, VmError> {
    let Some(stream) = shared_stream(eg, resource) else {
        return Ok(None);
    };
    let object = stream.borrow().object.clone();
    Ok(Some(
        invoke_callback(eg, &object, "stream_flush", vec![])?
            .is_some_and(|value| value.is_truthy()),
    ))
}

pub(crate) fn metadata(eg: &mut ExecutorGlobals, resource: i64) -> Result<Option<Value>, VmError> {
    let Some(stream) = shared_stream(eg, resource) else {
        return Ok(None);
    };
    let _ = eof(eg, resource)?;
    if eg.exception.is_some() {
        return Ok(None);
    }
    let state = stream.borrow();
    let mut result = PhpArray::with_hash_capacity(10);
    result.set_str("timed_out", Value::bool(false));
    result.set_str("blocked", Value::bool(true));
    result.set_str("eof", Value::bool(state.eof));
    result.set_str("wrapper_data", state.object.clone());
    result.set_str("wrapper_type", Value::string("user-space"));
    result.set_str("stream_type", Value::string("user-space"));
    result.set_str("mode", Value::string(state.mode.clone()));
    result.set_str("unread_bytes", Value::long(0));
    result.set_str("seekable", Value::bool(true));
    result.set_str("uri", Value::string(state.uri.clone()));
    Ok(Some(Value::array(result)))
}

fn discard_resource(eg: &mut ExecutorGlobals, resource: i64) {
    forget_open_resource(eg, resource);
    let _ = super::super::resource::close_for_request::<SharedUserStream>(eg, resource);
}

pub(crate) fn close(eg: &mut ExecutorGlobals, resource: i64) -> Result<Option<bool>, VmError> {
    let Some(stream) = shared_stream(eg, resource) else {
        return Ok(None);
    };
    let (object, method) = {
        let mut state = stream.borrow_mut();
        if state.closed {
            return Ok(Some(false));
        }
        state.closed = true;
        (
            state.object.clone(),
            if state.kind == UserStreamKind::File {
                "stream_close"
            } else {
                "dir_closedir"
            },
        )
    };
    let result = invoke_callback(eg, &object, method, vec![]);
    discard_resource(eg, resource);
    result.map(|_| Some(true))
}

pub(crate) fn directory_read(
    eg: &mut ExecutorGlobals,
    resource: i64,
) -> Result<Option<Option<String>>, VmError> {
    let Some(stream) = shared_stream(eg, resource) else {
        return Ok(None);
    };
    if stream.borrow().kind != UserStreamKind::Directory {
        return Ok(None);
    }
    let object = stream.borrow().object.clone();
    let value = invoke_callback(eg, &object, "dir_readdir", vec![])?;
    Ok(Some(value.and_then(|value| {
        (value.value_type() != ValueType::False).then(|| value.echo_to_string())
    })))
}

pub(crate) fn directory_rewind(
    eg: &mut ExecutorGlobals,
    resource: i64,
) -> Result<Option<()>, VmError> {
    let Some(stream) = shared_stream(eg, resource) else {
        return Ok(None);
    };
    if stream.borrow().kind != UserStreamKind::Directory {
        return Ok(None);
    }
    let object = stream.borrow().object.clone();
    let _ = invoke_callback(eg, &object, "dir_rewinddir", vec![])?;
    Ok(Some(()))
}

pub(crate) fn url_stat_value(
    eg: &mut ExecutorGlobals,
    path: &str,
    flags: i64,
) -> Result<Option<Value>, VmError> {
    let Some(definition) = definition_for_url(eg, path) else {
        return Ok(None);
    };
    let object = instantiate_wrapper(eg, &definition)?;
    let value = invoke_callback(
        eg,
        &object,
        "url_stat",
        vec![Value::string(path), Value::long(flags)],
    )?;
    Ok(Some(value.unwrap_or_else(|| Value::bool(false))))
}

pub(crate) fn url_stat(
    eg: &mut ExecutorGlobals,
    path: &str,
    flags: i64,
) -> Result<Option<bool>, VmError> {
    Ok(url_stat_value(eg, path, flags)?.map(|value| value.is_truthy()))
}

pub(crate) fn open_include_source(
    eg: &mut ExecutorGlobals,
    path: &str,
) -> Result<IncludeOpenResult, VmError> {
    let opened = open_file(eg, path, "rb", 65_665)?;
    let OpenResult::Opened(value) = opened else {
        return Ok(match opened {
            OpenResult::NotRegistered => IncludeOpenResult::NotRegistered,
            OpenResult::Declined { class } => IncludeOpenResult::Declined { class },
            OpenResult::Opened(_) => unreachable!(),
        });
    };
    let resource = value
        .as_resource_id()
        .expect("opened user include stream has a resource id");
    let canonical = shared_stream(eg, resource)
        .map(|stream| stream.borrow().opened_path.clone())
        .unwrap_or_else(|| path.to_string());
    let _ = invoke_on_stream(
        eg,
        resource,
        "stream_set_option",
        vec![Value::long(2), Value::long(0), Value::long(USER_READ_SIZE)],
    )?;
    if eg.exception.is_some() {
        discard_resource(eg, resource);
        return Ok(IncludeOpenResult::Opened {
            source: Vec::new(),
            canonical,
        });
    }
    let _ = invoke_on_stream(eg, resource, "stream_stat", vec![])?;
    if eg.exception.is_some() {
        discard_resource(eg, resource);
        return Ok(IncludeOpenResult::Opened {
            source: Vec::new(),
            canonical,
        });
    }

    let mut source = Vec::new();
    loop {
        match read(eg, resource, USER_READ_SIZE as usize)? {
            Some(bytes) => source.extend_from_slice(&bytes),
            None => {
                discard_resource(eg, resource);
                return Ok(IncludeOpenResult::Opened { source, canonical });
            }
        }
        if eg.exception.is_some() {
            discard_resource(eg, resource);
            return Ok(IncludeOpenResult::Opened { source, canonical });
        }
        let eof = shared_stream(eg, resource).is_none_or(|stream| stream.borrow().eof);
        if eof {
            break;
        }
    }
    let _ = close(eg, resource)?;
    Ok(IncludeOpenResult::Opened { source, canonical })
}

pub(crate) fn shutdown_open_streams(eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let mut resources = registry(eg)
        .map(|state| {
            state
                .iter()
                .filter(|(key, _)| key.starts_with(OPEN_PREFIX))
                .filter_map(|(_, value)| value.as_long())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    resources.sort_unstable();
    for resource in resources.into_iter().rev() {
        match close(eg, resource) {
            Ok(_) => {}
            Err(VmError::Fatal(message)) => {
                // PHP continues request resource teardown after a bailout from
                // one user wrapper's close callback. Preserve each fatal in
                // order while still giving every remaining wrapper one close.
                eg.write_output(format!("\nFatal error: {message}\n").as_bytes());
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(super) fn fn_stream_wrapper_register(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(protocol) = crate::stdlib::typed_internal_string_argument(
        ed,
        eg,
        "stream_wrapper_register",
        0,
        "protocol",
    )?
    else {
        return Ok(());
    };
    let Some(class_name) = crate::stdlib::typed_internal_string_argument(
        ed,
        eg,
        "stream_wrapper_register",
        1,
        "class",
    )?
    else {
        return Ok(());
    };
    let flags = if super::optional_argument(ed, 2).is_some() {
        let Some(flags) = crate::stdlib::typed_internal_int_argument(
            ed,
            eg,
            "stream_wrapper_register",
            2,
            "flags",
        )?
        else {
            return Ok(());
        };
        flags
    } else {
        0
    };
    if !valid_protocol(&protocol) {
        crate::stdlib::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!(
                "stream_wrapper_register(): Invalid protocol scheme specified. Unable to register wrapper class {class_name} to {protocol}://"
            ),
        )?;
        return super::return_value(rv, Value::bool(false));
    }
    if is_builtin_enabled(eg, &protocol) || definition_for_protocol(eg, &protocol).is_some() {
        crate::stdlib::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!("stream_wrapper_register(): Protocol {protocol}:// is already defined."),
        )?;
        return super::return_value(rv, Value::bool(false));
    }
    if eg.find_class(&class_name).is_none() {
        let _ = crate::stdlib::autoload::ensure_symbol_loaded(eg, &class_name)?;
    }
    let Some(canonical) = eg.find_class(&class_name).map(|class| class.name.clone()) else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "stream_wrapper_register(): Argument #2 ($class) must be a valid class name, {class_name} given"
            ),
        ));
        return Ok(());
    };
    let definition = WrapperDefinition {
        scheme: protocol.clone(),
        class: canonical,
        flags,
    };
    eg.static_vars
        .entry(REGISTRY_STATE.to_string())
        .or_default()
        .insert(custom_key(&protocol), definition_value(&definition));
    append_protocol(eg, &protocol);
    super::return_value(rv, Value::bool(true))
}

pub(super) fn fn_stream_wrapper_unregister(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(protocol) = crate::stdlib::typed_internal_string_argument(
        ed,
        eg,
        "stream_wrapper_unregister",
        0,
        "protocol",
    )?
    else {
        return Ok(());
    };
    if definition_for_protocol(eg, &protocol).is_some() {
        if let Some(state) = eg.static_vars.get_mut(REGISTRY_STATE) {
            state.remove(&custom_key(&protocol));
        }
        remove_protocol_from_order(eg, &protocol);
        return super::return_value(rv, Value::bool(true));
    }
    if is_builtin_enabled(eg, &protocol) {
        eg.static_vars
            .entry(REGISTRY_STATE.to_string())
            .or_default()
            .insert(disabled_key(&protocol), Value::bool(true));
        return super::return_value(rv, Value::bool(true));
    }
    crate::stdlib::report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!("stream_wrapper_unregister(): Unable to unregister protocol {protocol}://"),
    )?;
    super::return_value(rv, Value::bool(false))
}

pub(super) fn fn_stream_wrapper_restore(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(protocol) = crate::stdlib::typed_internal_string_argument(
        ed,
        eg,
        "stream_wrapper_restore",
        0,
        "protocol",
    )?
    else {
        return Ok(());
    };
    if BUILTIN_WRAPPERS.contains(&protocol.as_str()) {
        let restored = eg.static_vars.get_mut(REGISTRY_STATE).is_some_and(|state| {
            let restored = state.remove(&disabled_key(&protocol)).is_some();
            state.remove(&custom_key(&protocol));
            restored
        });
        if restored {
            remove_protocol_from_order(eg, &protocol);
            return super::return_value(rv, Value::bool(true));
        }
        crate::stdlib::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!(
                "stream_wrapper_restore(): {protocol}:// was never changed, nothing to restore"
            ),
        )?;
        return super::return_value(rv, Value::bool(true));
    }
    crate::stdlib::report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!("stream_wrapper_restore(): {protocol}:// never existed, nothing to restore"),
    )?;
    super::return_value(rv, Value::bool(false))
}
