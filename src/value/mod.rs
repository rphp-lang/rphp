use std::borrow::Borrow;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::hash::{BuildHasherDefault, Hasher};
use std::marker::PhantomData;
use std::ops::Deref;
use std::rc::Rc;

use crate::vm::stats;
use crate::vm::generator::GeneratorRef;
use crate::vm::function::FunctionCommon;

/// PHP converts only canonical decimal strings that fit in `i64` to integer
/// array keys. Syntax-first validation avoids an allocating parse/format
/// round-trip for ordinary string keys.
#[inline]
pub(crate) fn canonical_decimal_array_key(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    let digits = match bytes {
        [b'0'] => return Some(0),
        [b'1'..=b'9', rest @ ..] => rest,
        [b'-', b'1'..=b'9', rest @ ..] => rest,
        _ => return None,
    };
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    value.parse().ok()
}

/// Shared declared-property layout for all instances of a class.
///
/// Names are resolved only on cold/cache-miss paths. Hot property access stores
/// the numeric slot in the instruction inline cache and indexes `property_values`
/// directly.
#[derive(Debug)]
pub struct ObjectLayout {
    /// Canonical class name shared by the class definition and every instance.
    /// A declared object must not allocate and free an identical class-name
    /// String on every construction.
    class_name: Option<Rc<str>>,
    keys: Vec<String>,
    slots: HashMap<String, usize>,
}

impl ObjectLayout {
    pub fn new(class_name: impl Into<Rc<str>>, keys: Vec<String>) -> Self {
        let mut slots = HashMap::with_capacity(keys.len());
        for (slot, key) in keys.iter().enumerate() {
            slots.insert(key.clone(), slot);
        }
        Self {
            class_name: Some(class_name.into()),
            keys,
            slots,
        }
    }

    pub fn empty() -> Self {
        Self {
            class_name: None,
            keys: Vec::new(),
            slots: HashMap::new(),
        }
    }

    #[inline]
    pub fn class_name(&self) -> Rc<str> {
        self.class_name
            .clone()
            .expect("declared object layout must carry its class name")
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

const SMALL_DYNAMIC_PROPERTY_CAPACITY: usize = 3;
const LINEAR_DYNAMIC_PROPERTY_CAPACITY: usize = 8;

#[derive(Clone)]
struct SmallDynamicProperties {
    entries: [Option<(String, Value)>; SMALL_DYNAMIC_PROPERTY_CAPACITY],
}

impl SmallDynamicProperties {
    fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
        }
    }

    #[inline]
    fn len(&self) -> usize {
        self.entries.iter().take_while(|entry| entry.is_some()).count()
    }

    #[inline]
    fn find(&self, key: &str) -> Option<usize> {
        self.entries[..self.len()]
            .iter()
            .position(|entry| entry.as_ref().is_some_and(|entry| entry.0 == key))
    }

    #[inline]
    fn push(&mut self, key: String, value: Value) {
        let len = self.len();
        debug_assert!(len < SMALL_DYNAMIC_PROPERTY_CAPACITY);
        self.entries[len] = Some((key, value));
    }
}

#[derive(Clone)]
struct LinearDynamicProperties {
    entries: Vec<(String, Value)>,
}

impl LinearDynamicProperties {
    #[inline]
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity.min(LINEAR_DYNAMIC_PROPERTY_CAPACITY)),
        }
    }

    #[inline]
    fn from_small(small: SmallDynamicProperties) -> Self {
        let mut entries = Vec::with_capacity(LINEAR_DYNAMIC_PROPERTY_CAPACITY);
        entries.extend(small.entries.into_iter().flatten());
        Self { entries }
    }

    #[inline(always)]
    fn find(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|entry| entry.0 == key)
    }

    #[inline(always)]
    fn get_pair_at_positions(
        &self,
        keys: [&str; 2],
        positions: [Option<usize>; 2],
    ) -> [*const Value; 2] {
        let mut result = [std::ptr::null(); 2];
        for index in 0..2 {
            if let Some(position) = positions[index] {
                if let Some((stored_key, value)) = self.entries.get(position) {
                    if stored_key == keys[index] {
                        result[index] = value as *const Value;
                    }
                }
            }
        }
        if result[0].is_null() || result[1].is_null() {
            self.fill_pair_by_name(keys, result)
        } else {
            result
        }
    }

    /// Different insertion orders are correct but outside the monomorphic hot
    /// path. Keeping their bounded scans out of the caller protects the small-
    /// property kernel's instruction layout.
    #[inline(never)]
    fn fill_pair_by_name(
        &self,
        keys: [&str; 2],
        mut result: [*const Value; 2],
    ) -> [*const Value; 2] {
        for index in 0..2 {
            if result[index].is_null() {
                result[index] = self
                    .find(keys[index])
                    .map_or(std::ptr::null(), |position| {
                        &self.entries[position].1 as *const Value
                    });
            }
        }
        result
    }
}

#[derive(Clone)]
struct IndexedDynamicProperties {
    entries: Vec<(SharedStringKey, Value)>,
    index: HashMap<SharedStringKey, usize>,
}

impl IndexedDynamicProperties {
    #[inline]
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            index: HashMap::with_capacity(capacity),
        }
    }

    fn from_hash_map(properties: HashMap<String, Value>) -> Self {
        let mut result = Self::with_capacity(properties.len());
        for (position, (key, value)) in properties.into_iter().enumerate() {
            let key = SharedStringKey::from_owned(key);
            result.entries.push((key.clone(), value));
            result.index.insert(key, position);
        }
        result
    }

    fn from_linear_with_entry(
        linear: LinearDynamicProperties,
        new_key: String,
        new_value: Value,
    ) -> Self {
        let mut result = Self::with_capacity(linear.entries.len() + 1);
        for (position, (key, value)) in linear.entries.into_iter().enumerate() {
            let key = SharedStringKey::from_owned(key);
            result.entries.push((key.clone(), value));
            result.index.insert(key, position);
        }
        result.insert_owned(new_key, new_value);
        result
    }

    #[inline]
    fn find(&self, key: &str) -> Option<usize> {
        self.index.get(key).copied()
    }

    #[inline(always)]
    fn get_pair_at_positions(
        &self,
        keys: [&str; 2],
        positions: [Option<usize>; 2],
    ) -> [*const Value; 2] {
        let mut result = [std::ptr::null(); 2];
        for index in 0..2 {
            if let Some(position) = positions[index] {
                if let Some((stored_key, value)) = self.entries.get(position) {
                    if stored_key.as_ref() == keys[index] {
                        result[index] = value as *const Value;
                    }
                }
            }
        }
        if result[0].is_null() || result[1].is_null() {
            self.fill_pair_by_name(keys, result)
        } else {
            result
        }
    }

    /// Keep secure name hashing on cache misses outside the positional foreach
    /// kernel. Mixed insertion orders remain correct without enlarging its hot
    /// instruction path.
    #[inline(never)]
    fn fill_pair_by_name(
        &self,
        keys: [&str; 2],
        mut result: [*const Value; 2],
    ) -> [*const Value; 2] {
        for index in 0..2 {
            if result[index].is_null() {
                result[index] = self
                    .find(keys[index])
                    .and_then(|position| self.entries.get(position))
                    .map_or(std::ptr::null(), |entry| &entry.1 as *const Value);
            }
        }
        result
    }

    #[inline]
    fn insert_owned(&mut self, key: String, value: Value) {
        if let Some(position) = self.find(&key) {
            self.entries[position].1 = value;
            return;
        }
        let key = SharedStringKey::from_owned(key);
        let position = self.entries.len();
        self.entries.push((key.clone(), value));
        self.index.insert(key, position);
    }
}

#[derive(Clone)]
enum DynamicPropertyStorage {
    Small(SmallDynamicProperties),
    Linear(LinearDynamicProperties),
    Indexed(IndexedDynamicProperties),
}

/// Dynamic-object properties with one owning string allocation per key. Up to
/// three properties stay inline, four to eight use bounded linear storage, and
/// wider objects use ordered slots plus a secure name-to-position index. Every
/// tier preserves insertion order and exposes guarded positions to inline
/// caches; indexed entries and their index share one string allocation.
#[derive(Clone)]
pub struct DynamicPropertyMap {
    storage: DynamicPropertyStorage,
}

impl DynamicPropertyMap {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        let storage = if capacity <= SMALL_DYNAMIC_PROPERTY_CAPACITY {
            DynamicPropertyStorage::Small(SmallDynamicProperties::new())
        } else if capacity <= LINEAR_DYNAMIC_PROPERTY_CAPACITY {
            DynamicPropertyStorage::Linear(LinearDynamicProperties::with_capacity(capacity))
        } else {
            DynamicPropertyStorage::Indexed(IndexedDynamicProperties::with_capacity(capacity))
        };
        Self { storage }
    }

    fn from_hash_map(properties: HashMap<String, Value>) -> Self {
        if properties.len() > LINEAR_DYNAMIC_PROPERTY_CAPACITY {
            return Self {
                storage: DynamicPropertyStorage::Indexed(
                    IndexedDynamicProperties::from_hash_map(properties),
                ),
            };
        }
        let mut result = Self::with_capacity(properties.len());
        for (key, value) in properties {
            result.insert_owned(key, value);
        }
        result
    }

    #[inline]
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        match &self.storage {
            DynamicPropertyStorage::Small(small) => small
                .find(key)
                .and_then(|position| small.entries[position].as_ref().map(|entry| &entry.1)),
            DynamicPropertyStorage::Linear(linear) => linear
                .find(key)
                .map(|position| &linear.entries[position].1),
            DynamicPropertyStorage::Indexed(indexed) => indexed
                .find(key)
                .and_then(|position| indexed.entries.get(position))
                .map(|entry| &entry.1),
        }
    }

    #[inline]
    pub(crate) fn get_with_position(&self, key: &str) -> Option<(&Value, Option<usize>)> {
        match &self.storage {
            DynamicPropertyStorage::Small(small) => {
                let position = small.find(key)?;
                let value = &small.entries[position].as_ref().unwrap().1;
                Some((value, Some(position)))
            }
            DynamicPropertyStorage::Linear(linear) => {
                let position = linear.find(key)?;
                Some((&linear.entries[position].1, Some(position)))
            }
            DynamicPropertyStorage::Indexed(indexed) => {
                let position = indexed.find(key)?;
                Some((&indexed.entries.get(position)?.1, Some(position)))
            }
        }
    }

    #[inline(always)]
    pub(crate) fn get_at_position(&self, position: usize, key: &str) -> Option<&Value> {
        match &self.storage {
            DynamicPropertyStorage::Small(small) => {
                let (stored_key, value) = small.entries.get(position)?.as_ref()?;
                (stored_key == key).then_some(value)
            }
            DynamicPropertyStorage::Linear(linear) => {
                let (stored_key, value) = linear.entries.get(position)?;
                (stored_key == key).then_some(value)
            }
            DynamicPropertyStorage::Indexed(indexed) => {
                let (stored_key, value) = indexed.entries.get(position)?;
                (stored_key.as_ref() == key).then_some(value)
            }
        }
    }

    /// Resolve two independent property reads while branching on the backing
    /// storage only once. Cached small-map positions are guards, not
    /// assumptions: a receiver with a different insertion order falls back to
    /// the property name independently for each result.
    #[inline(always)]
    pub(crate) fn get_pair_at_positions(
        &self,
        keys: [&str; 2],
        positions: [Option<usize>; 2],
    ) -> [*const Value; 2] {
        let mut result = [std::ptr::null(); 2];
        match &self.storage {
            DynamicPropertyStorage::Small(small) => {
                for index in 0..2 {
                    let key = keys[index];
                    let cached = positions[index].and_then(|position| {
                        let (stored_key, value) = small.entries.get(position)?.as_ref()?;
                        (stored_key == key).then_some(value)
                    });
                    let value = cached.or_else(|| {
                        small.find(key).and_then(|position| {
                            small.entries[position].as_ref().map(|entry| &entry.1)
                        })
                    });
                    result[index] =
                        value.map_or(std::ptr::null(), |value| value as *const Value);
                }
            }
            DynamicPropertyStorage::Linear(linear) => {
                return linear.get_pair_at_positions(keys, positions);
            }
            DynamicPropertyStorage::Indexed(indexed) => {
                return indexed.get_pair_at_positions(keys, positions);
            }
        }
        result
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match &mut self.storage {
            DynamicPropertyStorage::Small(small) => {
                let position = small.find(key)?;
                small.entries[position].as_mut().map(|entry| &mut entry.1)
            }
            DynamicPropertyStorage::Linear(linear) => {
                let position = linear.find(key)?;
                Some(&mut linear.entries[position].1)
            }
            DynamicPropertyStorage::Indexed(indexed) => {
                let position = indexed.find(key)?;
                indexed.entries.get_mut(position).map(|entry| &mut entry.1)
            }
        }
    }

    pub(crate) fn insert_owned(&mut self, key: String, value: Value) {
        if let DynamicPropertyStorage::Indexed(indexed) = &mut self.storage {
            indexed.insert_owned(key, value);
            return;
        }
        if let DynamicPropertyStorage::Linear(linear) = &mut self.storage {
            if let Some(position) = linear.find(&key) {
                linear.entries[position].1 = value;
                return;
            }
            if linear.entries.len() < LINEAR_DYNAMIC_PROPERTY_CAPACITY {
                linear.entries.push((key, value));
                return;
            }

            let DynamicPropertyStorage::Linear(linear) = std::mem::replace(
                &mut self.storage,
                DynamicPropertyStorage::Small(SmallDynamicProperties::new()),
            ) else {
                unreachable!();
            };
            self.storage = DynamicPropertyStorage::Indexed(
                IndexedDynamicProperties::from_linear_with_entry(linear, key, value),
            );
            return;
        }
        if let DynamicPropertyStorage::Small(small) = &mut self.storage {
            if let Some(position) = small.find(&key) {
                small.entries[position].as_mut().unwrap().1 = value;
                return;
            }
            if small.len() < SMALL_DYNAMIC_PROPERTY_CAPACITY {
                small.push(key, value);
                return;
            }
        }

        let DynamicPropertyStorage::Small(small) = std::mem::replace(
            &mut self.storage,
            DynamicPropertyStorage::Small(SmallDynamicProperties::new()),
        ) else {
            unreachable!();
        };
        let mut linear = LinearDynamicProperties::from_small(small);
        linear.entries.push((key, value));
        self.storage = DynamicPropertyStorage::Linear(linear);
    }

    #[inline]
    pub(crate) fn insert(&mut self, key: &str, value: Value) {
        if let Some(existing) = self.get_mut(key) {
            *existing = value;
        } else {
            self.insert_owned(key.to_string(), value);
        }
    }

    #[inline]
    pub(crate) fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        match &self.storage {
            DynamicPropertyStorage::Small(small) => small.len(),
            DynamicPropertyStorage::Linear(linear) => linear.entries.len(),
            DynamicPropertyStorage::Indexed(indexed) => indexed.entries.len(),
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn for_each(&self, mut visitor: impl FnMut(&str, &Value)) {
        match &self.storage {
            DynamicPropertyStorage::Small(small) => {
                for (key, value) in small.entries[..small.len()].iter().flatten() {
                    visitor(key, value);
                }
            }
            DynamicPropertyStorage::Linear(linear) => {
                for (key, value) in &linear.entries {
                    visitor(key, value);
                }
            }
            DynamicPropertyStorage::Indexed(indexed) => {
                for (key, value) in &indexed.entries {
                    visitor(key.as_ref(), value);
                }
            }
        }
    }
}

impl std::fmt::Debug for DynamicPropertyMap {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DynamicPropertyMap")
            .field("len", &self.len())
            .finish()
    }
}

/// PHP object — class instance with properties.
#[derive(Debug, Clone)]
pub struct PhpObject {
    /// Shared with the class layout for declared objects. Dynamic/internal
    /// objects still own one interned name for their lifetime.
    pub class_name: Rc<str>,
    /// Stable numeric class ID — matches ClassDef.class_id. Used for inline cache keying.
    pub class_id: u32,
    /// Shared name → slot mapping owned by the class definition.
    pub property_layout: Rc<ObjectLayout>,
    /// Declared properties in compact numeric slots.
    pub property_values: Vec<Value>,
    /// Dynamic properties are uncommon and allocated lazily.
    pub dynamic_properties: Option<Box<DynamicPropertyMap>>,
    /// If this object is a Generator, holds the generator state
    pub generator: Option<GeneratorRef>,
}

thread_local! {
    /// Every decoded JSON object is the same dynamic `stdClass`. Sharing its
    /// immutable name and empty declared-property layout removes two heap
    /// allocations per object while keeping dynamic properties per instance.
    static STD_CLASS_METADATA: (Rc<str>, Rc<ObjectLayout>) = (
        Rc::from("stdClass"),
        Rc::new(ObjectLayout::empty()),
    );
}

impl PhpObject {
    pub fn with_layout(
        class_id: u32,
        property_layout: Rc<ObjectLayout>,
        property_values: Vec<Value>,
    ) -> Self {
        debug_assert_eq!(property_layout.len(), property_values.len());
        let class_name = property_layout.class_name();
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
            class_name: Rc::from(class_name),
            class_id,
            property_layout: Rc::new(ObjectLayout::empty()),
            property_values: Vec::new(),
            dynamic_properties: if properties.is_empty() {
                None
            } else {
                Some(Box::new(DynamicPropertyMap::from_hash_map(properties)))
            },
            generator: None,
        }
    }

    /// Construct the canonical dynamic `stdClass` used by `json_decode`.
    /// Class metadata is immutable and shared; the property map remains owned
    /// exclusively by this object.
    pub fn std_class(properties: HashMap<String, Value>) -> Self {
        Self::std_class_from_properties(DynamicPropertyMap::from_hash_map(properties))
    }

    /// Construct canonical `stdClass` from ordered streaming properties.
    pub(crate) fn std_class_from_properties(properties: DynamicPropertyMap) -> Self {
        let (class_name, property_layout) = STD_CLASS_METADATA.with(|metadata| {
            (Rc::clone(&metadata.0), Rc::clone(&metadata.1))
        });
        Self {
            class_name,
            class_id: 0,
            property_layout,
            property_values: Vec::new(),
            dynamic_properties: if properties.is_empty() {
                None
            } else {
                Some(Box::new(properties))
            },
            generator: None,
        }
    }

    /// Exact guard for the runtime's canonical dynamic `stdClass` shape.
    /// User-declared objects receive a non-zero class ID; other internal
    /// class-id-zero objects must retain full visibility and magic resolution.
    #[inline(always)]
    pub(crate) fn is_dynamic_std_class(&self) -> bool {
        self.class_id == 0
            && self.class_name.as_ref() == "stdClass"
            && self.property_layout.len() == 0
            && self.property_values.is_empty()
    }

    #[inline(always)]
    pub(crate) fn property_layout_ptr(&self) -> *const ObjectLayout {
        Rc::as_ptr(&self.property_layout)
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
    pub(crate) fn get_dynamic_property_with_position(
        &self,
        key: &str,
    ) -> Option<(&Value, Option<usize>)> {
        self.dynamic_properties.as_ref()?.get_with_position(key)
    }

    #[inline]
    pub fn contains_property(&self, key: &str) -> bool {
        self.property_layout.slot(key).is_some()
            || self
                .dynamic_properties
                .as_ref()
                .is_some_and(|properties| properties.contains_key(key))
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
                .get_or_insert_with(|| Box::new(DynamicPropertyMap::with_capacity(1)))
                .insert(key, value);
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
            dynamic.for_each(visitor);
        }
    }
}

#[cfg(test)]
mod object_tests {
    use super::{
        DynamicPropertyMap, DynamicPropertyStorage, ObjectLayout, PhpObject, Value,
    };
    use std::rc::Rc;

    #[test]
    fn declared_properties_use_shared_slots() {
        let layout = Rc::new(ObjectLayout::new("Counter", vec!["count".to_string()]));
        let mut object =
            PhpObject::with_layout(7, layout.clone(), vec![Value::long(1)]);

        assert_eq!(object.set_property("count", Value::long(2)), Some(0));
        assert_eq!(object.get_property("count").and_then(Value::as_long), Some(2));
        assert!(object.dynamic_properties.is_none());
        assert!(Rc::ptr_eq(&object.property_layout, &layout));
    }

    #[test]
    fn dynamic_properties_are_allocated_lazily() {
        let mut object = PhpObject::with_layout(
            8,
            Rc::new(ObjectLayout::new("Dynamic", Vec::new())),
            Vec::new(),
        );

        assert!(object.dynamic_properties.is_none());
        assert_eq!(object.set_property("extra", Value::long(9)), None);
        assert_eq!(object.get_property("extra").and_then(Value::as_long), Some(9));
        assert!(object.dynamic_properties.is_some());
    }

    #[test]
    fn decoded_std_classes_share_immutable_metadata() {
        let first = PhpObject::std_class(std::collections::HashMap::new());
        let second = PhpObject::std_class(std::collections::HashMap::new());

        assert_eq!(first.class_name.as_ref(), "stdClass");
        assert!(Rc::ptr_eq(&first.class_name, &second.class_name));
        assert!(Rc::ptr_eq(
            &first.property_layout,
            &second.property_layout
        ));
        assert!(first.dynamic_properties.is_none());
        assert!(second.dynamic_properties.is_none());
    }

    #[test]
    fn dynamic_property_map_promotes_small_to_linear_then_indexed() {
        assert!(std::mem::size_of::<DynamicPropertyMap>() <= 176);
        assert_eq!(
            std::mem::size_of::<Option<Box<DynamicPropertyMap>>>(),
            std::mem::size_of::<usize>()
        );

        let mut properties = DynamicPropertyMap::with_capacity(0);
        for (position, key) in ["a", "b", "c"]
            .into_iter()
            .enumerate()
        {
            properties.insert_owned(key.to_string(), Value::long(position as i64 + 1));
        }
        properties.insert_owned("b".to_string(), Value::long(20));
        assert_eq!(properties.len(), 3);
        assert!(matches!(properties.storage, DynamicPropertyStorage::Small(_)));

        let mut keys = Vec::new();
        properties.for_each(|key, _| keys.push(key.to_string()));
        assert_eq!(keys, ["a", "b", "c"]);
        assert_eq!(properties.get("b").and_then(Value::as_long), Some(20));

        let cloned = properties.clone();
        assert!(matches!(cloned.storage, DynamicPropertyStorage::Small(_)));

        properties.insert_owned("d".to_string(), Value::long(4));
        assert!(matches!(properties.storage, DynamicPropertyStorage::Linear(_)));
        for (position, key) in ["e", "f", "g", "h"].into_iter().enumerate() {
            properties.insert_owned(key.to_string(), Value::long(position as i64 + 5));
        }
        assert_eq!(properties.len(), 8);
        assert!(matches!(properties.storage, DynamicPropertyStorage::Linear(_)));
        assert_eq!(
            properties
                .get_with_position("h")
                .map(|(value, position)| (value.as_long(), position)),
            Some((Some(8), Some(7)))
        );

        let mut keys = Vec::new();
        properties.for_each(|key, _| keys.push(key.to_string()));
        assert_eq!(keys, ["a", "b", "c", "d", "e", "f", "g", "h"]);

        let cloned = properties.clone();
        assert!(matches!(cloned.storage, DynamicPropertyStorage::Linear(_)));
        assert_eq!(cloned.get("h").and_then(Value::as_long), Some(8));

        properties.insert_owned("i".to_string(), Value::long(9));
        assert!(matches!(properties.storage, DynamicPropertyStorage::Indexed(_)));
        assert_eq!(properties.get("b").and_then(Value::as_long), Some(20));
        assert_eq!(properties.get("i").and_then(Value::as_long), Some(9));
        assert_eq!(
            properties
                .get_with_position("i")
                .map(|(value, position)| (value.as_long(), position)),
            Some((Some(9), Some(8)))
        );

        let DynamicPropertyStorage::Indexed(indexed) = &properties.storage else {
            unreachable!();
        };
        let entry_key = &indexed.entries[8].0;
        let index_key = indexed
            .index
            .keys()
            .find(|key| key.as_ref() == "i")
            .unwrap();
        assert!(Rc::ptr_eq(&entry_key.0, &index_key.0));

        let mut keys = Vec::new();
        properties.for_each(|key, _| keys.push(key.to_string()));
        assert_eq!(
            keys,
            ["a", "b", "c", "d", "e", "f", "g", "h", "i"]
        );

        properties.insert_owned("b".to_string(), Value::long(200));
        *properties.get_mut("b").unwrap() = Value::long(201);
        assert_eq!(
            properties
                .get_with_position("b")
                .map(|(value, position)| (value.as_long(), position)),
            Some((Some(201), Some(1)))
        );
        let cloned = properties.clone();
        assert!(matches!(cloned.storage, DynamicPropertyStorage::Indexed(_)));
        assert_eq!(cloned.get("b").and_then(Value::as_long), Some(201));
        let mut cloned_keys = Vec::new();
        cloned.for_each(|key, _| cloned_keys.push(key.to_string()));
        assert_eq!(cloned_keys, keys);

        assert!(matches!(
            DynamicPropertyMap::with_capacity(4).storage,
            DynamicPropertyStorage::Linear(_)
        ));
        assert!(matches!(
            DynamicPropertyMap::with_capacity(9).storage,
            DynamicPropertyStorage::Indexed(_)
        ));

        let direct = DynamicPropertyMap::from_hash_map(
            (0..9)
                .map(|index| (format!("key{index}"), Value::long(index)))
                .collect(),
        );
        assert!(matches!(direct.storage, DynamicPropertyStorage::Indexed(_)));
        assert_eq!(direct.len(), 9);
        assert_eq!(
            direct
                .get_with_position("key8")
                .map(|(value, position)| (value.as_long(), position.is_some())),
            Some((Some(8), true))
        );
    }

    #[test]
    fn dynamic_property_pair_validates_positions_across_all_storage_tiers() {
        let mut properties = DynamicPropertyMap::with_capacity(0);
        properties.insert_owned("name".to_string(), Value::long(5));
        properties.insert_owned("value".to_string(), Value::long(11));
        properties.insert_owned("extra".to_string(), Value::long(17));

        // Both cached positions intentionally describe the opposite insertion
        // order. Name fallback must resolve each property independently.
        let pair = properties.get_pair_at_positions(
            ["value", "name"],
            [Some(0), Some(1)],
        );
        assert_eq!(unsafe { (*pair[0]).as_long() }, Some(11));
        assert_eq!(unsafe { (*pair[1]).as_long() }, Some(5));

        let missing = properties.get_pair_at_positions(
            ["value", "missing"],
            [Some(1), Some(2)],
        );
        assert!(!missing[0].is_null());
        assert!(missing[1].is_null());

        properties.insert_owned("fourth".to_string(), Value::long(23));
        assert!(matches!(properties.storage, DynamicPropertyStorage::Linear(_)));
        let pair = properties.get_pair_at_positions(
            ["value", "name"],
            [Some(99), Some(99)],
        );
        assert_eq!(unsafe { (*pair[0]).as_long() }, Some(11));
        assert_eq!(unsafe { (*pair[1]).as_long() }, Some(5));

        for (key, value) in [
            ("fifth", 29),
            ("sixth", 31),
            ("seventh", 37),
            ("eighth", 41),
        ] {
            properties.insert_owned(key.to_string(), Value::long(value));
        }
        assert!(matches!(properties.storage, DynamicPropertyStorage::Linear(_)));
        let pair = properties.get_pair_at_positions(
            ["value", "name"],
            [Some(1), Some(0)],
        );
        assert_eq!(unsafe { (*pair[0]).as_long() }, Some(11));
        assert_eq!(unsafe { (*pair[1]).as_long() }, Some(5));

        properties.insert_owned("ninth".to_string(), Value::long(43));
        assert!(matches!(properties.storage, DynamicPropertyStorage::Indexed(_)));
        let pair = properties.get_pair_at_positions(
            ["value", "name"],
            [Some(8), Some(7)],
        );
        assert_eq!(unsafe { (*pair[0]).as_long() }, Some(11));
        assert_eq!(unsafe { (*pair[1]).as_long() }, Some(5));
    }
}

/// PHP array — ordered hash map with integer and string keys.
/// Preserves insertion order, supports auto-incrementing integer keys.
///
/// Four internal representations selected dynamically:
/// - **Packed**: `Vec<Value>` — keys are implicit 0..N-1. No per-element key storage.
///   Used for sequential integer-indexed arrays (`[1,2,3]`, `$a[] = x`).
///   Push = `Vec::push`. Read = `Vec[i]`. Clone = clone values only (no keys).
/// - **SmallHash**: up to three explicit entries held in the `PhpArray`
///   allocation. Reads are short linear scans or validated positions, with no
///   entry-vector or hash-index allocation.
/// - **LinearHash**: four to eight ordered entries in one compact vector.
///   It starts with bounded linear lookups and creates a string index only if
///   repeated reads prove that the index will amortize its allocation.
/// - **Hash**: ordered compact entries + split integer/string indexes.
///   Integer keys stay inline; string entries and their index share one key allocation.
///   Used when string keys, sparse int keys, or structural mutations occur.
///
/// Transitions from packed to an explicit-key representation, then from the
/// bounded representations to the general hash, are one-way and automatic.
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
    /// Up to three explicit keys stored inside the `PhpArray` allocation.
    /// The first `None` terminates the dense prefix, so no separate length or
    /// heap allocation is needed.
    SmallHash(SmallHashStorage),
    /// Four to eight explicit keys in insertion order, without secondary
    /// indexes. Mutations and reads scan a strictly bounded entry window.
    LinearHash(LinearHashStorage),
    /// General ordered map — explicit keys + split hash indexes.
    Hash {
        entries: Vec<(ArrayEntryKey, Value)>,
        str_index: HashMap<SharedStringKey, usize>,
        int_index: IntIndex,
    },
}

/// Thin shared string key used by hash entries and their string index.
/// Keeping the `String` header behind one pointer makes `ArrayEntryKey` 16
/// bytes while both structures share the same key allocation.
#[derive(Clone, PartialEq, Eq, Hash)]
struct SharedStringKey(Rc<String>);

impl SharedStringKey {
    #[inline]
    fn new(value: &str) -> Self {
        Self(Rc::new(value.to_string()))
    }

    #[inline]
    fn from_owned(value: String) -> Self {
        Self(Rc::new(value))
    }

    /// Share the immutable Rc-backed storage already owned by a PHP string.
    /// Later mutation of the source string detaches through normal COW.
    #[inline]
    fn from_value(value: &Value) -> Option<Self> {
        let ptr = value.string_rc_ptr()?;
        unsafe {
            Rc::increment_strong_count(ptr);
            Some(Self(Rc::from_raw(ptr)))
        }
    }
}

impl Deref for SharedStringKey {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0.as_str()
    }
}

impl Borrow<str> for SharedStringKey {
    #[inline]
    fn borrow(&self) -> &str {
        self
    }
}

impl AsRef<str> for SharedStringKey {
    #[inline]
    fn as_ref(&self) -> &str {
        self
    }
}

#[derive(Clone)]
enum ArrayEntryKey {
    Int(i64),
    String(SharedStringKey),
}

const SMALL_HASH_CAPACITY: usize = 3;
const LINEAR_HASH_CAPACITY: usize = 8;
const LINEAR_HASH_INDEX_THRESHOLD: u8 = 4;

#[derive(Clone)]
struct SmallHashStorage {
    entries: [Option<(ArrayEntryKey, Value)>; SMALL_HASH_CAPACITY],
}

struct LinearHashStorage {
    entries: Vec<(ArrayEntryKey, Value)>,
    string_lookups: Cell<u8>,
    str_index: OnceCell<HashMap<SharedStringKey, usize>>,
}

impl LinearHashStorage {
    #[inline]
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            string_lookups: Cell::new(0),
            str_index: OnceCell::new(),
        }
    }

    #[inline]
    fn from_entries(entries: Vec<(ArrayEntryKey, Value)>) -> Self {
        Self {
            entries,
            string_lookups: Cell::new(0),
            str_index: OnceCell::new(),
        }
    }

    #[inline]
    fn find_int(&self, key: i64) -> Option<usize> {
        linear_find_int(&self.entries, key)
    }

    #[inline]
    fn find_str_for_update(&self, key: &str) -> Option<usize> {
        self.str_index
            .get()
            .and_then(|index| index.get(key).copied())
            .or_else(|| linear_find_str(&self.entries, key))
    }

    #[inline]
    fn find_str(&self, key: &str) -> Option<usize> {
        if let Some(index) = self.str_index.get() {
            return index.get(key).copied();
        }
        let position = linear_find_str(&self.entries, key);
        let lookups = self.string_lookups.get().saturating_add(1);
        if lookups >= LINEAR_HASH_INDEX_THRESHOLD {
            let string_keys = self
                .entries
                .iter()
                .filter(|entry| matches!(&entry.0, ArrayEntryKey::String(_)))
                .count();
            let mut index = HashMap::with_capacity(string_keys);
            for (position, (key, _)) in self.entries.iter().enumerate() {
                if let ArrayEntryKey::String(key) = key {
                    index.insert(key.clone(), position);
                }
            }
            let _ = self.str_index.set(index);
        } else {
            self.string_lookups.set(lookups);
        }
        position
    }

    #[inline]
    fn invalidate_index(&mut self) {
        self.str_index.take();
        self.string_lookups.set(0);
    }
}

impl Clone for LinearHashStorage {
    fn clone(&self) -> Self {
        Self::from_entries(self.entries.clone())
    }
}

impl SmallHashStorage {
    #[inline]
    fn new() -> Self {
        Self {
            entries: [const { None }; SMALL_HASH_CAPACITY],
        }
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.entries
            .iter()
            .position(Option::is_none)
            .unwrap_or(SMALL_HASH_CAPACITY)
    }

    #[inline(always)]
    fn get(&self, position: usize) -> Option<&(ArrayEntryKey, Value)> {
        self.entries.get(position)?.as_ref()
    }

    #[inline(always)]
    fn push(&mut self, key: ArrayEntryKey, value: Value) -> bool {
        let len = self.len();
        if len == SMALL_HASH_CAPACITY {
            return false;
        }
        self.entries[len] = Some((key, value));
        true
    }

    #[inline]
    fn find_int(&self, key: i64) -> Option<usize> {
        self.entries[..self.len()]
            .iter()
            .position(|entry| matches!(entry, Some((ArrayEntryKey::Int(found), _)) if *found == key))
    }

    #[inline]
    fn find_str(&self, key: &str) -> Option<usize> {
        self.entries[..self.len()].iter().position(
            |entry| matches!(entry, Some((ArrayEntryKey::String(found), _)) if found.as_ref() == key),
        )
    }

    #[inline]
    fn remove_at(&mut self, position: usize) -> Option<(ArrayEntryKey, Value)> {
        let len = self.len();
        if position >= len {
            return None;
        }
        let removed = self.entries[position].take();
        for index in position..len - 1 {
            self.entries[index] = self.entries[index + 1].take();
        }
        removed
    }
}

impl ArrayEntryKey {
    #[inline]
    fn to_public(&self) -> ArrayKey {
        match self {
            Self::Int(value) => ArrayKey::Int(*value),
            Self::String(value) => ArrayKey::String(value.to_string()),
        }
    }
}

#[inline]
fn linear_find_int(entries: &[(ArrayEntryKey, Value)], key: i64) -> Option<usize> {
    entries
        .iter()
        .position(|entry| matches!(entry.0, ArrayEntryKey::Int(found) if found == key))
}

#[inline]
fn linear_find_str(entries: &[(ArrayEntryKey, Value)], key: &str) -> Option<usize> {
    entries.iter().position(
        |entry| matches!(&entry.0, ArrayEntryKey::String(found) if found.as_ref() == key),
    )
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

    /// Create packed storage with capacity known from an array literal.
    pub fn with_packed_capacity(capacity: usize) -> Self {
        Self {
            storage: ArrayStorage::Packed(Vec::with_capacity(capacity)),
            next_int_key: 0,
        }
    }

    /// Create string-indexed hash storage directly when a literal string key
    /// proves that a packed representation would immediately transition.
    pub fn with_hash_capacity(capacity: usize) -> Self {
        if capacity <= SMALL_HASH_CAPACITY {
            return Self {
                storage: ArrayStorage::SmallHash(SmallHashStorage::new()),
                next_int_key: 0,
            };
        }
        Self {
            storage: ArrayStorage::Hash {
                entries: Vec::with_capacity(capacity),
                str_index: HashMap::with_capacity(capacity),
                int_index: int_index_with_capacity(0),
            },
            next_int_key: 0,
        }
    }

    /// Create associative storage for a streaming producer that does not know
    /// its final member count. Up to three entries remain inline, four to eight
    /// use bounded linear storage, and wider results build the general indexes.
    pub(crate) fn with_deferred_hash_capacity(capacity: usize) -> Self {
        if capacity <= SMALL_HASH_CAPACITY {
            return Self {
                storage: ArrayStorage::SmallHash(SmallHashStorage::new()),
                next_int_key: 0,
            };
        }
        if capacity <= LINEAR_HASH_CAPACITY {
            return Self {
                storage: ArrayStorage::LinearHash(LinearHashStorage::with_capacity(capacity)),
                next_int_key: 0,
            };
        }
        Self::with_hash_capacity(capacity)
    }

    /// Transition from packed to hash mode. Moves values into entries with explicit keys.
    fn transition_to_hash(&mut self) {
        if let ArrayStorage::Packed(values) = &mut self.storage {
            let len = values.len();
            if len <= SMALL_HASH_CAPACITY {
                let mut small = SmallHashStorage::new();
                for (index, value) in std::mem::take(values).into_iter().enumerate() {
                    let inserted = small.push(ArrayEntryKey::Int(index as i64), value);
                    debug_assert!(inserted);
                }
                self.storage = ArrayStorage::SmallHash(small);
                return;
            }
            let mut entries = Vec::with_capacity(len);
            let mut int_index = int_index_with_capacity(len);
            for (i, val) in std::mem::take(values).into_iter().enumerate() {
                int_index.insert(i as i64, i);
                entries.push((ArrayEntryKey::Int(i as i64), val));
            }
            *&mut self.storage = ArrayStorage::Hash {
                entries,
                str_index: HashMap::new(),
                int_index,
            };
        }
    }

    /// Promote full inline storage to the general indexed representation.
    /// Existing entries are moved without cloning their keys or values.
    fn promote_small_hash(
        &mut self,
        additional_string_capacity: usize,
        additional_int_capacity: usize,
    ) {
        if !matches!(&self.storage, ArrayStorage::SmallHash(_)) {
            return;
        };
        let small = match std::mem::replace(
            &mut self.storage,
            ArrayStorage::Packed(Vec::new()),
        ) {
            ArrayStorage::SmallHash(small) => small,
            _ => unreachable!(),
        };
        let len = small.len();
        let capacity = len
            .saturating_add(additional_string_capacity)
            .saturating_add(additional_int_capacity);
        let mut entries = Vec::with_capacity(capacity);
        entries.extend(small.entries.into_iter().flatten());
        let string_keys = entries
            .iter()
            .filter(|entry| matches!(&entry.0, ArrayEntryKey::String(_)))
            .count();
        let mut str_index =
            HashMap::with_capacity(string_keys.saturating_add(additional_string_capacity));
        let mut int_index =
            int_index_with_capacity((len - string_keys).saturating_add(additional_int_capacity));
        for (position, (key, _)) in entries.iter().enumerate() {
            match key {
                ArrayEntryKey::Int(key) => {
                    int_index.insert(*key, position);
                }
                ArrayEntryKey::String(key) => {
                    str_index.insert(key.clone(), position);
                }
            }
        }
        self.storage = ArrayStorage::Hash {
            entries,
            str_index,
            int_index,
        };
    }

    /// A streaming materializer with unknown final width may deliberately
    /// retain four to eight entries without secondary indexes.
    fn promote_small_hash_to_linear(&mut self) {
        let ArrayStorage::SmallHash(_) = &self.storage else {
            return;
        };
        let ArrayStorage::SmallHash(small) =
            std::mem::replace(&mut self.storage, ArrayStorage::Packed(Vec::new()))
        else {
            unreachable!();
        };
        let mut entries = Vec::with_capacity(LINEAR_HASH_CAPACITY);
        entries.extend(small.entries.into_iter().flatten());
        self.storage = ArrayStorage::LinearHash(LinearHashStorage::from_entries(entries));
    }

    /// Build split indexes once the bounded linear representation is full.
    /// The ordered entry vector is retained and only reserves its next growth
    /// step, avoiding a second move of keys and values.
    fn promote_linear_hash(
        &mut self,
        additional_string_capacity: usize,
        additional_int_capacity: usize,
    ) {
        let ArrayStorage::LinearHash(_) = &self.storage else {
            return;
        };
        let ArrayStorage::LinearHash(linear) =
            std::mem::replace(&mut self.storage, ArrayStorage::Packed(Vec::new()))
        else {
            unreachable!();
        };
        let LinearHashStorage {
            mut entries,
            str_index,
            ..
        } = linear;
        let len = entries.len();
        let requested = len
            .saturating_add(additional_string_capacity)
            .saturating_add(additional_int_capacity);
        let capacity = requested.next_power_of_two();
        entries.reserve(capacity.saturating_sub(len));
        let string_keys = entries
            .iter()
            .filter(|entry| matches!(&entry.0, ArrayEntryKey::String(_)))
            .count();
        let mut str_index = str_index.into_inner().unwrap_or_else(|| {
            HashMap::with_capacity(string_keys.saturating_add(additional_string_capacity))
        });
        str_index.reserve(additional_string_capacity);
        let mut int_index =
            int_index_with_capacity((len - string_keys).saturating_add(additional_int_capacity));
        for (position, (key, _)) in entries.iter().enumerate() {
            match key {
                ArrayEntryKey::Int(key) => {
                    int_index.insert(*key, position);
                }
                ArrayEntryKey::String(key) => {
                    if !str_index.contains_key(key.as_ref()) {
                        str_index.insert(key.clone(), position);
                    }
                }
            }
        }
        self.storage = ArrayStorage::Hash {
            entries,
            str_index,
            int_index,
        };
    }

    /// Append with auto-incrementing key ($a[] = val)
    #[inline]
    pub fn push(&mut self, val: Value) {
        let key = self.next_int_key;
        self.next_int_key = key + 1;
        match &self.storage {
            ArrayStorage::SmallHash(small) if small.len() == SMALL_HASH_CAPACITY => {
                self.promote_small_hash(0, 1);
            }
            ArrayStorage::LinearHash(linear)
                if linear.entries.len() == LINEAR_HASH_CAPACITY =>
            {
                self.promote_linear_hash(0, 1);
            }
            _ => {}
        }
        match &mut self.storage {
            ArrayStorage::Packed(values) => {
                values.push(val);
            }
            ArrayStorage::SmallHash(small) => {
                let inserted = small.push(ArrayEntryKey::Int(key), val);
                debug_assert!(inserted);
            }
            ArrayStorage::LinearHash(linear) => {
                linear.invalidate_index();
                linear.entries.push((ArrayEntryKey::Int(key), val));
            }
            ArrayStorage::Hash { entries, int_index, .. } => {
                let idx = entries.len();
                entries.push((ArrayEntryKey::Int(key), val));
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
        if let ArrayStorage::SmallHash(small) =
            &mut self.storage
        {
            if let Some(index) = small.find_int(key) {
                small.entries[index].as_mut().unwrap().1 = val;
                return;
            }
            if small.len() < SMALL_HASH_CAPACITY {
                let inserted = small.push(ArrayEntryKey::Int(key), val);
                debug_assert!(inserted);
                if key >= self.next_int_key {
                    self.next_int_key = key + 1;
                }
                return;
            }
        }
        self.promote_small_hash(0, 1);
        if let ArrayStorage::LinearHash(linear) = &mut self.storage {
            if let Some(index) = linear.find_int(key) {
                linear.entries[index].1 = val;
                return;
            }
            if linear.entries.len() < LINEAR_HASH_CAPACITY {
                linear.invalidate_index();
                linear.entries.push((ArrayEntryKey::Int(key), val));
                if key >= self.next_int_key {
                    self.next_int_key = key + 1;
                }
                return;
            }
        }
        self.promote_linear_hash(0, 1);
        // Now in indexed hash mode.
        let storage = &mut self.storage;
        if let ArrayStorage::Hash { entries, int_index, .. } = storage {
            if let Some(&idx) = int_index.get(&key) {
                entries[idx].1 = val;
            } else {
                let idx = entries.len();
                entries.push((ArrayEntryKey::Int(key), val));
                int_index.insert(key, idx);
                if key >= self.next_int_key {
                    self.next_int_key = key + 1;
                }
            }
        }
    }

    /// Insert an integer key for a streaming associative materializer whose
    /// final width is unknown. A fourth unique entry selects bounded linear
    /// storage; ordinary PHP array writes retain their usual indexed policy.
    pub(crate) fn set_streamed_int(&mut self, key: i64, val: Value) {
        let promote = matches!(
            &self.storage,
            ArrayStorage::SmallHash(small)
                if small.len() == SMALL_HASH_CAPACITY && small.find_int(key).is_none()
        );
        if promote {
            self.promote_small_hash_to_linear();
        }
        let promote_linear = matches!(
            &self.storage,
            ArrayStorage::LinearHash(linear)
                if linear.entries.len() == LINEAR_HASH_CAPACITY
                    && linear.find_int(key).is_none()
        );
        if promote_linear {
            self.promote_linear_hash(0, 1);
        }
        self.set_int(key, val);
    }

    /// Set by string key
    pub fn set_str(&mut self, key: &str, val: Value) {
        // String key → always hash mode
        if matches!(&self.storage, ArrayStorage::Packed(_)) {
            self.transition_to_hash();
        }
        if let ArrayStorage::SmallHash(small) =
            &mut self.storage
        {
            if let Some(index) = small.find_str(key) {
                small.entries[index].as_mut().unwrap().1 = val;
                return;
            }
            if small.len() < SMALL_HASH_CAPACITY {
                let inserted = small.push(ArrayEntryKey::String(SharedStringKey::new(key)), val);
                debug_assert!(inserted);
                return;
            }
        }
        self.promote_small_hash(1, 0);
        if let ArrayStorage::LinearHash(linear) = &mut self.storage {
            if let Some(index) = linear.find_str_for_update(key) {
                linear.entries[index].1 = val;
                return;
            }
            if linear.entries.len() < LINEAR_HASH_CAPACITY {
                linear.invalidate_index();
                linear.entries.push((
                    ArrayEntryKey::String(SharedStringKey::new(key)),
                    val,
                ));
                return;
            }
        }
        self.promote_linear_hash(1, 0);
        if let ArrayStorage::Hash { entries, str_index, .. } = &mut self.storage {
            if let Some(&idx) = str_index.get(key) {
                // Key exists — overwrite value, no allocation for key
                entries[idx].1 = val;
            } else {
                // New key — one shared allocation for both entry and index.
                let owned = SharedStringKey::new(key);
                let idx = entries.len();
                entries.push((ArrayEntryKey::String(owned.clone()), val));
                str_index.insert(owned, idx);
            }
        }
    }

    /// Set a non-numeric string key while taking ownership of its existing
    /// allocation. Streaming decoders use this to move parsed object keys
    /// directly into PHP array storage instead of copying their bytes.
    pub(crate) fn set_owned_str(&mut self, key: String, val: Value) {
        if matches!(&self.storage, ArrayStorage::Packed(_)) {
            self.transition_to_hash();
        }
        if let ArrayStorage::SmallHash(small) =
            &mut self.storage
        {
            if let Some(index) = small.find_str(&key) {
                small.entries[index].as_mut().unwrap().1 = val;
                return;
            }
            if small.len() < SMALL_HASH_CAPACITY {
                let inserted = small.push(
                    ArrayEntryKey::String(SharedStringKey::from_owned(key)),
                    val,
                );
                debug_assert!(inserted);
                return;
            }
        }
        self.promote_small_hash(1, 0);
        if let ArrayStorage::LinearHash(linear) = &mut self.storage {
            if let Some(index) = linear.find_str_for_update(&key) {
                linear.entries[index].1 = val;
                return;
            }
            if linear.entries.len() < LINEAR_HASH_CAPACITY {
                linear.invalidate_index();
                linear.entries.push((
                    ArrayEntryKey::String(SharedStringKey::from_owned(key)),
                    val,
                ));
                return;
            }
        }
        self.promote_linear_hash(1, 0);
        if let ArrayStorage::Hash { entries, str_index, .. } = &mut self.storage {
            if let Some(&idx) = str_index.get(key.as_str()) {
                entries[idx].1 = val;
            } else {
                let owned = SharedStringKey::from_owned(key);
                let idx = entries.len();
                entries.push((ArrayEntryKey::String(owned.clone()), val));
                str_index.insert(owned, idx);
            }
        }
    }

    /// Owned-string counterpart of `set_streamed_int`. Parsed key bytes move
    /// into canonical array storage without forcing a full index at key four.
    pub(crate) fn set_streamed_owned_str(&mut self, key: String, val: Value) {
        let promote = matches!(
            &self.storage,
            ArrayStorage::SmallHash(small)
                if small.len() == SMALL_HASH_CAPACITY && small.find_str(&key).is_none()
        );
        if promote {
            self.promote_small_hash_to_linear();
        }
        let promote_linear = matches!(
            &self.storage,
            ArrayStorage::LinearHash(linear)
                if linear.entries.len() == LINEAR_HASH_CAPACITY
                    && linear.find_str_for_update(&key).is_none()
        );
        if promote_linear {
            self.promote_linear_hash(1, 0);
        }
        self.set_owned_str(key, val);
    }

    /// Set a non-numeric PHP string key while sharing its Rc allocation with
    /// the source Value. This avoids materializing an intermediate ArrayKey and
    /// allocating a second copy of the same immutable key bytes.
    pub fn set_str_value(&mut self, key: &Value, val: Value) {
        let key_text = key.as_str().expect("set_str_value requires a string Value");
        if matches!(&self.storage, ArrayStorage::Packed(_)) {
            self.transition_to_hash();
        }
        if let ArrayStorage::SmallHash(small) =
            &mut self.storage
        {
            if let Some(index) = small.find_str(key_text) {
                small.entries[index].as_mut().unwrap().1 = val;
                return;
            }
            if small.len() < SMALL_HASH_CAPACITY {
                let owned = SharedStringKey::from_value(key)
                    .expect("set_str_value requires Rc-backed string storage");
                let inserted = small.push(ArrayEntryKey::String(owned), val);
                debug_assert!(inserted);
                return;
            }
        }
        self.promote_small_hash(1, 0);
        if let ArrayStorage::LinearHash(linear) = &mut self.storage {
            if let Some(index) = linear.find_str_for_update(key_text) {
                linear.entries[index].1 = val;
                return;
            }
            if linear.entries.len() < LINEAR_HASH_CAPACITY {
                let owned = SharedStringKey::from_value(key)
                    .expect("set_str_value requires Rc-backed string storage");
                linear.invalidate_index();
                linear.entries.push((ArrayEntryKey::String(owned), val));
                return;
            }
        }
        self.promote_linear_hash(1, 0);
        if let ArrayStorage::Hash { entries, str_index, .. } = &mut self.storage {
            if let Some(&idx) = str_index.get(key_text) {
                entries[idx].1 = val;
            } else {
                let owned = SharedStringKey::from_value(key)
                    .expect("set_str_value requires Rc-backed string storage");
                let idx = entries.len();
                entries.push((ArrayEntryKey::String(owned.clone()), val));
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

    #[inline(always)]
    fn hash_entry_at(&self, position: usize) -> Option<&(ArrayEntryKey, Value)> {
        match &self.storage {
            ArrayStorage::Hash { entries, .. } => entries.get(position),
            ArrayStorage::LinearHash(linear) => linear.entries.get(position),
            ArrayStorage::SmallHash(small) => {
                small.get(position)
            }
            ArrayStorage::Packed(_) => None,
        }
    }

    #[inline(always)]
    fn hash_len(&self) -> Option<usize> {
        match &self.storage {
            ArrayStorage::Hash { entries, .. } => Some(entries.len()),
            ArrayStorage::LinearHash(linear) => Some(linear.entries.len()),
            ArrayStorage::SmallHash(small) => {
                Some(small.len())
            }
            ArrayStorage::Packed(_) => None,
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
            ArrayStorage::SmallHash(small) => small
                .find_int(key)
                .and_then(|position| small.get(position))
                .map(|entry| &entry.1),
            ArrayStorage::LinearHash(linear) => linear
                .find_int(key)
                .and_then(|position| linear.entries.get(position))
                .map(|entry| &entry.1),
            ArrayStorage::Hash { entries, int_index, .. } => {
                // Ordered PHP arrays commonly retain a contiguous integer run
                // after transitioning to hash storage. Derive the likely entry
                // position from its first key and validate it; irregular
                // layouts fall through to the general integer hash index.
                if let Some((ArrayEntryKey::Int(first_key), _)) = entries.first() {
                    if let Some(position) = key
                        .checked_sub(*first_key)
                        .and_then(|offset| usize::try_from(offset).ok())
                    {
                        if let Some((ArrayEntryKey::Int(found_key), value)) = entries.get(position) {
                            if *found_key == key {
                                return Some(value);
                            }
                        }
                    }
                } else if key >= 0 {
                    if let Some((ArrayEntryKey::Int(found_key), value)) = entries.get(key as usize) {
                        if *found_key == key {
                            return Some(value);
                        }
                    }
                }
                int_index.get(&key).map(|&idx| &entries[idx].1)
            }
        }
    }

    /// Mutable lookup used only after the caller has established unique COW
    /// ownership. Replacing the returned entry cannot change array structure.
    #[cfg(feature = "quick-loops")]
    #[inline(always)]
    pub(crate) fn get_int_mut(&mut self, key: i64) -> Option<&mut Value> {
        match &mut self.storage {
            ArrayStorage::Packed(values) if key >= 0 => values.get_mut(key as usize),
            ArrayStorage::Packed(_) => None,
            ArrayStorage::SmallHash(small) => {
                let position = small.find_int(key)?;
                small.entries.get_mut(position)?.as_mut().map(|entry| &mut entry.1)
            }
            ArrayStorage::LinearHash(linear) => {
                let position = linear.find_int(key)?;
                linear.entries.get_mut(position).map(|entry| &mut entry.1)
            }
            ArrayStorage::Hash { entries, int_index, .. } => {
                let position = *int_index.get(&key)?;
                entries.get_mut(position).map(|entry| &mut entry.1)
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
        let Some(len) = self.hash_len() else {
            return false;
        };
        if len == 1 {
            return matches!(self.hash_entry_at(0), Some((ArrayEntryKey::Int(_), _)));
        }
        match (self.hash_entry_at(0), self.hash_entry_at(1)) {
            (
                Some((ArrayEntryKey::Int(first), _)),
                Some((ArrayEntryKey::Int(second), _)),
            ) => first.checked_add(1) == Some(*second),
            _ => false,
        }
    }

    /// Derive an ordered-entry position hint from a short integer-key window.
    ///
    /// The hint never establishes correctness on its own. Guarded readers
    /// validate the key stored at the derived position and retain the integer
    /// index as a fallback for holes, interleaved string keys, and irregular
    /// entries. Prefix classification preserves the common hash-transition
    /// path; a suffix window also recognizes regularly appended data after a
    /// short metadata prefix without scanning the complete array.
    #[cfg(any(feature = "quick-loops", test))]
    #[inline]
    pub(crate) fn integer_position_hint(&self) -> Option<(i64, i64)> {
        let len = self.hash_len()?;
        if len == 1 {
            if let Some((ArrayEntryKey::Int(first), _)) = self.hash_entry_at(0) {
                return Some((*first, 1));
            }
        }

        let window_hint = |start: usize| {
            let end = start.saturating_add(8).min(len);
            if end.saturating_sub(start) < 2 {
                return None;
            }
            let (ArrayEntryKey::Int(first), _) = self.hash_entry_at(start)? else {
                return None;
            };
            let (ArrayEntryKey::Int(second), _) = self.hash_entry_at(start + 1)? else {
                return None;
            };
            let stride = second
                .checked_sub(*first)
                .filter(|stride| *stride != 0)?;
            for (offset, position) in (start..end).enumerate() {
                let (key, _) = self.hash_entry_at(position)?;
                let expected = stride
                    .checked_mul(offset as i64)
                    .and_then(|delta| first.checked_add(delta))?;
                if !matches!(key, ArrayEntryKey::Int(found) if *found == expected) {
                    return None;
                }
            }

            // Encode the anchor as the key that would occupy entry position 0.
            // This keeps the hot hint at two i64 values even for suffix-derived
            // progressions: position = (key - position_zero_key) / stride.
            let start = i64::try_from(start).ok()?;
            let position_zero_key = stride
                .checked_mul(start)
                .and_then(|delta| first.checked_sub(delta))?;
            Some((position_zero_key, stride))
        };

        window_hint(0).or_else(|| {
            let suffix_start = len.saturating_sub(8);
            (suffix_start != 0).then(|| window_hint(suffix_start)).flatten()
        })
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
        self.hash_len()?;
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
            if let Some((ArrayEntryKey::Int(found_key), value)) = self.hash_entry_at(position) {
                if *found_key == key {
                    return Some(value);
                }
            }
        }
        match &self.storage {
            ArrayStorage::SmallHash(small) => small
                .find_int(key)
                .and_then(|position| small.get(position))
                .map(|entry| &entry.1),
            ArrayStorage::LinearHash(linear) => linear
                .find_int(key)
                .and_then(|position| linear.entries.get(position))
                .map(|entry| &entry.1),
            ArrayStorage::Hash { entries, int_index, .. } => {
                int_index.get(&key).map(|&index| &entries[index].1)
            }
            ArrayStorage::Packed(_) => None,
        }
    }

    /// Integer lookup that deliberately skips the ordered-entry fast path.
    /// Guarded quick regions use this for arrays classified as irregular once
    /// at activation instead of repeating a known-to-fail positional probe.
    #[inline]
    pub fn get_indexed_int(&self, key: i64) -> Option<&Value> {
        match &self.storage {
            ArrayStorage::SmallHash(small) => small
                .find_int(key)
                .and_then(|position| small.get(position))
                .map(|entry| &entry.1),
            ArrayStorage::LinearHash(linear) => linear
                .find_int(key)
                .and_then(|position| linear.entries.get(position))
                .map(|entry| &entry.1),
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
            ArrayStorage::Hash { entries, str_index, .. } => {
                str_index.get(key).map(|&idx| &entries[idx].1)
            }
            ArrayStorage::LinearHash(linear) => linear
                .find_str(key)
                .and_then(|position| linear.entries.get(position))
                .map(|entry| &entry.1),
            ArrayStorage::Packed(_) => None, // packed arrays have no string keys
            ArrayStorage::SmallHash(small) => small
                .find_str(key)
                .and_then(|position| small.get(position))
                .map(|entry| &entry.1),
        }
    }

    /// Mutable string-key lookup for guarded replacement of an existing
    /// entry. The key/index storage remains untouched.
    #[cfg(feature = "quick-loops")]
    #[inline(always)]
    pub(crate) fn get_str_mut(&mut self, key: &str) -> Option<&mut Value> {
        match &mut self.storage {
            ArrayStorage::Packed(_) => None,
            ArrayStorage::SmallHash(small) => {
                let position = small.find_str(key)?;
                small.entries.get_mut(position)?.as_mut().map(|entry| &mut entry.1)
            }
            ArrayStorage::LinearHash(linear) => {
                let position = linear.find_str(key)?;
                linear.entries.get_mut(position).map(|entry| &mut entry.1)
            }
            ArrayStorage::Hash { entries, str_index, .. } => {
                let position = *str_index.get(key)?;
                entries.get_mut(position).map(|entry| &mut entry.1)
            }
        }
    }

    /// Validate a cached ordered-entry position for a string key. A position
    /// is only a hint: mutations, COW detaches, and unrelated array layouts
    /// safely return `None` before exposing the value.
    #[inline]
    pub(crate) fn get_positioned_str(&self, key: &str, position: usize) -> Option<&Value> {
        match self.hash_entry_at(position) {
            Some((ArrayEntryKey::String(found), value)) if found.as_ref() == key => Some(value),
            _ => None,
        }
    }

    /// Resolve a string key through the index while retaining its ordered
    /// position for a guarded call-site cache.
    #[inline]
    pub(crate) fn get_str_with_position(&self, key: &str) -> Option<(usize, &Value)> {
        match &self.storage {
            ArrayStorage::Hash {
                entries,
                str_index,
                ..
            } => {
                let position = *str_index.get(key)?;
                Some((position, &entries.get(position)?.1))
            }
            ArrayStorage::LinearHash(linear) => {
                let position = linear.find_str(key)?;
                Some((position, &linear.entries.get(position)?.1))
            }
            ArrayStorage::SmallHash(small) => {
                let position = small.find_str(key)?;
                Some((position, &small.get(position)?.1))
            }
            ArrayStorage::Packed(_) => None,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        match &self.storage {
            ArrayStorage::Packed(values) => values.len(),
            ArrayStorage::SmallHash(small) => small.len(),
            ArrayStorage::LinearHash(linear) => linear.entries.len(),
            ArrayStorage::Hash { entries, .. } => entries.len(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        match &self.storage {
            ArrayStorage::Packed(values) => values.is_empty(),
            ArrayStorage::SmallHash(small) => {
                small.len() == 0
            }
            ArrayStorage::LinearHash(linear) => linear.entries.is_empty(),
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
            ArrayStorage::SmallHash(small) => small
                .get(pos)
                .map(|(key, value)| (value, key.to_public())),
            ArrayStorage::LinearHash(linear) => {
                linear.entries.get(pos).map(|(key, value)| (value, key.to_public()))
            }
            ArrayStorage::Hash { entries, .. } => {
                entries.get(pos).map(|(k, v)| (v, k.to_public()))
            }
        }
    }

    /// Get value at position — for foreach when key is not needed.
    #[inline]
    pub fn get_value_at(&self, pos: usize) -> Option<&Value> {
        match &self.storage {
            ArrayStorage::Packed(values) => values.get(pos),
            ArrayStorage::SmallHash(small) => {
                small.get(pos).map(|entry| &entry.1)
            }
            ArrayStorage::LinearHash(linear) => {
                linear.entries.get(pos).map(|entry| &entry.1)
            }
            ArrayStorage::Hash { entries, .. } => entries.get(pos).map(|(_, v)| v),
        }
    }

    /// Iterate over (key, &value) pairs — works for both packed and hash modes.
    /// No transition, no allocation for packed arrays.
    /// This is the preferred read-only iteration method.
    pub fn iter(&self) -> PhpArrayIter<'_> {
        let inner = match &self.storage {
            ArrayStorage::Packed(values) => PhpArrayIterInner::Packed(values.iter().enumerate()),
            ArrayStorage::SmallHash(small) => {
                PhpArrayIterInner::Small(small.entries[..small.len()].iter())
            }
            ArrayStorage::LinearHash(linear) => {
                PhpArrayIterInner::Hash(linear.entries.iter())
            }
            ArrayStorage::Hash { entries, .. } => PhpArrayIterInner::Hash(entries.iter()),
        };
        PhpArrayIter { inner }
    }

    /// Iterate over values without materializing public array keys.
    ///
    /// Hash arrays keep string keys in a compact shared representation. The
    /// general `iter()` API converts those keys to owned `ArrayKey` values;
    /// value-only operations should use this iterator to avoid that allocation.
    pub fn values(&self) -> PhpArrayValues<'_> {
        let inner = match &self.storage {
            ArrayStorage::Packed(values) => PhpArrayValuesInner::Packed(values.iter()),
            ArrayStorage::SmallHash(small) => {
                PhpArrayValuesInner::Small(small.entries[..small.len()].iter())
            }
            ArrayStorage::LinearHash(linear) => {
                PhpArrayValuesInner::Hash(linear.entries.iter())
            }
            ArrayStorage::Hash { entries, .. } => PhpArrayValuesInner::Hash(entries.iter()),
        };
        PhpArrayValues { inner }
    }

    /// Whether the array contains at least one string key.
    ///
    /// This inspects the private hash index directly, avoiding the owned public
    /// key materialization performed by `iter()`. Packed arrays can never have
    /// string keys.
    #[inline]
    pub fn has_string_keys(&self) -> bool {
        match &self.storage {
            ArrayStorage::Packed(_) => false,
            ArrayStorage::SmallHash(small) => small.entries[..small.len()]
                .iter()
                .any(|entry| matches!(entry, Some((ArrayEntryKey::String(_), _)))),
            ArrayStorage::LinearHash(linear) => linear.entries
                .iter()
                .any(|entry| matches!(entry.0, ArrayEntryKey::String(_))),
            ArrayStorage::Hash { str_index, .. } => !str_index.is_empty(),
        }
    }

    /// Materialize public keys for cold callers that need the complete entry
    /// list. Internal hash storage keeps a smaller key representation.
    /// If array is in packed mode, transitions to hash mode first.
    /// This is a cold-path method — hot paths should use get_at() or get_value_at().
    ///
    /// Takes `&mut self` to safely transition packed→hash.
    /// Callers with `&PhpArray` should use get_at()/get_value_at()/iter() instead.
    pub fn entries(&mut self) -> Vec<(ArrayKey, &Value)> {
        self.transition_to_hash();
        match &self.storage {
            ArrayStorage::SmallHash(small) => small.entries[..small.len()]
                .iter()
                .map(|entry| {
                    let (key, value) = entry.as_ref().unwrap();
                    (key.to_public(), value)
                })
                .collect(),
            ArrayStorage::LinearHash(linear) => linear.entries
                .iter()
                .map(|(key, value)| (key.to_public(), value))
                .collect(),
            ArrayStorage::Hash { entries, .. } => entries
                .iter()
                .map(|(key, value)| (key.to_public(), value))
                .collect(),
            _ => unreachable!(),
        }
    }

    /// Remove element by key
    pub fn remove(&mut self, key: &ArrayKey) -> bool {
        // Remove breaks packed invariant
        if matches!(&self.storage, ArrayStorage::Packed(_)) {
            self.transition_to_hash();
        }
        if let ArrayStorage::SmallHash(small) =
            &mut self.storage
        {
            let position = match key {
                ArrayKey::Int(key) => small.find_int(*key),
                ArrayKey::String(key) => small.find_str(key),
            };
            return position
                .and_then(|position| small.remove_at(position))
                .is_some();
        }
        if let ArrayStorage::LinearHash(linear) = &mut self.storage {
            let position = match key {
                ArrayKey::Int(key) => linear.find_int(*key),
                ArrayKey::String(key) => linear.find_str_for_update(key),
            };
            if let Some(position) = position {
                linear.entries.remove(position);
                linear.invalidate_index();
                return true;
            }
            return false;
        }
        if let ArrayStorage::Hash { entries, int_index, str_index, .. } = &mut self.storage {
            let found_idx = match key {
                ArrayKey::Int(n) => int_index.get(n).copied(),
                ArrayKey::String(s) => str_index.get(s.as_str()).copied(),
            };
            if let Some(idx) = found_idx {
                let (removed_key, _) = entries.remove(idx);
                match removed_key {
                    ArrayEntryKey::Int(n) => { int_index.remove(&n); }
                    ArrayEntryKey::String(s) => { str_index.remove(s.as_ref()); }
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
            ArrayStorage::SmallHash(small) => {
                let position = small.len().checked_sub(1)?;
                let (key, value) = small.remove_at(position)?;
                if matches!(key, ArrayEntryKey::Int(key) if key == self.next_int_key - 1) {
                    self.next_int_key -= 1;
                }
                Some(value)
            }
            ArrayStorage::LinearHash(linear) => {
                let (key, value) = linear.entries.pop()?;
                linear.invalidate_index();
                if matches!(key, ArrayEntryKey::Int(key) if key == self.next_int_key - 1) {
                    self.next_int_key -= 1;
                }
                Some(value)
            }
            ArrayStorage::Hash { entries, int_index, str_index, .. } => {
                if let Some((key, val)) = entries.pop() {
                    match &key {
                        ArrayEntryKey::Int(n) => {
                            int_index.remove(n);
                            // PHP: only decrement if popped key was the auto-index boundary
                            if *n == self.next_int_key - 1 {
                                self.next_int_key -= 1;
                            }
                        }
                        ArrayEntryKey::String(s) => {
                            str_index.remove(s.as_ref());
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
        if let ArrayStorage::SmallHash(small) =
            &mut self.storage
        {
            let (_key, value) = small.remove_at(0)?;
            let mut next_int_key = 0i64;
            let len = small.len();
            for entry in &mut small.entries[..len] {
                if let Some((ArrayEntryKey::Int(key), _)) = entry {
                    *key = next_int_key;
                    next_int_key += 1;
                }
            }
            self.next_int_key = next_int_key;
            return Some(value);
        }
        if let ArrayStorage::LinearHash(linear) = &mut self.storage {
            if linear.entries.is_empty() {
                return None;
            }
            let (_key, value) = linear.entries.remove(0);
            linear.invalidate_index();
            let mut next_int_key = 0i64;
            for (key, _) in linear.entries.iter_mut() {
                if let ArrayEntryKey::Int(key) = key {
                    *key = next_int_key;
                    next_int_key += 1;
                }
            }
            self.next_int_key = next_int_key;
            return Some(value);
        }
        if let ArrayStorage::Hash { entries, int_index, str_index } = &mut self.storage {
            if entries.is_empty() { return None; }
            let (_key, val) = entries.remove(0);

            // Renumber: rebuild with int keys starting from 0, string keys preserved
            let mut new_int_counter: i64 = 0;
            int_index.clear();
            str_index.clear();
            for (i, (key, _)) in entries.iter_mut().enumerate() {
                match key {
                    ArrayEntryKey::Int(n) => {
                        *n = new_int_counter;
                        int_index.insert(new_int_counter, i);
                        new_int_counter += 1;
                    }
                    ArrayEntryKey::String(s) => {
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
        entries: &[(ArrayEntryKey, Value)],
        int_index: &mut IntIndex,
        str_index: &mut HashMap<SharedStringKey, usize>,
        from: usize,
    ) {
        for (i, (k, _)) in entries.iter().enumerate() {
            if i >= from {
                match k {
                    ArrayEntryKey::Int(n) => { int_index.insert(*n, i); }
                    ArrayEntryKey::String(s) => { str_index.insert(s.clone(), i); }
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

    /// Describe the ordered hash value layout without exposing its private key.
    /// Guarded quick regions use the stable base address and stride for
    /// positional reads after proving that their closed body cannot mutate it.
    #[cfg(feature = "quick-loops")]
    #[inline]
    pub(crate) fn ordered_hash_value_layout(&self) -> Option<(*const u8, usize)> {
        match &self.storage {
            ArrayStorage::SmallHash(small) => small.get(0).map(|entry| {
                (
                    (&entry.1 as *const Value).cast(),
                    std::mem::size_of::<Option<(ArrayEntryKey, Value)>>(),
                )
            }),
            ArrayStorage::LinearHash(linear) => linear.entries.first().map(|entry| {
                (
                    (&entry.1 as *const Value).cast(),
                    std::mem::size_of::<(ArrayEntryKey, Value)>(),
                )
            }),
            ArrayStorage::Hash { entries, .. } => entries.first().map(|entry| {
                (
                    (&entry.1 as *const Value).cast(),
                    std::mem::size_of::<(ArrayEntryKey, Value)>(),
                )
            }),
            ArrayStorage::Packed(_) => None,
        }
    }
}

/// Iterator over PhpArray entries — works for both packed and hash modes.
/// Yields `(ArrayKey, &Value)` without allocating keys for packed arrays.
pub struct PhpArrayIter<'a> {
    inner: PhpArrayIterInner<'a>,
}

enum PhpArrayIterInner<'a> {
    Packed(std::iter::Enumerate<std::slice::Iter<'a, Value>>),
    Small(std::slice::Iter<'a, Option<(ArrayEntryKey, Value)>>),
    Hash(std::slice::Iter<'a, (ArrayEntryKey, Value)>),
}

impl<'a> Iterator for PhpArrayIter<'a> {
    type Item = (ArrayKey, &'a Value);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            PhpArrayIterInner::Packed(iter) => {
                iter.next().map(|(i, v)| (ArrayKey::Int(i as i64), v))
            }
            PhpArrayIterInner::Small(iter) => iter.next().map(|entry| {
                let (key, value) = entry.as_ref().unwrap();
                (key.to_public(), value)
            }),
            PhpArrayIterInner::Hash(iter) => {
                iter.next().map(|(k, v)| (k.to_public(), v))
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.inner {
            PhpArrayIterInner::Packed(iter) => iter.size_hint(),
            PhpArrayIterInner::Small(iter) => iter.size_hint(),
            PhpArrayIterInner::Hash(iter) => iter.size_hint(),
        }
    }
}

impl<'a> ExactSizeIterator for PhpArrayIter<'a> {}

/// Value-only PHP array iterator. Unlike `PhpArrayIter`, this never clones or
/// allocates string keys for hash-backed arrays.
pub struct PhpArrayValues<'a> {
    inner: PhpArrayValuesInner<'a>,
}

enum PhpArrayValuesInner<'a> {
    Packed(std::slice::Iter<'a, Value>),
    Small(std::slice::Iter<'a, Option<(ArrayEntryKey, Value)>>),
    Hash(std::slice::Iter<'a, (ArrayEntryKey, Value)>),
}

impl<'a> Iterator for PhpArrayValues<'a> {
    type Item = &'a Value;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            PhpArrayValuesInner::Packed(iter) => iter.next(),
            PhpArrayValuesInner::Small(iter) => {
                iter.next().map(|entry| &entry.as_ref().unwrap().1)
            }
            PhpArrayValuesInner::Hash(iter) => iter.next().map(|(_, value)| value),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.inner {
            PhpArrayValuesInner::Packed(iter) => iter.size_hint(),
            PhpArrayValuesInner::Small(iter) => iter.size_hint(),
            PhpArrayValuesInner::Hash(iter) => iter.size_hint(),
        }
    }
}

impl DoubleEndedIterator for PhpArrayValues<'_> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            PhpArrayValuesInner::Packed(iter) => iter.next_back(),
            PhpArrayValuesInner::Small(iter) => {
                iter.next_back().map(|entry| &entry.as_ref().unwrap().1)
            }
            PhpArrayValuesInner::Hash(iter) => iter.next_back().map(|(_, value)| value),
        }
    }
}

impl ExactSizeIterator for PhpArrayValues<'_> {}

impl Clone for PhpArray {
    fn clone(&self) -> Self {
        let cloned_storage = match &self.storage {
            ArrayStorage::Packed(values) => ArrayStorage::Packed(values.clone()),
            ArrayStorage::SmallHash(small) => ArrayStorage::SmallHash(small.clone()),
            ArrayStorage::LinearHash(linear) => ArrayStorage::LinearHash(linear.clone()),
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
            ArrayStorage::SmallHash(small) => {
                f.debug_struct("PhpArray")
                    .field("mode", &"small-hash")
                    .field("len", &small.len())
                    .finish()
            }
            ArrayStorage::LinearHash(linear) => {
                f.debug_struct("PhpArray")
                    .field("mode", &"linear-hash")
                    .field("len", &linear.entries.len())
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
    use std::rc::Rc;

    use super::{ArrayEntryKey, ArrayKey, ArrayStorage, PhpArray, Value};

    #[test]
    fn hash_entry_layout_stays_compact() {
        assert_eq!(std::mem::size_of::<ArrayEntryKey>(), 16);
        assert_eq!(std::mem::size_of::<(ArrayEntryKey, Value)>(), 32);
        assert_eq!(
            std::mem::size_of::<Option<(ArrayEntryKey, Value)>>(),
            32
        );
        assert_eq!(std::mem::size_of::<ArrayStorage>(), 104);
        assert_eq!(std::mem::size_of::<PhpArray>(), 112);
    }

    #[test]
    fn hash_entry_and_string_index_share_key_allocation() {
        let mut array = PhpArray::with_hash_capacity(9);
        array.set_str("shared", Value::long(7));

        let ArrayStorage::Hash { entries, str_index, .. } = &array.storage else {
            panic!("string key should select hash storage");
        };
        let ArrayEntryKey::String(entry_key) = &entries[0].0 else {
            panic!("entry should retain its string key");
        };
        let index_key = str_index.keys().next().unwrap();
        assert!(Rc::ptr_eq(&entry_key.0, &index_key.0));
    }

    #[test]
    fn small_hash_promotes_to_linear_before_the_general_index() {
        let mut array = PhpArray::with_deferred_hash_capacity(3);
        array.set_str("a", Value::long(1));
        array.set_int(8, Value::long(2));
        array.set_str("c", Value::long(3));
        assert!(matches!(
            &array.storage,
            ArrayStorage::SmallHash(_)
        ));

        array.set_str("a", Value::long(10));
        assert!(matches!(
            &array.storage,
            ArrayStorage::SmallHash(_)
        ));
        array.set_streamed_owned_str("d".to_string(), Value::long(4));
        assert!(matches!(&array.storage, ArrayStorage::LinearHash(_)));
        array.set_str("e", Value::long(5));
        array.set_str("f", Value::long(6));
        array.set_str("g", Value::long(7));
        array.set_str("h", Value::long(8));
        assert!(matches!(&array.storage, ArrayStorage::LinearHash(_)));
        array.set_streamed_owned_str("d".to_string(), Value::long(40));
        assert!(matches!(&array.storage, ArrayStorage::LinearHash(_)));
        array.set_streamed_owned_str("i".to_string(), Value::long(9));
        assert!(matches!(&array.storage, ArrayStorage::Hash { .. }));
        assert_eq!(array.get_str("a").and_then(Value::as_long), Some(10));
        assert_eq!(array.get_int(8).and_then(Value::as_long), Some(2));
        assert_eq!(array.get_str("c").and_then(Value::as_long), Some(3));
        assert_eq!(array.get_str("d").and_then(Value::as_long), Some(40));
        assert_eq!(
            array
                .iter()
                .map(|(key, _)| key)
                .collect::<Vec<ArrayKey>>(),
            vec![
                ArrayKey::String("a".to_string()),
                ArrayKey::Int(8),
                ArrayKey::String("c".to_string()),
                ArrayKey::String("d".to_string()),
                ArrayKey::String("e".to_string()),
                ArrayKey::String("f".to_string()),
                ArrayKey::String("g".to_string()),
                ArrayKey::String("h".to_string()),
                ArrayKey::String("i".to_string()),
            ]
        );
    }

    #[test]
    fn linear_hash_supports_clone_remove_shift_and_pop() {
        let mut original = PhpArray::with_deferred_hash_capacity(6);
        original.set_str("first", Value::long(1));
        original.set_int(10, Value::long(2));
        original.set_str("third", Value::long(3));
        original.set_int(20, Value::long(4));
        assert!(matches!(&original.storage, ArrayStorage::LinearHash(_)));

        let mut changed = original.clone();
        assert!(changed.remove(&ArrayKey::String("third".to_string())));
        assert_eq!(changed.shift().and_then(|value| value.as_long()), Some(1));
        assert_eq!(changed.pop().and_then(|value| value.as_long()), Some(4));
        assert_eq!(changed.get_int(0).and_then(Value::as_long), Some(2));

        assert_eq!(original.len(), 4);
        assert_eq!(original.get_str("first").and_then(Value::as_long), Some(1));
        assert_eq!(original.get_str("third").and_then(Value::as_long), Some(3));
        assert_eq!(original.get_int(10).and_then(Value::as_long), Some(2));
        assert_eq!(original.get_int(20).and_then(Value::as_long), Some(4));
    }

    #[test]
    fn repeated_linear_string_reads_build_and_mutations_invalidate_the_lazy_index() {
        let mut array = PhpArray::with_deferred_hash_capacity(8);
        for key in ["a", "b", "c", "d", "e", "f", "g", "h"] {
            array.set_str(key, Value::long(1));
        }
        let ArrayStorage::LinearHash(linear) = &array.storage else {
            panic!("eight entries should retain bounded linear storage");
        };
        assert!(linear.str_index.get().is_none());

        for _ in 0..4 {
            assert_eq!(array.get_str("h").and_then(Value::as_long), Some(1));
        }
        let ArrayStorage::LinearHash(linear) = &array.storage else {
            unreachable!();
        };
        assert!(linear.str_index.get().is_some());

        assert!(array.remove(&ArrayKey::String("b".to_string())));
        let ArrayStorage::LinearHash(linear) = &array.storage else {
            unreachable!();
        };
        assert!(linear.str_index.get().is_none());
        assert_eq!(array.get_str("h").and_then(Value::as_long), Some(1));
    }

    #[test]
    fn string_position_hint_validates_layout_changes() {
        let mut array = PhpArray::with_hash_capacity(3);
        array.set_str("first", Value::long(1));
        array.set_str("target", Value::long(2));
        array.set_str("last", Value::long(3));

        let (position, value) = array.get_str_with_position("target").unwrap();
        assert_eq!(position, 1);
        assert_eq!(value.as_long(), Some(2));
        assert_eq!(
            array
                .get_positioned_str("target", position)
                .and_then(Value::as_long),
            Some(2)
        );

        array.remove(&ArrayKey::String("first".to_string()));
        assert!(array.get_positioned_str("target", position).is_none());
        let (new_position, value) = array.get_str_with_position("target").unwrap();
        assert_eq!(new_position, 0);
        assert_eq!(value.as_long(), Some(2));
    }

    #[test]
    fn string_value_key_reuses_source_allocation_and_keeps_cow() {
        let mut key = Value::string("shared");
        let original_ptr = key.string_rc_ptr().unwrap();
        let mut array = PhpArray::with_hash_capacity(1);
        array.set_str_value(&key, Value::long(7));

        let Some((ArrayEntryKey::String(entry_key), _)) = array.hash_entry_at(0) else {
            panic!("entry should retain its string key");
        };
        assert_eq!(Rc::as_ptr(&entry_key.0), original_ptr);

        unsafe { key.as_string_mut().unwrap().push_str("-changed") };
        assert_eq!(key.as_str(), Some("shared-changed"));
        assert_eq!(array.get_str("shared").and_then(Value::as_long), Some(7));
    }

    #[test]
    fn value_iterator_preserves_order_for_packed_and_hash_arrays() {
        let mut packed = PhpArray::new();
        packed.push(Value::long(1));
        packed.push(Value::long(2));
        assert_eq!(
            packed.values().filter_map(Value::as_long).collect::<Vec<_>>(),
            vec![1, 2]
        );

        let mut hash = PhpArray::new();
        hash.set_int(7, Value::long(3));
        hash.set_str("name", Value::long(4));
        hash.set_int(-2, Value::long(5));
        assert_eq!(
            hash.values().filter_map(Value::as_long).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        assert_eq!(hash.values().next_back().and_then(Value::as_long), Some(5));
    }

    #[test]
    fn string_key_detection_uses_array_storage_metadata() {
        let mut packed = PhpArray::new();
        packed.push(Value::long(1));
        assert!(!packed.has_string_keys());

        let mut integer_hash = PhpArray::new();
        integer_hash.set_int(7, Value::long(2));
        assert!(!integer_hash.has_string_keys());

        integer_hash.set_str("name", Value::long(3));
        assert!(integer_hash.has_string_keys());
    }

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
    fn integer_position_hint_accepts_negative_stride_and_rejects_irregular_layout() {
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
    fn integer_position_hint_routes_regular_suffix_after_irregular_prefix() {
        let mut array = PhpArray::new();
        for key in [11, 30, 31, 70, -4, 900, 2, 88] {
            array.set_int(key, Value::long(-1));
        }
        for key in [100, 107, 114, 121, 128, 135, 142, 149] {
            array.set_int(key, Value::long(key));
        }

        // The regular suffix starts at entry position 8, so its virtual key
        // at entry position 0 is 100 - 8 * 7 = 44.
        assert_eq!(array.integer_position_hint(), Some((44, 7)));
        assert_eq!(
            array
                .get_positioned_int(100, 44, 7)
                .and_then(Value::as_long),
            Some(100)
        );
        assert_eq!(
            array
                .get_positioned_int(149, 44, 7)
                .and_then(Value::as_long),
            Some(149)
        );
        assert_eq!(
            array
                .get_positioned_int(30, 44, 7)
                .and_then(Value::as_long),
            Some(-1)
        );

        array.set_int(9_999, Value::long(-1));
        assert_eq!(array.integer_position_hint(), None);
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

    /// Read the shared property-layout identity without a `RefCell` borrow.
    /// SAFETY: Only valid when `value_type() == ValueType::Object`.
    #[inline(always)]
    pub unsafe fn object_property_layout_ptr_unchecked(&self) -> *const ObjectLayout {
        debug_assert!(self.value_type() == ValueType::Object);
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        (*refcell.as_ptr()).property_layout_ptr()
    }

    /// Check the canonical dynamic `stdClass` receiver shape without a
    /// `RefCell` borrow. The single-threaded dispatch loop guarantees that the
    /// object metadata cannot change concurrently.
    /// SAFETY: Only valid when `value_type() == ValueType::Object`.
    #[inline(always)]
    pub unsafe fn object_is_dynamic_std_class_unchecked(&self) -> bool {
        debug_assert!(self.value_type() == ValueType::Object);
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        (*refcell.as_ptr()).is_dynamic_std_class()
    }

    /// Read directly from the dynamic property map of a receiver already
    /// guarded as canonical `stdClass`.
    /// SAFETY: The receiver must pass `object_is_dynamic_std_class_unchecked`;
    /// the returned pointer is invalidated by a mutable property operation.
    #[inline(always)]
    pub unsafe fn object_dynamic_property_unchecked(&self, name: &str) -> *const Value {
        debug_assert!(self.object_is_dynamic_std_class_unchecked());
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        let obj = &*refcell.as_ptr();
        obj.dynamic_properties
            .as_ref()
            .and_then(|properties| properties.get(name))
            .map_or(std::ptr::null(), |value| value as *const Value)
    }

    /// Validate and read a cached inline dynamic-property position. A null
    /// result means the current receiver has a different insertion order.
    /// SAFETY: The receiver must pass the canonical stdClass shape guard.
    #[inline(always)]
    pub unsafe fn object_dynamic_property_at_unchecked(
        &self,
        name: &str,
        position: usize,
    ) -> *const Value {
        debug_assert!(self.object_is_dynamic_std_class_unchecked());
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        let obj = &*refcell.as_ptr();
        obj.dynamic_properties
            .as_ref()
            .and_then(|properties| properties.get_at_position(position, name))
            .map_or(std::ptr::null(), |value| value as *const Value)
    }

    /// Guard a dynamic receiver layout and resolve two property reads through
    /// one object dereference and one dynamic-storage dispatch. Individual
    /// null pointers preserve the exact side-exit point for a missing value.
    /// SAFETY: Only valid when `value_type() == ValueType::Object`; returned
    /// pointers are invalidated by a mutable property operation.
    #[inline(always)]
    pub unsafe fn object_dynamic_property_pair_guarded_unchecked(
        &self,
        expected_layout: *const ObjectLayout,
        names: [&str; 2],
        positions: [Option<usize>; 2],
    ) -> Option<[*const Value; 2]> {
        debug_assert!(self.value_type() == ValueType::Object);
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        let obj = &*refcell.as_ptr();
        if obj.property_layout_ptr() != expected_layout {
            return None;
        }
        Some(
            obj.dynamic_properties
                .as_ref()
                .map_or([std::ptr::null(); 2], |properties| {
                    properties.get_pair_at_positions(names, positions)
                }),
        )
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

    /// Raw identity of the Rc-backed string. Dynamic callback inline caches
    /// retain this identity so later in-place mutations detach through COW.
    #[inline(always)]
    pub(crate) fn string_rc_ptr(&self) -> Option<*const String> {
        if self.value_type() == ValueType::String {
            Some(unsafe { self.data.ptr as *const String })
        } else {
            None
        }
    }

    /// Add/remove the strong reference owned by a dynamic callback cache.
    /// String retention is PHP-observable only as memory lifetime: unlike
    /// objects, strings have no destructor side effects.
    #[inline]
    pub(crate) unsafe fn retain_cached_string(ptr: *const String) {
        Rc::increment_strong_count(ptr);
    }

    #[inline]
    pub(crate) unsafe fn release_cached_string(ptr: *const String) {
        Rc::decrement_strong_count(ptr);
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

    /// Get a mutable string only when this Value is its sole COW owner.
    /// Guarded regions use this before retaining a raw string pointer; shared
    /// strings fall back so the canonical opcode performs the detach.
    #[inline]
    pub(crate) fn as_string_mut_if_unique(&mut self) -> Option<&mut String> {
        if self.value_type() != ValueType::String {
            return None;
        }
        unsafe {
            let rc_ptr = self.data.ptr as *mut String;
            let rc = std::mem::ManuallyDrop::new(Rc::from_raw(rc_ptr));
            (Rc::strong_count(&rc) == 1).then(|| &mut *rc_ptr)
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

    /// Get a mutable array only when this Value is its sole COW owner.
    ///
    /// Guarded execution regions use this before retaining a raw array pointer:
    /// a shared array must fall back so the canonical opcode performs the PHP
    /// copy-on-write detach at the first mutation.
    #[inline]
    pub(crate) fn as_array_mut_if_unique(&mut self) -> Option<&mut PhpArray> {
        if self.value_type() != ValueType::Array {
            return None;
        }
        unsafe {
            let rc_ptr = self.data.ptr as *mut PhpArray;
            let rc = std::mem::ManuallyDrop::new(Rc::from_raw(rc_ptr));
            (Rc::strong_count(&rc) == 1).then(|| &mut *rc_ptr)
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

    /// Read the raw f64 without a type check. SAFETY: caller must guarantee
    /// that this value has the Double tag.
    #[inline(always)]
    pub unsafe fn raw_double(&self) -> f64 {
        self.data.double
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

    /// Append the PHP echo representation directly to an existing buffer.
    /// This avoids allocating a temporary String in join/formatting paths.
    pub fn append_echo_to(&self, output: &mut String) {
        match self.value_type() {
            ValueType::Undef | ValueType::Null | ValueType::False => {}
            ValueType::True => output.push('1'),
            ValueType::Long => {
                let _ = write!(output, "{}", unsafe { self.data.long });
            }
            ValueType::Double => {
                let d = unsafe { self.data.double };
                if d == d.floor() && d.abs() < 1e15 {
                    let _ = write!(output, "{}", d as i64);
                } else {
                    let _ = write!(output, "{}", d);
                }
            }
            ValueType::String => {
                output.push_str(unsafe { &*(self.data.ptr as *const String) });
            }
            ValueType::Array => output.push_str("Array"),
            ValueType::Object => {
                let refcell = unsafe { &*(self.data.ptr as *const RefCell<PhpObject>) };
                let obj = refcell.borrow();
                let _ = write!(output, "{} Object", obj.class_name);
            }
            _ => output.push_str("<unsupported>"),
        }
    }

    /// Capacity hint for append_echo_to. Exact for fixed/string cases and a
    /// small safe overestimate for numeric values.
    pub fn echo_len_hint(&self) -> usize {
        match self.value_type() {
            ValueType::Undef | ValueType::Null | ValueType::False => 0,
            ValueType::True => 1,
            ValueType::Long => 20,
            ValueType::Double => 24,
            ValueType::String => unsafe { (&*(self.data.ptr as *const String)).len() },
            ValueType::Array => 5,
            ValueType::Object => {
                let refcell = unsafe { &*(self.data.ptr as *const RefCell<PhpObject>) };
                refcell.borrow().class_name.len() + 7
            }
            _ => 13,
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
