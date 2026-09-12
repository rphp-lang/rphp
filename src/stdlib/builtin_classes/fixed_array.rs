//! Fixed null-initialized slots, independently owned by each native object.
//! All PHP edges are traced; callbacks run only after releasing object borrows.
use super::*;
use crate::value::NativeObjectState;
use crate::vm::function::InternalFunctionHandler;

#[derive(Default)]
struct FixedArray {
    slots: Vec<Value>,
    initialized: bool,
    resizing: bool,
    pending_size: Option<usize>,
}

impl NativeObjectState for FixedArray {
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        Box::new(Self {
            slots: self
                .slots
                .iter()
                .map(|v| v.dereferenced().clone())
                .collect(),
            initialized: self.initialized,
            resizing: false,
            pending_size: None,
        })
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        for value in &self.slots {
            visit(value);
        }
    }
    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        pending.extend(self.slots.drain(..).rev());
    }
}

struct FixedCursor {
    owner: Value,
    position: usize,
}
impl Default for FixedCursor {
    fn default() -> Self {
        Self {
            owner: Value::null(),
            position: 0,
        }
    }
}
impl NativeObjectState for FixedCursor {
    fn retains_cycle_root(&self) -> bool {
        true
    }
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        // InternalIterator cloning is rejected by the existing object handler.
        Box::new(Self {
            owner: self.owner.clone(),
            position: self.position,
        })
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        visit(&self.owner);
    }
    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        pending.push(std::mem::replace(&mut self.owner, Value::null()));
    }
}

fn error(eg: &mut ExecutorGlobals, kind: &str, message: &str) {
    eg.exception = Some(make_error_value(kind, message));
}
fn size(receiver: &Value) -> usize {
    receiver
        .as_object()
        .expect("fixed-array receiver")
        .native_object_state::<FixedArray>()
        .map_or(0, |state| state.slots.len())
}
fn snapshot(receiver: &Value) -> PhpArray {
    let object = receiver.as_object().expect("fixed-array receiver");
    let Some(state) = object.native_object_state::<FixedArray>() else {
        return PhpArray::new();
    };
    let mut result = PhpArray::with_packed_capacity(state.slots.len());
    for value in &state.slots {
        result.push(value.dereferenced().clone());
    }
    result
}

#[cold]
pub(in crate::stdlib) fn slot_projection(
    receiver: &Value,
    eg: &ExecutorGlobals,
) -> Option<PhpArray> {
    if !has_fixed_slots(receiver, eg) {
        return None;
    }
    Some(snapshot(receiver))
}

#[cold]
pub(in crate::stdlib) fn has_fixed_slots(receiver: &Value, eg: &ExecutorGlobals) -> bool {
    let Some(object) = receiver.as_object() else {
        return false;
    };
    object.native_object_state::<FixedArray>().is_some()
        || eg.class_is_a(&object.class_name, "SplFixedArray")
}

#[cold]
pub(in crate::stdlib) fn member_projection(
    receiver: &Value,
    eg: &ExecutorGlobals,
) -> Option<PhpArray> {
    if !has_fixed_slots(receiver, eg) {
        return None;
    }
    Some(array_object::member_properties(receiver, eg))
}

#[cold]
pub(in crate::stdlib) fn array_cast(receiver: &Value, eg: &ExecutorGlobals) -> Option<Value> {
    let mut array = slot_projection(receiver, eg)?;
    for (key, value) in array_object::member_properties(receiver, eg).iter() {
        match key {
            ArrayKey::Int(key) => array.set_int(key, value.clone_for_php_storage()),
            ArrayKey::String(key) => array.set_str(&key, value.clone_for_php_storage()),
        }
    }
    Some(Value::array(array))
}

fn object(eg: &ExecutorGlobals, name: &str) -> Value {
    let class = eg.find_class(name).expect("registered native class");
    Value::object(PhpObject::with_layout_from_defaults(
        class.class_id,
        class.property_layout.clone(),
        class.property_defaults.as_ref(),
    ))
}

fn size_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
    optional: bool,
) -> Result<Option<usize>, VmError> {
    let argument = if optional {
        arg_opt!(ed, 1)
    } else {
        Some(arg!(ed, 1))
    };
    let Some(argument) = argument else {
        return Ok(Some(0));
    };
    let Some(value) = typed_internal_int_value_argument_expected(
        ed,
        eg,
        argument,
        &format!("SplFixedArray::{method}"),
        0,
        "size",
        "int",
    )?
    else {
        return Ok(None);
    };
    if eg.exception.is_some() {
        return Ok(None);
    }
    if value < 0 {
        error(
            eg,
            "ValueError",
            &format!(
                "SplFixedArray::{method}(): Argument #1 ($size) must be greater than or equal to 0"
            ),
        );
        return Ok(None);
    }
    Ok(Some(value as usize))
}

fn reserve_slots(
    slots: &mut Vec<Value>,
    requested: usize,
    ed: *mut ExecuteData,
) -> Result<(), VmError> {
    if requested <= slots.len() {
        return Ok(());
    }
    let slot = std::mem::size_of::<Value>();
    let Some(bytes) = requested.checked_mul(slot) else {
        let (file, line) = internal_call_source(ed);
        return Err(VmError::Fatal(format!(
            "Possible integer overflow in memory allocation ({requested} * {slot} + 0) in {file} on line {line}"
        )));
    };
    // A PHP capacity must never unwind through Rust's Vec allocator. Host
    // allocation limits remain distinct from PHP's configured memory limit.
    slots
        .try_reserve_exact(requested - slots.len())
        .map_err(|_| {
            let (file, line) = internal_call_source(ed);
            VmError::Fatal(format!(
                "Unable to allocate {bytes} bytes for SplFixedArray in {file} on line {line}"
            ))
        })
}

fn resize(
    receiver: &Value,
    requested: usize,
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    {
        let mut object = receiver.as_object_mut().expect("fixed-array receiver");
        let state = object.native_object_state_mut::<FixedArray>();
        if !state.initialized {
            return Ok(());
        }
        if state.resizing {
            state.pending_size = Some(requested);
            return Ok(());
        }
        state.resizing = true;
    }
    let mut requested = requested;
    loop {
        let retired = {
            let mut object = receiver.as_object_mut().expect("fixed-array receiver");
            let state = object.native_object_state_mut::<FixedArray>();
            if requested < state.slots.len() {
                state.slots.split_off(requested)
            } else {
                if let Err(error) = reserve_slots(&mut state.slots, requested, ed) {
                    state.resizing = false;
                    return Err(error);
                }
                state.slots.resize_with(requested, Value::null);
                Vec::new()
            }
        };
        // Publish the size first, then release in ascending slot order. Nested
        // setSize requests are last-writer-wins and applied after retirement.
        for value in retired {
            let previous = eg.exception.take();
            let result = iterator_delegate::discard(value, eg);
            if let Some(previous) = previous {
                if let Some(current) = &eg.exception {
                    crate::vm::execute::append_replaced_exception(current, &previous, eg);
                } else {
                    eg.exception = Some(previous);
                }
            }
            result?;
        }
        let mut object = receiver.as_object_mut().expect("fixed-array receiver");
        let state = object.native_object_state_mut::<FixedArray>();
        if let Some(next) = state.pending_size.take() {
            requested = next;
        } else {
            state.resizing = false;
            return Ok(());
        }
    }
}

fn construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(size) = size_argument(ed, eg, "__construct", true)? else {
        return Ok(());
    };
    let receiver = arg!(ed, 0);
    let mut object = receiver.as_object_mut().expect("fixed-array receiver");
    let state = object.native_object_state_mut::<FixedArray>();
    if state.slots.is_empty() {
        reserve_slots(&mut state.slots, size, ed)?;
        state.slots.resize_with(size, Value::null);
        state.initialized = true;
    }
    Ok(())
}
fn get_size(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::long(size(arg!(ed, 0)) as i64));
}
fn set_size(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(size) = size_argument(ed, eg, "setSize", false)? else {
        return Ok(());
    };
    resize(arg!(ed, 0), size, ed, eg)?;
    ret!(rv, Value::bool(true));
}
fn to_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::array(snapshot(arg!(ed, 0))));
}

fn index(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<i64>, VmError> {
    let value = value.dereferenced();
    let key = if matches!(
        value.value_type(),
        ValueType::Null | ValueType::Object | ValueType::Array | ValueType::Closure
    ) {
        None
    } else {
        match array_object_offset_key(ed, eg, value, ArrayObjectOffsetOperation::Access)? {
            Some(ArrayKey::Int(key)) => Some(key),
            _ => None,
        }
    };
    if eg.exception.is_some() {
        return Ok(None);
    }
    if key.is_none() {
        error(
            eg,
            "TypeError",
            &format!(
                "Cannot access offset of type {} on SplFixedArray",
                value.diagnostic_type_name()
            ),
        );
    }
    Ok(key)
}
fn valid_index(receiver: &Value, key: i64, eg: &mut ExecutorGlobals) -> Option<usize> {
    if key >= 0 && (key as usize) < size(receiver) {
        Some(key as usize)
    } else {
        error(eg, "OutOfBoundsException", "Index invalid or out of range");
        None
    }
}
fn offset_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let key = if array_object_offset_get_context(ed) == ArrayObjectOffsetGetContext::AfterExists {
        match array_object_offset_key_after_exists(arg!(ed, 1)) {
            Some(ArrayKey::Int(key)) => Some(key),
            _ => None,
        }
    } else {
        index(ed, eg, arg!(ed, 1))?
    };
    let Some(key) = key else {
        return Ok(());
    };
    let Some(key) = valid_index(arg!(ed, 0), key, eg) else {
        return Ok(());
    };
    let result = arg!(ed, 0)
        .as_object()
        .expect("fixed-array receiver")
        .native_object_state::<FixedArray>()
        .expect("valid slot")
        .slots[key]
        .dereferenced()
        .clone();
    ret!(rv, result);
}
fn replace(
    receiver: &Value,
    key: usize,
    value: Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let previous = {
        let mut object = receiver.as_object_mut().expect("fixed-array receiver");
        let state = object.native_object_state_mut::<FixedArray>();
        std::mem::replace(&mut state.slots[key], value)
    };
    iterator_delegate::discard(previous, eg)
}
fn offset_set(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(key) = index(ed, eg, arg!(ed, 1))? else {
        return Ok(());
    };
    let Some(key) = valid_index(arg!(ed, 0), key, eg) else {
        return Ok(());
    };
    if crate::vm::execute::instruction_for_internal_call(ed).is_some_and(|(opcode, flags)| {
        opcode == OpCode::AssignDim && flags & crate::vm::instruction::ASSIGN_DIM_REFERENCE != 0
    }) {
        let notice_class = {
            let object = arg!(ed, 0).as_object().expect("fixed-array receiver");
            let value = &object
                .native_object_state::<FixedArray>()
                .expect("valid slot")
                .slots[key];
            (!matches!(
                value.dereferenced().value_type(),
                ValueType::Object | ValueType::Closure
            ))
            .then(|| object.class_name.to_string())
        };
        if let Some(name) = notice_class {
            report_internal_diagnostic(
                eg,
                ed,
                8,
                "Notice",
                &format!("Indirect modification of overloaded element of {name} has no effect"),
            )?;
        }
        if eg.exception.is_none() {
            error(
                eg,
                "Error",
                "Cannot assign by reference to an array dimension of an object",
            );
        }
        return Ok(());
    }
    replace(arg!(ed, 0), key, arg!(ed, 2).dereferenced().clone(), eg)
}
fn offset_unset(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(key) = index(ed, eg, arg!(ed, 1))? else {
        return Ok(());
    };
    let Some(key) = valid_index(arg!(ed, 0), key, eg) else {
        return Ok(());
    };
    replace(arg!(ed, 0), key, Value::null(), eg)
}
fn offset_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(key) = index(ed, eg, arg!(ed, 1))? else {
        return Ok(());
    };
    let found = key >= 0
        && arg!(ed, 0)
            .as_object()
            .expect("fixed-array receiver")
            .native_object_state::<FixedArray>()
            .and_then(|state| state.slots.get(key as usize))
            .is_some_and(|v| v.value_type() != ValueType::Null);
    ret!(rv, Value::bool(found));
}

fn from_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // Internal static methods retain the reserved receiver slot as CV0.
    let input = arg!(ed, 1).dereferenced().clone();
    if input.value_type() != ValueType::Array {
        typed_internal_argument_error(eg, "SplFixedArray::fromArray", &input, 1, "array", "array");
        return Ok(());
    }
    let preserve = match arg_opt!(ed, 2) {
        None => true,
        Some(value) => {
            let Some(value) = typed_internal_bool_value_argument(
                ed,
                eg,
                value,
                "SplFixedArray::fromArray",
                1,
                "preserveKeys",
            )?
            else {
                return Ok(());
            };
            value
        }
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    let input = input.as_array().expect("validated array");
    let mut count = input.len();
    if preserve {
        count = 0;
        for (key, _) in input.iter() {
            let ArrayKey::Int(key) = key else {
                error(
                    eg,
                    "InvalidArgumentException",
                    "array must contain only positive integer keys",
                );
                return Ok(());
            };
            if key < 0 {
                error(
                    eg,
                    "InvalidArgumentException",
                    "array must contain only positive integer keys",
                );
                return Ok(());
            }
            let Some(size) = key.checked_add(1) else {
                error(eg, "InvalidArgumentException", "integer overflow detected");
                return Ok(());
            };
            count = count.max(size as usize);
        }
    }
    let result = object(eg, "SplFixedArray");
    {
        let mut object = result.as_object_mut().expect("new fixed array");
        let state = object.native_object_state_mut::<FixedArray>();
        reserve_slots(&mut state.slots, count, ed)?;
        state.initialized = true;
        state.slots.resize_with(count, Value::null);
        for (position, (key, value)) in input.iter().enumerate() {
            let index = if preserve {
                let ArrayKey::Int(index) = key else {
                    unreachable!()
                };
                index as usize
            } else {
                position
            };
            state.slots[index] = value.dereferenced().clone();
        }
    }
    ret!(rv, result);
}

fn get_iterator(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let iterator = object(eg, "InternalIterator");
    iterator
        .as_object_mut()
        .expect("internal iterator")
        .native_object_state_mut::<FixedCursor>()
        .owner = arg!(ed, 0).clone();
    ret!(rv, iterator);
}

#[cold]
pub(in crate::stdlib) fn iterator_operation(
    receiver: &Value,
    method: &str,
    eg: &mut ExecutorGlobals,
) -> Option<Value> {
    let object = receiver.as_object()?;
    let cursor = object.native_object_state::<FixedCursor>()?;
    let owner = cursor.owner.clone();
    let position = cursor.position;
    drop(object);
    Some(match method {
        "rewind" | "next" => {
            let mut object = receiver.as_object_mut().expect("internal iterator");
            object.native_object_state_mut::<FixedCursor>().position = if method == "rewind" {
                0
            } else {
                position.saturating_add(1)
            };
            Value::null()
        }
        "key" => Value::long(position as i64),
        "valid" => Value::bool(position < size(&owner)),
        "current" => {
            if let Some(index) = valid_index(&owner, position as i64, eg) {
                owner
                    .as_object()
                    .expect("fixed owner")
                    .native_object_state::<FixedArray>()
                    .expect("valid slot")
                    .slots[index]
                    .dereferenced()
                    .clone()
            } else {
                Value::null()
            }
        }
        _ => unreachable!("internal cursor operation"),
    })
}

#[cold]
fn method(
    eg: &mut ExecutorGlobals,
    functions: &mut Vec<Box<InternalFunction>>,
    name: &'static str,
    handler: InternalFunctionHandler,
    names: &[&str],
    hints: Vec<ParamTypeHint>,
    defaults: &[Option<&str>],
    result: ParamTypeHint,
    tentative: bool,
    is_static: bool,
) {
    let required = defaults.iter().filter(|d| d.is_none()).count() as u32;
    eg.register_internal_method_contract(
        "SplFixedArray",
        name,
        is_static,
        required,
        names,
        hints.clone(),
        result.clone(),
        defaults,
        tentative,
    );
    let mut function = Box::new(make_internal_method(
        handler,
        names.len() as u32 + 1,
        required,
        names.iter().map(|n| n.to_string()).collect(),
    ));
    function.handler_validates_types = true;
    function.common.sig.param_type_hints = hints;
    if !tentative {
        function.common.sig.return_type_hint = result;
    }
    let pointer = &function.common as *const FunctionCommon;
    if is_static {
        eg.register_internal_static_method(pointer);
    }
    eg.function_table
        .insert(internal_method_lookup_name("SplFixedArray", name), pointer);
    eg.method_declaring_class
        .insert(pointer, "SplFixedArray".into());
    eg.register_internal_function_display_name(
        pointer,
        internal_method_display_name("SplFixedArray", name),
    );
    eg.register_internal_function_reflection_metadata(
        pointer,
        defaults
            .iter()
            .map(|d| {
                d.map(|d| match d {
                    "true" => Value::bool(true),
                    "0" => Value::long(0),
                    _ => unreachable!(),
                })
            })
            .collect(),
        "SPL",
    );
    functions.push(function);
}

#[cold]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Array, Bool, ClassName, Int, Mixed, None as NoType, Void};
    let mut functions = Vec::with_capacity(12);
    eg.reserve_internal_method_contracts("SplFixedArray", 12);
    method(
        eg,
        &mut functions,
        "__construct",
        construct,
        &["size"],
        vec![Int],
        &[Some("0")],
        NoType,
        false,
        false,
    );
    for (name, handler, result, tentative) in [
        ("count", get_size as InternalFunctionHandler, Int, true),
        ("getSize", get_size, Int, true),
        ("toArray", to_array, Array, true),
        ("jsonSerialize", to_array, Array, false),
        (
            "getIterator",
            get_iterator,
            ClassName("Iterator".into()),
            false,
        ),
    ] {
        method(
            eg,
            &mut functions,
            name,
            handler,
            &[],
            vec![],
            &[],
            result,
            tentative,
            false,
        );
    }
    method(
        eg,
        &mut functions,
        "setSize",
        set_size,
        &["size"],
        vec![Int],
        &[None],
        ClassName("true".into()),
        true,
        false,
    );
    method(
        eg,
        &mut functions,
        "fromArray",
        from_array,
        &["array", "preserveKeys"],
        vec![Array, Bool],
        &[None, Some("true")],
        ClassName("SplFixedArray".into()),
        true,
        true,
    );
    for (name, handler, result) in [
        ("offsetGet", offset_get as InternalFunctionHandler, Mixed),
        ("offsetExists", offset_exists, Bool),
        ("offsetUnset", offset_unset, Void),
    ] {
        method(
            eg,
            &mut functions,
            name,
            handler,
            &["index"],
            vec![NoType],
            &[None],
            result,
            true,
            false,
        );
    }
    method(
        eg,
        &mut functions,
        "offsetSet",
        offset_set,
        &["index", "value"],
        vec![NoType, Mixed],
        &[None, None],
        Void,
        true,
        false,
    );
    functions
}

#[cold]
pub(in crate::stdlib) fn value_after_exists(
    receiver: &Value,
    key: Option<&Value>,
    eg: &ExecutorGlobals,
) -> Option<Value> {
    if !has_fixed_slots(receiver, eg) {
        return None;
    }
    let resolved = resolve_object_public_method(eg, receiver, "offsetExists")?;
    if eg.find_function(&internal_method_lookup_name(
        "SplFixedArray",
        "offsetExists",
    )) != Some(resolved.func_ptr)
    {
        // The native has-dimension handler treats an overridden existence
        // result as authoritative; it never invokes a second user offsetGet.
        return Some(Value::bool(true));
    }
    let Some(ArrayKey::Int(key)) = key.and_then(array_object_offset_key_after_exists) else {
        return Some(Value::null());
    };
    let object = receiver.as_object()?;
    Some(if key < 0 {
        Value::null()
    } else {
        object
            .native_object_state::<FixedArray>()
            .and_then(|state| state.slots.get(key as usize))
            .map_or_else(Value::null, |value| value.dereferenced().clone())
    })
}

#[cold]
pub(in crate::stdlib) fn reject_native_append(
    receiver: &Value,
    method: &str,
    eg: &mut ExecutorGlobals,
) -> bool {
    let Some(resolved) = resolve_object_public_method(eg, receiver, method) else {
        return false;
    };
    if eg.find_function(&internal_method_lookup_name("SplFixedArray", method))
        != Some(resolved.func_ptr)
    {
        return false;
    }
    error(eg, "Error", "[] operator not supported for SplFixedArray");
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn fixed_slot_edges_are_hidden_traced_and_drained_in_php_order() {
        let mut object = PhpObject::dynamic("SlotOwner".into(), 0, HashMap::new());
        let state = object.native_object_state_mut::<FixedArray>();
        state.initialized = true;
        state.slots = vec![
            Value::long(7),
            Value::array(PhpArray::new()),
            Value::long(9),
        ];
        object.for_each_property(|_, _| panic!("native slot is not a PHP property"));
        let mut tags = Vec::new();
        object.for_each_owned_value(|value| tags.push(value.value_type()));
        assert_eq!(
            tags,
            vec![ValueType::Long, ValueType::Array, ValueType::Long]
        );
        assert!(object.any_property_value(|value| value.value_type() == ValueType::Array));
        let mut pending = Vec::new();
        object
            .native_object_state_mut::<FixedArray>()
            .append_values_reversed(&mut pending);
        assert_eq!(pending.pop().unwrap().as_long(), Some(7));
        assert_eq!(pending.pop().unwrap().value_type(), ValueType::Array);
        assert_eq!(pending.pop().unwrap().as_long(), Some(9));
        assert!(!object.any_property_value(|_| true));
    }

    #[test]
    fn cursor_exposes_its_strong_owner_and_clone_copies_slots() {
        let mut source = FixedArray {
            slots: vec![Value::long(4)],
            initialized: true,
            ..FixedArray::default()
        };
        let cloned = source.clone_state();
        source.slots[0] = Value::long(8);
        assert_eq!(
            cloned.as_any().downcast_ref::<FixedArray>().unwrap().slots[0].as_long(),
            Some(4)
        );
        let mut cursor = FixedCursor {
            owner: Value::array(PhpArray::new()),
            position: 3,
        };
        let mut count = 0;
        cursor.for_each_value(&mut |value| {
            assert_eq!(value.value_type(), ValueType::Array);
            count += 1;
        });
        assert_eq!(count, 1);
        let mut pending = Vec::new();
        cursor.append_values_reversed(&mut pending);
        assert_eq!(pending.len(), 1);
        assert_eq!(cursor.owner.value_type(), ValueType::Null);
    }
}
