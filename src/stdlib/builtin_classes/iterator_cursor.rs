//! Empty, repeating and parallel policies over the ordinary Iterator protocol.
//! Parallel membership owns traced edges, never a borrow across PHP callbacks.
use super::*;
use crate::value::NativeObjectState;
use crate::vm::function::InternalFunctionHandler;
use std::collections::HashMap;

struct Entry {
    iterator: Value,
    info: Value,
}
#[derive(Default)]
struct Parallel {
    entries: Vec<Option<Entry>>,
    identities: HashMap<usize, usize>,
    flags: i64,
    scans: usize,
}
impl NativeObjectState for Parallel {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        let mut copy = Parallel::default();
        for entry in self.entries.iter().flatten() {
            let slot = copy.entries.len();
            copy.identities
                .insert(entry.iterator.object_identity().unwrap(), slot);
            copy.entries.push(Some(Entry {
                iterator: entry.iterator.clone(),
                info: entry.info.clone_for_php_storage(),
            }));
        }
        Box::new(copy)
    }
    fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        for entry in self.entries.iter().flatten() {
            visit(&entry.iterator);
            visit(&entry.info);
        }
    }
    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        for entry in self.entries.iter_mut().rev().filter_map(Option::take) {
            pending.push(entry.info);
            pending.push(entry.iterator);
        }
        self.entries.clear();
        self.identities.clear();
    }
}
fn read<T>(receiver: &Value, body: impl FnOnce(&Parallel) -> T) -> T {
    let object = receiver.as_object().expect("iterator receiver");
    match object.native_object_state::<Parallel>() {
        Some(state) => body(state),
        None => body(&Parallel::default()),
    }
}
fn change<T>(receiver: &Value, body: impl FnOnce(&mut Parallel) -> T) -> T {
    body(
        receiver
            .as_object_mut()
            .expect("iterator receiver")
            .native_object_state_mut::<Parallel>(),
    )
}
fn error(eg: &mut ExecutorGlobals, class: &str, message: &str) {
    let replacement = make_error_value(class, message);
    if let Some(previous) = eg.exception.take() {
        crate::vm::execute::append_replaced_exception(&replacement, &previous, eg);
    }
    eg.exception = Some(replacement);
}
fn is_iterator(value: &Value, eg: &ExecutorGlobals) -> bool {
    value
        .as_object()
        .is_some_and(|o| eg.class_is_a(&o.class_name, "Iterator"))
}
fn iterator_argument(value: &Value, method: &str, eg: &mut ExecutorGlobals) -> bool {
    if is_iterator(value, eg) {
        return true;
    }
    typed_internal_argument_error(
        eg,
        &format!("MultipleIterator::{method}"),
        value,
        1,
        "iterator",
        "Iterator",
    );
    false
}
fn insert(
    receiver: &Value,
    iterator: Value,
    info: Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let identity = iterator.object_identity().expect("validated iterator");
    let retired = change(receiver, |state| {
        if let Some(&slot) = state.identities.get(&identity) {
            return Some(std::mem::replace(
                &mut state.entries[slot].as_mut().unwrap().info,
                info,
            ));
        }
        if state.scans == 0
            && state.entries.len() >= 32
            && state.identities.len() < state.entries.len() / 2
        {
            state.entries.retain(Option::is_some);
            for (slot, entry) in state.entries.iter().enumerate() {
                state.identities.insert(
                    entry.as_ref().unwrap().iterator.object_identity().unwrap(),
                    slot,
                );
            }
        }
        state.identities.insert(identity, state.entries.len());
        state.entries.push(Some(Entry { iterator, info }));
        None
    });
    if let Some(value) = retired {
        iterator_delegate::discard(value, eg)?;
    }
    Ok(())
}
fn remove(receiver: &Value, iterator: &Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let retired = iterator.object_identity().and_then(|identity| {
        change(receiver, |state| {
            state
                .identities
                .remove(&identity)
                .and_then(|slot| state.entries[slot].take())
        })
    });
    if let Some(entry) = retired {
        iterator_delegate::discard(entry.iterator, eg)?;
        iterator_delegate::discard(entry.info, eg)?;
    }
    Ok(())
}
/// Scan live physical slots. Mutation is published before callbacks retire an
/// entry; only the selected entry remains rooted while its protocol executes.
fn scan(
    receiver: &Value,
    eg: &mut ExecutorGlobals,
    mut body: impl FnMut(&Value, &Value, &mut ExecutorGlobals) -> Result<bool, VmError>,
) -> Result<(), VmError> {
    change(receiver, |state| state.scans += 1);
    let result = (|| {
        let mut next = 0;
        loop {
            let entry = read(receiver, |state| {
                state
                    .entries
                    .iter()
                    .enumerate()
                    .skip(next)
                    .find_map(|(slot, entry)| {
                        entry
                            .as_ref()
                            .map(|e| (slot, e.iterator.clone(), e.info.clone()))
                    })
            });
            let Some((slot, iterator, info)) = entry else {
                break;
            };
            next = slot + 1;
            let action = body(&iterator, &info, eg);
            let release_iterator = iterator_delegate::discard(iterator, eg);
            let release_info = iterator_delegate::discard(info, eg);
            let more = action?;
            release_iterator?;
            release_info?;
            if !more || eg.exception.is_some() {
                break;
            }
        }
        Ok(())
    })();
    change(receiver, |state| state.scans -= 1);
    result
}
fn empty_noop(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::null());
}
fn empty_valid(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::bool(false));
}
fn empty_current(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    error(
        eg,
        "BadMethodCallException",
        "Accessing the value of an EmptyIterator",
    );
    Ok(())
}
fn empty_key(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    error(
        eg,
        "BadMethodCallException",
        "Accessing the key of an EmptyIterator",
    );
    Ok(())
}
fn infinite_construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    iterator_delegate::construct(ed, rv, eg, "InfiniteIterator")
}
fn infinite_next(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !iterator_delegate::initialized(&receiver, eg) {
        ret!(rv, Value::null());
    }
    iterator_delegate::clear(&receiver, eg)?;
    if eg.exception.is_some() {
        ret!(rv, Value::null());
    }
    let inner = iterator_delegate::inner(&receiver);
    iterator_delegate::protocol(eg, &inner, "next")?;
    if eg.exception.is_some() {
        ret!(rv, Value::null());
    }
    let valid = iterator_delegate::protocol(eg, &inner, "valid")?;
    if eg.exception.is_some() {
        ret!(rv, Value::null());
    }
    if !valid.is_truthy() {
        iterator_delegate::protocol(eg, &inner, "rewind")?;
        if eg.exception.is_none() {
            iterator_delegate::fetch(&receiver, eg)?;
        }
    } else {
        // The valid callback has already run; publish current then key once.
        let current = iterator_delegate::protocol(eg, &inner, "current")?;
        if eg.exception.is_none() {
            receiver
                .as_object_mut()
                .unwrap()
                .native_iterator_delegate_mut()
                .unwrap()
                .current = current;
            let key = iterator_delegate::protocol(eg, &inner, "key")?;
            if eg.exception.is_none() {
                receiver
                    .as_object_mut()
                    .unwrap()
                    .native_iterator_delegate_mut()
                    .unwrap()
                    .key = key;
            }
        }
    }
    ret!(rv, Value::null());
}
fn set_flags_impl(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    construct: bool,
) -> Result<(), VmError> {
    let value = arg_opt!(ed, 1).cloned().unwrap_or_else(|| Value::long(1));
    let method = if construct {
        "MultipleIterator::__construct"
    } else {
        "MultipleIterator::setFlags"
    };
    if let Some(flags) =
        typed_internal_int_value_argument_expected(ed, eg, &value, method, 0, "flags", "int")?
    {
        change(arg!(ed, 0), |state| state.flags = flags);
    }
    ret!(rv, Value::null());
}
fn multiple_construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    set_flags_impl(ed, rv, eg, true)
}
fn set_flags(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    set_flags_impl(ed, rv, eg, false)
}
fn get_flags(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::long(read(arg!(ed, 0), |s| s.flags)));
}
fn count(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(
        rv,
        Value::long(read(arg!(ed, 0), |s| s.identities.len() as i64))
    );
}
fn attach(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    let iterator = owned_argument(ed, 1);
    if !iterator_argument(&iterator, "attachIterator", eg) {
        ret!(rv, Value::null());
    }
    let value = arg_opt!(ed, 2).cloned().unwrap_or_else(Value::null);
    let info = match value.dereferenced().value_type() {
        ValueType::String | ValueType::Long | ValueType::Null => value.dereferenced().clone(),
        ValueType::Object => {
            let Some(info) = typed_internal_string_value_expected(
                ed,
                eg,
                &value,
                "MultipleIterator::attachIterator",
                1,
                "info",
                "string|int|null",
                "string|int|null",
            )?
            else {
                ret!(rv, Value::null());
            };
            info
        }
        _ => {
            let Some(info) = typed_internal_int_value_argument_expected(
                ed,
                eg,
                &value,
                "MultipleIterator::attachIterator",
                1,
                "info",
                "string|int|null",
            )?
            else {
                ret!(rv, Value::null());
            };
            Value::long(info)
        }
    };
    if info.value_type() != ValueType::Null
        && read(&receiver, |state| {
            state
                .entries
                .iter()
                .flatten()
                .any(|e| values_identical_checked(&info, &e.info).unwrap_or(false))
        })
    {
        error(eg, "InvalidArgumentException", "Key duplication error");
        ret!(rv, Value::null());
    }
    insert(&receiver, iterator, info, eg)?;
    ret!(rv, Value::null());
}
fn detach(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let iterator = owned_argument(ed, 1);
    if iterator_argument(&iterator, "detachIterator", eg) {
        remove(arg!(ed, 0), &iterator, eg)?;
    }
    ret!(rv, Value::null());
}
fn contains(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let iterator = owned_argument(ed, 1);
    if !iterator_argument(&iterator, "containsIterator", eg) {
        ret!(rv, Value::null());
    }
    ret!(
        rv,
        Value::bool(read(arg!(ed, 0), |state| state
            .identities
            .contains_key(&iterator.object_identity().unwrap())))
    );
}
fn move_all(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    method: &str,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    scan(&receiver, eg, |iterator, _, eg| {
        iterator_delegate::protocol(eg, iterator, method)?;
        Ok(true)
    })?;
    ret!(rv, Value::null());
}
fn rewind(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    move_all(ed, rv, eg, "rewind")
}
fn next(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    move_all(ed, rv, eg, "next")
}
fn valid(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    let all = read(&receiver, |state| state.flags & 1 != 0);
    let mut result = all && read(&receiver, |state| !state.identities.is_empty());
    scan(&receiver, eg, |iterator, _, eg| {
        let valid = iterator_delegate::protocol(eg, iterator, "valid")?.is_truthy();
        if valid != all {
            result = valid;
            return Ok(false);
        }
        Ok(true)
    })?;
    ret!(rv, Value::bool(result));
}
fn projection(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    method: &str,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    let (flags, size) = read(&receiver, |state| (state.flags, state.identities.len()));
    if size == 0 {
        error(
            eg,
            "RuntimeException",
            &format!("Called {method}() on an invalid iterator"),
        );
        ret!(rv, Value::null());
    }
    let mut result = if flags & 2 == 0 {
        PhpArray::with_packed_capacity(size)
    } else {
        PhpArray::with_hash_capacity(size)
    };
    scan(&receiver, eg, |iterator, info, eg| {
        let valid = iterator_delegate::protocol(eg, iterator, "valid")?.is_truthy();
        if !valid && read(&receiver, |state| state.flags & 1 != 0) {
            error(
                eg,
                "RuntimeException",
                &format!("Called {method}() with non valid sub iterator"),
            );
            return Ok(false);
        }
        if eg.exception.is_some() {
            return Ok(false);
        }
        let value = if valid {
            iterator_delegate::protocol(eg, iterator, method)?
        } else {
            Value::null()
        };
        if eg.exception.is_some() {
            return Ok(false);
        }
        if read(&receiver, |state| state.flags & 2 == 0) {
            result.push(value);
        } else if !matches!(info.value_type(), ValueType::String | ValueType::Long) {
            iterator_delegate::discard(value, eg)?;
            error(
                eg,
                "InvalidArgumentException",
                "Sub-Iterator is associated with NULL",
            );
            return Ok(false);
        } else if let Ok(key) = value_to_array_key(info) {
            result.set(key, value);
        } else {
            iterator_delegate::discard(value, eg)?;
            error(eg, "TypeError", "Illegal offset type");
            return Ok(false);
        }
        Ok(true)
    })?;
    if eg.exception.is_some() {
        iterator_delegate::discard(Value::array(result), eg)?;
        ret!(rv, Value::null());
    }
    ret!(rv, Value::array(result));
}
fn key(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    projection(ed, rv, eg, "key")
}
fn current(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    projection(ed, rv, eg, "current")
}
fn debug_info(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    let storage = read(receiver, |state| {
        let mut result = PhpArray::with_packed_capacity(state.identities.len());
        for entry in state.entries.iter().flatten() {
            let mut pair = PhpArray::with_hash_capacity(2);
            pair.set_str("obj", entry.iterator.clone());
            pair.set_str("inf", entry.info.clone());
            result.push(Value::array(pair));
        }
        result
    });
    let mut members = array_object::serialization::members(receiver, eg);
    members.set_str("\0SplObjectStorage\0storage", Value::array(storage));
    ret!(rv, Value::array(members));
}
/// MultipleIterator inherits native storage write/unset handlers, but exposes
/// neither ArrayAccess nor read/exists handlers. Do not invent public methods.
#[cold]
#[inline(never)]
pub(crate) fn dimension(
    eg: &mut ExecutorGlobals,
    receiver: &Value,
    method: &str,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(iterator) = args.first() else {
        return Ok(None);
    };
    if method == "offsetSet" || method == "offsetSetAppend" {
        if !is_iterator(iterator, eg) {
            error(
                eg,
                "TypeError",
                "Can only attach objects that implement the Iterator interface",
            );
        } else {
            insert(
                receiver,
                iterator.clone(),
                args.get(1).cloned().unwrap_or_else(Value::null),
                eg,
            )?;
        }
        Ok(Some(Value::null()))
    } else if method == "offsetUnset" && iterator.as_object().is_some() {
        remove(receiver, iterator, eg)?;
        Ok(Some(Value::null()))
    } else {
        Ok(None)
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Array, Bool, ClassName, Int, Never, String, Union, Void};
    eg.register_class(empty_internal_type(
        "EmptyIterator",
        vec!["Iterator".into()],
        false,
        false,
    ))
    .unwrap();
    let mut infinite = empty_internal_type("InfiniteIterator", vec![], false, false);
    infinite.parent = Some("IteratorIterator".into());
    eg.register_class_with_complete_native_parent(infinite)
        .unwrap();
    let mut multiple =
        empty_internal_type("MultipleIterator", vec!["Iterator".into()], false, false);
    for (name, value) in [
        ("MIT_NEED_ANY", 0),
        ("MIT_NEED_ALL", 1),
        ("MIT_KEYS_NUMERIC", 0),
        ("MIT_KEYS_ASSOC", 2),
    ] {
        multiple.constants.push(recursive_iterator::constant(
            "MultipleIterator",
            name,
            value,
        ));
    }
    eg.register_class(multiple).unwrap();
    eg.reserve_internal_method_contracts("EmptyIterator", 5);
    eg.reserve_internal_method_contracts("InfiniteIterator", 2);
    eg.reserve_internal_method_contracts("MultipleIterator", 13);
    type Row = (
        &'static str,
        &'static str,
        InternalFunctionHandler,
        &'static [&'static str],
        Vec<ParamTypeHint>,
        &'static [Option<&'static str>],
        ParamTypeHint,
    );
    let rows: [Row; 20] = [
        (
            "EmptyIterator",
            "current",
            empty_current,
            &[],
            vec![],
            &[],
            Never,
        ),
        ("EmptyIterator", "next", empty_noop, &[], vec![], &[], Void),
        ("EmptyIterator", "key", empty_key, &[], vec![], &[], Never),
        (
            "EmptyIterator",
            "valid",
            empty_valid,
            &[],
            vec![],
            &[],
            ClassName("false".into()),
        ),
        (
            "EmptyIterator",
            "rewind",
            empty_noop,
            &[],
            vec![],
            &[],
            Void,
        ),
        (
            "InfiniteIterator",
            "__construct",
            infinite_construct,
            &["iterator"],
            vec![ClassName("Iterator".into())],
            &[None],
            ParamTypeHint::None,
        ),
        (
            "InfiniteIterator",
            "next",
            infinite_next,
            &[],
            vec![],
            &[],
            Void,
        ),
        (
            "MultipleIterator",
            "__construct",
            multiple_construct,
            &["flags"],
            vec![Int],
            &[Some("1")],
            ParamTypeHint::None,
        ),
        (
            "MultipleIterator",
            "getFlags",
            get_flags,
            &[],
            vec![],
            &[],
            Int,
        ),
        (
            "MultipleIterator",
            "setFlags",
            set_flags,
            &["flags"],
            vec![Int],
            &[None],
            Void,
        ),
        (
            "MultipleIterator",
            "attachIterator",
            attach,
            &["iterator", "info"],
            vec![
                ClassName("Iterator".into()),
                Union(vec![String, Int, ClassName("null".into())]),
            ],
            &[None, Some("null")],
            Void,
        ),
        (
            "MultipleIterator",
            "detachIterator",
            detach,
            &["iterator"],
            vec![ClassName("Iterator".into())],
            &[None],
            Void,
        ),
        (
            "MultipleIterator",
            "containsIterator",
            contains,
            &["iterator"],
            vec![ClassName("Iterator".into())],
            &[None],
            Bool,
        ),
        (
            "MultipleIterator",
            "countIterators",
            count,
            &[],
            vec![],
            &[],
            Int,
        ),
        ("MultipleIterator", "rewind", rewind, &[], vec![], &[], Void),
        ("MultipleIterator", "valid", valid, &[], vec![], &[], Bool),
        ("MultipleIterator", "key", key, &[], vec![], &[], Array),
        (
            "MultipleIterator",
            "current",
            current,
            &[],
            vec![],
            &[],
            Array,
        ),
        ("MultipleIterator", "next", next, &[], vec![], &[], Void),
        (
            "MultipleIterator",
            "__debugInfo",
            debug_info,
            &[],
            vec![],
            &[],
            Array,
        ),
    ];
    let mut functions = Vec::with_capacity(rows.len());
    for (owner, name, handler, names, hints, defaults, result) in rows {
        recursive_iterator::register_method(
            eg,
            &mut functions,
            owner,
            name,
            handler,
            names,
            hints,
            defaults,
            result,
        );
    }
    functions
}
