use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::cell::RefCell;

use crate::vm::stats;
use crate::vm::generator::GeneratorRef;
use crate::vm::function::FunctionCommon;

/// PHP object — class instance with properties.
#[derive(Debug, Clone)]
pub struct PhpObject {
    pub class_name: String,
    /// Stable numeric class ID — matches ClassDef.class_id. Used for inline cache keying.
    pub class_id: u32,
    pub properties: HashMap<String, Value>,
    /// If this object is a Generator, holds the generator state
    pub generator: Option<GeneratorRef>,
}

/// PHP array — ordered hash map with integer and string keys.
/// Preserves insertion order, supports auto-incrementing integer keys.
///
/// Two internal representations:
/// - **Packed**: `Vec<Value>` — keys are implicit 0..N-1. No per-element key storage.
///   Used for sequential integer-indexed arrays (`[1,2,3]`, `$a[] = x`).
///   Push = `Vec::push`. Read = `Vec[i]`. Clone = clone values only (no keys).
/// - **Hash**: `Vec<(ArrayKey, Value)>` + split `HashMap` indexes.
///   Used when string keys, sparse int keys, or structural mutations occur.
///
/// Transition from packed→hash is one-way and happens automatically.
pub struct PhpArray {
    storage: ArrayStorage,
    next_int_key: i64,
}

/// Internal storage representation. Not exposed outside PhpArray.
enum ArrayStorage {
    /// Sequential 0..N-1 integer keys — values only, no key storage.
    Packed(Vec<Value>),
    /// General ordered map — explicit keys + split hash indexes.
    Hash {
        entries: Vec<(ArrayKey, Value)>,
        str_index: HashMap<String, usize>,
        int_index: HashMap<i64, usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArrayKey {
    Int(i64),
    String(String),
}

impl PhpArray {
    pub fn new() -> Self {
        Self {
            storage: ArrayStorage::Packed(Vec::new()),
            next_int_key: 0,
        }
    }

    /// Transition from packed to hash mode. Moves values into entries with explicit keys.
    fn transition_to_hash(&mut self) {
        if let ArrayStorage::Packed(values) = &mut self.storage {
            let len = values.len();
            let mut entries = Vec::with_capacity(len);
            let mut int_index = HashMap::with_capacity(len);
            for (i, val) in std::mem::take(values).into_iter().enumerate() {
                int_index.insert(i as i64, i);
                entries.push((ArrayKey::Int(i as i64), val));
            }
            *&mut self.storage = ArrayStorage::Hash {
                entries,
                str_index: HashMap::new(),
                int_index,
            };
        }
    }

    /// Append with auto-incrementing key ($a[] = val)
    #[inline]
    pub fn push(&mut self, val: Value) {
        let key = self.next_int_key;
        self.next_int_key = key + 1;
        match &mut self.storage {
            ArrayStorage::Packed(values) => {
                values.push(val);
            }
            ArrayStorage::Hash { entries, int_index, .. } => {
                let idx = entries.len();
                entries.push((ArrayKey::Int(key), val));
                int_index.insert(key, idx);
            }
        }
    }

    /// Set by integer key
    pub fn set_int(&mut self, key: i64, val: Value) {
        // Use raw pointer to avoid borrow conflict with next_int_key
        let storage = &mut self.storage;
        if let ArrayStorage::Packed(values) = storage {
            // Can stay packed if key == next sequential
            if key == self.next_int_key {
                self.next_int_key = key + 1;
                values.push(val);
                return;
            }
            // Overwrite existing packed slot?
            if key >= 0 && (key as usize) < values.len() {
                values[key as usize] = val;
                return;
            }
        }
        // Need hash mode — transition if packed
        self.transition_to_hash();
        // Now in hash mode
        let storage = &mut self.storage;
        if let ArrayStorage::Hash { entries, int_index, .. } = storage {
            if let Some(&idx) = int_index.get(&key) {
                entries[idx].1 = val;
            } else {
                let idx = entries.len();
                entries.push((ArrayKey::Int(key), val));
                int_index.insert(key, idx);
                if key >= self.next_int_key {
                    self.next_int_key = key + 1;
                }
            }
        }
    }

    /// Set by string key
    pub fn set_str(&mut self, key: &str, val: Value) {
        // String key → always hash mode
        if matches!(&self.storage, ArrayStorage::Packed(_)) {
            self.transition_to_hash();
        }
        if let ArrayStorage::Hash { entries, str_index, .. } = &mut self.storage {
            if let Some(&idx) = str_index.get(key) {
                // Key exists — overwrite value, no allocation for key
                entries[idx].1 = val;
            } else {
                // New key — allocate once for both entry and index
                let owned = key.to_string();
                let idx = entries.len();
                entries.push((ArrayKey::String(owned.clone()), val));
                str_index.insert(owned, idx);
            }
        }
    }

    /// Set by ArrayKey
    pub fn set(&mut self, key: ArrayKey, val: Value) {
        match key {
            ArrayKey::Int(k) => self.set_int(k, val),
            ArrayKey::String(ref k) => self.set_str(k, val),
        }
    }

    /// Get by integer key — O(1)
    #[inline]
    pub fn get_int(&self, key: i64) -> Option<&Value> {
        match &self.storage {
            ArrayStorage::Packed(values) => {
                if key >= 0 {
                    values.get(key as usize)
                } else {
                    None
                }
            }
            ArrayStorage::Hash { entries, int_index, .. } => {
                int_index.get(&key).map(|&idx| &entries[idx].1)
            }
        }
    }

    /// Get by string key — O(1), zero allocation.
    /// Uses `HashMap<String, usize>::get(&str)` via `Borrow<str>` trait.
    #[inline]
    pub fn get_str(&self, key: &str) -> Option<&Value> {
        match &self.storage {
            ArrayStorage::Packed(_) => None, // packed arrays have no string keys
            ArrayStorage::Hash { entries, str_index, .. } => {
                str_index.get(key).map(|&idx| &entries[idx].1)
            }
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        match &self.storage {
            ArrayStorage::Packed(values) => values.len(),
            ArrayStorage::Hash { entries, .. } => entries.len(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        match &self.storage {
            ArrayStorage::Packed(values) => values.is_empty(),
            ArrayStorage::Hash { entries, .. } => entries.is_empty(),
        }
    }

    /// Get entry at position — for foreach and positional access.
    /// Works efficiently for both packed and hash modes without transition.
    /// Packed: key is the implicit integer index. Hash: key from stored entry.
    #[inline]
    pub fn get_at(&self, pos: usize) -> Option<(&Value, ArrayKey)> {
        match &self.storage {
            ArrayStorage::Packed(values) => {
                values.get(pos).map(|v| (v, ArrayKey::Int(pos as i64)))
            }
            ArrayStorage::Hash { entries, .. } => {
                entries.get(pos).map(|(k, v)| (v, k.clone()))
            }
        }
    }

    /// Get value at position — for foreach when key is not needed.
    #[inline]
    pub fn get_value_at(&self, pos: usize) -> Option<&Value> {
        match &self.storage {
            ArrayStorage::Packed(values) => values.get(pos),
            ArrayStorage::Hash { entries, .. } => entries.get(pos).map(|(_, v)| v),
        }
    }

    /// Iterate over (key, &value) pairs — works for both packed and hash modes.
    /// No transition, no allocation for packed arrays.
    /// This is the preferred read-only iteration method.
    pub fn iter(&self) -> PhpArrayIter<'_> {
        match &self.storage {
            ArrayStorage::Packed(values) => PhpArrayIter::Packed(values.iter().enumerate()),
            ArrayStorage::Hash { entries, .. } => PhpArrayIter::Hash(entries.iter()),
        }
    }

    /// Access entries as `&[(ArrayKey, Value)]` — for stdlib iteration.
    /// If array is in packed mode, transitions to hash mode first.
    /// This is a cold-path method — hot paths should use get_at() or get_value_at().
    ///
    /// Takes `&mut self` to safely transition packed→hash.
    /// Callers with `&PhpArray` should use get_at()/get_value_at()/iter() instead.
    pub fn entries(&mut self) -> &[(ArrayKey, Value)] {
        self.transition_to_hash();
        match &self.storage {
            ArrayStorage::Hash { entries, .. } => entries,
            _ => unreachable!(),
        }
    }

    /// Remove element by key
    pub fn remove(&mut self, key: &ArrayKey) -> bool {
        // Remove breaks packed invariant
        if matches!(&self.storage, ArrayStorage::Packed(_)) {
            self.transition_to_hash();
        }
        if let ArrayStorage::Hash { entries, int_index, str_index, .. } = &mut self.storage {
            let found_idx = match key {
                ArrayKey::Int(n) => int_index.get(n).copied(),
                ArrayKey::String(s) => str_index.get(s.as_str()).copied(),
            };
            if let Some(idx) = found_idx {
                entries.remove(idx);
                match key {
                    ArrayKey::Int(n) => { int_index.remove(n); }
                    ArrayKey::String(s) => { str_index.remove(s.as_str()); }
                }
                // Re-index entries after removed position
                Self::reindex_entries(entries, int_index, str_index, idx);
                return true;
            }
        }
        false
    }

    /// Remove and return last element.
    /// PHP semantics: if popped key was int and == next_int_key - 1, decrement.
    /// Otherwise next_int_key stays unchanged.
    pub fn pop(&mut self) -> Option<Value> {
        match &mut self.storage {
            ArrayStorage::Packed(values) => {
                let result = values.pop();
                if result.is_some() {
                    self.next_int_key = values.len() as i64;
                }
                result
            }
            ArrayStorage::Hash { entries, int_index, str_index, .. } => {
                if let Some((key, val)) = entries.pop() {
                    match &key {
                        ArrayKey::Int(n) => {
                            int_index.remove(n);
                            // PHP: only decrement if popped key was the auto-index boundary
                            if *n == self.next_int_key - 1 {
                                self.next_int_key -= 1;
                            }
                        }
                        ArrayKey::String(s) => {
                            str_index.remove(s.as_str());
                            // String key: next_int_key unchanged
                        }
                    }
                    Some(val)
                } else {
                    None
                }
            }
        }
    }

    /// Remove and return first element.
    /// PHP semantics: integer keys are renumbered from 0, string keys preserved.
    pub fn shift(&mut self) -> Option<Value> {
        // Transition to hash — shift requires key renumbering
        self.transition_to_hash();
        if let ArrayStorage::Hash { entries, int_index, str_index } = &mut self.storage {
            if entries.is_empty() { return None; }
            let (_key, val) = entries.remove(0);

            // Renumber: rebuild with int keys starting from 0, string keys preserved
            let mut new_int_counter: i64 = 0;
            int_index.clear();
            str_index.clear();
            for (i, (key, _)) in entries.iter_mut().enumerate() {
                match key {
                    ArrayKey::Int(n) => {
                        *n = new_int_counter;
                        int_index.insert(new_int_counter, i);
                        new_int_counter += 1;
                    }
                    ArrayKey::String(s) => {
                        str_index.insert(s.clone(), i);
                    }
                }
            }
            self.next_int_key = new_int_counter;
            Some(val)
        } else {
            None
        }
    }


    /// Rebuild index entries from position `from` onward (after remove/shift).
    fn reindex_entries(
        entries: &[(ArrayKey, Value)],
        int_index: &mut HashMap<i64, usize>,
        str_index: &mut HashMap<String, usize>,
        from: usize,
    ) {
        for (i, (k, _)) in entries.iter().enumerate() {
            if i >= from {
                match k {
                    ArrayKey::Int(n) => { int_index.insert(*n, i); }
                    ArrayKey::String(s) => { str_index.insert(s.clone(), i); }
                }
            }
        }
    }

    /// Check if array is in packed mode (sequential 0..N-1 int keys).
    #[inline]
    pub fn is_packed(&self) -> bool {
        matches!(&self.storage, ArrayStorage::Packed(_))
    }

    /// Get packed values slice — only valid when is_packed() is true.
    /// Used for fast iteration when caller knows array is packed.
    #[inline]
    pub fn packed_values(&self) -> Option<&[Value]> {
        match &self.storage {
            ArrayStorage::Packed(values) => Some(values),
            _ => None,
        }
    }
}

/// Iterator over PhpArray entries — works for both packed and hash modes.
/// Yields `(ArrayKey, &Value)` without allocating keys for packed arrays.
pub enum PhpArrayIter<'a> {
    Packed(std::iter::Enumerate<std::slice::Iter<'a, Value>>),
    Hash(std::slice::Iter<'a, (ArrayKey, Value)>),
}

impl<'a> Iterator for PhpArrayIter<'a> {
    type Item = (ArrayKey, &'a Value);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            PhpArrayIter::Packed(iter) => {
                iter.next().map(|(i, v)| (ArrayKey::Int(i as i64), v))
            }
            PhpArrayIter::Hash(iter) => {
                iter.next().map(|(k, v)| (k.clone(), v))
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            PhpArrayIter::Packed(iter) => iter.size_hint(),
            PhpArrayIter::Hash(iter) => iter.size_hint(),
        }
    }
}

impl<'a> ExactSizeIterator for PhpArrayIter<'a> {}

impl Clone for PhpArray {
    fn clone(&self) -> Self {
        let cloned_storage = match &self.storage {
            ArrayStorage::Packed(values) => ArrayStorage::Packed(values.clone()),
            ArrayStorage::Hash { entries, str_index, int_index } => ArrayStorage::Hash {
                entries: entries.clone(),
                str_index: str_index.clone(),
                int_index: int_index.clone(),
            },
        };
        Self {
            storage: cloned_storage,
            next_int_key: self.next_int_key,
        }
    }
}

impl std::fmt::Debug for PhpArray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.storage {
            ArrayStorage::Packed(values) => {
                f.debug_struct("PhpArray")
                    .field("mode", &"packed")
                    .field("len", &values.len())
                    .finish()
            }
            ArrayStorage::Hash { entries, .. } => {
                f.debug_struct("PhpArray")
                    .field("mode", &"hash")
                    .field("len", &entries.len())
                    .finish()
            }
        }
    }
}

/// PHP closure — function pointer + captured values.
/// Stored behind Box in Value, like String and Array.
pub struct PhpClosure {
    /// Direct pointer to the resolved function. No string lookup needed at call time.
    pub func: *const FunctionCommon,
    /// Captured `use` variable values, in declaration order.
    pub captures: Vec<Value>,
    /// True if any captured value needs_cleanup (String/Array/Object/Closure).
    /// When false, captures are all scalars — clone is a cheap memcpy.
    pub has_heap_captures: bool,
}

impl Clone for PhpClosure {
    fn clone(&self) -> Self {
        Self {
            func: self.func,
            captures: self.captures.clone(),
            has_heap_captures: self.has_heap_captures,
        }
    }
}

impl std::fmt::Debug for PhpClosure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhpClosure")
            .field("func", &self.func)
            .field("captures", &self.captures.len())
            .finish()
    }
}

/// PHP Value — tagged union, 16 bytes.
/// Layout matches zend_value + type_info.
#[repr(C)]
pub struct Value {
    data: ValueData,
    type_info: u32,
    _not_send: PhantomData<*mut ()>,
}

#[repr(C)]
union ValueData {
    long: i64,
    double: f64,
    ptr: *mut u8,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Undef = 0,
    Null = 1,
    False = 2,
    True = 3,
    Long = 4,
    Double = 5,
    String = 6,
    Array = 7,
    Object = 8,
    Resource = 9,
    Reference = 10,
    Closure = 11,
}

impl Value {
    #[inline]
    pub fn undef() -> Self {
        Self {
            data: ValueData { long: 0 },
            type_info: ValueType::Undef as u32,
            _not_send: PhantomData,
        }
    }

    #[inline]
    pub fn null() -> Self {
        Self {
            data: ValueData { long: 0 },
            type_info: ValueType::Null as u32,
            _not_send: PhantomData,
        }
    }

    #[inline]
    pub fn long(v: i64) -> Self {
        Self {
            data: ValueData { long: v },
            type_info: ValueType::Long as u32,
            _not_send: PhantomData,
        }
    }

    #[inline]
    pub fn double(v: f64) -> Self {
        Self {
            data: ValueData { double: v },
            type_info: ValueType::Double as u32,
            _not_send: PhantomData,
        }
    }

    // ── In-place scalar writers ─────────────────────────────────────────
    // Write a scalar value directly into a target slot, avoiding construction
    // of an intermediate Value on the Rust stack. Used by the slot lifecycle API
    // for hot-path arithmetic results (Add, Sub, IsSmaller, etc.).

    /// Write a Long value directly into a slot.
    #[inline(always)]
    pub unsafe fn write_long(ptr: *mut Value, v: i64) {
        ptr.write(Self {
            data: ValueData { long: v },
            type_info: ValueType::Long as u32,
            _not_send: PhantomData,
        });
    }

    /// Write a Double value directly into a slot.
    #[inline(always)]
    pub unsafe fn write_double(ptr: *mut Value, v: f64) {
        ptr.write(Self {
            data: ValueData { double: v },
            type_info: ValueType::Double as u32,
            _not_send: PhantomData,
        });
    }

    /// Write a Bool value directly into a slot.
    #[inline(always)]
    pub unsafe fn write_bool(ptr: *mut Value, v: bool) {
        ptr.write(Self {
            data: ValueData { long: 0 },
            type_info: if v { ValueType::True } else { ValueType::False } as u32,
            _not_send: PhantomData,
        });
    }

    /// Raw 16-byte copy from src to dst. No type-checking, no clone ceremony.
    /// Use for TMP→arg copies where the source is a scalar (Long/Double/Bool/Null)
    /// and will not be read again (consumed TMP).
    /// For heap-backed values (String/Array/Object), caller must handle refcount.
    #[inline(always)]
    pub unsafe fn raw_copy(src: *const Value, dst: *mut Value) {
        std::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, std::mem::size_of::<Value>());
    }

    /// Write Null directly into a slot.
    #[inline(always)]
    pub unsafe fn write_null(ptr: *mut Value) {
        ptr.write(Self {
            data: ValueData { long: 0 },
            type_info: ValueType::Null as u32,
            _not_send: PhantomData,
        });
    }

    /// Create a string value. Stores a reference-counted String (Rc).
    /// Clone = Rc refcount bump (no heap allocation). Drop = Rc decrement.
    /// Mutation (.=) uses COW: detach if shared, mutate in place if sole owner.
    #[inline]
    pub fn string(s: impl Into<String>) -> Self {
        let rc = Rc::new(s.into());
        Self {
            data: ValueData { ptr: Rc::into_raw(rc) as *mut u8 },
            type_info: ValueType::String as u32,
            _not_send: PhantomData,
        }
    }

    /// Create an array value from a PhpArray (reference-counted).
    /// Clone = Rc refcount bump (no deep copy). Drop = Rc decrement.
    /// Mutation uses COW: detach if shared, mutate in place if sole owner.
    #[inline]
    pub fn array(arr: PhpArray) -> Self {
        let rc = Rc::new(arr);
        Self {
            data: ValueData { ptr: Rc::into_raw(rc) as *mut u8 },
            type_info: ValueType::Array as u32,
            _not_send: PhantomData,
        }
    }

    /// Create an object value from a PhpObject (reference-counted).
    /// Stores Rc pointer directly — no Box wrapper. Clone = Rc increment, Drop = Rc decrement.
    #[inline]
    pub fn object(obj: PhpObject) -> Self {
        let rc = Rc::new(RefCell::new(obj));
        let ptr = Rc::into_raw(rc) as *mut u8;
        Self {
            data: ValueData { ptr },
            type_info: ValueType::Object as u32,
            _not_send: PhantomData,
        }
    }

    /// Create a closure value from a PhpClosure.
    #[inline]
    pub fn closure(c: PhpClosure) -> Self {
        let boxed = Box::new(c);
        Self {
            data: ValueData { ptr: Box::into_raw(boxed) as *mut u8 },
            type_info: ValueType::Closure as u32,
            _not_send: PhantomData,
        }
    }

    /// Get closure reference. Only valid for Closure values.
    #[inline]
    pub fn as_closure(&self) -> Option<&PhpClosure> {
        if self.value_type() == ValueType::Closure {
            Some(unsafe { &*(self.data.ptr as *const PhpClosure) })
        } else {
            None
        }
    }

    /// Get mutable closure reference. Only valid for Closure values.
    #[inline]
    pub fn as_closure_mut(&mut self) -> Option<&mut PhpClosure> {
        if self.value_type() == ValueType::Closure {
            Some(unsafe { &mut *(self.data.ptr as *mut PhpClosure) })
        } else {
            None
        }
    }

    /// Get the Rc<RefCell<PhpObject>> for shared access.
    /// Returns a temporary Rc handle without affecting the refcount.
    /// The caller must NOT drop the returned Rc (use for borrow/clone only).
    #[inline]
    pub fn as_object_rc(&self) -> Option<std::mem::ManuallyDrop<Rc<RefCell<PhpObject>>>> {
        if self.value_type() == ValueType::Object {
            Some(unsafe {
                std::mem::ManuallyDrop::new(Rc::from_raw(self.data.ptr as *const RefCell<PhpObject>))
            })
        } else {
            None
        }
    }

    /// Get object reference (borrows the RefCell). Only valid for Object values.
    #[inline]
    pub fn as_object(&self) -> Option<std::cell::Ref<'_, PhpObject>> {
        if self.value_type() == ValueType::Object {
            let refcell = unsafe { &*(self.data.ptr as *const RefCell<PhpObject>) };
            Some(refcell.borrow())
        } else {
            None
        }
    }

    /// Get mutable object reference (borrows the RefCell). Only valid for Object values.
    #[inline]
    pub fn as_object_mut(&self) -> Option<std::cell::RefMut<'_, PhpObject>> {
        if self.value_type() == ValueType::Object {
            let refcell = unsafe { &*(self.data.ptr as *const RefCell<PhpObject>) };
            Some(refcell.borrow_mut())
        } else {
            None
        }
    }

    /// Read class_id for Object values without RefCell borrow check.
    /// Single-threaded VM guarantees no concurrent mutations during dispatch.
    /// SAFETY: Only valid when value_type() == ValueType::Object.
    #[inline(always)]
    pub unsafe fn object_class_id_unchecked(&self) -> u32 {
        debug_assert!(self.value_type() == ValueType::Object);
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        (*refcell.as_ptr()).class_id
    }

    /// Read a property value pointer from an Object without RefCell borrow.
    /// Returns a pointer to the Value inside the HashMap, or null if not found.
    /// Single-threaded VM guarantees no concurrent mutations during dispatch.
    /// SAFETY: Only valid when value_type() == ValueType::Object. Returned pointer
    /// is valid only while no mutable borrow of the object's properties exists.
    #[inline(always)]
    pub unsafe fn object_property_unchecked(&self, name: &str) -> *const Value {
        debug_assert!(self.value_type() == ValueType::Object);
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        let obj = &*refcell.as_ptr();
        match obj.properties.get(name) {
            Some(v) => v as *const Value,
            None => std::ptr::null(),
        }
    }

    /// Write a scalar value to a property of an Object without RefCell borrow.
    /// Single-threaded VM guarantees no concurrent mutations during dispatch.
    /// SAFETY: Only valid when value_type() == ValueType::Object.
    #[inline(always)]
    pub unsafe fn object_set_property_unchecked(&self, name: &str, val: Value) {
        debug_assert!(self.value_type() == ValueType::Object);
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        let obj = &mut *refcell.as_ptr();
        // Fast path: if property already exists, overwrite in-place (no String alloc).
        if let Some(slot) = obj.properties.get_mut(name) {
            *slot = val;
        } else {
            obj.properties.insert(name.to_string(), val);
        }
    }

    /// Get string reference. Only valid for String values.
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        if self.value_type() == ValueType::String {
            Some(unsafe { &*(self.data.ptr as *const String) })
        } else {
            None
        }
    }

    /// Get mutable string reference with COW semantics.
    /// If sole owner (refcount == 1): returns mutable reference in place (no allocation).
    /// If shared (refcount > 1): detaches — clones the String into a new Rc, updates pointer.
    /// SAFETY: caller must ensure no outstanding borrows of this string exist.
    #[inline]
    pub unsafe fn as_string_mut(&mut self) -> Option<&mut String> {
        if self.value_type() != ValueType::String {
            return None;
        }
        let rc_ptr = self.data.ptr as *mut String;
        // Reconstruct Rc without consuming it (ManuallyDrop prevents decrement)
        let rc = std::mem::ManuallyDrop::new(Rc::from_raw(rc_ptr));
        if Rc::strong_count(&rc) == 1 {
            // Sole owner — mutate in place. No allocation, no Rc overhead.
            Some(&mut *rc_ptr)
        } else {
            // Shared — COW detach: clone String, create new sole-owner Rc
            let cloned = (*rc_ptr).clone();
            // Drop the ManuallyDrop wrapper and then the actual from_raw:
            // we need to decrement our old reference
            Rc::decrement_strong_count(rc_ptr as *const String);
            let new_rc = Rc::new(cloned);
            self.data.ptr = Rc::into_raw(new_rc) as *mut u8;
            Some(&mut *(self.data.ptr as *mut String))
        }
    }

    /// Get array reference. Only valid for Array values.
    #[inline]
    pub fn as_array(&self) -> Option<&PhpArray> {
        if self.value_type() == ValueType::Array {
            Some(unsafe { &*(self.data.ptr as *const PhpArray) })
        } else {
            None
        }
    }

    /// Get mutable array reference with COW semantics.
    /// If sole owner (refcount == 1): returns mutable reference in place (no copy).
    /// If shared (refcount > 1): detaches — clones the PhpArray into a new Rc, updates pointer.
    #[inline]
    pub fn as_array_mut(&mut self) -> Option<&mut PhpArray> {
        if self.value_type() != ValueType::Array {
            return None;
        }
        unsafe {
            let rc_ptr = self.data.ptr as *mut PhpArray;
            let rc = std::mem::ManuallyDrop::new(Rc::from_raw(rc_ptr));
            if Rc::strong_count(&rc) == 1 {
                // Sole owner — mutate in place. No copy.
                Some(&mut *rc_ptr)
            } else {
                // Shared — COW detach: deep clone PhpArray, create new sole-owner Rc
                let cloned = (*rc_ptr).clone();
                Rc::decrement_strong_count(rc_ptr as *const PhpArray);
                let new_rc = Rc::new(cloned);
                self.data.ptr = Rc::into_raw(new_rc) as *mut u8;
                Some(&mut *(self.data.ptr as *mut PhpArray))
            }
        }
    }

    #[inline]
    pub fn value_type(&self) -> ValueType {
        // Safety: type_info low byte is always a valid ValueType
        unsafe { std::mem::transmute((self.type_info & 0xFF) as u8) }
    }

    /// Human-readable PHP type name for error messages.
    pub fn type_name(&self) -> &'static str {
        match self.value_type() {
            ValueType::Undef => "unknown",
            ValueType::Null => "null",
            ValueType::False | ValueType::True => "bool",
            ValueType::Long => "int",
            ValueType::Double => "float",
            ValueType::String => "string",
            ValueType::Array => "array",
            ValueType::Object => "object",
            ValueType::Resource => "resource",
            ValueType::Reference => "reference",
            ValueType::Closure => "Closure",
        }
    }

    #[inline]
    pub fn as_long(&self) -> Option<i64> {
        if self.value_type() == ValueType::Long {
            Some(unsafe { self.data.long })
        } else {
            None
        }
    }

    /// Read the raw i64 without type check. SAFETY: caller must guarantee value is Long.
    #[inline(always)]
    pub unsafe fn raw_long(&self) -> i64 {
        self.data.long
    }

    #[inline]
    pub fn as_double(&self) -> Option<f64> {
        if self.value_type() == ValueType::Double {
            Some(unsafe { self.data.double })
        } else {
            None
        }
    }

    /// Convert to f64 for arithmetic (type juggling)
    pub fn to_double(&self) -> Option<f64> {
        match self.value_type() {
            ValueType::Long => Some(unsafe { self.data.long } as f64),
            ValueType::Double => Some(unsafe { self.data.double }),
            _ => None,
        }
    }

    /// Structural equality check for compile-time constant values.
    /// Used for trait property collision detection.
    /// Supports scalars, null, and arrays (recursive). Objects always return false.
    pub fn structurally_equal(&self, other: &Value) -> bool {
        if self.value_type() != other.value_type() {
            return false;
        }
        match self.value_type() {
            ValueType::Undef | ValueType::Null | ValueType::True | ValueType::False => true,
            ValueType::Long => unsafe { self.data.long == other.data.long },
            ValueType::Double => unsafe { self.data.double == other.data.double },
            ValueType::String => self.as_str() == other.as_str(),
            ValueType::Array => {
                let a = self.as_array().unwrap();
                let b = other.as_array().unwrap();
                if a.len() != b.len() {
                    return false;
                }
                a.iter().zip(b.iter()).all(|((ka, va), (kb, vb))| {
                    ka == kb && va.structurally_equal(vb)
                })
            }
            _ => false,
        }
    }

    /// Display value as PHP would echo it
    pub fn echo_to_string(&self) -> String {
        match self.value_type() {
            ValueType::Undef | ValueType::Null => String::new(),
            ValueType::False => String::new(),
            ValueType::True => "1".to_string(),
            ValueType::Long => unsafe { self.data.long }.to_string(),
            ValueType::Double => {
                let d = unsafe { self.data.double };
                if d == d.floor() && d.abs() < 1e15 {
                    format!("{}", d as i64)
                } else {
                    format!("{}", d)
                }
            }
            ValueType::String => {
                unsafe { &*(self.data.ptr as *const String) }.clone()
            }
            ValueType::Array => "Array".to_string(),
            ValueType::Object => {
                let refcell = unsafe { &*(self.data.ptr as *const RefCell<PhpObject>) };
                let obj = refcell.borrow();
                format!("{} Object", obj.class_name)
            }
            _ => "<unsupported>".to_string(),
        }
    }

    /// Check if value is undef
    #[inline]
    pub fn is_undef(&self) -> bool {
        self.value_type() == ValueType::Undef
    }

    /// PHP truthiness — matches PHP's casting rules for (bool).
    #[inline]
    pub fn is_truthy(&self) -> bool {
        match self.value_type() {
            ValueType::Undef | ValueType::Null | ValueType::False => false,
            ValueType::True => true,
            ValueType::Long => (unsafe { self.data.long }) != 0,
            ValueType::Double => (unsafe { self.data.double }) != 0.0,
            ValueType::String => {
                let s = unsafe { &*(self.data.ptr as *const String) };
                !s.is_empty() && s != "0"
            }
            ValueType::Array => {
                let arr = unsafe { &*(self.data.ptr as *const PhpArray) };
                !arr.is_empty()
            }
            _ => true, // objects, resources are truthy
        }
    }

    /// Convert to i64 following PHP type juggling rules
    pub fn to_long_val(&self) -> i64 {
        match self.value_type() {
            ValueType::Long => unsafe { self.data.long },
            ValueType::Double => (unsafe { self.data.double }) as i64,
            ValueType::True => 1,
            ValueType::False | ValueType::Null | ValueType::Undef => 0,
            ValueType::String => {
                let s = unsafe { &*(self.data.ptr as *const String) };
                let s = s.trim();
                if s.is_empty() { return 0; }
                let bytes = s.as_bytes();
                let mut end = 0;
                if bytes[0] == b'-' || bytes[0] == b'+' { end = 1; }
                while end < bytes.len() && bytes[end].is_ascii_digit() { end += 1; }
                if end == 0 || (end == 1 && (bytes[0] == b'-' || bytes[0] == b'+')) { return 0; }
                s[..end].parse().unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Convert to f64 following PHP type juggling rules (extended)
    pub fn to_float_val(&self) -> f64 {
        match self.value_type() {
            ValueType::Long => (unsafe { self.data.long }) as f64,
            ValueType::Double => unsafe { self.data.double },
            ValueType::True => 1.0,
            ValueType::False | ValueType::Null | ValueType::Undef => 0.0,
            ValueType::String => {
                let s = unsafe { &*(self.data.ptr as *const String) };
                s.trim().parse::<f64>().unwrap_or(0.0)
            }
            _ => 0.0,
        }
    }

    /// Create a boolean value
    #[inline]
    pub fn bool(v: bool) -> Self {
        Self {
            data: ValueData { long: 0 },
            type_info: if v { ValueType::True as u32 } else { ValueType::False as u32 },
            _not_send: PhantomData,
        }
    }

    /// Create a reference value — points to another Value slot (e.g. caller's CV).
    /// SAFETY: `ptr` must remain valid for the lifetime of this Value.
    #[inline]
    pub fn reference(ptr: *mut Value) -> Self {
        Self {
            data: ValueData { ptr: ptr as *mut u8 },
            type_info: ValueType::Reference as u32,
            _not_send: PhantomData,
        }
    }

    /// Check if this value is a reference.
    #[inline]
    pub fn is_reference(&self) -> bool {
        self.value_type() == ValueType::Reference
    }

    #[inline]
    pub fn needs_cleanup(&self) -> bool {
        matches!(self.value_type(), ValueType::String | ValueType::Array | ValueType::Object | ValueType::Closure)
    }

    /// Get the target pointer of a reference value.
    /// SAFETY: only valid when is_reference() is true.
    #[inline]
    pub unsafe fn as_ref_ptr(&self) -> *mut Value {
        self.data.ptr as *mut Value
    }
}

impl Clone for Value {
    #[inline(always)]
    fn clone(&self) -> Self {
        stats::inc_value_clone(self.value_type() as usize);
        match self.value_type() {
            ValueType::String => {
                // Clone = Rc refcount bump. No heap allocation.
                unsafe {
                    Rc::increment_strong_count(self.data.ptr as *const String);
                }
                Self {
                    data: ValueData { ptr: unsafe { self.data.ptr } },
                    type_info: self.type_info,
                    _not_send: PhantomData,
                }
            }
            ValueType::Array => {
                // Clone = Rc refcount bump. No deep copy.
                unsafe {
                    Rc::increment_strong_count(self.data.ptr as *const PhpArray);
                }
                Self {
                    data: ValueData { ptr: unsafe { self.data.ptr } },
                    type_info: self.type_info,
                    _not_send: PhantomData,
                }
            }
            ValueType::Object => {
                // Clone = Rc increment. No heap allocation.
                unsafe {
                    Rc::increment_strong_count(self.data.ptr as *const RefCell<PhpObject>);
                }
                Self {
                    data: ValueData { ptr: unsafe { self.data.ptr } },
                    type_info: self.type_info,
                    _not_send: PhantomData,
                }
            }
            ValueType::Closure => {
                let c = unsafe { &*(self.data.ptr as *const PhpClosure) };
                Value::closure(c.clone())
            }
            ValueType::Reference => {
                // Clone a reference: clone the TARGET value (dereference + deep clone)
                let target = unsafe { &*(self.data.ptr as *const Value) };
                target.clone()
            }
            _ => Self {
                data: ValueData {
                    long: unsafe { self.data.long },
                },
                type_info: self.type_info,
                _not_send: PhantomData,
            },
        }
    }
}

impl Drop for Value {
    #[inline(always)]
    fn drop(&mut self) {
        stats::inc_value_drop(self.value_type() as usize);
        match self.value_type() {
            ValueType::String => {
                // Drop = Rc decrement. Frees String when refcount reaches 0.
                unsafe { Rc::decrement_strong_count(self.data.ptr as *const String) };
            }
            ValueType::Array => {
                // Drop = Rc decrement. Frees PhpArray when refcount reaches 0.
                unsafe { Rc::decrement_strong_count(self.data.ptr as *const PhpArray) };
            }
            ValueType::Object => {
                // Drop = Rc decrement. Frees PhpObject when refcount reaches 0.
                unsafe { Rc::decrement_strong_count(self.data.ptr as *const RefCell<PhpObject>) };
            }
            ValueType::Closure => {
                unsafe { drop(Box::from_raw(self.data.ptr as *mut PhpClosure)) };
            }
            // Reference doesn't own the target — no-op
            _ => {}
        }
    }
}

/// Create an Error/TypeError/Exception object with a message property.
/// Used by the VM to throw PHP-compatible exceptions.
pub fn make_error_value(class_name: &str, message: &str) -> Value {
    let mut props = std::collections::HashMap::new();
    props.insert("message".to_string(), Value::string(message));
    Value::object(PhpObject {
        class_name: class_name.to_string(),
        class_id: 0, // error objects don't need cache-valid class_id
        properties: props,
        generator: None,
    })
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.value_type() {
            ValueType::Undef => write!(f, "Value(undef)"),
            ValueType::Null => write!(f, "Value(null)"),
            ValueType::False => write!(f, "Value(false)"),
            ValueType::True => write!(f, "Value(true)"),
            ValueType::Long => write!(f, "Value(long={})", unsafe { self.data.long }),
            ValueType::Double => write!(f, "Value(double={})", unsafe { self.data.double }),
            ValueType::String => write!(f, "Value(string={:?})", unsafe { &*(self.data.ptr as *const String) }),
            ValueType::Array => write!(f, "Value(array[{}])", unsafe { &*(self.data.ptr as *const PhpArray) }.len()),
            ValueType::Object => {
                let refcell = unsafe { &*(self.data.ptr as *const RefCell<PhpObject>) };
                let obj = refcell.borrow();
                write!(f, "Value(object({}))", obj.class_name)
            }
            ValueType::Reference => write!(f, "Value(ref={:p})", unsafe { self.data.ptr }),
            ValueType::Closure => write!(f, "Value(Closure)"),
            _ => write!(f, "Value({:?})", self.value_type()),
        }
    }
}
