//! Live native cursor and public Iterator methods. Foreach may use the same
//! primitive directly after checking that the protocol has no user override.
use super::*;
use crate::value::{NativeArrayBuckets, NativeArrayCursor};
use std::cell::RefCell;

pub(crate) fn native_protocol(receiver: &Value, eg: &ExecutorGlobals) -> bool {
    let Some(object) = receiver.as_object() else {
        return false;
    };
    if object.property_slot(ARRAY_ITERATOR_STORAGE).is_none() {
        return false;
    }
    if object.class_name.as_ref() == "ArrayIterator" {
        return true;
    }
    drop(object);
    for (name, native) in [
        ("rewind", "arrayiterator::rewind"),
        ("valid", "arrayiterator::valid"),
        ("current", "arrayiterator::current"),
        ("key", "arrayiterator::key"),
        ("next", "arrayiterator::next"),
    ] {
        let Some(method) = resolve_object_public_method(eg, receiver, name) else {
            return false;
        };
        if eg.find_function(native) != Some(method.func_ptr) {
            return false;
        }
    }
    true
}

#[derive(Clone, Copy)]
pub(crate) enum Move {
    Current,
    Rewind,
    Next,
    Seek(usize),
}

#[derive(Clone, Copy)]
pub(crate) enum Projection {
    None,
    Key,
    Value,
    Both,
}

impl Projection {
    fn key(self) -> bool {
        matches!(self, Self::Key | Self::Both)
    }

    fn value(self) -> bool {
        matches!(self, Self::Value | Self::Both)
    }
}

// Borrow the live storage chain instead of cloning an owner and hashing its
// property names on each step. No pointer or PHP value escapes the borrow.
// Deep chains, object backing, mutation/rebinding and reference materialization
// retain the general path; the bounded depth here is only fast-path admission.
fn borrowed_array_entry(
    receiver: &Value,
    object: &PhpObject,
    state: &mut NativeArrayCursor,
    movement: Move,
    depth: usize,
    projection: Projection,
) -> Option<Option<(Value, Value)>> {
    let iteration = object.native_array_iteration()?;
    let storage = object.get_property_slot(iteration.storage_slot?)?;
    if let Some(array) = storage.as_array() {
        let buckets = iteration.buckets.as_ref()?;
        if state.owner != receiver.object_identity()?
            || state.generation != buckets.generation
            || state.sorted != buckets.sorted
            || buckets.object_keys.is_some()
            || buckets.positions.len() != array.len()
        {
            return None;
        }
        let len = array.len();
        let index = match movement {
            Move::Current => buckets.index(state.live),
            Move::Rewind => {
                state.saved = 0;
                state.live = 0;
                buckets.index(0)
            }
            Move::Next => (buckets.index(state.live) + 1).min(len),
            Move::Seek(offset) => {
                state.live = buckets.end;
                offset.min(len)
            }
        };
        state.live = buckets
            .positions
            .get(index)
            .copied()
            .unwrap_or(state.live.max(buckets.end));
        if matches!(movement, Move::Next | Move::Seek(_)) {
            state.saved = state.live;
        }
        if index >= len {
            return Some(None);
        }
        let (key, value) = if projection.key() {
            let (value, key) = array.get_at(index)?;
            let key = match key {
                ArrayKey::Int(number) => Value::long(number),
                ArrayKey::String(name) if array.has_external_byte_keys() => {
                    Value::binary_string_from_storage(name)
                }
                ArrayKey::String(name) => Value::string(name),
            };
            let value = if projection.value() {
                value.dereferenced().clone()
            } else {
                Value::null()
            };
            (key, value)
        } else if projection.value() {
            (
                Value::null(),
                array.get_value_at(index)?.dereferenced().clone(),
            )
        } else {
            (Value::null(), Value::null())
        };
        return Some(Some((key, value)));
    }
    if depth == 0 {
        return None;
    }
    let backing = storage.as_object()?;
    borrowed_array_entry(storage, &backing, state, movement, depth - 1, projection)
}

fn existing_array_entry(
    receiver: &Value,
    movement: Move,
    projection: Projection,
) -> Option<Option<(Value, Value)>> {
    let object = receiver.as_object()?;
    let cursor = object.native_array_iteration()?.cursor.as_ref()?;
    borrowed_array_entry(
        receiver,
        &object,
        &mut cursor.borrow_mut(),
        movement,
        4,
        projection,
    )
}

/// A native, override-free array consumer cannot invoke PHP while traversing.
/// Project once (preserving stored references), then exhaust the shared cursor.
/// Object backing and any overridden protocol retain stepwise dispatch.
pub(crate) fn consume_array(
    receiver: &Value,
    eg: &ExecutorGlobals,
    project: Option<bool>,
) -> Option<(usize, Option<PhpArray>)> {
    let Backing::Array(owner, key) = backing(receiver)? else {
        return None;
    };
    let object = owner.as_object()?;
    let array = object.get_property(key)?.as_array()?;
    let count = array.len();
    let projection = project.map(|preserve_keys| {
        if preserve_keys {
            array.clone()
        } else {
            let mut output = PhpArray::with_packed_capacity(count);
            for (_, value) in array.iter() {
                output.push(value.clone_for_php_storage());
            }
            output
        }
    });
    drop(object);
    drop(owner);
    entry(receiver, Move::Seek(count), false, Projection::None, eg);
    Some((count, projection))
}

struct ObjectEntry {
    name: String,
    value: Value,
    public: bool,
}

fn object_entries(object: &PhpObject, eg: &ExecutorGlobals) -> Vec<ObjectEntry> {
    let mut entries = Vec::new();
    for slot in eg.instance_property_slots_in_iteration_order(object.class_id) {
        let Some(definition) = eg.instance_property_definition(object.class_id, slot) else {
            continue;
        };
        if definition.is_virtual_hook_property() {
            continue;
        }
        let Some(value) = object.get_property_slot(slot) else {
            continue;
        };
        entries.push(ObjectEntry {
            name: object
                .property_name_at_slot(slot)
                .expect("declared slot")
                .to_string(),
            value: value.clone_for_php_storage(),
            public: definition.visibility == Visibility::Public,
        });
    }
    object.for_each_dynamic_property(|name, value| {
        entries.push(ObjectEntry {
            name: name.to_string(),
            value: value.clone_for_php_storage(),
            public: !name.starts_with('\0'),
        });
    });
    entries
}

#[inline]
pub(crate) fn entry(
    receiver: &Value,
    movement: Move,
    by_reference: bool,
    projection: Projection,
    eg: &ExecutorGlobals,
) -> Option<(Value, Value)> {
    if !by_reference && let Some(entry) = existing_array_entry(receiver, movement, projection) {
        return entry;
    }
    entry_slow(receiver, movement, by_reference, projection, eg)
}

/// Movement/probing must not create unused aliases before a consumer callback.
/// Reference materialization uses the selected entry components above.
pub(crate) fn projected_entry(
    receiver: &Value,
    movement: Move,
    projection: Projection,
    eg: &ExecutorGlobals,
) -> Option<(Value, Value)> {
    entry(receiver, movement, false, projection, eg)
}

/// A delegated native iterator caches an existing reference container without
/// creating a new reference. Public current() and by-reference foreach have
/// different contracts and retain their existing projection paths.
#[cold]
pub(crate) fn cached_value(receiver: &Value, eg: &ExecutorGlobals) -> Value {
    if projected_entry(receiver, Move::Current, Projection::None, eg).is_none() {
        return Value::null();
    }
    let state = receiver.as_object().and_then(|o| {
        o.native_array_iteration()?
            .cursor
            .as_ref()
            .map(|cursor| *cursor.borrow())
    });
    let Some(state) = state else {
        return Value::null();
    };
    let Some(storage) = backing(receiver) else {
        return Value::null();
    };
    let (owner, key) = match storage {
        Backing::Array(owner, key) => (owner, Some(key)),
        Backing::Object(owner) => (owner, None),
    };
    let Some(object) = owner.as_object() else {
        return Value::null();
    };
    let Some(buckets) = object
        .native_array_iteration()
        .and_then(|i| i.buckets.as_ref())
    else {
        return Value::null();
    };
    let index = buckets.index(state.live);
    if let Some(key) = key {
        object
            .get_property(key)
            .and_then(Value::as_array)
            .and_then(|a| a.get_value_at(index))
            .map_or_else(Value::null, Value::clone_for_php_storage)
    } else {
        object_entries(&object, eg)
            .get(index)
            .map_or_else(Value::null, |row| row.value.clone_for_php_storage())
    }
}

#[cold]
#[inline(never)]
fn entry_slow(
    receiver: &Value,
    movement: Move,
    by_reference: bool,
    projection: Projection,
    eg: &ExecutorGlobals,
) -> Option<(Value, Value)> {
    let cursor = {
        let mut object = receiver.as_object_mut()?;
        let slot = object.property_slot(array_object_storage_key(&object));
        let iteration = object.native_array_iteration_mut();
        iteration.storage_slot = slot;
        iteration
            .cursor
            .get_or_insert_with(|| Rc::new(RefCell::new(NativeArrayCursor::default())))
            .clone()
    };
    let mut state = *cursor.borrow();
    let storage = backing(receiver)?;
    let (owner, storage_key) = match storage {
        Backing::Array(owner, key) => (owner, Some(key)),
        Backing::Object(owner) => (owner, None),
    };
    let identity = owner.object_identity()?;
    let mut object = owner.as_object_mut()?;
    let rows = storage_key.is_none().then(|| object_entries(&object, eg));
    let len = if let Some(key) = storage_key {
        object.get_property(key)?.as_array()?.len()
    } else {
        rows.as_ref()?.len()
    };
    let slot = storage_key.and_then(|key| object.property_slot(key));
    let iteration = object.native_array_iteration_mut();
    iteration.storage_slot = slot;
    let buckets = iteration
        .buckets
        .get_or_insert_with(|| NativeArrayBuckets::new(len));
    if let Some(rows) = &rows {
        let mut keys = Vec::with_capacity(rows.len());
        for row in rows {
            keys.push(row.name.clone());
        }
        buckets.synchronize_object(keys);
    } else {
        buckets.extend(len);
    }
    let rebound = state.owner != identity || state.generation != buckets.generation;
    if rebound {
        state.owner = identity;
        state.generation = buckets.generation;
        state.live = state.saved;
    } else if state.sorted != buckets.sorted {
        state.live = state.live.min(buckets.end);
    }
    state.sorted = buckets.sorted;
    let eligible = |index: usize, allow_unset: bool| {
        rows.as_ref()
            .is_none_or(|rows| rows[index].public && (allow_unset || !rows[index].value.is_undef()))
    };
    let mut index = match movement {
        Move::Current => buckets.index(state.live),
        Move::Rewind => {
            state.saved = 0;
            state.live = 0;
            buckets.index(0)
        }
        Move::Next => {
            let index = buckets.index(state.live);
            if index < len { index + 1 } else { len }
        }
        Move::Seek(offset) => {
            state.live = buckets.end;
            if rows.is_none() {
                offset.min(len)
            } else {
                let mut remaining = offset;
                let mut index = 0;
                while index < len {
                    if eligible(index, false) {
                        if remaining == 0 {
                            break;
                        }
                        remaining -= 1;
                    }
                    index += 1;
                }
                index
            }
        }
    };
    let allow_unset = matches!(movement, Move::Current) && !rebound;
    while index < len && !eligible(index, allow_unset) {
        index += 1;
    }
    state.live = buckets
        .positions
        .get(index)
        .copied()
        .unwrap_or(state.live.max(buckets.end));
    if matches!(movement, Move::Next | Move::Seek(_)) {
        state.saved = state.live;
    }
    let result = if index >= len {
        None
    } else if let Some(key) = storage_key {
        let array = object.get_property(key)?.as_array()?;
        let key = if projection.key() {
            match array.get_at(index)?.1 {
                ArrayKey::Int(number) => Value::long(number),
                ArrayKey::String(name) if array.has_external_byte_keys() => {
                    Value::binary_string_from_storage(name)
                }
                ArrayKey::String(name) => Value::string(name),
            }
        } else {
            Value::null()
        };
        let value = if !projection.value() {
            Value::null()
        } else if by_reference {
            object
                .get_property_mut(storage_key?)?
                .as_array_mut()?
                .argument_unpack_reference_at(index)?
        } else {
            array.get_value_at(index)?.dereferenced().clone()
        };
        Some((key, value))
    } else {
        let row = &rows.as_ref()?[index];
        let value = if !projection.value() {
            Value::null()
        } else if by_reference {
            let slot = property_slot(&object, &ArrayKey::String(row.name.clone()), eg);
            let value = object_slot(&mut object, &slot)?;
            if !value.is_owned_reference() {
                *value = Value::owned_reference(value.dereferenced().clone());
            }
            value.clone_owned_reference_alias()
        } else if row.value.is_undef() {
            Value::null()
        } else {
            row.value.dereferenced().clone()
        };
        let key = if projection.key() {
            Value::string(row.name.clone())
        } else {
            Value::null()
        };
        Some((key, value))
    };
    drop(object);
    *cursor.borrow_mut() = state;
    result
}

pub(super) fn storage_replaced(object: &mut PhpObject, len: usize, new_table: bool) {
    if object
        .native_array_iteration()
        .is_some_and(|state| state.buckets.is_some())
    {
        object
            .native_array_iteration_mut()
            .buckets
            .as_mut()
            .expect("active buckets")
            .replace(len, new_table);
    }
}

pub(crate) fn removed(object: &mut PhpObject, position: usize) {
    if object
        .native_array_iteration()
        .is_some_and(|state| state.buckets.is_some())
    {
        let buckets = object
            .native_array_iteration_mut()
            .buckets
            .as_mut()
            .expect("active buckets");
        if position < buckets.positions.len() {
            buckets.positions.remove(position);
        }
    }
}

pub(super) fn object_removed(
    object: &mut PhpObject,
    identity: usize,
    receiver_cursor: Option<&Rc<RefCell<NativeArrayCursor>>>,
    name: &str,
    eg: &ExecutorGlobals,
) {
    let Some(cursor) = receiver_cursor else {
        return;
    };
    if cursor.borrow().owner != identity {
        return;
    }
    if !object
        .native_array_iteration()
        .is_some_and(|state| state.buckets.is_some())
    {
        return;
    }
    let rows = object_entries(object, eg);
    let buckets = object
        .native_array_iteration_mut()
        .buckets
        .as_mut()
        .expect("active object buckets");
    let Some(keys) = &buckets.object_keys else {
        return;
    };
    let Some(index) = keys.iter().position(|key| key == name) else {
        return;
    };
    let removed = buckets.positions[index];
    let mut next = buckets.end;
    for (index, key) in keys.iter().enumerate() {
        let position = buckets.positions[index];
        if position > removed
            && position < next
            && rows
                .iter()
                .any(|row| &row.name == key && row.public && !row.value.is_undef())
        {
            next = position;
        }
    }
    // Only the iterator performing offsetUnset advances. Other views retain
    // their position, just as they do after an external property unset.
    let mut cursor = cursor.borrow_mut();
    if cursor.generation == buckets.generation && cursor.live == removed {
        cursor.live = next;
    }
}

fn rewind(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    projected_entry(arg!(ed, 0), Move::Rewind, Projection::None, eg);
    ret!(rv, Value::null());
}
fn next(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    projected_entry(arg!(ed, 0), Move::Next, Projection::None, eg);
    ret!(rv, Value::null());
}
fn current(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let value = projected_entry(arg!(ed, 0), Move::Current, Projection::Value, eg)
        .map_or_else(Value::null, |entry| entry.1);
    ret!(rv, value);
}
fn key(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let key = projected_entry(arg!(ed, 0), Move::Current, Projection::Key, eg)
        .map_or_else(Value::null, |entry| entry.0);
    ret!(rv, key);
}
fn valid(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(
        rv,
        Value::bool(projected_entry(arg!(ed, 0), Move::Current, Projection::None, eg).is_some())
    );
}
fn seek(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let value = arg!(ed, 1).dereferenced().clone();
    let Some(offset) = typed_internal_int_value_argument_expected(
        ed,
        eg,
        &value,
        "ArrayIterator::seek",
        0,
        "offset",
        "int",
    )?
    else {
        ret!(rv, Value::null());
    };
    if offset < 0
        || projected_entry(
            arg!(ed, 0),
            Move::Seek(offset as usize),
            Projection::None,
            eg,
        )
        .is_none()
    {
        eg.exception = Some(make_error_value(
            "OutOfBoundsException",
            &format!("Seek position {offset} is out of range"),
        ));
    }
    ret!(rv, Value::null());
}

pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut functions = Vec::new();
    for (name, handler, return_type) in [
        (
            "rewind",
            rewind as InternalFunctionHandler,
            ParamTypeHint::Void,
        ),
        ("current", current, ParamTypeHint::Mixed),
        (
            "key",
            key,
            ParamTypeHint::Union(vec![
                ParamTypeHint::String,
                ParamTypeHint::Int,
                ParamTypeHint::Nullable(Box::new(ParamTypeHint::None)),
            ]),
        ),
        ("next", next, ParamTypeHint::Void),
        ("valid", valid, ParamTypeHint::Bool),
        ("seek", seek, ParamTypeHint::Void),
    ] {
        let names = if name == "seek" {
            vec!["offset"]
        } else {
            vec![]
        };
        let hints = if name == "seek" {
            vec![ParamTypeHint::Int]
        } else {
            vec![]
        };
        eg.register_internal_method_contract(
            "ArrayIterator",
            name,
            false,
            names.len() as u32,
            &names,
            hints.clone(),
            return_type,
            &vec![None; names.len()],
            true,
        );
        let mut function = Box::new(make_internal_method(
            handler,
            names.len() as u32 + 1,
            names.len() as u32,
            names.iter().map(|name| name.to_string()).collect(),
        ));
        function.common.sig.param_type_hints = hints;
        function.handler_validates_types = true;
        let ptr = &function.common as *const FunctionCommon;
        eg.function_table
            .insert(format!("arrayiterator::{name}"), ptr);
        eg.method_declaring_class
            .insert(ptr, "ArrayIterator".into());
        eg.register_internal_function_display_name(ptr, format!("ArrayIterator::{name}"));
        eg.register_internal_function_reflection_metadata(ptr, vec![None; names.len()], "SPL");
        functions.push(function);
    }
    functions
}
