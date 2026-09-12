//! A heap owns entries once, independently of the intermediate slot views
//! published by percolation. No object borrow spans a comparison or release.
use super::*;
use crate::value::NativeObjectState;
use crate::vm::function::InternalFunctionHandler;

mod serialization;

struct Entry {
    value: Value,
    priority: Value,
}
impl Clone for Entry {
    fn clone(&self) -> Self {
        use crate::stdlib::serialization::clone_unserialized_storage_value as retained;
        Self {
            value: retained(&self.value),
            priority: retained(&self.priority),
        }
    }
}
#[derive(Default)]
struct Heap {
    entries: Vec<Option<Entry>>,
    free: Vec<usize>,
    order: Vec<usize>,
    len: usize,
    flags: u8,
    queue: bool,
    native_comparison: Option<bool>,
    locked: bool,
    corrupted: bool,
}
impl Heap {
    fn entry(&self, slot: usize) -> &Entry {
        self.entries[slot].as_ref().expect("owned heap entry")
    }
    fn allocate(&mut self, entry: Entry) -> usize {
        if let Some(slot) = self.free.pop() {
            self.entries[slot] = Some(entry);
            slot
        } else {
            self.entries.push(Some(entry));
            self.entries.len() - 1
        }
    }
    fn project(&self, slot: usize, flags: u8) -> Value {
        let entry = self.entry(slot);
        match flags {
            2 if self.queue => entry.priority.clone(),
            3 if self.queue => {
                let mut pair = PhpArray::with_hash_capacity(2);
                pair.set_str("data", entry.value.clone());
                pair.set_str("priority", entry.priority.clone());
                Value::array(pair)
            }
            _ => entry.value.clone(),
        }
    }
}
impl NativeObjectState for Heap {
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        let mut copy = Heap {
            entries: Vec::with_capacity(self.len),
            order: Vec::with_capacity(self.len),
            len: self.len,
            flags: self.flags,
            queue: self.queue,
            native_comparison: self.native_comparison,
            corrupted: self.corrupted,
            ..Heap::default()
        };
        for &slot in &self.order[..self.len] {
            let slot = copy.allocate(self.entry(slot).clone());
            copy.order.push(slot);
        }
        Box::new(copy)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        if !self.locked {
            for &slot in &self.order {
                let entry = self.entry(slot);
                visit(&entry.value);
                visit(&entry.priority);
            }
            return;
        }
        // A moving hole can expose an entry at two indices, but it still owns
        // exactly one edge. Include unpublished insertion and retirement roots.
        let mut seen = vec![false; self.entries.len()];
        for &slot in &self.order {
            if !std::mem::replace(&mut seen[slot], true) {
                let entry = self.entry(slot);
                visit(&entry.value);
                visit(&entry.priority);
            }
        }
        for (slot, entry) in self.entries.iter().enumerate() {
            if !seen[slot]
                && let Some(entry) = entry
            {
                visit(&entry.value);
                visit(&entry.priority);
            }
        }
    }
    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        for &slot in self.order.iter().rev() {
            if let Some(entry) = self.entries[slot].take() {
                pending.push(entry.priority);
                pending.push(entry.value);
            }
        }
        if self.locked {
            for entry in self.entries.iter_mut().rev().filter_map(Option::take) {
                pending.push(entry.priority);
                pending.push(entry.value);
            }
        }
        self.len = 0;
        self.order.clear();
    }
}
fn ensure(receiver: &Value, eg: &ExecutorGlobals) {
    let mut object = receiver.as_object_mut().expect("heap receiver");
    if object.native_object_state::<Heap>().is_none() {
        let queue = eg.class_is_a(&object.class_name, "SplPriorityQueue");
        let (_, _, _, owner) = find_method_in_class_hierarchy(eg, &object.class_name, "compare")
            .expect("heap comparator");
        let native_comparison = match owner {
            "SplMinHeap" => Some(true),
            "SplHeap" | "SplMaxHeap" | "SplPriorityQueue" => Some(false),
            _ => None,
        };
        let state = object.native_object_state_mut::<Heap>();
        state.queue = queue;
        state.flags = u8::from(queue);
        // Class declarations are immutable. Cache only the native semantic
        // kind, not a callable pointer or an untraced PHP owner.
        state.native_comparison = native_comparison;
    }
}
fn read<T>(receiver: &Value, f: impl FnOnce(&Heap) -> T) -> T {
    let object = receiver.as_object().expect("heap receiver");
    f(object
        .native_object_state::<Heap>()
        .expect("initialized heap"))
}
fn change<T>(receiver: &Value, f: impl FnOnce(&mut Heap) -> T) -> T {
    let mut object = receiver.as_object_mut().expect("heap receiver");
    f(object.native_object_state_mut::<Heap>())
}
fn error(eg: &mut ExecutorGlobals, message: &str) {
    eg.exception = Some(make_error_value("RuntimeException", message));
}
fn healthy(receiver: &Value, eg: &mut ExecutorGlobals) -> bool {
    if read(receiver, |h| h.corrupted) {
        error(
            eg,
            "Heap is corrupted, heap properties are no longer ensured.",
        );
        false
    } else {
        true
    }
}
fn begin(receiver: &Value, eg: &mut ExecutorGlobals) -> bool {
    ensure(receiver, eg);
    if read(receiver, |h| h.locked) {
        error(
            eg,
            "Heap cannot be changed when it is already being modified.",
        );
        return false;
    }
    if !healthy(receiver, eg) {
        return false;
    }
    change(receiver, |h| h.locked = true);
    true
}
fn finish(receiver: &Value, failed: bool) {
    change(receiver, |h| {
        h.corrupted |= failed;
        h.locked = false;
    });
}
enum Comparator {
    Native(bool),
    User(ResolvedCallback),
}
fn comparator(receiver: &Value, eg: &ExecutorGlobals) -> Comparator {
    if let Some(reverse) = read(receiver, |h| h.native_comparison) {
        return Comparator::Native(reverse);
    }
    let name = receiver.as_object().unwrap().class_name.to_string();
    let (_, _, function, owner) =
        find_method_in_class_hierarchy(eg, &name, "compare").expect("heap comparator");
    if matches!(
        owner,
        "SplHeap" | "SplMaxHeap" | "SplMinHeap" | "SplPriorityQueue"
    ) {
        Comparator::Native(owner == "SplMinHeap")
    } else {
        Comparator::User(ResolvedCallback {
            func_ptr: function,
            prepend_args: vec![receiver.clone()],
            use_vars: vec![],
            called_scope_class_id: receiver.as_object().unwrap().class_id,
            closure_scope_class_id: None,
            bound_this: None,
            closure_static_vars: None,
            is_magic_call: false,
        })
    }
}
fn compare_values(left: &Value, right: &Value, eg: &mut ExecutorGlobals) -> Result<i64, VmError> {
    sort_regular_value_order_runtime(eg, left, right).map(|order| match order {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    })
}
fn compare_slots(
    receiver: &Value,
    comparator: &Comparator,
    left: usize,
    right: usize,
    eg: &mut ExecutorGlobals,
) -> Result<i64, VmError> {
    if let Comparator::Native(reverse) = comparator {
        let integer = read(receiver, |h| {
            let a = h.entry(left);
            let b = h.entry(right);
            let (a, b) = if h.queue {
                (&a.priority, &b.priority)
            } else {
                (&a.value, &b.value)
            };
            a.as_long().zip(b.as_long()).map(|(a, b)| match a.cmp(&b) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        });
        if let Some(result) = integer {
            return Ok(if *reverse { -result } else { result });
        }
    }
    let (left, right) = read(receiver, |h| {
        let a = h.entry(left);
        let b = h.entry(right);
        if h.queue {
            (a.priority.clone(), b.priority.clone())
        } else {
            (a.value.clone(), b.value.clone())
        }
    });
    match comparator {
        Comparator::Native(reverse) => {
            let result = compare_values(&left, &right, eg)?;
            Ok(if *reverse { -result } else { result })
        }
        Comparator::User(callback) => call_resolved_with_values(eg, callback, &[left, right])
            .map(|result| result.to_long_val()),
    }
}
fn insert(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    if !begin(receiver, eg) {
        return Ok(());
    }
    let queue = read(receiver, |h| h.queue);
    let entry = Entry {
        value: arg!(ed, 1).dereferenced().clone(),
        priority: if queue {
            arg!(ed, 2).dereferenced().clone()
        } else {
            Value::null()
        },
    };
    insert_entry(receiver, entry, eg)?;
    ret!(rv, Value::bool(true));
}

// Both public insertion and native restoration use the same locked
// percolation. Restore retains serialized references instead of dereferencing
// a public by-value argument. Callers acquire the lock before entering.
#[inline(always)]
fn insert_entry(receiver: &Value, entry: Entry, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let (slot, mut hole) = change(receiver, |h| {
        let slot = h.allocate(entry);
        h.order.push(slot);
        (slot, h.len)
    });
    let comparator = comparator(receiver, eg);
    let operation = (|| {
        while hole > 0 {
            let parent = (hole - 1) / 2;
            let ancestor = read(receiver, |h| h.order[parent]);
            let ordering = compare_slots(receiver, &comparator, ancestor, slot, eg)?;
            if ordering >= 0 {
                break;
            }
            change(receiver, |h| h.order[hole] = ancestor);
            hole = parent;
            if eg.exception.is_some() {
                break;
            }
        }
        Ok(())
    })();
    change(receiver, |h| {
        h.order[hole] = slot;
        h.len += 1;
    });
    finish(receiver, operation.is_err() || eg.exception.is_some());
    operation
}
fn retire_visible(receiver: &Value, slot: usize, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    // Prepare while the sole owner is still visible, then release the borrow
    // before dispatch. A destructor can read current(), count() and clone().
    for priority in [false, true] {
        let plan = read(receiver, |h| {
            let entry = h.entry(slot);
            let value = if priority {
                &entry.priority
            } else {
                &entry.value
            };
            if value.value_type() == ValueType::Array {
                crate::vm::execute::prepare_replaced_value_tree_destructor_with_references(
                    eg, value, 1,
                )
            } else {
                crate::vm::execute::prepare_replaced_value_destructor(eg, value)
            }
        });
        crate::vm::execute::run_prepared_value_destructor(eg, plan)?;
        if eg.exception.is_some() {
            break;
        }
    }
    Ok(())
}
fn remove(receiver: &Value, take: bool, eg: &mut ExecutorGlobals) -> Result<Value, VmError> {
    if !begin(receiver, eg) {
        return Ok(Value::null());
    }
    let len = read(receiver, |h| h.len);
    if len == 0 {
        finish(receiver, false);
        if take {
            error(eg, "Can't extract from an empty heap");
        }
        return Ok(Value::null());
    }
    let root = read(receiver, |h| h.order[0]);
    let result = if take {
        read(receiver, |h| h.project(root, h.flags))
    } else {
        Value::null()
    };
    let retirement = if take {
        Ok(())
    } else {
        retire_visible(receiver, root, eg)
    };
    let tail = change(receiver, |h| {
        h.len -= 1;
        h.order[h.len]
    });
    let mut hole = 0;
    let comparator = comparator(receiver, eg);
    let operation = (|| {
        retirement?;
        while eg.exception.is_none() {
            let child = hole * 2 + 1;
            let len = read(receiver, |h| h.len);
            if child >= len {
                break;
            }
            let mut winner = read(receiver, |h| h.order[child]);
            let mut destination = child;
            // The retained tail remains available during down-heap comparison,
            // including the child index equal to the newly published count.
            if child < len {
                let right = read(receiver, |h| h.order[child + 1]);
                let ordering = compare_slots(receiver, &comparator, right, winner, eg)?;
                if eg.exception.is_some() {
                    break;
                }
                if ordering > 0 {
                    winner = right;
                    destination += 1;
                }
            }
            let ordering = compare_slots(receiver, &comparator, tail, winner, eg)?;
            if ordering >= 0 {
                break;
            }
            change(receiver, |h| h.order[hole] = winner);
            hole = destination;
            if eg.exception.is_some() {
                break;
            }
        }
        Ok(())
    })();
    let retired = change(receiver, |h| {
        h.order[hole] = tail;
        h.order.truncate(h.len);
        h.free.push(root);
        h.entries[root].take().expect("retired root")
    });
    finish(receiver, operation.is_err() || eg.exception.is_some());
    // Unselected projection values are retired after mutation. For next(),
    // their destructor phase already ran at the locked, visible boundary.
    iterator_delegate::discard(retired.value, eg)?;
    iterator_delegate::discard(retired.priority, eg)?;
    operation?;
    Ok(result)
}
fn extract(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let result = remove(arg!(ed, 0), true, eg)?;
    ret!(rv, result);
}
fn next(ed: *mut ExecuteData, _rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    remove(arg!(ed, 0), false, eg).map(|_| ())
}
fn top(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    ensure(receiver, eg);
    if !healthy(receiver, eg) {
        return Ok(());
    }
    if read(receiver, |h| h.len == 0) {
        error(eg, "Can't peek at an empty heap");
        return Ok(());
    }
    ret!(rv, read(receiver, |h| h.project(h.order[0], h.flags)));
}
fn current(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    ensure(receiver, eg);
    ret!(
        rv,
        read(receiver, |h| if h.len == 0 {
            Value::null()
        } else {
            h.project(h.order[0], h.flags)
        })
    );
}
macro_rules! query {
    ($name:ident, $result:expr) => {
        fn $name(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            ensure(arg!(ed, 0), eg);
            ret!(rv, read(arg!(ed, 0), $result));
        }
    };
}
query!(count, |h: &Heap| Value::long(h.len as i64));
query!(key, |h: &Heap| Value::long(h.len as i64 - 1));
query!(is_empty, |h: &Heap| Value::bool(h.len == 0));
query!(valid, |h: &Heap| Value::bool(h.len != 0));
query!(is_corrupted, |h: &Heap| Value::bool(h.corrupted));
query!(get_flags, |h: &Heap| Value::long(i64::from(h.flags)));
fn rewind(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    Ok(())
}
fn recover(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ensure(arg!(ed, 0), eg);
    change(arg!(ed, 0), |h| h.corrupted = false);
    ret!(rv, Value::bool(true));
}
fn set_flags(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(flags) = typed_internal_int_value_argument_expected(
        ed,
        eg,
        arg!(ed, 1),
        "SplPriorityQueue::setExtractFlags",
        0,
        "flags",
        "int",
    )?
    else {
        return Ok(());
    };
    let flags = (flags & 3) as u8;
    if flags == 0 {
        error(eg, "Must specify at least one extract flag");
        return Ok(());
    }
    ensure(arg!(ed, 0), eg);
    change(arg!(ed, 0), |h| h.flags = flags);
    ret!(rv, Value::long(i64::from(flags)));
}
fn compare_max(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let result = compare_values(arg!(ed, 1), arg!(ed, 2), eg)?;
    ret!(rv, Value::long(result));
}
fn compare_min(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let result = compare_values(arg!(ed, 1), arg!(ed, 2), eg)?;
    ret!(rv, Value::long(-result));
}
fn compare_abstract(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    eg.exception = Some(make_error_value(
        "Error",
        "Cannot call abstract method SplHeap::compare()",
    ));
    Ok(())
}
fn debug_info(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    ensure(receiver, eg);
    let mut result = array_object::member_properties(receiver, eg);
    read(receiver, |h| {
        let owner = if h.queue {
            "SplPriorityQueue"
        } else {
            "SplHeap"
        };
        result.set_str(
            &format!("\0{owner}\0flags"),
            Value::long(i64::from(h.flags)),
        );
        result.set_str(&format!("\0{owner}\0isCorrupted"), Value::bool(h.corrupted));
        let mut entries = PhpArray::with_packed_capacity(h.len);
        for &slot in &h.order[..h.len] {
            entries.push(h.project(slot, 3));
        }
        result.set_str(&format!("\0{owner}\0heap"), Value::array(entries));
    });
    ret!(rv, Value::array(result));
}

#[cold]
fn register_owner(
    eg: &mut ExecutorGlobals,
    owner: &'static str,
    queue: bool,
) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Array, Bool, Int, Mixed, Void};
    let common: &[(
        &str,
        InternalFunctionHandler,
        &[&str],
        &[ParamTypeHint],
        ParamTypeHint,
    )] = &[
        (
            "insert",
            insert,
            if queue {
                &["value", "priority"]
            } else {
                &["value"]
            },
            if queue { &[Mixed, Mixed] } else { &[Mixed] },
            ParamTypeHint::ClassName("true".into()),
        ),
        ("extract", extract, &[], &[], Mixed),
        ("top", top, &[], &[], Mixed),
        ("current", current, &[], &[], Mixed),
        ("next", next, &[], &[], Void),
        ("rewind", rewind, &[], &[], Void),
        ("valid", valid, &[], &[], Bool),
        ("count", count, &[], &[], Int),
        ("key", key, &[], &[], Int),
        ("isEmpty", is_empty, &[], &[], Bool),
        ("isCorrupted", is_corrupted, &[], &[], Bool),
        (
            "recoverFromCorruption",
            recover,
            &[],
            &[],
            ParamTypeHint::ClassName("true".into()),
        ),
        ("__debugInfo", debug_info, &[], &[], Array),
        ("__serialize", serialization::serialize, &[], &[], Array),
        (
            "__unserialize",
            serialization::unserialize,
            &["data"],
            &[Array],
            Void,
        ),
        (
            "compare",
            if queue { compare_max } else { compare_abstract },
            if queue {
                &["priority1", "priority2"]
            } else {
                &["value1", "value2"]
            },
            &[Mixed, Mixed],
            Int,
        ),
    ];
    let mut functions = Vec::with_capacity(common.len() + if queue { 2 } else { 0 });
    eg.reserve_internal_method_contracts(owner, functions.capacity());
    for (name, handler, names, hints, result) in common {
        functions.push(register_method(
            eg,
            owner,
            name,
            *handler,
            names,
            hints,
            result.clone(),
        ));
    }
    if queue {
        functions.push(register_method(
            eg,
            owner,
            "setExtractFlags",
            set_flags,
            &["flags"],
            &[Int],
            Int,
        ));
        functions.push(register_method(
            eg,
            owner,
            "getExtractFlags",
            get_flags,
            &[],
            &[],
            Int,
        ));
    } else {
        eg.set_internal_method_access(owner, "compare", Visibility::Protected, true);
    }
    functions
}
#[cold]
fn register_method(
    eg: &mut ExecutorGlobals,
    owner: &'static str,
    name: &'static str,
    handler: InternalFunctionHandler,
    names: &[&str],
    hints: &[ParamTypeHint],
    result: ParamTypeHint,
) -> Box<InternalFunction> {
    let required = names.len() as u32;
    eg.register_internal_method_contract(
        owner,
        name,
        false,
        required,
        names,
        hints.to_vec(),
        result,
        &vec![None; names.len()],
        true,
    );
    let mut function = Box::new(make_internal_method(
        handler,
        required + 1,
        required,
        names.iter().map(|name| name.to_string()).collect(),
    ));
    function.handler_validates_types = true;
    function.common.sig.param_type_hints = hints.to_vec();
    let pointer = &function.common as *const FunctionCommon;
    eg.function_table
        .insert(internal_method_lookup_name(owner, name), pointer);
    eg.method_declaring_class.insert(pointer, owner.into());
    eg.register_internal_function_display_name(pointer, internal_method_display_name(owner, name));
    eg.register_internal_function_reflection_metadata(pointer, vec![None; names.len()], "SPL");
    function
}
pub(super) fn register_priority_queue(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    register_owner(eg, "SplPriorityQueue", true)
}
pub(super) fn register_heaps(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut base = empty_internal_type(
        "SplHeap",
        vec!["Iterator".into(), "Countable".into()],
        false,
        false,
    );
    base.is_abstract = true;
    base.abstract_methods.push("compare".into());
    let mut functions = register_owner(eg, "SplHeap", false);
    eg.register_class(base).unwrap();
    for (name, handler) in [
        ("SplMaxHeap", compare_max as InternalFunctionHandler),
        ("SplMinHeap", compare_min as InternalFunctionHandler),
    ] {
        let mut class = empty_internal_type(name, vec![], false, false);
        class.parent = Some("SplHeap".into());
        eg.register_class(class).unwrap();
        eg.reserve_internal_method_contracts(name, 1);
        functions.push(register_method(
            eg,
            name,
            "compare",
            handler,
            &["value1", "value2"],
            &[ParamTypeHint::Mixed, ParamTypeHint::Mixed],
            ParamTypeHint::Int,
        ));
        eg.set_internal_method_access(name, "compare", Visibility::Protected, false);
    }
    functions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(value: i64) -> Entry {
        Entry {
            value: Value::long(value),
            priority: Value::null(),
        }
    }
    #[test]
    fn transient_views_trace_each_owned_edge_once() {
        let mut heap = Heap {
            locked: true,
            len: 2,
            ..Heap::default()
        };
        for value in [10, 20, 30] {
            heap.allocate(entry(value));
        }
        heap.order = vec![1, 1, 2];
        let mut values = Vec::new();
        heap.for_each_value(&mut |value| {
            if let Some(value) = value.as_long() {
                values.push(value);
            }
        });
        assert_eq!(values, [20, 30, 10]);
        let copy = heap.clone_state();
        let copy = copy.as_any().downcast_ref::<Heap>().unwrap();
        assert!(!copy.locked);
        assert_eq!(copy.len, 2);
        assert_eq!(copy.project(0, 0).as_long(), Some(20));
        assert_eq!(copy.project(1, 0).as_long(), Some(20));
    }

    #[test]
    fn normal_retirement_uses_heap_order_not_arena_order() {
        let mut heap = Heap {
            len: 3,
            ..Heap::default()
        };
        for value in [10, 20, 30] {
            heap.allocate(entry(value));
        }
        heap.order = vec![2, 0, 1];
        let mut stack = Vec::new();
        heap.append_values_reversed(&mut stack);
        let mut values = Vec::new();
        while let Some(value) = stack.pop() {
            if let Some(value) = value.as_long() {
                values.push(value);
            }
        }
        assert_eq!(values, [30, 10, 20]);
        assert!(heap.entries.iter().all(Option::is_none));
    }
}
