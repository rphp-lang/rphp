//! Delegated iterator state machines. No PHP callback executes under an
//! object/storage borrow; cached values remain ordinary traced ownership edges.
use super::*;
use crate::value::NativeIteratorDelegate;
use crate::vm::execute::{
    prepare_replaced_value_destructor, prepare_replaced_value_tree_destructor_with_references,
    run_prepared_value_destructor,
};
use crate::vm::function::InternalFunctionHandler;

/// SPL delegates missing method lookup to its retained inner object. The
/// caller has already preferred real wrapper methods and wrapper __call.
/// Never cache this result by the wrapper class: each instance owns a
/// different inner receiver, and both by-reference and named sends need the
/// actual method descriptor before argument evaluation.
#[cold]
pub(crate) fn resolve_method(
    eg: &ExecutorGlobals,
    receiver: &Value,
    method: &str,
) -> Option<ResolvedCallback> {
    let mut receiver = receiver.clone();
    let mut seen = Vec::new();
    loop {
        let object = receiver.as_object()?;
        let state = object.native_iterator_delegate()?;
        let next = state
            .recursive
            .as_ref()
            .and_then(|recursive| recursive.frames.last())
            .map_or(&state.inner, |frame| &frame.iterator)
            .clone();
        drop(object);
        let identity = next.object_identity()?;
        if seen.contains(&identity) {
            return None;
        }
        seen.push(identity);
        let object = next.as_object()?;
        let class = object.class_name.to_string();
        let class_id = object.class_id;
        drop(object);
        if let Some((_, is_static, function, _)) =
            find_method_in_class_hierarchy(eg, &class, method)
        {
            return Some(ResolvedCallback {
                func_ptr: function,
                prepend_args: vec![if is_static { Value::null() } else { next }],
                use_vars: vec![],
                called_scope_class_id: class_id,
                closure_scope_class_id: None,
                bound_this: None,
                closure_static_vars: None,
                is_magic_call: false,
            });
        }
        if let Some(magic) = resolve_magic_callback(eg, &class, method, "__call", Some(&next)) {
            return Some(magic);
        }
        receiver = next;
    }
}

fn error(eg: &mut ExecutorGlobals, kind: &str, message: &str) {
    eg.exception = Some(make_error_value(kind, message));
}

fn initialized(receiver: &Value, eg: &mut ExecutorGlobals) -> bool {
    if receiver
        .as_object()
        .is_some_and(|o| o.native_iterator_delegate().is_some())
    {
        return true;
    }
    error(
        eg,
        "Error",
        "The object is in an invalid state as the parent constructor was not called",
    );
    false
}

fn inner(receiver: &Value) -> Value {
    receiver
        .as_object()
        .expect("method receiver")
        .native_iterator_delegate()
        .expect("validated initialized iterator")
        .iterator
        .clone()
}

pub(super) fn discard(value: Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let release = if value.dereferenced().value_type() == ValueType::Array {
        prepare_replaced_value_tree_destructor_with_references(eg, &value, 1)
    } else {
        prepare_replaced_value_destructor(eg, &value)
    };
    drop(value);
    run_prepared_value_destructor(eg, release)
}

fn clear(receiver: &Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let (current, key) = {
        let mut object = receiver.as_object_mut().expect("method receiver");
        let state = object.native_iterator_delegate_mut().expect("initialized");
        (
            std::mem::replace(&mut state.current, Value::undef()),
            std::mem::replace(&mut state.key, Value::undef()),
        )
    };
    discard(current, eg)?;
    discard(key, eg)
}

pub(super) fn protocol(
    eg: &mut ExecutorGlobals,
    iterator: &Value,
    name: &str,
) -> Result<Value, VmError> {
    if array_object::cursor::native_protocol(iterator, eg) {
        use array_object::cursor::{Move, Projection, projected_entry};
        return Ok(match name {
            "rewind" | "next" => {
                projected_entry(
                    iterator,
                    if name == "rewind" {
                        Move::Rewind
                    } else {
                        Move::Next
                    },
                    Projection::None,
                    eg,
                );
                Value::null()
            }
            "valid" => Value::bool(
                projected_entry(iterator, Move::Current, Projection::None, eg).is_some(),
            ),
            "key" => projected_entry(iterator, Move::Current, Projection::Key, eg)
                .map_or_else(Value::null, |p| p.0),
            "current" => array_object::cursor::cached_value(iterator, eg),
            _ => unreachable!("iterator protocol name"),
        });
    }
    Ok(
        call_object_protocol_method(eg, iterator, "Iterator", name, &[])?
            .unwrap_or_else(Value::null),
    )
}

fn fetch(receiver: &Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let iterator = inner(receiver);
    let valid = protocol(eg, &iterator, "valid")?;
    if eg.exception.is_some() || !valid.is_truthy() {
        return Ok(());
    }
    let current = protocol(eg, &iterator, "current")?;
    if eg.exception.is_some() {
        return Ok(());
    }
    receiver
        .as_object_mut()
        .expect("receiver")
        .native_iterator_delegate_mut()
        .expect("initialized")
        .current = current;
    let key = protocol(eg, &iterator, "key")?;
    if eg.exception.is_none() {
        receiver
            .as_object_mut()
            .expect("receiver")
            .native_iterator_delegate_mut()
            .expect("initialized")
            .key = key;
    }
    Ok(())
}

fn construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    owner: &str,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if receiver
        .as_object()
        .is_some_and(|o| o.native_iterator_delegate().is_some())
    {
        error(
            eg,
            "BadMethodCallException",
            &format!("{owner}::getIterator() must be called exactly once per instance"),
        );
        ret!(rv, Value::null());
    }
    let source = owned_argument(ed, 1);
    let expected = if owner == "IteratorIterator" {
        "Traversable"
    } else {
        "Iterator"
    };
    if !source
        .as_object()
        .is_some_and(|o| eg.class_is_a(&o.class_name, expected))
    {
        typed_internal_argument_error(
            eg,
            &format!("{owner}::__construct"),
            &source,
            1,
            "iterator",
            expected,
        );
        ret!(rv, Value::null());
    }
    let mut offset = 0;
    let mut limit = -1;
    if owner == "LimitIterator" {
        for (index, name, minimum, destination) in
            [(2, "offset", 0, &mut offset), (3, "limit", -1, &mut limit)]
        {
            if let Some(value) = arg_opt!(ed, index) {
                let value = value.clone();
                let Some(number) = typed_internal_int_value_argument_expected(
                    ed,
                    eg,
                    &value,
                    "LimitIterator::__construct",
                    index - 1,
                    name,
                    "int",
                )?
                else {
                    ret!(rv, Value::null());
                };
                if number < minimum {
                    error(
                        eg,
                        "ValueError",
                        &format!(
                            "LimitIterator::__construct(): Argument #{index} (${name}) must be greater than or equal to {minimum}"
                        ),
                    );
                    ret!(rv, Value::null());
                }
                *destination = number;
            }
        }
    } else if owner == "IteratorIterator"
        && let Some(value) = arg_opt!(ed, 2)
        && value.dereferenced().value_type() != ValueType::Null
    {
        let value = value.clone();
        if typed_internal_string_value_expected(
            ed,
            eg,
            &value,
            "IteratorIterator::__construct",
            1,
            "class",
            "?string",
            "string",
        )?
        .is_none()
        {
            ret!(rv, Value::null());
        }
    }
    let mut iterator = source;
    let mut retained = None;
    let mut seen = Vec::new();
    loop {
        let class = iterator
            .as_object()
            .expect("traversable")
            .class_name
            .to_string();
        if !eg.class_is_a(&class, "IteratorAggregate") {
            break;
        }
        let identity = iterator.object_identity().expect("traversable object");
        if seen.contains(&identity) {
            error(
                eg,
                "Exception",
                &format!(
                    "Objects returned by {class}::getIterator() must be traversable or implement interface Iterator"
                ),
            );
            ret!(rv, Value::null());
        }
        seen.push(identity);
        let next =
            call_object_protocol_method(eg, &iterator, "IteratorAggregate", "getIterator", &[])?;
        if eg.exception.is_some() {
            ret!(rv, Value::null());
        }
        let Some(next) = next.filter(|v| {
            v.as_object()
                .is_some_and(|o| eg.class_is_a(&o.class_name, "Traversable"))
        }) else {
            error(
                eg,
                "Exception",
                &format!(
                    "Objects returned by {class}::getIterator() must be traversable or implement interface Iterator"
                ),
            );
            ret!(rv, Value::null());
        };
        if retained.is_none() {
            retained = Some(next.clone());
        }
        iterator = next;
    }
    let retained = retained.unwrap_or_else(|| iterator.clone());
    receiver
        .as_object_mut()
        .expect("receiver")
        .set_native_iterator_delegate(NativeIteratorDelegate::new(
            retained, iterator, offset, limit,
        ));
    ret!(rv, Value::null());
}

fn construct_iterator(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    construct(ed, rv, eg, "IteratorIterator")
}
fn construct_limit(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    construct(ed, rv, eg, "LimitIterator")
}
fn construct_no_rewind(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    construct(ed, rv, eg, "NoRewindIterator")
}

fn get_inner(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let value = receiver
        .as_object()
        .expect("receiver")
        .native_iterator_delegate()
        .expect("initialized")
        .inner
        .clone();
    ret!(rv, value);
}
fn current(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let value = receiver
        .as_object()
        .expect("receiver")
        .native_iterator_delegate()
        .expect("initialized")
        .current
        .dereferenced()
        .clone();
    ret!(
        rv,
        if value.is_undef() {
            Value::null()
        } else {
            value
        }
    );
}
fn key(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let value = receiver
        .as_object()
        .expect("receiver")
        .native_iterator_delegate()
        .expect("initialized")
        .key
        .dereferenced()
        .clone();
    ret!(
        rv,
        if value.is_undef() {
            Value::null()
        } else {
            value
        }
    );
}
fn valid(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let object = receiver.as_object().expect("receiver");
    let state = object.native_iterator_delegate().expect("initialized");
    ret!(rv, Value::bool(!state.current.is_undef()));
}
fn rewind(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    clear(&receiver, eg)?;
    if eg.exception.is_some() {
        ret!(rv, Value::null());
    }
    protocol(eg, &inner(&receiver), "rewind")?;
    if eg.exception.is_none() {
        fetch(&receiver, eg)?;
    }
    ret!(rv, Value::null());
}
fn next(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    clear(&receiver, eg)?;
    if eg.exception.is_some() {
        ret!(rv, Value::null());
    }
    protocol(eg, &inner(&receiver), "next")?;
    if eg.exception.is_none() {
        fetch(&receiver, eg)?;
    }
    ret!(rv, Value::null());
}
fn no_rewind(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::null());
}
fn live_valid(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let value = protocol(eg, &inner(&receiver), "valid")?;
    ret!(rv, value);
}
fn live_key(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let value = protocol(eg, &inner(&receiver), "key")?;
    ret!(rv, value);
}
fn lazy_current(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let iterator = inner(&receiver);
    if array_object::cursor::native_protocol(&iterator, eg) {
        let value = array_object::cursor::cached_value(&iterator, eg);
        ret!(rv, value.dereferenced().clone());
    }
    let empty = receiver
        .as_object()
        .expect("receiver")
        .native_iterator_delegate()
        .expect("initialized")
        .current
        .is_undef();
    if empty {
        let value = protocol(eg, &inner(&receiver), "current")?;
        if eg.exception.is_none() {
            receiver
                .as_object_mut()
                .expect("receiver")
                .native_iterator_delegate_mut()
                .expect("initialized")
                .current = value;
        }
    }
    current(ed, rv, eg)
}
fn live_next(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    clear(&receiver, eg)?;
    if eg.exception.is_none() {
        protocol(eg, &inner(&receiver), "next")?;
    }
    ret!(rv, Value::null());
}

fn seek_to(receiver: &Value, offset: i64, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let (start, limit, position) = {
        let object = receiver.as_object().expect("receiver");
        let state = object.native_iterator_delegate().expect("initialized");
        (state.offset, state.limit, state.position)
    };
    if offset < start {
        error(
            eg,
            "OutOfBoundsException",
            &format!("Cannot seek to {offset} which is below the offset {start}"),
        );
        return Ok(());
    }
    if limit != -1 && offset - start >= limit {
        error(
            eg,
            "OutOfBoundsException",
            &format!("Cannot seek to {offset} which is behind offset {start} plus count {limit}"),
        );
        return Ok(());
    }
    let iterator = inner(receiver);
    let seekable = iterator
        .as_object()
        .is_some_and(|o| eg.class_is_a(&o.class_name, "SeekableIterator"));
    if seekable && offset != position {
        call_object_protocol_method(
            eg,
            &iterator,
            "SeekableIterator",
            "seek",
            &[Value::long(offset)],
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        receiver
            .as_object_mut()
            .expect("receiver")
            .native_iterator_delegate_mut()
            .expect("initialized")
            .position = offset;
    } else {
        let mut position = position;
        if offset < position {
            protocol(eg, &iterator, "rewind")?;
            position = 0;
            receiver
                .as_object_mut()
                .expect("receiver")
                .native_iterator_delegate_mut()
                .expect("initialized")
                .position = position;
        }
        while eg.exception.is_none()
            && protocol(eg, &iterator, "valid")?.is_truthy()
            && position < offset
        {
            protocol(eg, &iterator, "next")?;
            if eg.exception.is_some() {
                break;
            }
            position += 1;
            receiver
                .as_object_mut()
                .expect("receiver")
                .native_iterator_delegate_mut()
                .expect("initialized")
                .position = position;
        }
    }
    if eg.exception.is_none() {
        fetch(receiver, eg)?;
    }
    Ok(())
}
fn limit_rewind(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    clear(&receiver, eg)?;
    if eg.exception.is_some() {
        ret!(rv, Value::null());
    }
    protocol(eg, &inner(&receiver), "rewind")?;
    let offset = {
        let mut object = receiver.as_object_mut().expect("receiver");
        let state = object.native_iterator_delegate_mut().expect("initialized");
        state.position = 0;
        state.offset
    };
    if eg.exception.is_none() {
        seek_to(&receiver, offset, eg)?;
    }
    ret!(rv, Value::null());
}
fn limit_next(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    clear(&receiver, eg)?;
    if eg.exception.is_some() {
        ret!(rv, Value::null());
    }
    protocol(eg, &inner(&receiver), "next")?;
    let within = {
        let mut object = receiver.as_object_mut().expect("receiver");
        let state = object.native_iterator_delegate_mut().expect("initialized");
        state.position = state.position.saturating_add(1);
        state.limit == -1 || state.position.saturating_sub(state.offset) < state.limit
    };
    if within && eg.exception.is_none() {
        fetch(&receiver, eg)?;
    }
    ret!(rv, Value::null());
}
fn position(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let position = receiver
        .as_object()
        .expect("receiver")
        .native_iterator_delegate()
        .expect("initialized")
        .position;
    ret!(rv, Value::long(position));
}
fn seek(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let value = owned_argument(ed, 1);
    let Some(offset) = typed_internal_int_value_argument_expected(
        ed,
        eg,
        &value,
        "LimitIterator::seek",
        0,
        "offset",
        "int",
    )?
    else {
        ret!(rv, Value::null());
    };
    clear(&receiver, eg)?;
    if eg.exception.is_none() {
        seek_to(&receiver, offset, eg)?;
    }
    position(ed, rv, eg)
}

macro_rules! abstract_method {
    ($handler:ident, $declaration:literal) => {
        fn $handler(
            _ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            error(
                eg,
                "Error",
                concat!("Cannot call abstract method ", $declaration, "()"),
            );
            ret!(rv, Value::null());
        }
    };
}
abstract_method!(abstract_current, "Iterator::current");
abstract_method!(abstract_next, "Iterator::next");
abstract_method!(abstract_key, "Iterator::key");
abstract_method!(abstract_valid, "Iterator::valid");
abstract_method!(abstract_rewind, "Iterator::rewind");
abstract_method!(abstract_inner, "OuterIterator::getInnerIterator");

pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Bool, ClassName, Int, Mixed, Nullable, String, Void};
    let nullable_iterator = Nullable(Box::new(ClassName("Iterator".into())));
    let mut functions = Vec::new();
    for (owner, name, handler, names, hints, defaults, result) in [
        (
            "Iterator",
            "current",
            abstract_current as InternalFunctionHandler,
            vec![],
            vec![],
            vec![],
            Mixed,
        ),
        (
            "Iterator",
            "next",
            abstract_next,
            vec![],
            vec![],
            vec![],
            Void,
        ),
        (
            "Iterator",
            "key",
            abstract_key,
            vec![],
            vec![],
            vec![],
            Mixed,
        ),
        (
            "Iterator",
            "valid",
            abstract_valid,
            vec![],
            vec![],
            vec![],
            Bool,
        ),
        (
            "Iterator",
            "rewind",
            abstract_rewind,
            vec![],
            vec![],
            vec![],
            Void,
        ),
        (
            "OuterIterator",
            "getInnerIterator",
            abstract_inner,
            vec![],
            vec![],
            vec![],
            nullable_iterator.clone(),
        ),
        (
            "IteratorIterator",
            "__construct",
            construct_iterator as InternalFunctionHandler,
            vec!["iterator", "class"],
            vec![ClassName("Traversable".into()), Nullable(Box::new(String))],
            vec![None, Some("null")],
            ParamTypeHint::None,
        ),
        (
            "IteratorIterator",
            "getInnerIterator",
            get_inner,
            vec![],
            vec![],
            vec![],
            nullable_iterator,
        ),
        (
            "IteratorIterator",
            "rewind",
            rewind,
            vec![],
            vec![],
            vec![],
            Void,
        ),
        (
            "IteratorIterator",
            "valid",
            valid,
            vec![],
            vec![],
            vec![],
            Bool,
        ),
        (
            "IteratorIterator",
            "key",
            key,
            vec![],
            vec![],
            vec![],
            Mixed,
        ),
        (
            "IteratorIterator",
            "current",
            current,
            vec![],
            vec![],
            vec![],
            Mixed,
        ),
        (
            "IteratorIterator",
            "next",
            next,
            vec![],
            vec![],
            vec![],
            Void,
        ),
        (
            "LimitIterator",
            "__construct",
            construct_limit,
            vec!["iterator", "offset", "limit"],
            vec![ClassName("Iterator".into()), Int, Int],
            vec![None, Some("0"), Some("-1")],
            ParamTypeHint::None,
        ),
        (
            "LimitIterator",
            "rewind",
            limit_rewind,
            vec![],
            vec![],
            vec![],
            Void,
        ),
        (
            "LimitIterator",
            "valid",
            valid,
            vec![],
            vec![],
            vec![],
            Bool,
        ),
        (
            "LimitIterator",
            "next",
            limit_next,
            vec![],
            vec![],
            vec![],
            Void,
        ),
        (
            "LimitIterator",
            "seek",
            seek,
            vec!["offset"],
            vec![Int],
            vec![None],
            Int,
        ),
        (
            "LimitIterator",
            "getPosition",
            position,
            vec![],
            vec![],
            vec![],
            Int,
        ),
        (
            "NoRewindIterator",
            "__construct",
            construct_no_rewind,
            vec!["iterator"],
            vec![ClassName("Iterator".into())],
            vec![None],
            ParamTypeHint::None,
        ),
        (
            "NoRewindIterator",
            "rewind",
            no_rewind,
            vec![],
            vec![],
            vec![],
            Void,
        ),
        (
            "NoRewindIterator",
            "valid",
            live_valid,
            vec![],
            vec![],
            vec![],
            Bool,
        ),
        (
            "NoRewindIterator",
            "key",
            live_key,
            vec![],
            vec![],
            vec![],
            Mixed,
        ),
        (
            "NoRewindIterator",
            "current",
            lazy_current,
            vec![],
            vec![],
            vec![],
            Mixed,
        ),
        (
            "NoRewindIterator",
            "next",
            live_next,
            vec![],
            vec![],
            vec![],
            Void,
        ),
    ] {
        let required = defaults.iter().filter(|d| d.is_none()).count() as u32;
        eg.register_internal_method_contract(
            owner,
            name,
            false,
            required,
            &names,
            hints.clone(),
            result,
            &defaults,
            name != "__construct",
        );
        let mut function = Box::new(make_internal_method(
            handler,
            names.len() as u32 + 1,
            required,
            names.iter().map(|s| s.to_string()).collect(),
        ));
        function.common.sig.param_type_hints = hints;
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.function_table
            .insert(format!("{owner}::{name}").to_ascii_lowercase(), pointer);
        eg.method_declaring_class.insert(pointer, owner.into());
        eg.register_internal_function_display_name(pointer, format!("{owner}::{name}"));
        let values = defaults
            .iter()
            .map(|d| {
                d.map(|d| match d {
                    "null" => Value::null(),
                    "0" => Value::long(0),
                    "-1" => Value::long(-1),
                    _ => unreachable!(),
                })
            })
            .collect();
        eg.register_internal_function_reflection_metadata(pointer, values, "SPL");
        functions.push(function);
    }
    functions
}
