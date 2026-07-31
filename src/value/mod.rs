use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::marker::PhantomData;
use std::rc::Rc;

use crate::vm::stats;
use crate::vm::generator::GeneratorRef;
use crate::vm::function::FunctionCommon;

/// Shared declared-property layout for all instances of a class.
///
/// Names are resolved only on cold/cache-miss paths. Hot property access stores
/// the numeric slot in the instruction inline cache and indexes `property_values`
/// directly.
#[derive(Debug, Default)]
pub struct ObjectLayout {
    keys: Vec<String>,
    slots: HashMap<String, usize>,
}

impl ObjectLayout {
    pub fn new(keys: Vec<String>) -> Self {
        let mut slots = HashMap::with_capacity(keys.len());
        for (slot, key) in keys.iter().enumerate() {
            slots.insert(key.clone(), slot);
        }
        Self { keys, slots }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    #[inline]
    pub fn slot(&self, key: &str) -> Option<usize> {
        self.slots.get(key).copied()
    }

    #[inline]
    pub fn key(&self, slot: usize) -> Option<&str> {
        self.keys.get(slot).map(String::as_str)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.keys.len()
    }
}

/// PHP object — class instance with properties.
#[derive(Debug, Clone)]
pub struct PhpObject {
    pub class_name: String,
    /// Stable numeric class ID — matches ClassDef.class_id. Used for inline cache keying.
    pub class_id: u32,
    /// Shared name → slot mapping owned by the class definition.
    pub property_layout: Rc<ObjectLayout>,
    /// Declared properties in compact numeric slots.
    pub property_values: Vec<Value>,
    /// Dynamic properties are uncommon and allocated lazily.
    pub dynamic_properties: Option<Box<HashMap<String, Value>>>,
    /// If this object is a Generator, holds the generator state
    pub generator: Option<GeneratorRef>,
}

impl PhpObject {
    pub fn with_layout(
        class_name: String,
        class_id: u32,
        property_layout: Rc<ObjectLayout>,
        property_values: Vec<Value>,
    ) -> Self {
        debug_assert_eq!(property_layout.len(), property_values.len());
        Self {
            class_name,
            class_id,
            property_layout,
            property_values,
            dynamic_properties: None,
            generator: None,
        }
    }

    pub fn dynamic(
        class_name: String,
        class_id: u32,
        properties: HashMap<String, Value>,
    ) -> Self {
        Self {
            class_name,
            class_id,
            property_layout: Rc::new(ObjectLayout::empty()),
            property_values: Vec::new(),
            dynamic_properties: if properties.is_empty() {
                None
            } else {
                Some(Box::new(properties))
            },
            generator: None,
        }
    }

    #[inline]
    pub fn property_slot(&self, key: &str) -> Option<usize> {
        self.property_layout.slot(key)
    }

    #[inline]
    pub fn get_property_slot(&self, slot: usize) -> Option<&Value> {
        self.property_values.get(slot)
    }

    #[inline]
    pub fn get_property_slot_mut(&mut self, slot: usize) -> Option<&mut Value> {
        self.property_values.get_mut(slot)
    }

    #[inline]
    pub fn get_property(&self, key: &str) -> Option<&Value> {
        if let Some(slot) = self.property_layout.slot(key) {
            self.property_values.get(slot)
        } else {
            self.dynamic_properties.as_ref()?.get(key)
        }
    }

    #[inline]
    pub fn get_property_mut(&mut self, key: &str) -> Option<&mut Value> {
        if let Some(slot) = self.property_layout.slot(key) {
            self.property_values.get_mut(slot)
        } else {
            self.dynamic_properties.as_mut()?.get_mut(key)
        }
    }

    #[inline]
    pub fn contains_property(&self, key: &str) -> bool {
        self.property_layout.slot(key).is_some()
            || self
                .dynamic_properties
                .as_ref()
                .is_some_and(|props| props.contains_key(key))
    }

    /// Set a declared slot or create/update a dynamic property.
    /// Returns the declared slot when one exists.
    #[inline]
    pub fn set_property(&mut self, key: &str, value: Value) -> Option<usize> {
        if let Some(slot) = self.property_layout.slot(key) {
            self.property_values[slot] = value;
            Some(slot)
        } else {
            self.dynamic_properties
                .get_or_insert_with(|| Box::new(HashMap::new()))
                .insert(key.to_string(), value);
            None
        }
    }

    pub fn for_each_property(&self, mut visitor: impl FnMut(&str, &Value)) {
        for (slot, value) in self.property_values.iter().enumerate() {
            if let Some(key) = self.property_layout.key(slot) {
                visitor(key, value);
            }
        }
        if let Some(dynamic) = &self.dynamic_properties {
            for (key, value) in dynamic.iter() {
                visitor(key, value);
            }
        }
    }
}

#[cfg(test)]
mod object_tests {
    use super::{ObjectLayout, PhpObject, Value};
    use std::rc::Rc;

    #[test]
    fn declared_properties_use_shared_slots() {
        let layout = Rc::new(ObjectLayout::new(vec!["count".to_string()]));
        let mut object =
            PhpObject::with_layout("Counter".to_string(), 7, layout.clone(), vec![Value::long(1)]);

        assert_eq!(object.set_property("count", Value::long(2)), Some(0));
        assert_eq!(object.get_property("count").and_then(Value::as_long), Some(2));
        assert!(object.dynamic_properties.is_none());
        assert!(Rc::ptr_eq(&object.property_layout, &layout));
    }

    #[test]
    fn dynamic_properties_are_allocated_lazily() {
        let mut object = PhpObject::with_layout(
            "Dynamic".to_string(),
            8,
            Rc::new(ObjectLayout::empty()),
            Vec::new(),
        );

        assert!(object.dynamic_properties.is_none());
        assert_eq!(object.set_property("extra", Value::long(9)), None);
        assert_eq!(object.get_property("extra").and_then(Value::as_long), Some(9));
        assert!(object.dynamic_properties.is_some());
    }
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

/// Fast deterministic hashing for integer-only PHP array keys.
///
/// `std::HashMap` otherwise uses the DOS-resistant general-purpose string
/// hasher for every integer lookup. SplitMix64's finalizer is a bijection over
/// `u64`, so distinct integer keys retain full-width entropy without paying
/// that general hashing cost. String keys keep the randomized default hasher.
#[derive(Default)]
struct IntKeyHasher {
    hash: u64,
}

impl IntKeyHasher {
    #[inline(always)]
    fn mix(mut value: u64) -> u64 {
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}

impl Hasher for IntKeyHasher {
    #[inline(always)]
    fn finish(&self) -> u64 {
        self.hash
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut value = 0xcbf2_9ce4_8422_2325u64;
        for byte in bytes {
            value ^= u64::from(*byte);
            value = value.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.hash = Self::mix(value);
    }

    #[inline(always)]
    fn write_i64(&mut self, value: i64) {
        self.hash = Self::mix(value as u64);
    }

    #[inline(always)]
    fn write_u64(&mut self, value: u64) {
        self.hash = Self::mix(value);
    }
}

type IntIndex = HashMap<i64, usize, BuildHasherDefault<IntKeyHasher>>;

#[inline]
fn int_index_with_capacity(capacity: usize) -> IntIndex {
    IntIndex::with_capacity_and_hasher(capacity, BuildHasherDefault::default())
}

/// Internal storage representation. Not exposed outside PhpArray.
enum ArrayStorage {
    /// Sequential 0..N-1 integer keys — values only, no key storage.
    Packed(Vec<Value>),
    /// General ordered map — explicit keys + split hash indexes.
    Hash {
        entries: Vec<(ArrayKey, Value)>,
        str_index: HashMap<String, usize>,
        int_index: IntIndex,
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
            let mut int_index = int_index_with_capacity(len);
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
                // Ordered PHP arrays commonly retain a contiguous integer run
                // after transitioning to hash storage. Derive the likely entry
                // position from its first key and validate it; irregular
                // layouts fall through to the general integer hash index.
                if let Some((ArrayKey::Int(first_key), _)) = entries.first() {
                    if let Some(position) = key
                        .checked_sub(*first_key)
                        .and_then(|offset| usize::try_from(offset).ok())
                    {
                        if let Some((ArrayKey::Int(found_key), value)) = entries.get(position) {
                            if *found_key == key {
                                return Some(value);
                            }
                        }
                    }
                } else if key >= 0 {
                    if let Some((ArrayKey::Int(found_key), value)) = entries.get(key as usize) {
                        if *found_key == key {
                            return Some(value);
                        }
                    }
                }
                int_index.get(&key).map(|&idx| &entries[idx].1)
            }
        }
    }

    /// Whether hash storage is likely to satisfy integer reads through the
    /// validated ordered-entry fast path.
    ///
    /// This is only a routing hint: `get_int()` still validates the exact key
    /// at the derived position and falls back to the hash index on mismatch.
    #[inline]
    pub fn prefers_positional_int_lookup(&self) -> bool {
        let ArrayStorage::Hash { entries, .. } = &self.storage else {
            return false;
        };
        match entries.as_slice() {
            [(ArrayKey::Int(_), _)] => true,
            [(ArrayKey::Int(first), _), (ArrayKey::Int(second), _), ..] => {
                first.checked_add(1) == Some(*second)
            }
            _ => false,
        }
    }

    /// Derive an ordered-entry position hint from the first integer keys.
    ///
    /// The hint never establishes correctness on its own. Guarded readers
    /// validate the key stored at the derived position and retain the integer
    /// index as a fallback for holes, interleaved string keys, and later
    /// irregular entries.
    #[cfg(any(feature = "quick-loops", test))]
    #[inline]
    pub(crate) fn integer_position_hint(&self) -> Option<(i64, i64)> {
        let ArrayStorage::Hash { entries, .. } = &self.storage else {
            return None;
        };
        match entries.as_slice() {
            [(ArrayKey::Int(first), _)] => Some((*first, 1)),
            [(ArrayKey::Int(first), _), (ArrayKey::Int(second), _), ..] => {
                let stride = second
                    .checked_sub(*first)
                    .filter(|stride| *stride != 0)?;
                for (position, (key, _)) in entries.iter().take(8).enumerate() {
                    let expected = stride
                        .checked_mul(position as i64)
                        .and_then(|offset| first.checked_add(offset))?;
                    if !matches!(key, ArrayKey::Int(found) if *found == expected) {
                        return None;
                    }
                }
                Some((*first, stride))
            }
            _ => None,
        }
    }

    /// Integer lookup through a preclassified ordered-entry progression.
    ///
    /// The stored key is checked before returning the value. A failed
    /// arithmetic candidate uses the canonical integer index, so this remains
    /// exact even when only the prefix follows the hinted progression.
    #[cfg(any(feature = "quick-loops", test))]
    #[inline(always)]
    pub(crate) fn get_positioned_int(
        &self,
        key: i64,
        first_key: i64,
        stride: i64,
    ) -> Option<&Value> {
        let ArrayStorage::Hash { entries, int_index, .. } = &self.storage else {
            return None;
        };
        let position = key.checked_sub(first_key).and_then(|offset| {
            if stride == 1 {
                usize::try_from(offset).ok()
            } else if stride != 0 && offset.checked_rem(stride) == Some(0) {
                offset.checked_div(stride).and_then(|value| usize::try_from(value).ok())
            } else {
                None
            }
        });
        if let Some(position) = position {
            if let Some((ArrayKey::Int(found_key), value)) = entries.get(position) {
                if *found_key == key {
                    return Some(value);
                }
            }
        }
        int_index.get(&key).map(|&idx| &entries[idx].1)
    }

    /// Integer lookup that deliberately skips the ordered-entry fast path.
    /// Guarded quick regions use this for arrays classified as irregular once
    /// at activation instead of repeating a known-to-fail positional probe.
    #[inline]
    pub fn get_indexed_int(&self, key: i64) -> Option<&Value> {
        match &self.storage {
            ArrayStorage::Hash { entries, int_index, .. } => {
                int_index.get(&key).map(|&idx| &entries[idx].1)
            }
            ArrayStorage::Packed(_) => None,
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
        int_index: &mut IntIndex,
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

#[cfg(test)]
mod php_array_tests {
    use super::{ArrayKey, PhpArray, Value};

    #[test]
    fn integer_lookup_routing_distinguishes_contiguous_and_irregular_hashes() {
        let mut contiguous = PhpArray::new();
        contiguous.set_int(100, Value::long(1));
        contiguous.set_int(101, Value::long(2));
        contiguous.set_str("sentinel", Value::long(3));
        assert!(contiguous.prefers_positional_int_lookup());

        let mut irregular = PhpArray::new();
        irregular.set_int(100, Value::long(4));
        irregular.set_int(107, Value::long(5));
        irregular.set_int(-3, Value::long(6));
        assert!(!irregular.prefers_positional_int_lookup());
        assert_eq!(irregular.integer_position_hint(), None);
        assert_eq!(
            irregular.get_indexed_int(107).and_then(Value::as_long),
            Some(5)
        );
        assert_eq!(
            irregular
                .get_positioned_int(107, 100, 7)
                .and_then(Value::as_long),
            Some(5)
        );
        assert_eq!(
            irregular
                .get_positioned_int(-3, 100, 7)
                .and_then(Value::as_long),
            Some(6)
        );
        assert!(irregular.get_positioned_int(101, 100, 7).is_none());
    }

    #[test]
    fn integer_index_handles_offset_and_irregular_hash_keys() {
        let mut array = PhpArray::new();
        for key in 1_000_000..1_000_100 {
            array.set_int(key, Value::long(key * 2));
        }
        array.set_str("separator", Value::long(7));
        array.set_int(-11, Value::long(22));
        array.set_int(9_000_007, Value::long(33));

        assert_eq!(array.get_int(1_000_000).and_then(Value::as_long), Some(2_000_000));
        assert_eq!(array.get_int(1_000_099).and_then(Value::as_long), Some(2_000_198));
        assert_eq!(array.get_int(-11).and_then(Value::as_long), Some(22));
        assert_eq!(array.get_int(9_000_007).and_then(Value::as_long), Some(33));
        assert!(array.get_int(1_000_101).is_none());
    }

    #[test]
    fn integer_position_hint_accepts_negative_stride_and_rejects_irregular_prefix() {
        let mut descending = PhpArray::new();
        for key in [100, 93, 86, 79, 72, 65, 58, 51] {
            descending.set_int(key, Value::long(key));
        }
        assert_eq!(descending.integer_position_hint(), Some((100, -7)));
        assert_eq!(
            descending
                .get_positioned_int(58, 100, -7)
                .and_then(Value::as_long),
            Some(58)
        );

        let mut irregular = PhpArray::new();
        for key in [10, 30, 31, 70, -4, 900, 2, 88] {
            irregular.set_int(key, Value::long(key));
        }
        assert_eq!(irregular.integer_position_hint(), None);
    }

    #[test]
    fn integer_index_remains_valid_after_remove_and_clone() {
        let mut array = PhpArray::new();
        for key in [17, 3, 9001, -4, 42] {
            array.set_int(key, Value::long(key));
        }
        assert!(array.remove(&ArrayKey::Int(9001)));

        let cloned = array.clone();
        for key in [17, 3, -4, 42] {
            assert_eq!(cloned.get_int(key).and_then(Value::as_long), Some(key));
        }
        assert!(cloned.get_int(9001).is_none());
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
        match obj.get_property(name) {
            Some(v) => v as *const Value,
            None => std::ptr::null(),
        }
    }

    /// Read a declared property by cached numeric slot.
    /// SAFETY: caller must validate object class_id against the cache entry.
    #[inline(always)]
    pub unsafe fn object_property_slot_unchecked(&self, slot: usize) -> *const Value {
        debug_assert!(self.value_type() == ValueType::Object);
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        let obj = &*refcell.as_ptr();
        debug_assert!(slot < obj.property_values.len());
        obj.property_values.as_ptr().add(slot)
    }

    /// Write a scalar value to a property of an Object without RefCell borrow.
    /// Single-threaded VM guarantees no concurrent mutations during dispatch.
    /// SAFETY: Only valid when value_type() == ValueType::Object.
    #[inline(always)]
    pub unsafe fn object_set_property_unchecked(&self, name: &str, val: Value) {
        debug_assert!(self.value_type() == ValueType::Object);
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        let obj = &mut *refcell.as_ptr();
        obj.set_property(name, val);
    }

    /// Write a declared property by cached numeric slot.
    /// SAFETY: caller must validate object class_id against the cache entry.
    #[inline(always)]
    pub unsafe fn object_set_property_slot_unchecked(&self, slot: usize, val: Value) {
        debug_assert!(self.value_type() == ValueType::Object);
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        let obj = &mut *refcell.as_ptr();
        debug_assert!(slot < obj.property_values.len());
        *obj.property_values.get_unchecked_mut(slot) = val;
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
    Value::object(PhpObject::dynamic(
        class_name.to_string(),
        0, // error objects don't need cache-valid class_id
        props,
    ))
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
