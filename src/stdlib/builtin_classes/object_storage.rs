//! One traced owner per stored object/info pair. Physical iteration is live;
//! callbacks never retain a table borrow, and mutation precedes retirement.
use super::*;
use crate::value::NativeObjectState;
use crate::vm::function::InternalFunctionHandler;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Hash, PartialEq, Eq)]
enum Key {
    Identity(usize),
    User(Rc<[u8]>),
}
struct Entry {
    key: Key,
    object: Value,
    info: Value,
}
impl Clone for Entry {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            object: self.object.clone(),
            info: crate::stdlib::serialization::clone_unserialized_storage_value(&self.info),
        }
    }
}
#[derive(Default)]
struct Storage {
    entries: Vec<Option<Entry>>,
    keys: HashMap<Key, usize>,
    position: usize,
    index: i64,
    custom_hash: bool,
    active_scans: usize,
}
impl Storage {
    fn position_at(&self, start: usize) -> usize {
        (start..self.entries.len())
            .find(|&slot| self.entries[slot].is_some())
            .unwrap_or(self.entries.len())
    }
    fn current(&self) -> Option<&Entry> {
        self.entries.get(self.position_at(self.position))?.as_ref()
    }
    fn rewind(&mut self) {
        self.position = self.position_at(0);
        self.index = 0;
    }
    fn compact(&mut self) {
        if self.active_scans != 0
            || self.entries.len() < 32
            || self.entries.len() - self.keys.len() < self.entries.len() / 2
        {
            return;
        }
        let old_position = self.position_at(self.position);
        self.position = self.entries[..old_position]
            .iter()
            .filter(|entry| entry.is_some())
            .count();
        self.entries.retain(Option::is_some);
        for (slot, entry) in self.entries.iter().enumerate() {
            *self.keys.get_mut(&entry.as_ref().unwrap().key).unwrap() = slot;
        }
    }
    fn insert(&mut self, key: Key, object: Value, info: Value) -> Option<Value> {
        if let Some(&slot) = self.keys.get(&key) {
            return Some(std::mem::replace(
                &mut self.entries[slot].as_mut().unwrap().info,
                info,
            ));
        }
        self.compact();
        self.keys.insert(key.clone(), self.entries.len());
        self.entries.push(Some(Entry { key, object, info }));
        None
    }
    fn remove(&mut self, key: &Key) -> Option<Entry> {
        let slot = self.keys.remove(key)?;
        let entry = self.entries[slot].take();
        if self.position == slot {
            self.position = self.position_at(slot);
        }
        entry
    }
}
impl NativeObjectState for Storage {
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        let mut state = Storage {
            custom_hash: self.custom_hash,
            ..Storage::default()
        };
        for entry in self.entries.iter().flatten() {
            let copy = entry.clone();
            state.keys.insert(copy.key.clone(), state.entries.len());
            state.entries.push(Some(copy));
        }
        Box::new(state)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        for entry in self.entries.iter().flatten() {
            visit(&entry.object);
            visit(&entry.info);
        }
    }
    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        for entry in self.entries.iter_mut().rev().filter_map(Option::take) {
            pending.push(entry.info);
            pending.push(entry.object);
        }
        self.entries.clear();
        self.keys.clear();
        self.position = 0;
        self.index = 0;
    }
}
fn ensure(receiver: &Value, eg: &ExecutorGlobals) {
    let mut object = receiver.as_object_mut().expect("storage receiver");
    if object.native_object_state::<Storage>().is_none() {
        let custom = find_method_in_class_hierarchy(eg, &object.class_name, "getHash")
            .is_some_and(|(_, _, _, owner)| owner != "SplObjectStorage");
        object.native_object_state_mut::<Storage>().custom_hash = custom;
    }
}
fn read<T>(receiver: &Value, action: impl FnOnce(&Storage) -> T) -> T {
    let object = receiver.as_object().expect("storage receiver");
    action(
        object
            .native_object_state::<Storage>()
            .expect("initialized storage"),
    )
}
fn change<T>(receiver: &Value, action: impl FnOnce(&mut Storage) -> T) -> T {
    let mut object = receiver.as_object_mut().expect("storage receiver");
    action(object.native_object_state_mut::<Storage>())
}
fn error(eg: &mut ExecutorGlobals, class: &str, message: &str) {
    eg.exception = Some(make_error_value(class, message));
}
/// Native storage compares membership and associated info, not member/debug
/// properties or cursor position. The caller owns the recursion guard.
#[cold]
pub(crate) fn compare_state<E>(
    left: &Value,
    right: &Value,
    mut compare: impl FnMut(Value, Value) -> Result<i32, E>,
) -> Result<i32, E> {
    let count = |value: &Value| {
        value
            .as_object()
            .unwrap()
            .native_object_state::<Storage>()
            .map_or(0, |state| state.keys.len())
    };
    let left_len = count(left);
    let right_len = count(right);
    if left_len != right_len {
        return Ok(if left_len < right_len { -1 } else { 1 });
    }
    if left_len == 0 {
        return Ok(0);
    }
    change(left, |state| state.active_scans += 1);
    let result = (|| {
        let mut slot = 0;
        loop {
            let entry = read(left, |state| {
                slot = state.position_at(slot);
                state
                    .entries
                    .get(slot)
                    .and_then(Option::as_ref)
                    .map(|entry| (entry.key.clone(), entry.info.clone()))
            });
            let Some((key, info)) = entry else {
                break;
            };
            let other = read(right, |state| {
                state
                    .keys
                    .get(&key)
                    .map(|&slot| state.entries[slot].as_ref().unwrap().info.clone())
            });
            let Some(other) = other else {
                return Ok(1);
            };
            // Only these two owners survive the callback, never table borrows.
            let comparison = compare(info, other)?;
            if comparison != 0 {
                return Ok(comparison);
            }
            slot += 1;
        }
        Ok(0)
    })();
    change(left, |state| state.active_scans -= 1);
    result
}

/// The VM-aware comparison can invoke PHP conversions and retire the last
/// owner of an info value removed by that callback.
#[cold]
pub(crate) fn compare_state_runtime(
    eg: &mut ExecutorGlobals,
    left: &Value,
    right: &Value,
    mut compare: impl FnMut(&mut ExecutorGlobals, &Value, &Value) -> Result<Result<i32, ()>, VmError>,
) -> Result<Result<i32, ()>, VmError> {
    enum Failure {
        Execution(VmError),
        Recursive,
        Exception,
    }
    let result = compare_state(left, right, |left, right| {
        let result = compare(eg, &left, &right);
        let release_left = iterator_delegate::discard(left, eg);
        let release_right = iterator_delegate::discard(right, eg);
        let result = result.map_err(Failure::Execution)?;
        release_left.map_err(Failure::Execution)?;
        release_right.map_err(Failure::Execution)?;
        if eg.exception.is_some() {
            return Err(Failure::Exception);
        }
        result.map_err(|()| Failure::Recursive)
    });
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(Failure::Execution(error)) => Err(error),
        Err(Failure::Recursive) => Ok(Err(())),
        Err(Failure::Exception) => Ok(Ok(0)),
    }
}
fn object_argument(value: &Value, method: &str, eg: &mut ExecutorGlobals) -> bool {
    if value.weak_object_identity().is_some() {
        return true;
    }
    error(
        eg,
        "TypeError",
        &format!(
            "SplObjectStorage::{method}(): Argument #1 ($object) must be of type object, {} given",
            value.diagnostic_type_name()
        ),
    );
    false
}
fn key(receiver: &Value, object: &Value, eg: &mut ExecutorGlobals) -> Result<Option<Key>, VmError> {
    ensure(receiver, eg);
    if !read(receiver, |state| state.custom_hash) {
        return Ok(Some(Key::Identity(
            object.weak_object_identity().expect("validated object"),
        )));
    }
    let result = call_object_public_method(eg, receiver, "getHash", std::slice::from_ref(object))?;
    if eg.exception.is_some() {
        return Ok(None);
    }
    let Some(value) = result else {
        return Ok(None);
    };
    let Some(bytes) = value.php_string_bytes() else {
        let class = receiver.as_object().unwrap().class_name.to_string();
        error(
            eg,
            "TypeError",
            &format!(
                "{class}::getHash(): Return value must be of type string, {} returned",
                value.diagnostic_type_name()
            ),
        );
        iterator_delegate::discard(value, eg)?;
        return Ok(None);
    };
    Ok(Some(Key::User(Rc::from(bytes.as_ref()))))
}
fn store(
    receiver: &Value,
    object: &Value,
    info: &Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(key) = key(receiver, object, eg)? else {
        return Ok(());
    };
    let old = change(receiver, |state| {
        state.insert(key, object.clone(), info.dereferenced().clone())
    });
    if let Some(old) = old {
        iterator_delegate::discard(old, eg)?;
    }
    Ok(())
}
fn remove(receiver: &Value, object: &Value, eg: &mut ExecutorGlobals) -> Result<bool, VmError> {
    let Some(key) = key(receiver, object, eg)? else {
        return Ok(false);
    };
    let Some(entry) = change(receiver, |state| state.remove(&key)) else {
        return Ok(false);
    };
    iterator_delegate::discard(entry.object, eg)?;
    iterator_delegate::discard(entry.info, eg)?;
    Ok(true)
}
fn contains(receiver: &Value, object: &Value, eg: &mut ExecutorGlobals) -> Result<bool, VmError> {
    let Some(key) = key(receiver, object, eg)? else {
        return Ok(false);
    };
    Ok(read(receiver, |state| state.keys.contains_key(&key)))
}
fn offset_set(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let object = arg!(ed, 1).dereferenced().clone();
    if !object_argument(&object, "offsetSet", eg) {
        return Ok(());
    }
    let info = arg_opt!(ed, 2).cloned().unwrap_or_else(Value::null);
    store(&receiver, &object, &info, eg)
}
fn offset_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let object = arg!(ed, 1).dereferenced().clone();
    if !object_argument(&object, "offsetGet", eg) {
        return Ok(());
    }
    let Some(key) = key(&receiver, &object, eg)? else {
        return Ok(());
    };
    let value = read(&receiver, |state| {
        state.keys.get(&key).map(|&slot| {
            state.entries[slot]
                .as_ref()
                .unwrap()
                .info
                .dereferenced()
                .clone()
        })
    });
    if let Some(value) = value {
        ret!(rv, value);
    }
    error(eg, "UnexpectedValueException", "Object not found");
    Ok(())
}
fn offset_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let object = arg!(ed, 1).dereferenced().clone();
    if !object_argument(&object, "offsetExists", eg) {
        return Ok(());
    }
    ret!(rv, Value::bool(contains(&receiver, &object, eg)?));
}
fn offset_unset(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let object = arg!(ed, 1).dereferenced().clone();
    if !object_argument(&object, "offsetUnset", eg) {
        return Ok(());
    }
    let result = remove(&receiver, &object, eg);
    change(&receiver, Storage::rewind);
    result.map(|_| ())
}
/// Native dimension handlers and explicit offsetUnset have distinct cursor
/// contracts. PHP gives subclasses the standard ArrayAccess handlers, even
/// when their inherited offsetUnset body is still the internal method.
#[cold]
#[inline(never)]
pub(crate) fn unset_dimension(
    eg: &mut ExecutorGlobals,
    receiver: &Value,
    object: &Value,
) -> Result<Option<Value>, VmError> {
    let native = object.weak_object_identity().is_some()
        && receiver
            .as_object()
            .is_some_and(|receiver| receiver.class_name.as_ref() == "SplObjectStorage");
    if native {
        remove(receiver, object, eg)?;
        return Ok(Some(Value::null()));
    }
    call_object_protocol_method(
        eg,
        receiver,
        "ArrayAccess",
        "offsetUnset",
        std::slice::from_ref(object),
    )
}
fn get_hash(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    if !object_argument(arg!(ed, 1), "getHash", eg) {
        return Ok(());
    }
    ret!(
        rv,
        Value::string(&format!(
            "{:016x}{:016x}",
            arg!(ed, 1).object_handle().unwrap(),
            0
        ))
    );
}
fn count(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    if let Some(value) = arg_opt!(ed, 1) {
        let value = value.clone();
        if typed_internal_int_value_argument_expected(
            ed,
            eg,
            &value,
            "SplObjectStorage::count",
            0,
            "mode",
            "int",
        )?
        .is_none()
        {
            return Ok(());
        }
    }
    ensure(arg!(ed, 0), eg);
    ret!(
        rv,
        Value::long(read(arg!(ed, 0), |state| state.keys.len() as i64))
    );
}
fn rewind(ed: *mut ExecuteData, _rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    change(arg!(ed, 0), Storage::rewind);
    Ok(())
}
fn valid(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    ret!(
        rv,
        Value::bool(read(arg!(ed, 0), |state| state.current().is_some()))
    );
}
fn current(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    if let Some(value) = read(arg!(ed, 0), |state| {
        state.current().map(|entry| entry.object.clone())
    }) {
        ret!(rv, value);
    }
    error(
        eg,
        "RuntimeException",
        "Called current() on invalid iterator",
    );
    Ok(())
}
fn get_info(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    ret!(
        rv,
        read(arg!(ed, 0), |state| state
            .current()
            .map_or_else(Value::null, |entry| entry.info.clone()))
    );
}
fn set_info(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    let old = change(arg!(ed, 0), |state| {
        let slot = state.position_at(state.position);
        state
            .entries
            .get_mut(slot)?
            .as_mut()
            .map(|entry| std::mem::replace(&mut entry.info, arg!(ed, 1).dereferenced().clone()))
    });
    if let Some(old) = old {
        iterator_delegate::discard(old, eg)?;
    }
    Ok(())
}
fn current_key(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    ret!(rv, Value::long(read(arg!(ed, 0), |state| state.index)));
}
fn next(ed: *mut ExecuteData, _rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    change(arg!(ed, 0), |state| {
        let current = state.position_at(state.position);
        state.position = state.position_at(current.saturating_add(1));
        state.index = state.index.wrapping_add(1);
    });
    Ok(())
}
fn seek(ed: *mut ExecuteData, _rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let argument = arg!(ed, 1).clone();
    let Some(position) = typed_internal_int_value_argument_expected(
        ed,
        eg,
        &argument,
        "SplObjectStorage::seek",
        0,
        "offset",
        "int",
    )?
    else {
        return Ok(());
    };
    ensure(arg!(ed, 0), eg);
    if position < 0 || position as usize >= read(arg!(ed, 0), |state| state.keys.len()) {
        error(
            eg,
            "OutOfBoundsException",
            &format!("Seek position {position} is out of range"),
        );
        return Ok(());
    }
    change(arg!(ed, 0), |state| {
        if position == 0 || position < state.index && state.index - position > position {
            state.rewind();
        }
        while state.index < position {
            state.position = state.position_at(state.position_at(state.position).saturating_add(1));
            state.index += 1;
        }
        while state.index > position {
            state.position = (0..state.position.min(state.entries.len()))
                .rev()
                .find(|&slot| state.entries[slot].is_some())
                .unwrap_or(state.entries.len());
            state.index -= 1;
        }
    });
    Ok(())
}
fn other_argument(ed: *mut ExecuteData, eg: &mut ExecutorGlobals, method: &str) -> Option<Value> {
    let value = arg!(ed, 1).dereferenced().clone();
    if value
        .as_object()
        .is_some_and(|object| eg.class_is_a(&object.class_name, "SplObjectStorage"))
    {
        return Some(value);
    }
    error(
        eg,
        "TypeError",
        &format!(
            "SplObjectStorage::{method}(): Argument #1 ($storage) must be of type SplObjectStorage, {} given",
            value.diagnostic_type_name()
        ),
    );
    None
}
fn add_all(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(other) = other_argument(ed, eg, "addAll") else {
        return Ok(());
    };
    let receiver = arg!(ed, 0).clone();
    ensure(&receiver, eg);
    ensure(&other, eg);
    change(&other, |state| state.active_scans += 1);
    let result = (|| {
        let mut slot = 0;
        loop {
            let entry = read(&other, |state| {
                slot = state.position_at(slot);
                state.entries.get(slot).and_then(Option::as_ref).cloned()
            });
            let Some(entry) = entry else {
                break;
            };
            store(&receiver, &entry.object, &entry.info, eg)?;
            iterator_delegate::discard(entry.info, eg)?;
            iterator_delegate::discard(entry.object, eg)?;
            if eg.exception.is_some() {
                break;
            }
            slot += 1;
        }
        Ok(())
    })();
    change(&other, |state| state.active_scans -= 1);
    change(&receiver, |state| state.index = 0);
    result?;
    ret!(
        rv,
        Value::long(read(&receiver, |state| state.keys.len() as i64))
    );
}
fn remove_all(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(other) = other_argument(ed, eg, "removeAll") else {
        return Ok(());
    };
    let receiver = arg!(ed, 0).clone();
    ensure(&receiver, eg);
    ensure(&other, eg);
    change(&other, |state| state.active_scans += 1);
    let result = (|| {
        let mut slot = 0;
        loop {
            let object = read(&other, |state| {
                slot = state.position_at(slot);
                state
                    .entries
                    .get(slot)
                    .and_then(Option::as_ref)
                    .map(|entry| entry.object.clone())
            });
            let Some(object) = object else {
                break;
            };
            let removed = remove(&receiver, &object, eg)?;
            iterator_delegate::discard(object, eg)?;
            if eg.exception.is_some() {
                break;
            }
            if !removed {
                slot += 1;
            }
        }
        Ok(())
    })();
    change(&other, |state| state.active_scans -= 1);
    change(&receiver, Storage::rewind);
    result?;
    ret!(
        rv,
        Value::long(read(&receiver, |state| state.keys.len() as i64))
    );
}
fn remove_all_except(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(other) = other_argument(ed, eg, "removeAllExcept") else {
        return Ok(());
    };
    let receiver = arg!(ed, 0).clone();
    ensure(&receiver, eg);
    ensure(&other, eg);
    change(&receiver, |state| state.active_scans += 1);
    let result = (|| {
        let mut slot = 0;
        loop {
            let object = read(&receiver, |state| {
                slot = state.position_at(slot);
                state
                    .entries
                    .get(slot)
                    .and_then(Option::as_ref)
                    .map(|entry| entry.object.clone())
            });
            let Some(object) = object else {
                break;
            };
            if !contains(&other, &object, eg)? && eg.exception.is_none() {
                remove(&receiver, &object, eg)?;
            }
            iterator_delegate::discard(object, eg)?;
            if eg.exception.is_some() {
                break;
            }
            slot += 1;
        }
        Ok(())
    })();
    change(&receiver, |state| state.active_scans -= 1);
    change(&receiver, Storage::rewind);
    result?;
    ret!(
        rv,
        Value::long(read(&receiver, |state| state.keys.len() as i64))
    );
}
#[cold]
fn debug_info(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    ensure(receiver, eg);
    let storage = read(receiver, |state| {
        let mut result = PhpArray::with_packed_capacity(state.keys.len());
        for entry in state.entries.iter().flatten() {
            let mut pair = PhpArray::with_hash_capacity(2);
            pair.set_str("obj", entry.object.clone());
            pair.set_str("inf", entry.info.clone());
            result.push(Value::array(pair));
        }
        result
    });
    let mut members = array_object::serialization::members(receiver, eg);
    members.set_str("\0SplObjectStorage\0storage", Value::array(storage));
    ret!(rv, Value::array(members));
}
#[cold]
fn serialize_state(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    ensure(receiver, eg);
    let entries = read(receiver, |state| {
        let mut result = PhpArray::with_packed_capacity(state.keys.len().saturating_mul(2));
        for entry in state.entries.iter().flatten() {
            result.push(entry.object.clone());
            result.push(entry.info.clone());
        }
        result
    });
    let mut state = PhpArray::with_packed_capacity(2);
    state.push(Value::array(entries));
    state.push(Value::array(array_object::serialization::members(
        receiver, eg,
    )));
    ret!(rv, Value::array(state));
}
#[cold]
fn unserialize_state(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let data = arg!(ed, 1).dereferenced().clone();
    ensure(&receiver, eg);
    let Some(data) = data.as_array() else {
        error(
            eg,
            "TypeError",
            "SplObjectStorage::__unserialize(): Argument #1 ($data) must be of type array",
        );
        return Ok(());
    };
    let parts = data
        .get_int(0)
        .and_then(Value::as_array)
        .zip(data.get_int(1).and_then(Value::as_array));
    let Some((entries, members)) = parts else {
        error(
            eg,
            "UnexpectedValueException",
            "Incomplete or ill-typed serialization data",
        );
        return Ok(());
    };
    if entries.len() % 2 != 0 {
        error(eg, "UnexpectedValueException", "Odd number of elements");
        return Ok(());
    }
    let mut values = entries.values();
    while let Some(object) = values.next() {
        let info = values.next().unwrap();
        if object.weak_object_identity().is_none() {
            error(eg, "UnexpectedValueException", "Non-object key");
            return Ok(());
        }
        store(&receiver, object, info, eg)?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    array_object::serialization::restore_container_members(&receiver, members, ed, eg, None)
}
fn deprecated(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    old: &str,
    replacement: &str,
) -> Result<(), VmError> {
    report_internal_deprecation(
        eg,
        ed,
        &format!(
            "Method SplObjectStorage::{old}() is deprecated since 8.5, use method SplObjectStorage::{replacement}() instead"
        ),
    )?;
    if eg.exception.is_none() {
        object_argument(arg!(ed, 1).dereferenced(), old, eg);
    }
    Ok(())
}
fn attach(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    deprecated(ed, eg, "attach", "offsetSet")?;
    if eg.exception.is_some() {
        return Ok(());
    }
    offset_set(ed, rv, eg)
}
fn detach(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    deprecated(ed, eg, "detach", "offsetUnset")?;
    if eg.exception.is_some() {
        return Ok(());
    }
    offset_unset(ed, rv, eg)
}
fn contains_method(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    deprecated(ed, eg, "contains", "offsetExists")?;
    if eg.exception.is_some() {
        return Ok(());
    }
    offset_exists(ed, rv, eg)
}
#[cold]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Array, Bool, ClassName, Int, Mixed, String, Void};
    type Row = (
        &'static str,
        InternalFunctionHandler,
        &'static [&'static str],
        Vec<ParamTypeHint>,
        ParamTypeHint,
        u32,
        Vec<Option<&'static str>>,
    );
    let object = ClassName("object".into());
    let storage = ClassName("SplObjectStorage".into());
    let rows: Vec<Row> = vec![
        (
            "offsetSet",
            offset_set,
            &["object", "info"],
            vec![ParamTypeHint::None, Mixed],
            Void,
            1,
            vec![None, Some("null")],
        ),
        (
            "attach",
            attach,
            &["object", "info"],
            vec![object.clone(), Mixed],
            Void,
            1,
            vec![None, Some("null")],
        ),
        (
            "offsetGet",
            offset_get,
            &["object"],
            vec![ParamTypeHint::None],
            Mixed,
            1,
            vec![None],
        ),
        (
            "offsetExists",
            offset_exists,
            &["object"],
            vec![ParamTypeHint::None],
            Bool,
            1,
            vec![None],
        ),
        (
            "offsetUnset",
            offset_unset,
            &["object"],
            vec![ParamTypeHint::None],
            Void,
            1,
            vec![None],
        ),
        (
            "detach",
            detach,
            &["object"],
            vec![object.clone()],
            Void,
            1,
            vec![None],
        ),
        (
            "contains",
            contains_method,
            &["object"],
            vec![object.clone()],
            Bool,
            1,
            vec![None],
        ),
        (
            "getHash",
            get_hash,
            &["object"],
            vec![object.clone()],
            String,
            1,
            vec![None],
        ),
        (
            "count",
            count,
            &["mode"],
            vec![Int],
            Int,
            0,
            vec![Some("COUNT_NORMAL")],
        ),
        ("rewind", rewind, &[], vec![], Void, 0, vec![]),
        ("valid", valid, &[], vec![], Bool, 0, vec![]),
        ("key", current_key, &[], vec![], Int, 0, vec![]),
        ("current", current, &[], vec![], object, 0, vec![]),
        ("next", next, &[], vec![], Void, 0, vec![]),
        ("getInfo", get_info, &[], vec![], Mixed, 0, vec![]),
        (
            "setInfo",
            set_info,
            &["info"],
            vec![Mixed],
            Void,
            1,
            vec![None],
        ),
        ("seek", seek, &["offset"], vec![Int], Void, 1, vec![None]),
        (
            "addAll",
            add_all,
            &["storage"],
            vec![storage.clone()],
            Int,
            1,
            vec![None],
        ),
        (
            "removeAll",
            remove_all,
            &["storage"],
            vec![storage.clone()],
            Int,
            1,
            vec![None],
        ),
        (
            "removeAllExcept",
            remove_all_except,
            &["storage"],
            vec![storage],
            Int,
            1,
            vec![None],
        ),
        ("__debugInfo", debug_info, &[], vec![], Array, 0, vec![]),
        (
            "__serialize",
            serialize_state,
            &[],
            vec![],
            Array,
            0,
            vec![],
        ),
        (
            "__unserialize",
            unserialize_state,
            &["data"],
            vec![Array],
            Void,
            1,
            vec![None],
        ),
    ];
    eg.reserve_internal_method_contracts("SplObjectStorage", rows.len());
    let mut functions = Vec::with_capacity(rows.len());
    for (name, handler, names, hints, result, required, defaults) in rows {
        eg.register_internal_method_contract(
            "SplObjectStorage",
            name,
            false,
            required,
            names,
            hints.clone(),
            result,
            &defaults,
            name != "seek",
        );
        let mut function = Box::new(make_internal_method(
            handler,
            names.len() as u32 + 1,
            required,
            names.iter().map(|name| name.to_string()).collect(),
        ));
        function.handler_validates_types = true;
        function.common.sig.param_type_hints = hints;
        if name == "seek" {
            function.common.sig.return_type_hint = Void;
        }
        let pointer = &function.common as *const FunctionCommon;
        eg.function_table.insert(
            internal_method_lookup_name("SplObjectStorage", name),
            pointer,
        );
        eg.method_declaring_class
            .insert(pointer, "SplObjectStorage".into());
        eg.register_internal_function_display_name(
            pointer,
            internal_method_display_name("SplObjectStorage", name),
        );
        eg.register_internal_function_reflection_metadata(
            pointer,
            defaults
                .iter()
                .map(|value| {
                    value.map(|name| {
                        if name == "null" {
                            Value::null()
                        } else {
                            Value::long(0)
                        }
                    })
                })
                .collect(),
            "SPL",
        );
        functions.push(function);
    }
    functions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(state: &mut Storage, id: usize) {
        state.insert(
            Key::Identity(id),
            Value::long(id as i64),
            Value::long(1000 + id as i64),
        );
    }

    #[test]
    fn native_storage_traces_once_and_retires_object_before_info() {
        let mut state = Storage::default();
        insert(&mut state, 1);
        insert(&mut state, 2);
        insert(&mut state, 3);
        state.remove(&Key::Identity(2));
        let mut traced = Vec::new();
        state.for_each_value(&mut |value| traced.push(value.as_long().unwrap()));
        assert_eq!(traced, [1, 1001, 3, 1003]);
        let mut pending = Vec::new();
        state.append_values_reversed(&mut pending);
        let retired: Vec<_> = pending
            .iter()
            .rev()
            .map(|value| value.as_long().unwrap())
            .collect();
        assert_eq!(retired, traced);
        assert!(state.keys.is_empty());
        assert!(state.current().is_none());
    }

    #[test]
    fn native_storage_compaction_preserves_live_position_and_public_key() {
        let mut state = Storage::default();
        for id in 0..80 {
            insert(&mut state, id);
        }
        state.position = 60;
        state.index = 17;
        for id in 0..60 {
            state.remove(&Key::Identity(id));
        }
        insert(&mut state, 80);
        assert_eq!(state.entries.len(), 21);
        assert_eq!(state.position, 0);
        assert_eq!(state.index, 17);
        assert_eq!(state.current().unwrap().object.as_long(), Some(60));
        for id in 60..81 {
            assert_eq!(
                state.entries[*state.keys.get(&Key::Identity(id)).unwrap()]
                    .as_ref()
                    .unwrap()
                    .object
                    .as_long(),
                Some(id as i64)
            );
        }
    }

    #[test]
    fn native_storage_active_scan_keeps_physical_slots_stable() {
        let mut state = Storage::default();
        for id in 0..80 {
            insert(&mut state, id);
        }
        state.active_scans = 1;
        for id in 0..60 {
            state.remove(&Key::Identity(id));
        }
        insert(&mut state, 80);
        assert_eq!(state.entries.len(), 81);
        assert_eq!(state.position_at(0), 60);
        state.active_scans = 0;
        insert(&mut state, 81);
        assert_eq!(state.entries.len(), 22);
    }
}
