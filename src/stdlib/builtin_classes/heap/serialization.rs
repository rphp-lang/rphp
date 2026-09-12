//! Cold wire projection and ordered restoration; no separate reference table
//! or hidden PHP owners. Native insertion publishes each accepted element.
use super::*;
use crate::stdlib::serialization::clone_unserialized_storage_value;

#[cold]
#[inline(never)]
pub(super) fn serialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    ensure(receiver, eg);
    if !healthy(receiver, eg) {
        return Ok(());
    }
    if read(receiver, |h| h.locked) {
        error(eg, "Cannot serialize heap while it is being modified.");
        return Ok(());
    }
    let members = array_object::member_properties(receiver, eg);
    let state = read(receiver, |h| {
        let mut elements = PhpArray::with_packed_capacity(h.len);
        for &slot in &h.order[..h.len] {
            let entry = h.entry(slot);
            let value = if h.queue {
                let mut pair = PhpArray::with_hash_capacity(2);
                pair.set_str("data", entry.value.clone_for_php_storage());
                pair.set_str("priority", entry.priority.clone_for_php_storage());
                Value::array(pair)
            } else {
                entry.value.clone_for_php_storage()
            };
            elements.push(value);
        }
        let mut state = PhpArray::with_hash_capacity(2);
        state.set_str("flags", Value::long(i64::from(h.flags)));
        state.set_str("heap_elements", Value::array(elements));
        state
    });
    let mut result = PhpArray::with_packed_capacity(2);
    result.push(Value::array(members));
    result.push(Value::array(state));
    ret!(rv, Value::array(result));
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
    let name = receiver.as_object().unwrap().class_name.to_string();
    let Some(data) = input.as_array() else {
        let owner = if eg.class_is_a(&name, "SplPriorityQueue") {
            "SplPriorityQueue"
        } else {
            "SplHeap"
        };
        typed_internal_argument_error(
            eg,
            &format!("{owner}::__unserialize"),
            &input,
            1,
            "data",
            "array",
        );
        return Ok(());
    };
    // Modification/corruption guards precede inspection of the wire fields.
    if !begin(&receiver, eg) {
        return Ok(());
    }
    finish(&receiver, false);
    let invalid = format!("Invalid serialization data for {name} object");
    let reject = |eg: &mut ExecutorGlobals| {
        eg.exception = Some(make_error_value("Exception", &invalid));
    };
    let Some(members) = data
        .get_int(0)
        .and_then(Value::as_array)
        .filter(|_| data.len() == 2)
    else {
        reject(eg);
        return Ok(());
    };
    // Members are committed before state validation, including when the
    // latter fails. Do not transactionally roll back PHP-visible updates.
    array_object::serialization::restore_container_members(
        &receiver,
        members,
        ed,
        eg,
        Some(&invalid),
    )?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let Some(state) = data.get_int(1).and_then(Value::as_array) else {
        reject(eg);
        return Ok(());
    };
    let Some(flags) = state
        .get_str("flags")
        .filter(|v| v.value_type() == ValueType::Long)
        .and_then(Value::as_long)
    else {
        reject(eg);
        return Ok(());
    };
    let queue = read(&receiver, |h| h.queue);
    if if queue { flags & 3 == 0 } else { flags != 0 } {
        reject(eg);
        return Ok(());
    }
    change(&receiver, |h| h.flags = flags as u8 & 3);
    let Some(elements) = state.get_str("heap_elements").and_then(Value::as_array) else {
        reject(eg);
        return Ok(());
    };
    for (_, value) in elements.iter() {
        let entry = if queue {
            let Some(pair) = value.as_array().filter(|pair| pair.len() == 2) else {
                reject(eg);
                return Ok(());
            };
            let (Some(value), Some(priority)) = (pair.get_str("data"), pair.get_str("priority"))
            else {
                reject(eg);
                return Ok(());
            };
            Entry {
                value: clone_unserialized_storage_value(value),
                priority: clone_unserialized_storage_value(priority),
            }
        } else {
            Entry {
                value: clone_unserialized_storage_value(value),
                priority: Value::null(),
            }
        };
        if !begin(&receiver, eg) {
            return Ok(());
        }
        insert_entry(&receiver, entry, eg)?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    Ok(())
}
