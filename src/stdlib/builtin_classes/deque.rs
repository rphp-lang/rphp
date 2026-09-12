//! Stable native deque nodes. Removed nodes lose their PHP value immediately;
//! scalar cursor owners may retain the empty node until the next movement.
use super::*;
use crate::value::NativeObjectState;
use crate::vm::function::InternalFunctionHandler;
use std::cell::RefCell;
use std::rc::Rc;

type NodeRef = Rc<RefCell<Node>>;
struct Node {
    slot: usize,
    previous: Option<usize>,
    next: Option<usize>,
    value: Value,
}
#[derive(Clone, Default)]
struct Position {
    node: Option<NodeRef>,
    key: i64,
}
#[derive(Default)]
struct Deque {
    nodes: Vec<Option<NodeRef>>,
    free: Vec<usize>,
    head: Option<usize>,
    tail: Option<usize>,
    len: usize,
    mode: u8,
    cursor: Position,
}
impl Deque {
    fn node(&self, slot: usize) -> NodeRef {
        self.nodes[slot]
            .as_ref()
            .expect("linked native node")
            .clone()
    }
    fn insert_before(&mut self, before: Option<usize>, value: Value) {
        let previous = before.map_or(self.tail, |slot| self.node(slot).borrow().previous);
        let slot = self.free.pop().unwrap_or_else(|| {
            self.nodes.push(None);
            self.nodes.len() - 1
        });
        self.nodes[slot] = Some(Rc::new(RefCell::new(Node {
            slot,
            previous,
            next: before,
            value,
        })));
        if let Some(previous) = previous {
            self.node(previous).borrow_mut().next = Some(slot);
        } else {
            self.head = Some(slot);
        }
        if let Some(before) = before {
            self.node(before).borrow_mut().previous = Some(slot);
        } else {
            self.tail = Some(slot);
        }
        self.len += 1;
    }
    fn remove(&mut self, node: &NodeRef, invalidate_manual: bool) -> Value {
        let mut node_value = node.borrow_mut();
        let slot = node_value.slot;
        if !self
            .nodes
            .get(slot)
            .and_then(Option::as_ref)
            .is_some_and(|current| Rc::ptr_eq(current, node))
        {
            return Value::null();
        }
        let previous = node_value.previous.take();
        let next = node_value.next.take();
        if let Some(previous) = previous {
            self.node(previous).borrow_mut().next = next;
        } else {
            self.head = next;
        }
        if let Some(next) = next {
            self.node(next).borrow_mut().previous = previous;
        } else {
            self.tail = previous;
        }
        self.nodes[slot] = None;
        self.free.push(slot);
        self.len -= 1;
        if invalidate_manual
            && self
                .cursor
                .node
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(current, node))
        {
            self.cursor.node = None;
        }
        std::mem::replace(&mut node_value.value, Value::null())
    }
    fn at(&self, index: usize) -> Option<NodeRef> {
        if index >= self.len {
            return None;
        }
        let physical = if self.mode & 2 != 0 {
            self.len - 1 - index
        } else {
            index
        };
        let from_tail = physical > self.len / 2;
        let mut slot = if from_tail { self.tail } else { self.head }?;
        let steps = if from_tail {
            self.len - 1 - physical
        } else {
            physical
        };
        for _ in 0..steps {
            let node = self.node(slot);
            let node = node.borrow();
            slot = if from_tail { node.previous } else { node.next }?;
        }
        Some(self.node(slot))
    }
    fn rewind(&self, position: &mut Position, mode: u8) {
        position.node =
            (if mode & 2 != 0 { self.tail } else { self.head }).map(|slot| self.node(slot));
        position.key = if mode & 2 != 0 {
            self.len as i64 - 1
        } else {
            0
        };
    }
    fn advance(&mut self, position: &mut Position, mode: u8, backward: bool) -> Option<Value> {
        let node = position.node.take()?;
        let reverse = (mode & 2 != 0) ^ backward;
        let next = {
            let node = node.borrow();
            if reverse { node.previous } else { node.next }
        };
        position.node = next.map(|slot| self.node(slot));
        if reverse {
            position.key = position.key.wrapping_sub(1);
        } else if mode & 1 == 0 {
            position.key = position.key.wrapping_add(1);
        }
        // DELETE drains the traversal end, not necessarily the current node:
        // a manual cursor may have moved in KEEP mode or changed direction.
        if mode & 1 != 0 {
            let end = if reverse { self.tail } else { self.head };
            end.map(|slot| self.remove(&self.node(slot), false))
        } else {
            None
        }
    }
}
impl NativeObjectState for Deque {
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        let mut copy = Deque {
            mode: self.mode,
            ..Deque::default()
        };
        let mut slot = self.head;
        while let Some(current) = slot {
            let node = self.node(current);
            let node = node.borrow();
            copy.insert_before(None, node.value.dereferenced().clone());
            slot = node.next;
        }
        copy.cursor.node = copy.head.map(|slot| copy.node(slot));
        Box::new(copy)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        // The VM destructor planner consumes visitation order, independently
        // of the reverse-drain stack used by plain/deep Value release.
        let mut slot = self.tail;
        while let Some(current) = slot {
            let node = self.node(current);
            let node = node.borrow();
            visit(&node.value);
            slot = node.previous;
        }
    }
    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        // PHP retires this container from the right end. The release walker
        // pops from the back, so move values onto it in forward order.
        let mut slot = self.head;
        while let Some(current) = slot {
            let node = self.node(current);
            let mut node = node.borrow_mut();
            slot = node.next;
            pending.push(std::mem::replace(&mut node.value, Value::null()));
        }
        self.nodes.clear();
        self.free.clear();
        self.head = None;
        self.tail = None;
        self.len = 0;
        self.cursor.node = None;
    }
}

struct ConsumerCursor {
    owner: Value,
    position: Position,
    mode: u8,
}
impl Default for ConsumerCursor {
    fn default() -> Self {
        Self {
            owner: Value::null(),
            position: Position::default(),
            mode: 0,
        }
    }
}
impl NativeObjectState for ConsumerCursor {
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        Box::new(Self {
            owner: self.owner.clone(),
            position: self.position.clone(),
            mode: self.mode,
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
        self.position.node = None;
        pending.push(std::mem::replace(&mut self.owner, Value::null()));
    }
}

fn error(eg: &mut ExecutorGlobals, kind: &str, message: &str) {
    eg.exception = Some(make_error_value(kind, message));
}
fn ensure(receiver: &Value, eg: &ExecutorGlobals) {
    let mut object = receiver.as_object_mut().expect("deque receiver");
    if object.native_object_state::<Deque>().is_none() {
        let mode = if eg.class_is_a(&object.class_name, "SplStack") {
            6
        } else if eg.class_is_a(&object.class_name, "SplQueue") {
            4
        } else {
            0
        };
        object.native_object_state_mut::<Deque>().mode = mode;
    }
}
fn length(receiver: &Value) -> usize {
    receiver
        .as_object()
        .expect("deque receiver")
        .native_object_state::<Deque>()
        .map_or(0, |state| state.len)
}
fn integer(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
    parameter: &str,
) -> Result<Option<i64>, VmError> {
    let result = typed_internal_int_value_argument_expected(
        ed,
        eg,
        arg!(ed, 1),
        &format!("SplDoublyLinkedList::{method}"),
        0,
        parameter,
        "int",
    )?;
    Ok(if eg.exception.is_some() { None } else { result })
}
fn index(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
) -> Result<Option<usize>, VmError> {
    let Some(index) = integer(ed, eg, method, "index")? else {
        return Ok(None);
    };
    let len = length(arg!(ed, 0));
    if index < 0 || index as usize > len || (index as usize == len && method != "add") {
        if method != "offsetExists" {
            error(
                eg,
                "OutOfRangeException",
                &format!("SplDoublyLinkedList::{method}(): Argument #1 ($index) is out of range"),
            );
        }
        return Ok(None);
    }
    Ok(Some(index as usize))
}
fn push(ed: *mut ExecuteData, _rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    arg!(ed, 0)
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<Deque>()
        .insert_before(None, arg!(ed, 1).dereferenced().clone());
    Ok(())
}
fn unshift(ed: *mut ExecuteData, _rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    let mut object = arg!(ed, 0).as_object_mut().unwrap();
    let state = object.native_object_state_mut::<Deque>();
    state.insert_before(state.head, arg!(ed, 1).dereferenced().clone());
    Ok(())
}
fn end(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    method: &str,
) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    let result = {
        let mut object = arg!(ed, 0).as_object_mut().unwrap();
        let state = object.native_object_state_mut::<Deque>();
        let slot = if matches!(method, "shift" | "bottom") {
            state.head
        } else {
            state.tail
        };
        slot.map(|slot| {
            let node = state.node(slot);
            if matches!(method, "shift" | "pop") {
                state.remove(&node, false)
            } else {
                node.borrow().value.dereferenced().clone()
            }
        })
    };
    let Some(value) = result else {
        let operation = if matches!(method, "top" | "bottom") {
            "peek at"
        } else {
            method
        };
        let message = if operation == "peek at" {
            "Can't peek at an empty datastructure".into()
        } else {
            format!("Can't {operation} from an empty datastructure")
        };
        error(eg, "RuntimeException", &message);
        return Ok(());
    };
    ret!(rv, value);
}
macro_rules! end_handler {
    ($name:ident) => {
        fn $name(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            end(ed, rv, eg, stringify!($name))
        }
    };
}
end_handler!(pop);
end_handler!(shift);
end_handler!(top);
end_handler!(bottom);
fn count(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::long(length(arg!(ed, 0)) as i64));
}
fn is_empty(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let value = call_object_protocol_method(eg, arg!(ed, 0), "Countable", "count", &[])?
        .unwrap_or_else(Value::null);
    ret!(rv, Value::bool(value.to_long_val() == 0));
}
fn get_mode(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    let mode = arg!(ed, 0)
        .as_object()
        .unwrap()
        .native_object_state::<Deque>()
        .unwrap()
        .mode;
    ret!(rv, Value::long(i64::from(mode)));
}
fn set_mode(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(mode) = integer(ed, eg, "setIteratorMode", "mode")? else {
        return Ok(());
    };
    ensure(arg!(ed, 0), eg);
    let mut object = arg!(ed, 0).as_object_mut().unwrap();
    let state = object.native_object_state_mut::<Deque>();
    if state.mode & 4 != 0 && state.mode & 2 != mode as u8 & 2 {
        error(
            eg,
            "RuntimeException",
            "Iterators' LIFO/FIFO modes for SplStack/SplQueue objects are frozen",
        );
        return Ok(());
    }
    state.mode = (state.mode & 4) | (mode as u8 & 3);
    ret!(rv, Value::long(i64::from(state.mode)));
}
fn add(ed: *mut ExecuteData, _rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    let Some(index) = index(ed, eg, "add")? else {
        return Ok(());
    };
    let mut object = arg!(ed, 0).as_object_mut().unwrap();
    let state = object.native_object_state_mut::<Deque>();
    let before = state.at(index).map(|node| node.borrow().slot);
    state.insert_before(before, arg!(ed, 2).dereferenced().clone());
    Ok(())
}
fn offset_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    let Some(index) = index(ed, eg, "offsetGet")? else {
        return Ok(());
    };
    let node = arg!(ed, 0)
        .as_object()
        .unwrap()
        .native_object_state::<Deque>()
        .unwrap()
        .at(index)
        .unwrap();
    let value = node.borrow().value.dereferenced().clone();
    ret!(rv, value);
}
fn offset_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exists = index(ed, eg, "offsetExists")?.is_some();
    ret!(rv, Value::bool(exists));
}
fn offset_unset(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    let Some(index) = index(ed, eg, "offsetUnset")? else {
        return Ok(());
    };
    let value = {
        let mut object = arg!(ed, 0).as_object_mut().unwrap();
        let state = object.native_object_state_mut::<Deque>();
        let node = state.at(index).unwrap();
        state.remove(&node, true)
    };
    iterator_delegate::discard(value, eg)
}
fn offset_set(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    if arg!(ed, 1).dereferenced().value_type() == ValueType::Null {
        arg!(ed, 0)
            .as_object_mut()
            .unwrap()
            .native_object_state_mut::<Deque>()
            .insert_before(None, arg!(ed, 2).dereferenced().clone());
        return Ok(());
    }
    let Some(index) = index(ed, eg, "offsetSet")? else {
        return Ok(());
    };
    let node = arg!(ed, 0)
        .as_object()
        .unwrap()
        .native_object_state::<Deque>()
        .unwrap()
        .at(index)
        .unwrap();
    if crate::vm::execute::instruction_for_internal_call(ed).is_some_and(|(opcode, flags)| {
        opcode == OpCode::AssignDim && flags & crate::vm::instruction::ASSIGN_DIM_REFERENCE != 0
    }) {
        if !matches!(
            node.borrow().value.value_type(),
            ValueType::Object | ValueType::Closure
        ) {
            let name = arg!(ed, 0).as_object().unwrap().class_name.to_string();
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
    let previous = std::mem::replace(
        &mut node.borrow_mut().value,
        arg!(ed, 2).dereferenced().clone(),
    );
    iterator_delegate::discard(previous, eg)
}

fn move_cursor(receiver: &Value, method: &str, eg: &mut ExecutorGlobals) -> Result<Value, VmError> {
    ensure(receiver, eg);
    let (value, retired) = {
        let mut object = receiver.as_object_mut().unwrap();
        let state = object.native_object_state_mut::<Deque>();
        let mut position = std::mem::take(&mut state.cursor);
        let retired = if method == "rewind" {
            state.rewind(&mut position, state.mode);
            None
        } else if matches!(method, "next" | "prev") {
            state.advance(&mut position, state.mode, method == "prev")
        } else {
            None
        };
        let value = project(&position, method);
        state.cursor = position;
        (value, retired)
    };
    if let Some(retired) = retired {
        iterator_delegate::discard(retired, eg)?;
    }
    Ok(value)
}
fn project(position: &Position, method: &str) -> Value {
    match method {
        "valid" => Value::bool(position.node.is_some()),
        "key" => Value::long(position.key),
        "current" => position.node.as_ref().map_or_else(Value::null, |node| {
            node.borrow().value.dereferenced().clone()
        }),
        _ => Value::null(),
    }
}
macro_rules! cursor_handler {
    ($name:ident) => {
        fn $name(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            let value = move_cursor(arg!(ed, 0), stringify!($name), eg)?;
            ret!(rv, value);
        }
    };
}
cursor_handler!(rewind);
cursor_handler!(next);
cursor_handler!(prev);
cursor_handler!(current);
cursor_handler!(key);
cursor_handler!(valid);

#[cold]
pub(crate) fn consumer(receiver: &Value, eg: &ExecutorGlobals) -> Option<Value> {
    let object = receiver.as_object()?;
    if !eg.class_is_a(&object.class_name, "SplDoublyLinkedList") {
        return None;
    }
    drop(object);
    for (name, native) in [
        ("rewind", "spldoublylinkedlist::rewind"),
        ("current", "spldoublylinkedlist::current"),
        ("key", "spldoublylinkedlist::key"),
        ("valid", "spldoublylinkedlist::valid"),
        ("next", "spldoublylinkedlist::next"),
    ] {
        if resolve_object_public_method(eg, receiver, name)?.func_ptr != eg.find_function(native)? {
            return None;
        }
    }
    ensure(receiver, eg);
    let mode = receiver.as_object()?.native_object_state::<Deque>()?.mode;
    let class = eg.find_class("InternalIterator")?;
    // Although the cursor never escapes, PHP reserves its object-store handle;
    // user allocations inside and after iteration observe that lifetime.
    let result = Value::object(PhpObject::with_layout_from_defaults(
        class.class_id,
        class.property_layout.clone(),
        class.property_defaults.as_ref(),
    ));
    let mut object = result.as_object_mut()?;
    let state = object.native_object_state_mut::<ConsumerCursor>();
    state.owner = receiver.clone();
    state.mode = mode;
    drop(object);
    Some(result)
}
#[cold]
pub(in crate::stdlib) fn consumer_operation(
    receiver: &Value,
    method: &str,
    eg: &mut ExecutorGlobals,
) -> Option<Result<Value, VmError>> {
    let mut object = receiver.as_object_mut()?;
    object.native_object_state::<ConsumerCursor>()?;
    let state = object.native_object_state_mut::<ConsumerCursor>();
    let owner = state.owner.clone();
    let mut position = std::mem::take(&mut state.position);
    let mode = state.mode;
    drop(object);
    let retired = {
        let mut object = owner.as_object_mut().expect("native cursor owner");
        let state = object.native_object_state_mut::<Deque>();
        if method == "rewind" {
            state.rewind(&mut position, mode);
            None
        } else if method == "next" {
            state.advance(&mut position, mode, false)
        } else {
            None
        }
    };
    let value = project(&position, method);
    receiver
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<ConsumerCursor>()
        .position = position;
    Some(if let Some(retired) = retired {
        iterator_delegate::discard(retired, eg).map(|()| value)
    } else {
        Ok(value)
    })
}
fn debug_info(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    let mut output = array_object::member_properties(arg!(ed, 0), eg);
    let object = arg!(ed, 0).as_object().unwrap();
    let state = object.native_object_state::<Deque>().unwrap();
    let mut values = PhpArray::with_packed_capacity(state.len);
    let mut slot = state.head;
    while let Some(current) = slot {
        let node = state.node(current);
        let node = node.borrow();
        values.push(node.value.dereferenced().clone());
        slot = node.next;
    }
    output.set_str(
        "\0SplDoublyLinkedList\0flags",
        Value::long(i64::from(state.mode)),
    );
    output.set_str("\0SplDoublyLinkedList\0dllist", Value::array(values));
    ret!(rv, Value::array(output));
}

#[cold]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Array, Bool, Int, Mixed, None as NoType, Void};
    let rows: &[(
        &str,
        &str,
        InternalFunctionHandler,
        &[&str],
        &[ParamTypeHint],
        ParamTypeHint,
    )] = &[
        (
            "SplDoublyLinkedList",
            "push",
            push,
            &["value"],
            &[Mixed],
            Void,
        ),
        (
            "SplDoublyLinkedList",
            "unshift",
            unshift,
            &["value"],
            &[Mixed],
            Void,
        ),
        (
            "SplDoublyLinkedList",
            "add",
            add,
            &["index", "value"],
            &[Int, Mixed],
            Void,
        ),
        ("SplDoublyLinkedList", "pop", pop, &[], &[], Mixed),
        ("SplDoublyLinkedList", "shift", shift, &[], &[], Mixed),
        ("SplDoublyLinkedList", "top", top, &[], &[], Mixed),
        ("SplDoublyLinkedList", "bottom", bottom, &[], &[], Mixed),
        ("SplDoublyLinkedList", "count", count, &[], &[], Int),
        ("SplDoublyLinkedList", "isEmpty", is_empty, &[], &[], Bool),
        (
            "SplDoublyLinkedList",
            "setIteratorMode",
            set_mode,
            &["mode"],
            &[Int],
            Int,
        ),
        (
            "SplDoublyLinkedList",
            "getIteratorMode",
            get_mode,
            &[],
            &[],
            Int,
        ),
        (
            "SplDoublyLinkedList",
            "offsetExists",
            offset_exists,
            &["index"],
            &[NoType],
            Bool,
        ),
        (
            "SplDoublyLinkedList",
            "offsetGet",
            offset_get,
            &["index"],
            &[NoType],
            Mixed,
        ),
        (
            "SplDoublyLinkedList",
            "offsetSet",
            offset_set,
            &["index", "value"],
            &[NoType, Mixed],
            Void,
        ),
        (
            "SplDoublyLinkedList",
            "offsetUnset",
            offset_unset,
            &["index"],
            &[NoType],
            Void,
        ),
        ("SplDoublyLinkedList", "rewind", rewind, &[], &[], Void),
        ("SplDoublyLinkedList", "next", next, &[], &[], Void),
        ("SplDoublyLinkedList", "prev", prev, &[], &[], Void),
        ("SplDoublyLinkedList", "current", current, &[], &[], Mixed),
        ("SplDoublyLinkedList", "key", key, &[], &[], Int),
        ("SplDoublyLinkedList", "valid", valid, &[], &[], Bool),
        (
            "SplDoublyLinkedList",
            "__debugInfo",
            debug_info,
            &[],
            &[],
            Array,
        ),
        ("SplQueue", "enqueue", push, &["value"], &[Mixed], Void),
        ("SplQueue", "dequeue", shift, &[], &[], Mixed),
    ];
    let mut functions = Vec::with_capacity(rows.len());
    eg.reserve_internal_method_contracts("SplDoublyLinkedList", 22);
    eg.reserve_internal_method_contracts("SplQueue", 2);
    for (owner, name, handler, names, hints, result) in rows {
        let required = names.len() as u32;
        let defaults = vec![None; names.len()];
        eg.register_internal_method_contract(
            owner,
            name,
            false,
            required,
            names,
            hints.to_vec(),
            result.clone(),
            &defaults,
            true,
        );
        let mut function = Box::new(make_internal_method(
            *handler,
            required + 1,
            required,
            names.iter().map(|n| n.to_string()).collect(),
        ));
        function.handler_validates_types = true;
        function.common.sig.param_type_hints = hints.to_vec();
        let pointer = &function.common as *const FunctionCommon;
        eg.function_table
            .insert(internal_method_lookup_name(owner, name), pointer);
        eg.method_declaring_class.insert(pointer, (*owner).into());
        eg.register_internal_function_display_name(
            pointer,
            internal_method_display_name(owner, name),
        );
        eg.register_internal_function_reflection_metadata(pointer, vec![None; names.len()], "SPL");
        functions.push(function);
    }
    functions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retired_node_cannot_alias_a_reused_arena_slot() {
        let mut deque = Deque::default();
        deque.insert_before(None, Value::long(11));
        let old = deque.node(deque.head.unwrap());
        let slot = old.borrow().slot;
        assert_eq!(deque.remove(&old, false).as_long().unwrap(), 11);
        deque.insert_before(None, Value::long(22));
        assert_eq!(deque.head, Some(slot));
        assert!(!Rc::ptr_eq(&old, &deque.node(slot)));
        assert_eq!(old.borrow().value.value_type(), ValueType::Null);
        assert_eq!(deque.remove(&old, false).value_type(), ValueType::Null);
        assert_eq!(deque.len, 1);
        assert_eq!(deque.node(slot).borrow().value.as_long().unwrap(), 22);
    }

    #[test]
    fn native_trace_and_release_publish_each_live_value_once() {
        let mut deque = Deque::default();
        for value in [11, 22, 33] {
            deque.insert_before(None, Value::long(value));
        }
        let removed = deque.at(1).unwrap();
        deque.remove(&removed, false);
        let mut traced = Vec::new();
        deque.for_each_value(&mut |value| traced.push(value.as_long().unwrap()));
        assert_eq!(traced, [33, 11]);
        let mut pending = Vec::new();
        deque.append_values_reversed(&mut pending);
        assert_eq!(pending.pop().unwrap().as_long().unwrap(), 33);
        assert_eq!(pending.pop().unwrap().as_long().unwrap(), 11);
        assert!(pending.is_empty());
        assert_eq!(deque.len, 0);
        assert!(deque.head.is_none() && deque.tail.is_none());
        assert_eq!(removed.borrow().value.value_type(), ValueType::Null);
    }

    #[test]
    fn consumer_traces_only_its_owner_not_duplicate_node_edges() {
        let mut deque = Deque::default();
        deque.insert_before(None, Value::long(31));
        let mut consumer = ConsumerCursor {
            owner: Value::long(47),
            ..ConsumerCursor::default()
        };
        deque.rewind(&mut consumer.position, 0);
        let mut traced = Vec::new();
        consumer.for_each_value(&mut |value| traced.push(value.as_long().unwrap()));
        assert_eq!(traced, [47]);
        let mut pending = Vec::new();
        consumer.append_values_reversed(&mut pending);
        assert!(consumer.position.node.is_none());
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].as_long().unwrap(), 47);
        assert_eq!(deque.len, 1);
    }
}
