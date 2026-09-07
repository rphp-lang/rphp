//! Filtered resources own their backend, but only weakly observe the PHP
//! handle. No registry or stream borrow survives entry into user code.

use super::*;
use crate::stdlib::stream::PhpStream;
use crate::stdlib::{self, resource};
use std::io::{self, SeekFrom};

const OPEN: &str = "\0rphp-open-filtered-streams";
const ORPHANS: &str = "\0rphp-failed-filter-creation";
const READ: i64 = 1;
const WRITE: i64 = 2;
type SharedStream = Rc<RefCell<FilteredStream>>;
type SharedFilter = Rc<RefCell<Filter>>;

struct FilteredStream {
    backend: PhpStream,
    owner: crate::value::WeakResourceValue,
    read: Vec<FilterLink>,
    write: Vec<FilterLink>,
    unread: VecDeque<u8>,
    position: Option<u64>,
    read_closed: bool,
    busy: bool,
    closed: bool,
}

struct FilterLink {
    // The chain, not a temporary returned by append(), owns the live filter
    // resource. The payload itself never owns its handle (no registry cycle).
    handle: Value,
    filter: SharedFilter,
}

struct Filter {
    object: Option<Value>,
    builtin: Option<BuiltinFilter>,
    stream: Weak<RefCell<FilteredStream>>,
    direction: i64,
    resource: i64,
}

#[cold]
pub(super) fn register_functions(
    eg: &mut ExecutorGlobals,
    functions: &mut Vec<Box<InternalFunction>>,
) {
    for (name, handler, remove) in [
        (
            "stream_filter_append",
            fn_append as InternalFunctionHandler,
            false,
        ),
        ("stream_filter_prepend", fn_prepend, false),
        ("stream_filter_remove", fn_remove, true),
    ] {
        let (names, required, hints, defaults) = if remove {
            (
                vec!["stream_filter"],
                1,
                vec![ParamTypeHint::None],
                vec![None],
            )
        } else {
            (
                vec!["stream", "filter_name", "mode", "params"],
                2,
                vec![
                    ParamTypeHint::None,
                    ParamTypeHint::String,
                    ParamTypeHint::Int,
                    ParamTypeHint::Mixed,
                ],
                vec![None, None, Some(Value::long(0)), Some(Value::null())],
            )
        };
        let mut function = Box::new(make_internal_function(
            handler,
            names.len() as u32,
            required,
            names.into_iter().map(str::to_owned).collect(),
        ));
        function.common.sig.param_type_hints = hints;
        function.common.sig.return_type_hint = if remove {
            ParamTypeHint::Bool
        } else {
            ParamTypeHint::None
        };
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(name, pointer).unwrap();
        eg.register_internal_function_reflection_metadata(pointer, defaults, "standard");
        functions.push(function);
    }
}

fn stream(eg: &mut ExecutorGlobals, id: i64) -> Option<SharedStream> {
    resource::with_request_payload_mut::<SharedStream, _>(eg, id, |state| state.clone())
}

pub(super) fn warn(eg: &mut ExecutorGlobals, message: &str) -> Result<(), VmError> {
    let (file, line) = super::diagnostic_source(eg);
    stdlib::report_diagnostic_from(
        eg,
        eg.current_execute_data.get(),
        &file,
        line,
        2,
        "Warning",
        message,
    )
    .map(|_| ())
}

fn warn_unprocessed_input(eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    // PHP suppresses entry into an eligible warning handler while unwinding
    // a callback/type error. Invoking it would consume the pending exception.
    // With no eligible handler, normal reporting still records the warning.
    if eg.exception.is_some() && eg.error_handler.is_some() && eg.error_handler_levels & 2 != 0 {
        return Ok(());
    }
    let message = format!(
        "{}(): Unprocessed filter buckets remaining on input brigade",
        super::diagnostic_function(eg),
    );
    warn(eg, &message)
}

fn keep_shutdown_phase(eg: &mut ExecutorGlobals) {
    eg.shutdown_functions
        .get_or_insert_with(|| Box::new(VecDeque::new()));
}

fn class_for_filter(eg: &ExecutorGlobals, name: &str) -> Option<String> {
    let registry = eg.static_vars.get(REGISTRY)?;
    if let Some(class) = registry
        .get(&format!("filter:{name}"))
        .and_then(Value::as_str)
    {
        return Some(class.into());
    }
    let mut prefix = name;
    while let Some((head, _)) = prefix.rsplit_once('.') {
        if let Some(class) = registry
            .get(&format!("filter:{head}.*"))
            .and_then(Value::as_str)
        {
            return Some(class.into());
        }
        prefix = head;
    }
    registry
        .get("filter:*")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Internal protocol projections address the actual declared slot, including
/// private storage. Missing `stream` is not manufactured as a dynamic field.
/// The canonical write machinery preserves hooks, reference constraints and
/// displaced-value cleanup. Uninitialized stored stream slots are left alone.
fn project(
    eg: &mut ExecutorGlobals,
    object: &Value,
    name: &str,
    value: Value,
    existing_only: bool,
) -> Result<bool, VmError> {
    let (file, line) = super::diagnostic_source(eg);
    crate::vm::execute::assign_internal_object_property(
        eg,
        object,
        name,
        value,
        existing_only,
        (&file, line),
    )
}

fn attach(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    prepend: bool,
) -> Result<(), VmError> {
    let function = if prepend {
        "stream_filter_prepend"
    } else {
        "stream_filter_append"
    };
    let owner = super::super::argument(ed, 0).clone();
    if !valid_resource(eg, &owner, function, 1, "stream", "stream") {
        return Ok(());
    }
    let Some(name) = stdlib::typed_internal_string_argument(ed, eg, function, 1, "filter_name")?
    else {
        return Ok(());
    };
    let mode = if super::super::optional_argument(ed, 2).is_some() {
        let Some(mode) = stdlib::typed_internal_int_argument(ed, eg, function, 2, "mode")? else {
            return Ok(());
        };
        mode
    } else {
        0
    };
    let params = super::super::optional_argument(ed, 3)
        .cloned()
        .unwrap_or_else(Value::null);
    let value = attach_value(eg, ed, &owner, &name, mode, params, prepend, function, true)?;
    super::super::return_value(rv, value)
}

/// URI opening and explicit attachment use the same filter factory. Only an
/// explicit attachment retains failed onCreate objects for request teardown;
/// an opening stream has not published ownership to PHP yet.
#[cold]
pub(super) fn attach_value(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    owner: &Value,
    name: &str,
    mode: i64,
    params: Value,
    prepend: bool,
    function: &str,
    retain_failed_creation: bool,
) -> Result<Value, VmError> {
    let builtin = super::builtin_filter(name);
    let class_id = if builtin.is_some() {
        None
    } else {
        let Some(class_name) = class_for_filter(eg, name) else {
            warn(
                eg,
                &format!("{function}(): Unable to locate filter \"{name}\""),
            )?;
            return Ok(Value::bool(false));
        };
        if eg.find_class(&class_name).is_none()
            && !stdlib::autoload::ensure_symbol_loaded(eg, &class_name)?
        {
            if eg.exception.is_some() {
                return Ok(Value::null());
            }
            warn(
                eg,
                &format!(
                    "{function}(): User-filter \"{name}\" requires class \"{class_name}\", but that class is not defined"
                ),
            )?;
            if eg.exception.is_none() {
                warn(
                    eg,
                    &format!("{function}(): Unable to create or locate filter \"{name}\""),
                )?;
            }
            return Ok(Value::bool(false));
        }
        Some(
            eg.find_class(&class_name)
                .expect("loaded filter class")
                .class_id,
        )
    };
    let id = owner.as_resource_id().expect("validated stream");
    let native_mode = super::super::with_stream(eg, id, |s| {
        i64::from(s.is_readable()) | (i64::from(s.is_writable()) << 1)
    })
    .or_else(|| {
        stream(eg, id).map(|s| {
            let s = s.borrow();
            i64::from(s.backend.is_readable()) | (i64::from(s.backend.is_writable()) << 1)
        })
    });
    let Some(native_mode) = native_mode else {
        return Ok(Value::bool(false));
    };
    let mode = if mode == 0 {
        native_mode
    } else {
        mode & (READ | WRITE)
    };
    let mut returned = Value::bool(false);
    for direction in [READ, WRITE] {
        if mode & direction == 0 {
            continue;
        }
        let object = if let Some(class_id) = class_id {
            let Some(object) = crate::vm::execute::instantiate_protocol_object(eg, ed, class_id)?
            else {
                warn(
                    eg,
                    &format!("{function}(): Unable to create or locate filter \"{name}\""),
                )?;
                return Ok(Value::bool(false));
            };
            if !project(
                eg,
                &object,
                "filtername",
                Value::string(name.to_string()),
                false,
            )? || !project(eg, &object, "params", params.clone(), false)?
            {
                return Ok(Value::null());
            }
            let creation = stdlib::call_object_public_method(eg, &object, "onCreate", &[]);
            if creation.is_err() || eg.exception.is_some() {
                if retain_failed_creation {
                    keep_shutdown_phase(eg);
                    let state = eg.static_vars.entry(ORPHANS.into()).or_default();
                    state.insert(state.len().to_string(), object);
                }
                creation?;
                return Ok(Value::null());
            }
            if creation?.is_some_and(|result| !result.is_truthy()) {
                warn(
                    eg,
                    &format!("{function}(): Unable to create or locate filter \"{name}\""),
                )?;
                return Ok(Value::bool(false));
            }
            Some(object)
        } else {
            None
        };
        if stream(eg, id).is_none() {
            let weak_owner = owner
                .weak_resource()
                .expect("stream-registry owns resource handles");
            if !resource::wrap_request_payload::<PhpStream, SharedStream>(eg, id, |mut backend| {
                let position = backend.position().ok();
                Rc::new(RefCell::new(FilteredStream {
                    backend,
                    owner: weak_owner,
                    read: vec![],
                    write: vec![],
                    unread: VecDeque::new(),
                    position,
                    read_closed: false,
                    busy: false,
                    closed: false,
                }))
            }) {
                return Ok(Value::bool(false));
            }
            owner.set_vm_resource_release(release_stream);
            keep_shutdown_phase(eg);
            eg.static_vars
                .entry(OPEN.into())
                .or_default()
                .insert(id.to_string(), Value::long(id));
        }
        let stream = stream(eg, id).expect("filtered backend installed");
        let filter = Rc::new(RefCell::new(Filter {
            object,
            builtin,
            stream: Rc::downgrade(&stream),
            direction,
            resource: 0,
        }));
        let handle = insert_resource(eg, "stream filter", filter.clone());
        filter.borrow_mut().resource = handle.as_resource_id().expect("filter handle");
        if direction == READ {
            let prepared = process_prebuffer(eg, &stream, &filter, owner, function, prepend);
            if !matches!(prepared, Ok(true)) {
                // The original unread bytes are still owned by the stream.
                // A failed callback/handler must not leak an unattached handle
                // or replace its exception with a secondary close callback.
                let retirement = if eg.exception.is_none() && prepared.is_ok() {
                    retire_filter(eg, &filter)
                } else {
                    resource::close_for_request::<SharedFilter>(
                        eg,
                        handle.as_resource_id().expect("filter handle"),
                    );
                    Ok(())
                };
                prepared?;
                retirement?;
                return Ok(Value::bool(false));
            }
        }
        let link = FilterLink {
            handle: handle.clone(),
            filter,
        };
        let mut state = stream.borrow_mut();
        let chain = if direction == READ {
            &mut state.read
        } else {
            &mut state.write
        };
        if prepend {
            chain.insert(0, link);
        } else {
            chain.push(link);
        }
        returned = handle;
    }
    Ok(returned)
}

/// Attachment is transactional with respect to already-prefetched bytes.
/// Snapshot outside PHP, then publish transformed bytes only on success.
fn process_prebuffer(
    eg: &mut ExecutorGlobals,
    stream: &SharedStream,
    filter: &SharedFilter,
    owner: &Value,
    function: &str,
    prepend: bool,
) -> Result<bool, VmError> {
    let (bytes, native) = {
        let state = stream.borrow();
        if state.unread.is_empty() {
            (state.backend.prefetched_bytes().to_vec(), true)
        } else {
            (state.unread.iter().copied().collect(), false)
        }
    };
    if bytes.is_empty() {
        return Ok(true);
    }
    // Prepending affects future input, not bytes already in the read buffer.
    if prepend {
        if native {
            let mut state = stream.borrow_mut();
            state.backend.discard_prefetched();
            state.unread.extend(bytes);
        }
        return Ok(true);
    }
    let Some(_busy) = BusyScope::enter(stream) else {
        return Ok(false);
    };
    let result = invoke_filter(eg, filter, Some(owner), bytes, false, false)?;
    if result.fatal {
        if eg.exception.is_none() {
            warn(
                eg,
                &format!("{function}(): Filter failed to process pre-buffered data"),
            )?;
        }
        return Ok(false);
    }
    let mut state = stream.borrow_mut();
    if native {
        state.backend.discard_prefetched();
    } else {
        state.unread.clear();
    }
    state.unread.extend(result.bytes);
    Ok(true)
}

fn fn_append(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    super::with_source(eg, ed, |eg| attach(ed, rv, eg, false))
}
fn fn_prepend(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    super::with_source(eg, ed, |eg| attach(ed, rv, eg, true))
}

fn brigade(eg: &mut ExecutorGlobals, bytes: Vec<u8>) -> (Value, SharedBrigade) {
    let state = Rc::new(RefCell::new(Brigade::default()));
    if !bytes.is_empty() {
        let bucket = insert_resource(
            eg,
            "stream bucket",
            Rc::new(RefCell::new(Bucket {
                bytes,
                owner: Rc::downgrade(&state),
            })),
        );
        state.borrow_mut().buckets.push_back(bucket);
    }
    (
        insert_resource(eg, "stream bucket brigade", state.clone()),
        state,
    )
}

struct FilterResult {
    bytes: Vec<u8>,
    consumed: usize,
    fatal: bool,
}

fn invoke_filter(
    eg: &mut ExecutorGlobals,
    filter: &SharedFilter,
    owner: Option<&Value>,
    mut bytes: Vec<u8>,
    closing: bool,
    removal: bool,
) -> Result<FilterResult, VmError> {
    if let Some(kind) = filter.borrow().builtin {
        match kind {
            BuiltinFilter::Uppercase => bytes.make_ascii_uppercase(),
        }
        return Ok(FilterResult {
            consumed: bytes.len(),
            bytes,
            fatal: false,
        });
    }
    let object = filter.borrow().object.clone();
    let Some(object) = object else {
        return Ok(FilterResult {
            bytes,
            consumed: 0,
            fatal: false,
        });
    };
    let projection = project(
        eg,
        &object,
        "stream",
        owner.cloned().unwrap_or_else(Value::null),
        true,
    );
    if !matches!(projection, Ok(true)) {
        // Rejected stream injection leaves every input byte unprocessed.
        // Report cleanup while the original exception is still pending.
        let diagnostic = if bytes.is_empty() {
            Ok(())
        } else {
            warn_unprocessed_input(eg)
        };
        projection?;
        diagnostic?;
        return Ok(FilterResult {
            bytes: vec![],
            consumed: 0,
            fatal: true,
        });
    }
    let (input, input_state) = brigade(eg, bytes);
    let (output, state) = brigade(eg, vec![]);
    let consumed = Value::owned_reference(if removal {
        Value::null()
    } else {
        Value::long(0)
    });
    // Value-only callback invocation intentionally reads reference targets.
    // This protocol instead owns an output cell: use the canonical argument
    // preparation that preserves aliases only for declared by-ref parameters.
    let result = if let Some(resolved) = stdlib::resolve_object_public_method(eg, &object, "filter")
    {
        let mut arguments = PhpArray::with_packed_capacity(4);
        arguments.push(input.clone());
        arguments.push(output.clone());
        arguments.push(consumed.clone_owned_reference_alias());
        arguments.push(Value::bool(closing));
        stdlib::call_resolved_with_php_array(eg, resolved, &arguments, true).map(Some)
    } else {
        Ok(None)
    };
    // Clear the ephemeral public projection on every exit, without replacing
    // a callback exception with a secondary reset error.
    let pending = eg.exception.take();
    let preserve_pending = pending.is_some();
    let reset = project(eg, &object, "stream", Value::null(), true);
    if pending.is_some() {
        eg.exception = pending;
    }
    let mut bytes = Vec::new();
    let handles = std::mem::take(&mut state.borrow_mut().buckets);
    for handle in handles {
        if let Some(bucket) = shared_payload::<SharedBucket>(eg, &handle) {
            bytes.extend_from_slice(&bucket.borrow().bytes);
        }
    }
    // Snapshot before invoking a warning handler: no brigade borrow may
    // survive PHP reentry. Cleanup must also finish if that handler throws.
    let unprocessed = !input_state.borrow().buckets.is_empty();
    let diagnostic = if unprocessed {
        warn_unprocessed_input(eg)
    } else {
        Ok(())
    };
    resource::close_for_request::<SharedBrigade>(eg, input.as_resource_id().expect("brigade"));
    resource::close_for_request::<SharedBrigade>(eg, output.as_resource_id().expect("brigade"));
    let status = result?.map_or(0, |value| value.to_long_val());
    if !preserve_pending {
        reset?;
    }
    diagnostic?;
    if status != PASS_ON {
        bytes.clear();
    }
    Ok(FilterResult {
        bytes,
        consumed: consumed.dereferenced().to_long_val().max(0) as usize,
        fatal: status == 0 || eg.exception.is_some(),
    })
}

fn chain(
    eg: &mut ExecutorGlobals,
    filters: &[SharedFilter],
    owner: Option<&Value>,
    mut bytes: Vec<u8>,
    closing: bool,
    removal: bool,
) -> Result<FilterResult, VmError> {
    let mut consumed = bytes.len();
    for (index, filter) in filters.iter().enumerate() {
        let result = invoke_filter(eg, filter, owner, bytes, closing, removal)?;
        if index == 0 {
            consumed = result.consumed;
        }
        bytes = result.bytes;
        if result.fatal {
            return Ok(FilterResult {
                bytes,
                consumed,
                fatal: true,
            });
        }
    }
    Ok(FilterResult {
        bytes,
        consumed,
        fatal: false,
    })
}

struct BusyScope(SharedStream);
impl BusyScope {
    fn enter(stream: &SharedStream) -> Option<Self> {
        let mut state = stream.borrow_mut();
        if state.busy || state.closed {
            return None;
        }
        state.busy = true;
        Some(Self(stream.clone()))
    }
}
impl Drop for BusyScope {
    fn drop(&mut self) {
        self.0.borrow_mut().busy = false;
    }
}

fn filters_for(stream: &SharedStream, direction: i64) -> Vec<SharedFilter> {
    let state = stream.borrow();
    let chain = if direction == READ {
        &state.read
    } else {
        &state.write
    };
    chain.iter().map(|link| link.filter.clone()).collect()
}

fn clear_stat_cache(eg: &mut ExecutorGlobals, stream: &SharedStream) {
    if stream.borrow_mut().backend.take_plain_file_io() {
        stdlib::filesystem::clear_filesystem_stat_cache(eg);
    }
}

/// Backend-only metadata operations cannot invoke a filter. I/O consumers
/// must use the named read/write/flush/seek entry points instead.
#[cold]
pub(in crate::stdlib::streams) fn with_backend<R>(
    eg: &mut ExecutorGlobals,
    id: i64,
    operation: impl FnOnce(&mut PhpStream) -> R,
) -> Option<R> {
    let stream = stream(eg, id)?;
    let mut state = stream.borrow_mut();
    (!state.closed).then(|| operation(&mut state.backend))
}

#[cold]
pub(in crate::stdlib::streams) fn write(
    eg: &mut ExecutorGlobals,
    id: i64,
    bytes: &[u8],
) -> Result<Option<io::Result<usize>>, VmError> {
    let Some(stream) = stream(eg, id) else {
        return Ok(None);
    };
    let Some(_busy) = BusyScope::enter(&stream) else {
        return Ok(Some(Err(io::Error::other("stream is busy"))));
    };
    let owner = stream.borrow().owner.upgrade();
    let result = chain(
        eg,
        &filters_for(&stream, WRITE),
        owner.as_ref(),
        bytes.to_vec(),
        false,
        false,
    )?;
    if result.fatal {
        return Ok(Some(Err(io::Error::other("filter failed"))));
    }
    let written = stream
        .borrow_mut()
        .backend
        .write(&result.bytes)
        .map(|_| result.consumed);
    if let Ok(count) = written {
        advance_position(&mut stream.borrow_mut(), count);
    }
    clear_stat_cache(eg, &stream);
    Ok(Some(written))
}

fn flush_write(
    eg: &mut ExecutorGlobals,
    stream: &SharedStream,
    owner: Option<&Value>,
    closing: bool,
) -> Result<bool, VmError> {
    let result = chain(
        eg,
        &filters_for(stream, WRITE),
        owner,
        vec![],
        closing,
        false,
    )?;
    if result.fatal {
        return Ok(false);
    }
    let success = {
        let mut state = stream.borrow_mut();
        state.backend.write(&result.bytes).is_ok() && state.backend.flush().is_ok()
    };
    clear_stat_cache(eg, stream);
    Ok(success)
}

#[cold]
pub(in crate::stdlib::streams) fn flush(
    eg: &mut ExecutorGlobals,
    id: i64,
) -> Result<Option<bool>, VmError> {
    let Some(stream) = stream(eg, id) else {
        return Ok(None);
    };
    let Some(_busy) = BusyScope::enter(&stream) else {
        return Ok(Some(false));
    };
    let owner = stream.borrow().owner.upgrade();
    flush_write(eg, &stream, owner.as_ref(), false).map(Some)
}

#[cold]
pub(in crate::stdlib::streams) fn seek(
    eg: &mut ExecutorGlobals,
    id: i64,
    position: SeekFrom,
) -> Result<Option<bool>, VmError> {
    let Some(stream) = stream(eg, id) else {
        return Ok(None);
    };
    let Some(_busy) = BusyScope::enter(&stream) else {
        return Ok(Some(false));
    };
    let owner = stream.borrow().owner.upgrade();
    // Flush failure does not prevent an independent seek on the underlying
    // stream; a thrown callback exception does.
    let _ = flush_write(eg, &stream, owner.as_ref(), false)?;
    if eg.exception.is_some() {
        return Ok(Some(false));
    }
    let mut state = stream.borrow_mut();
    let position = match (position, state.position) {
        (SeekFrom::Current(offset), Some(current)) => {
            let Some(position) = current.checked_add_signed(offset) else {
                return Ok(Some(false));
            };
            SeekFrom::Start(position)
        }
        (position, _) => position,
    };
    let success = state.backend.seek(position).is_ok();
    if success {
        state.unread.clear();
        state.read_closed = false;
        state.position = state.backend.position().ok();
    }
    Ok(Some(success))
}

#[cold]
pub(in crate::stdlib::streams) fn read(
    eg: &mut ExecutorGlobals,
    id: i64,
    requested: usize,
) -> Result<Option<Vec<u8>>, VmError> {
    let Some(stream) = stream(eg, id) else {
        return Ok(None);
    };
    let Some(_busy) = BusyScope::enter(&stream) else {
        return Ok(None);
    };
    let owner = stream.borrow().owner.upgrade();
    if !fill_read_buffer(eg, &stream, owner.as_ref(), requested)? {
        return Ok(None);
    }
    let mut state = stream.borrow_mut();
    let count = requested.min(state.unread.len());
    advance_position(&mut state, count);
    Ok(Some(state.unread.drain(..count).collect()))
}

fn fill_read_buffer(
    eg: &mut ExecutorGlobals,
    stream: &SharedStream,
    owner: Option<&Value>,
    requested: usize,
) -> Result<bool, VmError> {
    loop {
        {
            let state = stream.borrow();
            if state.unread.len() >= requested || state.read_closed {
                break;
            }
        }
        let mut bytes = vec![0; 8192];
        let read = stream.borrow_mut().backend.read_filter_input(&mut bytes);
        clear_stat_cache(eg, &stream);
        let count = match read {
            Ok(count) => count,
            Err(error) => {
                if let Some(message) =
                    super::super::read_error_message(super::diagnostic_function(eg), &error)
                {
                    let (file, line) = super::diagnostic_source(eg);
                    stdlib::report_diagnostic_from(
                        eg,
                        eg.current_execute_data.get(),
                        &file,
                        line,
                        8,
                        "Notice",
                        &message,
                    )?;
                }
                return Ok(false);
            }
        };
        bytes.truncate(count);
        let closing = stream.borrow().backend.is_eof();
        if closing {
            stream.borrow_mut().read_closed = true;
        }
        let result = chain(
            eg,
            &filters_for(&stream, READ),
            owner,
            bytes,
            closing,
            false,
        )?;
        if result.fatal {
            return Ok(false);
        }
        stream.borrow_mut().unread.extend(result.bytes);
        if count == 0 {
            break;
        }
    }
    Ok(true)
}

fn advance_position(state: &mut FilteredStream, count: usize) {
    state.position = state
        .position
        .and_then(|position| position.checked_add(count as u64));
}

pub(in crate::stdlib::streams) fn position(eg: &mut ExecutorGlobals, id: i64) -> Option<u64> {
    let stream = stream(eg, id)?;
    stream.borrow().position
}

pub(in crate::stdlib::streams) fn unread_bytes(eg: &mut ExecutorGlobals, id: i64) -> Option<usize> {
    let stream = stream(eg, id)?;
    let state = stream.borrow();
    Some(state.unread.len() + state.backend.prefetched_bytes().len())
}

/// Physical lines consume buffered bytes but retain all data beyond the
/// newline. Neither a stream borrow nor a CSV parser borrows PHP storage.
pub(in crate::stdlib::streams) fn read_line(
    eg: &mut ExecutorGlobals,
    id: i64,
    maximum: usize,
) -> Result<Option<Vec<u8>>, VmError> {
    let Some(stream) = stream(eg, id) else {
        return Ok(None);
    };
    let Some(_busy) = BusyScope::enter(&stream) else {
        return Ok(None);
    };
    let owner = stream.borrow().owner.upgrade();
    let mut result = Vec::new();
    while result.len() < maximum {
        if !fill_read_buffer(eg, &stream, owner.as_ref(), 1)? {
            return Ok(None);
        }
        let mut state = stream.borrow_mut();
        let available = state.unread.len().min(maximum - result.len());
        if available == 0 {
            break;
        }
        let newline = state
            .unread
            .iter()
            .take(available)
            .position(|byte| *byte == b'\n');
        let count = newline.map_or(available, |index| index + 1);
        result.extend(state.unread.drain(..count));
        advance_position(&mut state, count);
        if newline.is_some() {
            break;
        }
    }
    Ok((!result.is_empty()).then_some(result))
}

/// stream_get_line peeks one available buffer. An incomplete delimited
/// record is left untouched so a later bounded read can make progress.
#[cfg(feature = "stream-line")]
pub(in crate::stdlib::streams) fn read_record(
    eg: &mut ExecutorGlobals,
    id: i64,
    maximum: Option<usize>,
    ending: &[u8],
) -> Result<Option<Vec<u8>>, VmError> {
    let Some(stream) = stream(eg, id) else {
        return Ok(None);
    };
    let Some(_busy) = BusyScope::enter(&stream) else {
        return Ok(None);
    };
    let owner = stream.borrow().owner.upgrade();
    if !fill_read_buffer(eg, &stream, owner.as_ref(), 1)? {
        return Ok(None);
    }
    let mut state = stream.borrow_mut();
    let maximum = maximum.unwrap_or(8192);
    let available = state.unread.len().min(maximum);
    if available == 0 {
        return Ok(None);
    }
    let ending_at = if ending.is_empty() {
        None
    } else {
        state.unread.make_contiguous()[..available]
            .windows(ending.len())
            .position(|bytes| bytes == ending)
    };
    if !ending.is_empty() && ending_at.is_none() && available < maximum && !state.read_closed {
        return Ok(None);
    }
    let count = ending_at.map_or(available, |index| index + ending.len());
    let mut result: Vec<_> = state.unread.drain(..count).collect();
    advance_position(&mut state, count);
    if ending_at.is_some() {
        result.truncate(result.len() - ending.len());
    }
    Ok(Some(result))
}

pub(in crate::stdlib::streams) fn read_csv(
    eg: &mut ExecutorGlobals,
    id: i64,
    maximum: Option<usize>,
    separator: u8,
    enclosure: u8,
    escape: Option<u8>,
) -> Result<Option<Vec<Option<Vec<u8>>>>, VmError> {
    let Some(initial) = read_line(eg, id, maximum.unwrap_or(usize::MAX))? else {
        return Ok(None);
    };
    let mut parser = crate::stdlib::stream::CsvParser::new(separator, enclosure, escape);
    if parser.push_segment(&initial).is_err() {
        return Ok(None);
    }
    while parser.needs_continuation() && eof(eg, id) == Some(false) {
        let Some(segment) = read_line(eg, id, usize::MAX)? else {
            break;
        };
        if parser.push_segment(&segment).is_err() {
            return Ok(None);
        }
    }
    Ok(parser.finish(eof(eg, id) == Some(true)).ok())
}

#[cold]
#[cfg(feature = "stream-contents")]
pub(in crate::stdlib::streams) fn read_contents(
    eg: &mut ExecutorGlobals,
    id: i64,
    length: Option<usize>,
    offset: Option<u64>,
) -> Result<Option<Vec<u8>>, VmError> {
    if stream(eg, id).is_none() {
        return Ok(None);
    }
    if let Some(offset) = offset {
        if seek(eg, id, SeekFrom::Start(offset))? != Some(true) {
            return Ok(None);
        }
    }
    let mut bytes = Vec::new();
    let limit = length.unwrap_or(usize::MAX);
    while bytes.len() < limit {
        let Some(chunk) = read(eg, id, limit.saturating_sub(bytes.len()).min(8192))? else {
            return Ok(None);
        };
        if chunk.is_empty() {
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Some(bytes))
}

pub(in crate::stdlib::streams) fn eof(eg: &mut ExecutorGlobals, id: i64) -> Option<bool> {
    let stream = stream(eg, id)?;
    let state = stream.borrow();
    Some(if state.busy {
        state.backend.is_eof()
    } else {
        state.read_closed && state.unread.is_empty()
    })
}

fn retire_filter(eg: &mut ExecutorGlobals, filter: &SharedFilter) -> Result<(), VmError> {
    let (object, id) = {
        let mut filter = filter.borrow_mut();
        (filter.object.take(), filter.resource)
    };
    resource::close_for_request::<SharedFilter>(eg, id);
    if let Some(object) = object {
        stdlib::call_object_public_method(eg, &object, "onClose", &[])?;
    }
    Ok(())
}

fn fn_remove(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    super::with_source(eg, ed, |eg| remove(ed, rv, eg))
}

fn remove(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let handle = super::super::argument(ed, 0).clone();
    let Some(filter) = shared_payload::<SharedFilter>(eg, &handle) else {
        if handle.as_resource_id().is_none() {
            stdlib::typed_internal_argument_error(
                eg,
                "stream_filter_remove",
                &handle,
                1,
                "stream_filter",
                "resource",
            );
        } else {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                "stream_filter_remove(): supplied resource is not a valid stream filter resource",
            ));
        }
        return Ok(());
    };
    let Some(stream) = filter.borrow().stream.upgrade() else {
        return super::super::return_value(rv, Value::bool(false));
    };
    let Some(_busy) = BusyScope::enter(&stream) else {
        return super::super::return_value(rv, Value::bool(false));
    };
    let owner = stream.borrow().owner.upgrade();
    let result = invoke_filter(eg, &filter, owner.as_ref(), vec![], true, true)?;
    if result.fatal {
        if eg.exception.is_none() {
            warn(
                eg,
                "stream_filter_remove(): Unable to flush filter, not removing",
            )?;
        }
        return super::super::return_value(rv, Value::bool(false));
    }
    let direction = filter.borrow().direction;
    // A flushed tail still traverses every downstream filter.
    let downstream = {
        let state = stream.borrow();
        let links = if direction == READ {
            &state.read
        } else {
            &state.write
        };
        let index = links
            .iter()
            .position(|link| Rc::ptr_eq(&link.filter, &filter))
            .expect("attached filter");
        links[index + 1..]
            .iter()
            .map(|link| link.filter.clone())
            .collect::<Vec<_>>()
    };
    let tail = chain(eg, &downstream, owner.as_ref(), result.bytes, false, false)?;
    if tail.fatal {
        return super::super::return_value(rv, Value::bool(false));
    }
    {
        let mut state = stream.borrow_mut();
        if direction == READ {
            state.unread.extend(tail.bytes);
        } else {
            let _ = state.backend.write(&tail.bytes);
        }
        let links = if direction == READ {
            &mut state.read
        } else {
            &mut state.write
        };
        links.retain(|link| link.handle.as_resource_id() != handle.as_resource_id());
    }
    retire_filter(eg, &filter)?;
    super::super::return_value(rv, Value::bool(true))
}

#[cold]
pub(in crate::stdlib::streams) fn close(
    eg: &mut ExecutorGlobals,
    id: i64,
    automatic: bool,
) -> Result<Option<bool>, VmError> {
    let Some(stream) = stream(eg, id) else {
        return Ok(None);
    };
    let Some(_busy) = BusyScope::enter(&stream) else {
        warn(
            eg,
            "fclose(): cannot close the provided stream, as it must not be manually closed",
        )?;
        return Ok(Some(false));
    };
    let owner = if automatic {
        None
    } else {
        stream.borrow().owner.upgrade()
    };
    let mut result = if eg.exception.is_some() {
        Ok(false)
    } else {
        flush_write(eg, &stream, owner.as_ref(), true)
    };
    // Close is committed even when a filter throws. Retire every resource
    // without running another callback while that exception is pending.
    stream.borrow_mut().closed = true;
    let (reads, writes) = {
        let mut s = stream.borrow_mut();
        (std::mem::take(&mut s.read), std::mem::take(&mut s.write))
    };
    // Read filters flush at EOF (or explicit removal), not at fclose().
    for link in reads.into_iter().chain(writes) {
        if eg.exception.is_none() && result.is_ok() {
            if let Err(error) = retire_filter(eg, &link.filter) {
                result = Err(error);
            }
        } else {
            resource::close_for_request::<SharedFilter>(
                eg,
                link.handle.as_resource_id().expect("filter handle"),
            );
        }
    }
    if let Some(open) = eg.static_vars.get_mut(OPEN) {
        open.remove(&id.to_string());
    }
    resource::close_for_request::<SharedStream>(eg, id);
    result?;
    Ok(Some(true))
}

fn release_stream(eg: &mut ExecutorGlobals, id: i64) -> Result<(), VmError> {
    close(eg, id, true).map(|_| ())
}

#[cold]
pub(in crate::stdlib) fn shutdown(eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let mut ids: Vec<_> = eg
        .static_vars
        .get(OPEN)
        .map(|open| open.values().filter_map(Value::as_long).collect())
        .unwrap_or_default();
    ids.sort_unstable();
    for id in ids.into_iter().rev() {
        close(eg, id, true)?;
    }
    if let Some(orphaned) = eg.static_vars.remove(ORPHANS) {
        for object in orphaned.into_values() {
            stdlib::call_object_public_method(eg, &object, "onClose", &[])?;
        }
    }
    Ok(())
}
