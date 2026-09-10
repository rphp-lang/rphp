//! Recursive SPL traversal. Each callback observes its defined traversal phase;
//! no object borrow survives a callback or a released iterator's destructor.
use super::*;
use crate::value::{NativeIteratorDelegate, RecursiveFrame, RecursivePhase, RecursiveTraversal};
use crate::vm::function::InternalFunctionHandler;

fn error(eg: &mut ExecutorGlobals, kind: &str, message: &str) {
    eg.exception = Some(make_error_value(kind, message));
}

#[cold]
fn reject_child(child: Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    // Retire the rejected temporary before raising the validation failure.
    // A destructor failure becomes its previous exception; externally held
    // objects retain their normal owner and are not destroyed here.
    iterator_delegate::discard(child, eg)?;
    let exception = make_error_value(
        "UnexpectedValueException",
        "Objects returned by RecursiveIterator::getChildren() must implement RecursiveIterator",
    );
    if let Some(previous) = eg.exception.take() {
        crate::vm::execute::append_replaced_exception(&exception, &previous, eg);
    }
    eg.exception = Some(exception);
    Ok(())
}

fn initialized(receiver: &Value, eg: &mut ExecutorGlobals) -> bool {
    let object = receiver.as_object().expect("recursive method receiver");
    if object
        .native_iterator_delegate()
        .is_some_and(|s| s.recursive.is_some())
    {
        return true;
    }
    error(
        eg,
        "Error",
        &format!(
            "The {} instance wasn't initialized properly",
            object.class_name
        ),
    );
    false
}

fn read<T>(receiver: &Value, project: impl FnOnce(&RecursiveTraversal) -> T) -> T {
    let object = receiver.as_object().expect("recursive method receiver");
    project(
        object
            .native_iterator_delegate()
            .expect("initialized")
            .recursive
            .as_ref()
            .expect("recursive"),
    )
}

fn write<T>(receiver: &Value, change: impl FnOnce(&mut RecursiveTraversal) -> T) -> T {
    let mut object = receiver.as_object_mut().expect("recursive method receiver");
    change(
        object
            .native_iterator_delegate_mut()
            .expect("initialized")
            .recursive
            .as_mut()
            .expect("recursive"),
    )
}

fn inner(receiver: &Value) -> Value {
    read(receiver, |state| {
        state.frames.last().expect("root frame").iterator.clone()
    })
}

fn hook(
    ed: *mut ExecuteData,
    receiver: &Value,
    name: &str,
    eg: &mut ExecutorGlobals,
) -> Result<Value, VmError> {
    let Some(resolved) = resolve_object_public_method(eg, receiver, name) else {
        return Ok(Value::null());
    };
    let source = crate::vm::execute::instruction_for_internal_call(ed);
    let implicit_iteration = source.is_some_and(|(opcode, _)| {
        matches!(
            opcode,
            OpCode::ForeachInit
                | OpCode::ForeachNext
                | OpCode::ForeachNextPlain
                | OpCode::ForeachNextRef
                | OpCode::YieldFrom
        )
    });
    if resolved.common().fn_type == FunctionType::User && !implicit_iteration {
        return call_resolved_with_values_from_internal(ed, eg, &resolved, &[], true);
    }
    Ok(
        call_object_protocol_method(eg, receiver, "RecursiveIteratorIterator", name, &[])?
            .unwrap_or_else(Value::null),
    )
}

/// CATCH_GET_CHILD applies to traversal callbacks, not to directly invoked
/// public methods. A failed has-children callback is treated as a leaf.
fn traversal_hook(
    ed: *mut ExecuteData,
    receiver: &Value,
    name: &str,
    eg: &mut ExecutorGlobals,
) -> Result<Value, VmError> {
    let result = hook(ed, receiver, name, eg)?;
    if eg.exception.is_some() && read(receiver, |s| s.flags & 16 != 0) {
        let exception = eg.exception.take().expect("callback exception");
        iterator_delegate::discard(exception, eg)?;
        return Ok(Value::null());
    }
    Ok(result)
}

fn recursive_protocol(
    iterator: &Value,
    name: &str,
    eg: &mut ExecutorGlobals,
) -> Result<Value, VmError> {
    Ok(
        call_object_protocol_method(eg, iterator, "RecursiveIterator", name, &[])?
            .unwrap_or_else(Value::null),
    )
}

/// A callback may replace the driver's complete stack. Retain its active
/// iterator only through the protocol call, then retire that temporary through
/// PHP's destructor boundary instead of silently dropping the last Rust owner.
fn inner_protocol(
    receiver: &Value,
    name: &str,
    eg: &mut ExecutorGlobals,
) -> Result<Value, VmError> {
    let iterator = inner(receiver);
    let result = iterator_delegate::protocol(eg, &iterator, name);
    iterator_delegate::discard(iterator, eg)?;
    result
}

fn inner_recursive_protocol(
    receiver: &Value,
    name: &str,
    eg: &mut ExecutorGlobals,
) -> Result<Value, VmError> {
    let iterator = inner(receiver);
    let result = recursive_protocol(&iterator, name, eg);
    iterator_delegate::discard(iterator, eg)?;
    result
}

fn phase(receiver: &Value, phase: RecursivePhase) {
    write(receiver, |state| {
        state.frames.last_mut().expect("root frame").phase = phase
    });
}

fn yield_element(
    ed: *mut ExecuteData,
    receiver: &Value,
    following: RecursivePhase,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // A callback can recursively ask for this element before its phase is
    // advanced. A throwing callback still commits the following phase.
    let generation = read(receiver, |s| s.generation);
    let result = traversal_hook(ed, receiver, "nextElement", eg);
    if read(receiver, |s| s.generation == generation) {
        phase(receiver, following);
    }
    result.map(|_| ())
}

/// Find the next visible node. Child iterators are acquired once per branch,
/// not once per leaf; ordinary traversal reuses the same allocated depth stack.
fn advance(
    ed: *mut ExecuteData,
    receiver: &Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    use RecursivePhase::*;
    while eg.exception.is_none() {
        let (state, depth, mode, max_depth, generation) = read(receiver, |s| {
            let frame = s.frames.last().expect("root frame");
            (
                frame.phase,
                s.frames.len() - 1,
                s.mode,
                s.max_depth,
                s.generation,
            )
        });
        match state {
            Advance => {
                inner_protocol(receiver, "next", eg)?;
                if eg.exception.is_some() {
                    if read(receiver, |s| s.flags & 16 != 0) {
                        let exception = eg.exception.take().expect("advance exception");
                        iterator_delegate::discard(exception, eg)?;
                    }
                    return Ok(());
                }
                if read(receiver, |s| s.generation != generation) {
                    continue;
                }
                phase(receiver, Check);
            }
            Check => {
                let valid = inner_protocol(receiver, "valid", eg)?;
                if eg.exception.is_some() {
                    break;
                }
                if read(receiver, |s| s.generation != generation) {
                    continue;
                }
                if !valid.is_truthy() {
                    if depth == 0 {
                        return Ok(());
                    }
                    traversal_hook(ed, receiver, "endChildren", eg)?;
                    if eg.exception.is_some() {
                        break;
                    }
                    if read(receiver, |s| s.generation != generation) {
                        continue;
                    }
                    let retired = write(receiver, |s| s.frames.pop().expect("child").iterator);
                    iterator_delegate::discard(retired, eg)?;
                    continue;
                }
                phase(receiver, Advance);
                let has_children = traversal_hook(ed, receiver, "callHasChildren", eg)?;
                if eg.exception.is_some() {
                    break;
                }
                if read(receiver, |s| s.generation != generation) {
                    continue;
                }
                if !has_children.is_truthy() {
                    phase(receiver, Check);
                    return yield_element(ed, receiver, Advance, eg);
                }
                if max_depth >= 0 && depth as i64 >= max_depth {
                    if mode != 0 {
                        phase(receiver, Check);
                        return yield_element(ed, receiver, Advance, eg);
                    }
                    continue;
                }
                phase(receiver, Descend);
                if mode == 1 {
                    phase(receiver, Check);
                    return yield_element(ed, receiver, Descend, eg);
                }
            }
            Descend => {
                phase(receiver, Advance);
                let child = hook(ed, receiver, "callGetChildren", eg)?;
                if eg.exception.is_some() {
                    if read(receiver, |s| s.flags & 16 != 0) {
                        let exception = eg.exception.take().expect("child exception");
                        iterator_delegate::discard(exception, eg)?;
                        continue;
                    }
                    break;
                }
                if read(receiver, |s| s.generation != generation) {
                    iterator_delegate::discard(child, eg)?;
                    continue;
                }
                if !child
                    .as_object()
                    .is_some_and(|o| eg.class_is_a(&o.class_name, "RecursiveIterator"))
                {
                    reject_child(child, eg)?;
                    break;
                }
                phase(receiver, AfterChildren);
                write(receiver, |s| {
                    s.frames.push(RecursiveFrame {
                        iterator: child,
                        phase: Check,
                    })
                });
                inner_protocol(receiver, "rewind", eg)?;
                if read(receiver, |s| s.generation != generation) {
                    continue;
                }
                if eg.exception.is_none() {
                    traversal_hook(ed, receiver, "beginChildren", eg)?;
                }
            }
            AfterChildren => {
                if mode == 2 {
                    return yield_element(ed, receiver, Advance, eg);
                }
                phase(receiver, Advance);
            }
        }
    }
    Ok(())
}

fn construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    let mut iterator = owned_argument(ed, 1);
    if iterator.value_type() != ValueType::Object {
        typed_internal_argument_error(
            eg,
            "RecursiveIteratorIterator::__construct",
            &iterator,
            1,
            "iterator",
            "object",
        );
        ret!(rv, Value::null());
    }
    let mut mode = 0;
    let mut flags = 0;
    for (index, name, target) in [(2, "mode", &mut mode), (3, "flags", &mut flags)] {
        if let Some(argument) = arg_opt!(ed, index) {
            let argument = argument.clone();
            let Some(value) = typed_internal_int_value_argument_expected(
                ed,
                eg,
                &argument,
                "RecursiveIteratorIterator::__construct",
                index - 1,
                name,
                "int",
            )?
            else {
                ret!(rv, Value::null());
            };
            *target = value;
        }
    }
    if !matches!(mode, 1 | 2) {
        mode = 0;
    }
    let mut seen = Vec::new();
    loop {
        let object = iterator.as_object().expect("validated iterator");
        if eg.class_is_a(&object.class_name, "RecursiveIterator") {
            break;
        }
        let aggregate = eg.class_is_a(&object.class_name, "IteratorAggregate");
        let aggregate_name = object.class_name.to_string();
        drop(object);
        if !aggregate || seen.contains(&iterator.object_identity().expect("object")) {
            error(
                eg,
                "InvalidArgumentException",
                "An instance of RecursiveIterator or IteratorAggregate creating it is required",
            );
            ret!(rv, Value::null());
        }
        seen.push(iterator.object_identity().expect("object"));
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
                "LogicException",
                &format!(
                    "{aggregate_name}::getIterator() must return an object that implements Traversable"
                ),
            );
            ret!(rv, Value::null());
        };
        iterator = next;
    }
    // Retain old edges before replacing native state so callbacks can only see
    // the fully published replacement, never a partially initialized stack.
    let mut retired = Vec::new();
    let mut generation = 0;
    if let Some(state) = receiver
        .as_object()
        .expect("receiver")
        .native_iterator_delegate()
    {
        state.for_each_value(|value| retired.push(value.clone()));
        generation = state
            .recursive
            .as_ref()
            .map_or(0, |s| s.generation.wrapping_add(1));
    }
    let mut state = NativeIteratorDelegate::new(Value::null(), Value::null(), 0, -1);
    state.recursive = Some(Box::new(RecursiveTraversal {
        frames: vec![RecursiveFrame {
            iterator,
            phase: RecursivePhase::Check,
        }],
        mode,
        flags,
        max_depth: -1,
        in_iteration: false,
        generation,
    }));
    receiver
        .as_object_mut()
        .expect("receiver")
        .set_native_iterator_delegate(state);
    for value in retired.into_iter().rev() {
        iterator_delegate::discard(value, eg)?;
    }
    ret!(rv, Value::null());
}

/// Native get-iterator admission precedes public rewind, including an override
/// that never calls its parent. Direct method calls have their own diagnostic.
pub(crate) fn validate_start(receiver: &Value, eg: &mut ExecutorGlobals) -> bool {
    let Some(object) = receiver.as_object() else {
        return true;
    };
    if object
        .native_iterator_delegate()
        .is_some_and(|state| state.recursive.is_some())
        || !eg.class_is_a(&object.class_name, "RecursiveIteratorIterator")
    {
        return true;
    }
    drop(object);
    error(eg, "Error", "Object is not initialized");
    false
}

fn rewind(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let generation = write(&receiver, |s| {
        s.generation = s.generation.wrapping_add(1);
        s.generation
    });
    while read(&receiver, |s| s.frames.len()) > 1 {
        let retired = write(&receiver, |s| s.frames.pop().expect("child").iterator);
        iterator_delegate::discard(retired, eg)?;
        if eg.exception.is_some() {
            ret!(rv, Value::null());
        }
        if read(&receiver, |s| s.generation != generation) {
            ret!(rv, Value::null());
        }
        hook(ed, &receiver, "endChildren", eg)?;
        if eg.exception.is_some() {
            ret!(rv, Value::null());
        }
    }
    phase(&receiver, RecursivePhase::Check);
    inner_protocol(&receiver, "rewind", eg)?;
    if read(&receiver, |s| s.generation != generation) {
        ret!(rv, Value::null());
    }
    if eg.exception.is_none() && !read(&receiver, |s| s.in_iteration) {
        write(&receiver, |s| s.in_iteration = true);
        hook(ed, &receiver, "beginIteration", eg)?;
    }
    if eg.exception.is_none() {
        advance(ed, &receiver, eg)?;
    }
    ret!(rv, Value::null());
}

fn next(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if initialized(&receiver, eg) {
        advance(ed, &receiver, eg)?;
    }
    ret!(rv, Value::null());
}

fn valid(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let value = inner_protocol(&receiver, "valid", eg)?;
    if eg.exception.is_none() && !value.is_truthy() && read(&receiver, |s| s.in_iteration) {
        write(&receiver, |s| s.in_iteration = false);
        hook(ed, &receiver, "endIteration", eg)?;
    }
    ret!(rv, Value::bool(value.is_truthy()));
}

macro_rules! projection {
    ($function:ident, $protocol:literal) => {
        fn $function(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            let receiver = owned_argument(ed, 0);
            if !initialized(&receiver, eg) {
                ret!(rv, Value::null());
            }
            let value = inner_protocol(&receiver, $protocol, eg)?;
            ret!(rv, value);
        }
    };
}
projection!(current, "current");
projection!(key, "key");

fn get_depth(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    ret!(
        rv,
        Value::long(read(&receiver, |s| s.frames.len() as i64 - 1))
    );
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
    ret!(rv, inner(&receiver));
}

fn get_sub(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let mut level = read(&receiver, |s| s.frames.len() as i64 - 1);
    if let Some(argument) =
        arg_opt!(ed, 1).filter(|v| v.dereferenced().value_type() != ValueType::Null)
    {
        let argument = argument.clone();
        let Some(value) = typed_internal_int_value_argument_expected(
            ed,
            eg,
            &argument,
            "RecursiveIteratorIterator::getSubIterator",
            0,
            "level",
            "?int",
        )?
        else {
            ret!(rv, Value::null());
        };
        level = value;
    }
    ret!(
        rv,
        read(&receiver, |s| usize::try_from(level)
            .ok()
            .and_then(|level| s.frames.get(level))
            .map_or_else(Value::null, |f| f.iterator.clone()))
    );
}

fn no_op(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    initialized(&owned_argument(ed, 0), eg);
    ret!(rv, Value::null());
}

fn call_has(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let value = inner_recursive_protocol(&receiver, "hasChildren", eg)?;
    ret!(rv, Value::bool(value.is_truthy()));
}

fn call_get(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let value = inner_recursive_protocol(&receiver, "getChildren", eg)?;
    ret!(rv, value);
}

fn get_max(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let depth = read(&receiver, |s| s.max_depth);
    ret!(
        rv,
        if depth == -1 {
            Value::bool(false)
        } else {
            Value::long(depth)
        }
    );
}

fn set_max(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    let mut depth = -1;
    if let Some(argument) = arg_opt!(ed, 1) {
        let argument = argument.clone();
        let Some(value) = typed_internal_int_value_argument_expected(
            ed,
            eg,
            &argument,
            "RecursiveIteratorIterator::setMaxDepth",
            0,
            "maxDepth",
            "int",
        )?
        else {
            ret!(rv, Value::null());
        };
        depth = value;
    }
    if depth < -1 {
        error(
            eg,
            "ValueError",
            "RecursiveIteratorIterator::setMaxDepth(): Argument #1 ($maxDepth) must be greater than or equal to -1",
        );
    } else {
        write(&receiver, |s| s.max_depth = depth);
    }
    ret!(rv, Value::null());
}

fn array_has(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    use array_object::cursor::{Move, Projection, projected_entry};
    let receiver = owned_argument(ed, 0);
    let value = projected_entry(&receiver, Move::Current, Projection::Value, eg).map(|p| p.1);
    let arrays_only = receiver
        .as_object()
        .expect("receiver")
        .native_array_options()
        .flags
        & 4
        != 0;
    ret!(
        rv,
        Value::bool(value.is_some_and(|v| v.value_type() == ValueType::Array
            || (!arrays_only && v.value_type() == ValueType::Object)))
    );
}

fn array_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    use array_object::cursor::{Move, Projection, projected_entry};
    let receiver = owned_argument(ed, 0);
    let Some((_, value)) = projected_entry(&receiver, Move::Current, Projection::Value, eg) else {
        ret!(rv, Value::null());
    };
    let object = receiver.as_object().expect("receiver");
    let name = object.class_name.to_string();
    let flags = object.native_array_options().flags;
    let class_id = object.class_id;
    drop(object);
    if value
        .as_object()
        .is_some_and(|o| eg.class_is_a(&o.class_name, &name))
    {
        ret!(rv, value);
    }
    let class = eg
        .class_by_id(class_id)
        .expect("registered recursive array iterator");
    let child = Value::object(PhpObject::with_layout(
        class.class_id,
        class.property_layout.clone(),
        class.property_defaults.to_vec(),
    ));
    call_object_protocol_method(
        eg,
        &child,
        "RecursiveArrayIterator",
        "__construct",
        &[value, Value::long(flags as i64)],
    )?;
    ret!(rv, child);
}

fn abstract_has(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    error(
        eg,
        "Error",
        "Cannot call abstract method RecursiveIterator::hasChildren()",
    );
    ret!(rv, Value::null());
}
fn abstract_get(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    error(
        eg,
        "Error",
        "Cannot call abstract method RecursiveIterator::getChildren()",
    );
    ret!(rv, Value::null());
}

pub(super) fn constant(owner: &str, name: &str, value: i64) -> ClassConstantDefinition {
    ClassConstantDefinition {
        attributes: vec![],
        name: name.into(),
        value: Value::long(value),
        source_file: String::new(),
        evaluation_error: None,
        source_expression: None,
        callable_factory: None,
        evaluation_scope: None,
        value_is_deferred: false,
        visibility: Visibility::Public,
        declaring_class: owner.into(),
        type_hint: ParamTypeHint::Int,
        is_final: false,
    }
}

/// Registration runs once per request. Keep descriptor construction out of the
/// method bodies and borrow names/defaults instead of allocating a temporary
/// table whose contents would immediately be moved into runtime metadata.
#[cold]
#[inline(never)]
fn register_method(
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
        result,
        defaults,
        name != "__construct",
    );
    let mut function = Box::new(make_internal_method(
        handler,
        names.len() as u32 + 1,
        required,
        names.iter().map(|name| name.to_string()).collect(),
    ));
    function.common.sig.param_type_hints = hints;
    function.handler_validates_types = true;
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
                value.map(|value| match value {
                    "null" => Value::null(),
                    "-1" => Value::long(-1),
                    _ => Value::long(0),
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
    use ParamTypeHint::{Bool, ClassName, Int, Mixed, Nullable, Void};
    let mut functions = Vec::with_capacity(22);
    macro_rules! method {
        ($owner:expr, $name:expr, $handler:expr, $result:expr) => {
            register_method(
                eg,
                &mut functions,
                $owner,
                $name,
                $handler,
                &[],
                vec![],
                &[],
                $result,
            );
        };
    }
    method!("RecursiveIterator", "hasChildren", abstract_has, Bool);
    method!(
        "RecursiveIterator",
        "getChildren",
        abstract_get,
        Nullable(Box::new(ClassName("RecursiveIterator".into())))
    );
    method!("RecursiveArrayIterator", "hasChildren", array_has, Bool);
    method!(
        "RecursiveArrayIterator",
        "getChildren",
        array_get,
        Nullable(Box::new(ClassName("RecursiveArrayIterator".into())))
    );
    let owner = "RecursiveIteratorIterator";
    register_method(
        eg,
        &mut functions,
        owner,
        "__construct",
        construct,
        &["iterator", "mode", "flags"],
        vec![ClassName("Traversable".into()), Int, Int],
        &[
            None,
            Some("RecursiveIteratorIterator::LEAVES_ONLY"),
            Some("0"),
        ],
        ParamTypeHint::None,
    );
    method!(owner, "rewind", rewind, Void);
    method!(owner, "valid", valid, Bool);
    method!(owner, "key", key, Mixed);
    method!(owner, "current", current, Mixed);
    method!(owner, "next", next, Void);
    method!(owner, "getDepth", get_depth, Int);
    register_method(
        eg,
        &mut functions,
        owner,
        "getSubIterator",
        get_sub,
        &["level"],
        vec![Nullable(Box::new(Int))],
        &[Some("null")],
        Nullable(Box::new(ClassName("RecursiveIterator".into()))),
    );
    method!(
        owner,
        "getInnerIterator",
        get_inner,
        ClassName("RecursiveIterator".into())
    );
    method!(owner, "beginIteration", no_op, Void);
    method!(owner, "endIteration", no_op, Void);
    method!(owner, "callHasChildren", call_has, Bool);
    method!(
        owner,
        "callGetChildren",
        call_get,
        Nullable(Box::new(ClassName("RecursiveIterator".into())))
    );
    method!(owner, "beginChildren", no_op, Void);
    method!(owner, "endChildren", no_op, Void);
    method!(owner, "nextElement", no_op, Void);
    register_method(
        eg,
        &mut functions,
        owner,
        "setMaxDepth",
        set_max,
        &["maxDepth"],
        vec![Int],
        &[Some("-1")],
        Void,
    );
    method!(
        owner,
        "getMaxDepth",
        get_max,
        ParamTypeHint::Union(vec![Int, ClassName("false".into())])
    );
    functions
}
