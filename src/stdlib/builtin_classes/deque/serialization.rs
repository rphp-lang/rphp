//! Deque wire state is independent of the traversal cursor. Modern restore
//! appends in physical order and leaves all existing cursor owners intact.
use super::*;
use crate::stdlib::serialization::clone_unserialized_storage_value;

pub(in crate::stdlib) struct LegacyCursor {
    current: Option<NodeRef>,
    next: Option<NodeRef>,
}

impl LegacyCursor {
    pub(in crate::stdlib) fn value(&mut self, receiver: &Value) -> Option<Value> {
        let current = self.current.as_ref()?;
        let object = receiver.as_object().unwrap();
        let state = object.native_object_state::<Deque>().unwrap();
        let node = current.borrow();
        self.next = node.next.map(|slot| state.node(slot));
        Some(node.value.clone_for_php_storage())
    }

    pub(in crate::stdlib) fn advance(&mut self, receiver: &Value) {
        let Some(current) = self.current.take() else {
            return;
        };
        let object = receiver.as_object().unwrap();
        let state = object.native_object_state::<Deque>().unwrap();
        let live = |node: &NodeRef| {
            state
                .nodes
                .get(node.borrow().slot)
                .and_then(Option::as_ref)
                .is_some_and(|owned| Rc::ptr_eq(owned, node))
        };
        // A callback may remove the current node or its successor. Stable
        // node owners cannot alias a subsequently reused arena slot.
        self.current = if live(&current) {
            current.borrow().next.map(|slot| state.node(slot))
        } else {
            self.next.take()
        }
        .filter(live);
        self.next = None;
    }
}

#[cold]
pub(in crate::stdlib) fn legacy_start(
    receiver: &Value,
    eg: &ExecutorGlobals,
) -> (i32, LegacyCursor) {
    ensure(receiver, eg);
    let object = receiver.as_object().unwrap();
    let state = object.native_object_state::<Deque>().unwrap();
    (
        state.mode,
        LegacyCursor {
            current: state.head.map(|slot| state.node(slot)),
            next: None,
        },
    )
}

#[cold]
pub(in crate::stdlib) fn legacy_clear(
    receiver: &Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ensure(receiver, eg);
    loop {
        let retired = {
            let mut object = receiver.as_object_mut().unwrap();
            let state = object.native_object_state_mut::<Deque>();
            state
                .tail
                .map(|slot| state.remove(&state.node(slot), false))
        };
        let Some(retired) = retired else {
            return Ok(());
        };
        iterator_delegate::discard(retired, eg)?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
}

pub(in crate::stdlib) fn legacy_flags(receiver: &Value, flags: i64) {
    receiver
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<Deque>()
        .mode = flags as i32;
}

pub(in crate::stdlib) fn legacy_append(receiver: &Value, value: &Value) {
    // The legacy Serializable format consumes global reference slots, but
    // inserts dereferenced values. Modern __unserialize preserves the cells.
    receiver
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<Deque>()
        .insert_before(None, value.dereferenced().clone());
}

#[cold]
#[inline(never)]
pub(super) fn legacy_serialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = crate::stdlib::serialization::serialize_deque(arg!(ed, 0), eg)?;
    ret!(rv, value);
}

#[cold]
#[inline(never)]
pub(super) fn legacy_unserialize(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let argument = arg!(ed, 1).dereferenced().clone();
    let Some(input) = typed_internal_string_value_expected(
        ed,
        eg,
        &argument,
        "SplDoublyLinkedList::unserialize",
        0,
        "data",
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    crate::stdlib::serialization::unserialize_deque(
        &receiver,
        &input.php_string_bytes().expect("converted payload"),
        ed,
        eg,
    );
    Ok(())
}

#[cold]
#[inline(never)]
pub(super) fn serialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    ensure(receiver, eg);
    let (mode, values) = {
        let object = receiver.as_object().unwrap();
        let state = object.native_object_state::<Deque>().unwrap();
        let mut values = PhpArray::with_packed_capacity(state.len);
        let mut slot = state.head;
        while let Some(current) = slot {
            let node = state.node(current);
            let node = node.borrow();
            values.push(node.value.clone_for_php_storage());
            slot = node.next;
        }
        (state.mode, values)
    };
    let mut data = PhpArray::with_packed_capacity(3);
    data.push(Value::long(i64::from(mode)));
    data.push(Value::array(values));
    data.push(Value::array(array_object::member_properties(receiver, eg)));
    ret!(rv, Value::array(data));
}

#[cold]
#[inline(never)]
pub(super) fn unserialize(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let input = arg!(ed, 1).dereferenced().clone();
    let Some(data) = input.as_array() else {
        typed_internal_argument_error(
            eg,
            "SplDoublyLinkedList::__unserialize",
            &input,
            1,
            "data",
            "array",
        );
        return Ok(());
    };
    let valid = (|| {
        let flags = data
            .get_int(0)
            .filter(|v| v.value_type() == ValueType::Long)?
            .as_long()?;
        Some((
            flags,
            data.get_int(1)?.as_array()?,
            data.get_int(2)?.as_array()?,
        ))
    })();
    let Some((flags, values, members)) = valid else {
        error(
            eg,
            "UnexpectedValueException",
            "Incomplete or ill-typed serialization data",
        );
        return Ok(());
    };
    ensure(&receiver, eg);
    {
        let mut object = receiver.as_object_mut().unwrap();
        let state = object.native_object_state_mut::<Deque>();
        // Serialized flags have the native signed 32-bit domain, including
        // reserved bits; only setIteratorMode masks its public mode input.
        state.mode = flags as i32;
        for (_, value) in values.iter() {
            state.insert_before(None, clone_unserialized_storage_value(value));
        }
    }
    array_object::serialization::restore_container_members(&receiver, members, ed, eg, None)
}
