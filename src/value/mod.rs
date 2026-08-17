use std::borrow::{Borrow, Cow};
use std::cell::{Cell, OnceCell, RefCell, UnsafeCell};
use std::collections::{HashMap, hash_map::Entry};
use std::fmt::Write as _;
use std::hash::{BuildHasherDefault, Hasher};
use std::marker::PhantomData;
use std::ops::Deref;
use std::rc::Rc;

#[cold]
#[inline(never)]
pub(crate) fn php_byte_string_bytes(value: &str) -> Vec<u8> {
    value.chars().map(|character| character as u8).collect()
}

#[cold]
#[inline(never)]
pub(crate) fn php_byte_string_from_bytes(bytes: impl IntoIterator<Item = u8>) -> String {
    bytes.into_iter().map(char::from).collect()
}

#[cold]
#[inline(never)]
pub(crate) fn php_byte_string_binary(
    left: &str,
    right: &str,
    operation: fn(u8, u8) -> u8,
    preserve_longer_tail: bool,
) -> String {
    let left = php_byte_string_bytes(left);
    let right = php_byte_string_bytes(right);
    let common = left.len().min(right.len());
    let capacity = if preserve_longer_tail {
        left.len().max(right.len())
    } else {
        common
    };
    let mut result = Vec::with_capacity(capacity);
    result.extend(
        left[..common]
            .iter()
            .zip(&right[..common])
            .map(|(&left, &right)| operation(left, right)),
    );
    if preserve_longer_tail {
        if left.len() > common {
            result.extend_from_slice(&left[common..]);
        } else {
            result.extend_from_slice(&right[common..]);
        }
    }
    php_byte_string_from_bytes(result)
}

#[cfg(feature = "resource-lifetime")]
use crate::resource_handle::ResourceHandle;
use crate::vm::function::{FunctionCommon, FunctionType, ParamTypeHint, UserFunction};
use crate::vm::generator::GeneratorRef;
use crate::vm::stats;

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
        self.entries
            .iter()
            .take_while(|entry| entry.is_some())
            .count()
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
                result[index] = self.find(keys[index]).map_or(std::ptr::null(), |position| {
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
pub struct DynamicPropertyMap {
    storage: DynamicPropertyStorage,
    /// Magic-property recursion guards share this already-cold allocation.
    /// The map itself may contain no user-visible properties while a guard is
    /// active and is released again when the last guard leaves.
    property_guards: Option<Box<HashMap<String, u8>>>,
}

impl Clone for DynamicPropertyMap {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            // A cloned PHP object starts outside any magic operation even if
            // cloning was requested from inside a getter or setter.
            property_guards: None,
        }
    }
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
        Self {
            storage,
            property_guards: None,
        }
    }

    fn from_hash_map(properties: HashMap<String, Value>) -> Self {
        if properties.len() > LINEAR_DYNAMIC_PROPERTY_CAPACITY {
            return Self {
                storage: DynamicPropertyStorage::Indexed(IndexedDynamicProperties::from_hash_map(
                    properties,
                )),
                property_guards: None,
            };
        }
        let mut result = Self::with_capacity(properties.len());
        for (key, value) in properties {
            result.insert_owned(key, value);
        }
        result
    }

    fn clone_for_php_object(&self) -> Self {
        let mut clone = Self::with_capacity(self.len());
        self.for_each(|name, value| {
            let value = if value.is_owned_reference() && value.owned_reference_is_aliased() {
                value.clone_owned_reference_alias()
            } else {
                value.clone()
            };
            clone.insert_owned(name.to_string(), value);
        });
        clone
    }

    #[inline]
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        match &self.storage {
            DynamicPropertyStorage::Small(small) => small
                .find(key)
                .and_then(|position| small.entries[position].as_ref().map(|entry| &entry.1)),
            DynamicPropertyStorage::Linear(linear) => {
                linear.find(key).map(|position| &linear.entries[position].1)
            }
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
                    result[index] = value.map_or(std::ptr::null(), |value| value as *const Value);
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

    pub(crate) fn remove(&mut self, key: &str) -> bool {
        match &mut self.storage {
            DynamicPropertyStorage::Small(small) => {
                let Some(position) = small.find(key) else {
                    return false;
                };
                let len = small.len();
                for index in position..len - 1 {
                    small.entries[index] = small.entries[index + 1].take();
                }
                small.entries[len - 1] = None;
            }
            DynamicPropertyStorage::Linear(linear) => {
                let Some(position) = linear.find(key) else {
                    return false;
                };
                linear.entries.remove(position);
            }
            DynamicPropertyStorage::Indexed(indexed) => {
                let Some(position) = indexed.find(key) else {
                    return false;
                };
                indexed.entries.remove(position);
                indexed.index.remove(key);
                for new_position in position..indexed.entries.len() {
                    indexed
                        .index
                        .insert(indexed.entries[new_position].0.clone(), new_position);
                }
            }
        }
        true
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

    #[inline]
    fn property_guard_active(&self, key: &str, operation: u8) -> bool {
        self.property_guards
            .as_ref()
            .and_then(|guards| guards.get(key))
            .is_some_and(|guard| guard & operation != 0)
    }

    #[inline]
    fn set_property_guard(&mut self, key: &str, operation: u8, active: bool) {
        if active {
            let guards = self
                .property_guards
                .get_or_insert_with(|| Box::new(HashMap::new()));
            *guards.entry(key.to_string()).or_insert(0) |= operation;
            return;
        }

        let Some(guards) = self.property_guards.as_mut() else {
            return;
        };
        let mut remove = false;
        if let Some(guard) = guards.get_mut(key) {
            *guard &= !operation;
            remove = *guard == 0;
        }
        if remove {
            guards.remove(key);
        }
        if guards.is_empty() {
            self.property_guards = None;
        }
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
    /// Low bits hold the request-local Zend object-store handle; the high bit
    /// records that this allocation has entered its destructor. Packing both
    /// lifecycle values here preserves the 72-byte PhpObject layout.
    pub(crate) lifecycle: u32,
    /// Shared name → slot mapping owned by the class definition.
    pub property_layout: Rc<ObjectLayout>,
    /// Declared properties in compact numeric slots.
    pub property_values: Vec<Value>,
    /// Dynamic properties are uncommon and allocated lazily.
    pub dynamic_properties: Option<Box<DynamicPropertyMap>>,
    /// If this object is a Generator, holds the generator state
    pub generator: Option<GeneratorRef>,
}

#[inline]
fn instance_property_reference_owner(handle: u32, slot: usize) -> usize {
    debug_assert!(handle != 0);
    debug_assert!(slot <= u32::MAX as usize);
    ((handle as usize) << 32) | slot
}

#[cfg(target_pointer_width = "64")]
const _: [(); 72] = [(); std::mem::size_of::<PhpObject>()];

thread_local! {
    /// Every decoded JSON object is the same dynamic `stdClass`. Sharing its
    /// immutable name and empty declared-property layout removes two heap
    /// allocations per object while keeping dynamic properties per instance.
    static STD_CLASS_METADATA: (Rc<str>, Rc<ObjectLayout>) = (
        Rc::from("stdClass"),
        Rc::new(ObjectLayout::empty()),
    );

    /// One bounded thread-local declared-property buffer per common width.
    /// PHP evaluates a replacement object before releasing the previous CV, so
    /// one slot per width still covers the steady state after two allocations.
    static DECLARED_PROPERTY_STORAGE_POOL: RefCell<[Option<Vec<Value>>; 6]> =
        RefCell::new(std::array::from_fn(|_| None));
    /// Zend exposes a request-local object-store handle in var_dump(),
    /// spl_object_id() and related diagnostics. Keep that compatibility state
    /// beside the Rc allocation rather than enlarging every PhpObject.
    static OBJECT_HANDLES: std::cell::UnsafeCell<ObjectHandleState> =
        std::cell::UnsafeCell::new(ObjectHandleState::default());
}

#[derive(Default)]
struct ObjectHandleState {
    next: u32,
    released: Vec<u32>,
    before_request: Vec<usize>,
    stale: Vec<usize>,
    in_request: bool,
}

impl ObjectHandleState {
    fn allocate(&mut self) -> u32 {
        let handle = self.released.pop().unwrap_or_else(|| {
            let handle = if self.next == 0 { 1 } else { self.next };
            self.next = handle
                .checked_add(1)
                .expect("PHP object handle space exhausted");
            handle
        });
        assert!(
            handle <= OBJECT_HANDLE_MASK,
            "PHP object handle space exhausted"
        );
        handle
    }

    fn register_identity(&mut self, identity: usize) {
        if !self.in_request {
            self.before_request.push(identity);
        }
    }

    fn release(&mut self, identity: usize, handle: u32) {
        if let Some(position) = self
            .stale
            .iter()
            .position(|candidate| *candidate == identity)
        {
            self.stale.swap_remove(position);
            return;
        }
        if let Some(position) = self
            .before_request
            .iter()
            .position(|candidate| *candidate == identity)
        {
            self.before_request.swap_remove(position);
        }
        self.released.push(handle);
    }
}

const OBJECT_DESTRUCTOR_RAN: u32 = 1 << 31;
const OBJECT_HANDLE_MASK: u32 = !OBJECT_DESTRUCTOR_RAN;

fn with_object_handles<T>(callback: impl FnOnce(&mut ObjectHandleState) -> T) -> T {
    OBJECT_HANDLES.with(|state| {
        // This state is thread-local, and none of its operations invokes PHP
        // code or otherwise re-enters object allocation/drop.
        callback(unsafe { &mut *state.get() })
    })
}

fn allocate_object_handle() -> (u32, bool) {
    with_object_handles(|state| (state.allocate(), state.in_request))
}

fn register_object_identity(identity: usize) {
    with_object_handles(|state| state.register_identity(identity));
}

fn release_object_handle(identity: usize, handle: u32) {
    with_object_handles(|state| state.release(identity, handle));
}

/// Start a fresh request numbering sequence once every owner from the prior
/// request has gone away. A still-live object means the caller intentionally
/// reuses one ExecutorGlobals and its request-local state.
pub(crate) fn begin_object_handle_request() {
    with_object_handles(|state| {
        let before_request = std::mem::take(&mut state.before_request);
        state.stale.extend(before_request);
        state.next = 1;
        state.released.clear();
        state.in_request = true;
    });
}

pub(crate) fn end_object_handle_request() {
    with_object_handles(|state| state.in_request = false);
}

const MAX_POOLED_DECLARED_PROPERTIES: usize = 5;

fn materialize_declared_property_defaults(defaults: &[Value]) -> Vec<Value> {
    if defaults.is_empty() {
        return Vec::new();
    }

    let pooled = (defaults.len() <= MAX_POOLED_DECLARED_PROPERTIES)
        .then(|| {
            DECLARED_PROPERTY_STORAGE_POOL.with(|pool| pool.borrow_mut()[defaults.len()].take())
        })
        .flatten();
    let reused = pooled.is_some();
    let mut values = pooled.unwrap_or_else(|| {
        stats::inc_declared_property_storage_allocation();
        Vec::with_capacity(defaults.len())
    });
    if values.capacity() < defaults.len() {
        stats::inc_declared_property_storage_allocation();
        values.reserve_exact(defaults.len() - values.capacity());
    } else if !values.is_empty() {
        unreachable!("pooled declared-property storage must be cleared before reuse");
    } else if reused {
        stats::inc_declared_property_storage_reuse();
    }
    values.extend(defaults.iter().cloned());
    values
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
            lifecycle: 0,
            property_layout,
            property_values,
            dynamic_properties: None,
            generator: None,
        }
    }

    /// Materialize immutable class defaults into object-owned slots, reusing
    /// one bounded buffer for the common small declared-object widths.
    pub(crate) fn with_layout_from_defaults(
        class_id: u32,
        property_layout: Rc<ObjectLayout>,
        property_defaults: &[Value],
    ) -> Self {
        Self::with_layout(
            class_id,
            property_layout,
            materialize_declared_property_defaults(property_defaults),
        )
    }

    pub fn dynamic(class_name: String, class_id: u32, properties: HashMap<String, Value>) -> Self {
        Self {
            class_name: Rc::from(class_name),
            class_id,
            lifecycle: 0,
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
        let (class_name, property_layout) =
            STD_CLASS_METADATA.with(|metadata| (Rc::clone(&metadata.0), Rc::clone(&metadata.1)));
        Self {
            class_name,
            class_id: 0,
            lifecycle: 0,
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

    #[inline(always)]
    pub(crate) fn property_name_at_slot(&self, slot: usize) -> Option<&str> {
        self.property_layout.key(slot)
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
    pub(crate) fn get_dynamic_property_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.dynamic_properties.as_mut()?.get_mut(key)
    }

    #[inline]
    pub(crate) fn set_dynamic_property(&mut self, key: &str, value: Value) {
        self.dynamic_properties
            .get_or_insert_with(|| Box::new(DynamicPropertyMap::with_capacity(1)))
            .insert(key, value);
    }

    #[inline]
    pub(crate) fn remove_dynamic_property(&mut self, key: &str) -> bool {
        self.dynamic_properties
            .as_mut()
            .is_some_and(|properties| properties.remove(key))
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

    #[inline]
    pub(crate) fn property_guard_active(&self, key: &str, operation: u8) -> bool {
        self.dynamic_properties
            .as_ref()
            .is_some_and(|properties| properties.property_guard_active(key, operation))
    }

    #[inline]
    pub(crate) fn set_property_guard(&mut self, key: &str, operation: u8, active: bool) {
        if active {
            self.dynamic_properties
                .get_or_insert_with(|| Box::new(DynamicPropertyMap::with_capacity(0)))
                .set_property_guard(key, operation, true);
            return;
        }

        let Some(properties) = self.dynamic_properties.as_mut() else {
            return;
        };
        properties.set_property_guard(key, operation, false);
        if properties.is_empty() && properties.property_guards.is_none() {
            self.dynamic_properties = None;
        }
    }

    /// Set a declared slot or create/update a dynamic property.
    /// Returns the declared slot when one exists.
    #[inline]
    pub fn set_property(&mut self, key: &str, value: Value) -> Option<usize> {
        if let Some(slot) = self.property_layout.slot(key) {
            let handle = self.lifecycle & OBJECT_HANDLE_MASK;
            if handle != 0 {
                self.property_values[slot].remove_reference_property_constraint(
                    instance_property_reference_owner(handle, slot),
                );
            }
            self.property_values[slot] = value;
            Some(slot)
        } else {
            self.dynamic_properties
                .get_or_insert_with(|| Box::new(DynamicPropertyMap::with_capacity(1)))
                .insert(key, value);
            None
        }
    }

    /// Unset a declared property or remove a dynamic property.
    pub fn unset_property(&mut self, key: &str) -> bool {
        if let Some(slot) = self.property_layout.slot(key) {
            let handle = self.lifecycle & OBJECT_HANDLE_MASK;
            if handle != 0 {
                self.property_values[slot].remove_reference_property_constraint(
                    instance_property_reference_owner(handle, slot),
                );
            }
            self.property_values[slot] = Value::undef();
            true
        } else {
            self.dynamic_properties
                .as_mut()
                .is_some_and(|properties| properties.remove(key))
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

    pub fn for_each_dynamic_property(&self, visitor: impl FnMut(&str, &Value)) {
        if let Some(dynamic) = &self.dynamic_properties {
            dynamic.for_each(visitor);
        }
    }

    pub(crate) fn clone_for_php(&self) -> Self {
        Self {
            class_name: self.class_name.clone(),
            class_id: self.class_id,
            lifecycle: 0,
            property_layout: self.property_layout.clone(),
            property_values: self
                .property_values
                .iter()
                .map(|value| {
                    if value.is_owned_reference() && value.owned_reference_is_aliased() {
                        value.clone_owned_reference_alias()
                    } else {
                        value.clone()
                    }
                })
                .collect(),
            dynamic_properties: self
                .dynamic_properties
                .as_ref()
                .map(|properties| Box::new(properties.clone_for_php_object())),
            generator: None,
        }
    }

    #[inline]
    pub(crate) fn instance_property_reference_owner(&self, slot: usize) -> usize {
        instance_property_reference_owner(self.lifecycle & OBJECT_HANDLE_MASK, slot)
    }
}

impl Drop for PhpObject {
    fn drop(&mut self) {
        let handle = self.lifecycle & OBJECT_HANDLE_MASK;
        if handle != 0 {
            for (slot, value) in self.property_values.iter().enumerate() {
                value.remove_reference_property_constraint(instance_property_reference_owner(
                    handle, slot,
                ));
            }
        }
        let width = self.property_values.len();
        if width == 0 || width > MAX_POOLED_DECLARED_PROPERTIES {
            return;
        }

        let mut values = std::mem::take(&mut self.property_values);
        // Drop heap-bearing property values before borrowing the pool: a
        // nested final object release may itself return a declared buffer.
        values.clear();
        // Retain only the exact small buffer created by the declared-default
        // materializer. Other internal constructors may supply a wider Vec;
        // keeping it would make thread-local retention unbounded.
        if values.capacity() != width {
            return;
        }
        let mut available = Some(values);
        let retained = DECLARED_PROPERTY_STORAGE_POOL
            .try_with(|pool| {
                let mut pool = pool.borrow_mut();
                if pool[width].is_none() {
                    pool[width] = available.take();
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if retained {
            stats::inc_declared_property_storage_return();
        }
    }
}

#[cfg(test)]
mod declared_property_storage_tests {
    use super::{ObjectLayout, PhpObject, Value};
    use std::rc::Rc;

    #[test]
    fn pooled_declared_storage_is_cleared_before_reuse() {
        let layout = Rc::new(ObjectLayout::new(
            "PooledRow",
            vec!["first".to_string(), "second".to_string()],
        ));
        let first = PhpObject::with_layout_from_defaults(
            1,
            Rc::clone(&layout),
            &[Value::long(1), Value::long(2)],
        );
        drop(first);

        let second =
            PhpObject::with_layout_from_defaults(1, layout, &[Value::long(7), Value::long(8)]);
        assert_eq!(
            second.get_property_slot(0).and_then(Value::as_long),
            Some(7)
        );
        assert_eq!(
            second.get_property_slot(1).and_then(Value::as_long),
            Some(8)
        );
    }

    #[test]
    fn nested_heap_properties_drop_before_the_buffer_is_returned() {
        let child_layout = Rc::new(ObjectLayout::new("Child", vec!["value".to_string()]));
        let parent_layout = Rc::new(ObjectLayout::new("Parent", vec!["child".to_string()]));
        let child = Value::object(PhpObject::with_layout_from_defaults(
            2,
            child_layout,
            &[Value::long(3)],
        ));
        let parent = PhpObject::with_layout_from_defaults(1, Rc::clone(&parent_layout), &[child]);
        drop(parent);

        let reused = PhpObject::with_layout_from_defaults(1, parent_layout, &[Value::long(9)]);
        assert_eq!(
            reused.get_property_slot(0).and_then(Value::as_long),
            Some(9)
        );
    }

    #[test]
    fn oversized_internal_capacity_is_not_retained() {
        let layout = Rc::new(ObjectLayout::new(
            "WideCapacity",
            vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
            ],
        ));
        let mut oversized = Vec::with_capacity(64);
        oversized.extend([
            Value::long(1),
            Value::long(2),
            Value::long(3),
            Value::long(4),
        ]);
        drop(PhpObject::with_layout(1, Rc::clone(&layout), oversized));

        let ordinary = PhpObject::with_layout_from_defaults(
            1,
            layout,
            &[
                Value::long(5),
                Value::long(6),
                Value::long(7),
                Value::long(8),
            ],
        );
        assert_eq!(ordinary.property_values.capacity(), 4);
    }
}

#[cfg(test)]
#[path = "object_tests.rs"]
mod object_tests;

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
    cursor: Cell<usize>,
}

/// Fast deterministic hashing for integer-only PHP array keys.
///
/// `std::HashMap` otherwise uses the DOS-resistant general-purpose string
/// hasher for every integer lookup. Odd multiplicative mixing is bijective over
/// `u64`; the folded high half diffuses high-bit differences into bucket-index
/// bits without paying for a byte-oriented hash. String keys keep the
/// randomized default hasher.
#[derive(Default)]
struct IntKeyHasher {
    hash: u64,
}

impl IntKeyHasher {
    #[inline(always)]
    fn mix(value: u64) -> u64 {
        let mixed = value.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        mixed ^ (mixed >> 32)
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

/// Canonical integer-index payload. Ordinary entries store only their ordered
/// position. When both the position and a Long payload fit in 24/39 bits, the
/// same machine word also carries the Long value used by typed read regions.
/// This preserves the compact HashMap bucket while removing the dependent
/// ordered-entry load for the common small-integer case.
#[derive(Clone, Copy)]
struct IntIndexValue(usize);

impl IntIndexValue {
    const CACHED_LONG_BIT: usize = 1usize << (usize::BITS - 1);
    const POSITION_BITS: u32 = 24;
    const LONG_BITS: u32 = usize::BITS - 1 - Self::POSITION_BITS;
    const POSITION_SHIFT: u32 = Self::LONG_BITS;
    const POSITION_MASK: usize = (1usize << Self::POSITION_BITS) - 1;
    const LONG_MASK: usize = (1usize << Self::LONG_BITS) - 1;
    const LONG_MIN: i64 = -(1i64 << (Self::LONG_BITS - 1));
    const LONG_MAX: i64 = (1i64 << (Self::LONG_BITS - 1)) - 1;

    #[inline(always)]
    fn new(position: usize, value: &Value) -> Self {
        let Some(long) = value.as_long() else {
            return Self(position);
        };
        if !(Self::LONG_MIN..=Self::LONG_MAX).contains(&long) || position > Self::POSITION_MASK {
            return Self(position);
        }
        Self(
            Self::CACHED_LONG_BIT
                | (position << Self::POSITION_SHIFT)
                | (long as usize & Self::LONG_MASK),
        )
    }

    #[inline(always)]
    fn position(self) -> usize {
        if self.0 & Self::CACHED_LONG_BIT != 0 {
            (self.0 >> Self::POSITION_SHIFT) & Self::POSITION_MASK
        } else {
            self.0
        }
    }

    #[inline(always)]
    #[cfg(any(feature = "quick-loops", test))]
    fn cached_long(self) -> Option<i64> {
        (self.0 & Self::CACHED_LONG_BIT != 0).then(|| {
            let value = (self.0 & Self::LONG_MASK) as i64;
            let shift = i64::BITS - Self::LONG_BITS;
            (value << shift) >> shift
        })
    }

    #[inline(always)]
    fn clear_cached_long(&mut self) {
        self.0 = self.position();
    }
}

type IntIndex = HashMap<i64, IntIndexValue, BuildHasherDefault<IntKeyHasher>>;

/// Stable read-only view used by native guarded regions for typed integer
/// lookups. The owning `PhpArray` remains alive and immutable for the whole
/// region, so neither allocation can move while generated code calls the
/// lookup helper.
#[cfg(any(test, all(feature = "quick-loops", feature = "jit-prototype")))]
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct NativeIndexedLongLookupContext {
    int_index: *const IntIndex,
    entries: *const (ArrayEntryKey, Value),
    entries_len: usize,
}

/// Exact native-call boundary for an indexed Long read. A zero return asks
/// the caller to side-exit without modifying the destination slot.
#[cfg(any(test, all(feature = "quick-loops", feature = "jit-prototype")))]
#[inline(never)]
pub(crate) unsafe extern "C" fn native_indexed_long_lookup(
    context: *const NativeIndexedLongLookupContext,
    key: i64,
    output: *mut i64,
) -> u32 {
    let Some(context) = context.as_ref() else {
        return 0;
    };
    let Some(int_index) = context.int_index.as_ref() else {
        return 0;
    };
    let Some(indexed) = int_index.get(&key).copied() else {
        return 0;
    };
    let value = match indexed.cached_long() {
        Some(value) => value,
        None => {
            if indexed.position() >= context.entries_len || context.entries.is_null() {
                return 0;
            }
            let entry = &*context.entries.add(indexed.position());
            let Some(value) = entry.1.as_long() else {
                return 0;
            };
            value
        }
    };
    let Some(output) = output.as_mut() else {
        return 0;
    };
    *output = value;
    1
}

/// Mutable state retained by one native structural integer-write context.
/// Reservation is attempted lazily so already-hot code can receive a fresh
/// small array on a later invocation without losing its capacity hint.
#[cfg(any(test, all(feature = "quick-loops", feature = "jit-prototype")))]
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct NativeLongArraySetContext {
    array: *mut PhpArray,
    reserve_remaining: usize,
    reserve_checks: u8,
}

#[cfg(any(test, all(feature = "quick-loops", feature = "jit-prototype")))]
impl NativeLongArraySetContext {
    pub(crate) fn new(array: *mut PhpArray, reserve_remaining: usize) -> Self {
        Self {
            array,
            reserve_remaining,
            reserve_checks: u8::from(reserve_remaining != 0) * 8,
        }
    }
}

/// Exact native-call boundary for a structural integer-key Long write. The
/// mutable array is resolved through the normal unique-COW guard before the
/// native region starts; the helper deliberately keeps `set_int` as the one
/// canonical implementation of storage promotion, growth and replacement.
#[cfg(any(test, all(feature = "quick-loops", feature = "jit-prototype")))]
#[inline(never)]
pub(crate) unsafe extern "C" fn native_long_array_set(
    array: *mut PhpArray,
    key: i64,
    value: i64,
) -> u32 {
    let Some(array) = array.as_mut() else {
        return 0;
    };
    array.set_int(key, Value::long(value));
    1
}

/// Deferred structural write used only when an already-compiled loop receives
/// a fresh array that has not reached general Hash storage yet.
#[cfg(any(test, all(feature = "quick-loops", feature = "jit-prototype")))]
#[inline(never)]
pub(crate) unsafe extern "C" fn native_long_array_set_deferred(
    context: *mut NativeLongArraySetContext,
    key: i64,
    value: i64,
) -> u32 {
    let Some(context) = context.as_mut() else {
        return 0;
    };
    let Some(array) = context.array.as_mut() else {
        return 0;
    };
    if context.reserve_remaining != 0
        && (array.indexed_int_write_reservation_is_unneeded(key)
            || array.reserve_indexed_int_writes(context.reserve_remaining))
    {
        context.reserve_remaining = 0;
    }
    array.set_int(key, Value::long(value));
    if context.reserve_remaining != 0 {
        if array.indexed_int_write_reservation_is_unneeded(key)
            || array.reserve_indexed_int_writes(context.reserve_remaining)
        {
            context.reserve_remaining = 0;
        } else {
            context.reserve_checks = context.reserve_checks.saturating_sub(1);
            if context.reserve_checks == 0 {
                context.reserve_remaining = 0;
            }
        }
    }
    1
}

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
        /// Exact arithmetic integer prefix represented without `int_index`.
        verified_int_prefix: usize,
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

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
pub(crate) struct ExactOrderedIntLayout {
    first_value: std::ptr::NonNull<Value>,
    run_end: usize,
    position_zero_key: i64,
}

fn verified_int_prefix_len(entries: &[(ArrayEntryKey, Value)]) -> usize {
    let Some((ArrayEntryKey::Int(first), _)) = entries.first() else {
        return 0;
    };
    let Some((ArrayEntryKey::Int(second), _)) = entries.get(1) else {
        return 1;
    };
    let Some(stride) = second.checked_sub(*first).filter(|stride| *stride != 0) else {
        return 1;
    };
    let mut len = 1;
    while let Some((ArrayEntryKey::Int(key), _)) = entries.get(len) {
        let expected = i64::try_from(len)
            .ok()
            .and_then(|offset| stride.checked_mul(offset))
            .and_then(|offset| first.checked_add(offset));
        if expected != Some(*key) {
            break;
        }
        len += 1;
    }
    len
}

#[inline]
fn verified_int_position(
    entries: &[(ArrayEntryKey, Value)],
    verified_int_prefix: usize,
    key: i64,
) -> Option<usize> {
    if verified_int_prefix == 0 {
        return None;
    }
    let (ArrayEntryKey::Int(first), _) = entries.first()? else {
        return None;
    };
    let position = if verified_int_prefix == 1 {
        (*first == key).then_some(0)?
    } else {
        let (ArrayEntryKey::Int(second), _) = entries.get(1)? else {
            return None;
        };
        let stride = second.checked_sub(*first).filter(|stride| *stride != 0)?;
        let offset = key.checked_sub(*first)?;
        if offset.checked_rem(stride) != Some(0) {
            return None;
        }
        usize::try_from(offset.checked_div(stride)?).ok()?
    };
    if position >= verified_int_prefix {
        return None;
    }
    matches!(entries.get(position), Some((ArrayEntryKey::Int(found), _)) if *found == key)
        .then_some(position)
}

fn rebuild_int_index(
    entries: &[(ArrayEntryKey, Value)],
    int_index: &mut IntIndex,
    additional_capacity: usize,
) -> usize {
    int_index.clear();
    let verified_int_prefix = verified_int_prefix_len(entries);
    if entries[verified_int_prefix..]
        .iter()
        .all(|entry| matches!(entry.0, ArrayEntryKey::String(_)))
    {
        return verified_int_prefix;
    }

    materialize_int_index(entries, int_index, additional_capacity);
    0
}

fn materialize_int_index(
    entries: &[(ArrayEntryKey, Value)],
    int_index: &mut IntIndex,
    additional_capacity: usize,
) {
    int_index.clear();
    let integer_keys = entries
        .iter()
        .filter(|entry| matches!(entry.0, ArrayEntryKey::Int(_)))
        .count();
    int_index.reserve(integer_keys.saturating_add(additional_capacity));
    for (position, (key, value)) in entries.iter().enumerate() {
        if let ArrayEntryKey::Int(key) = key {
            int_index.insert(*key, IntIndexValue::new(position, value));
        }
    }
}

#[inline]
fn indexed_int_position(
    entries: &[(ArrayEntryKey, Value)],
    int_index: &IntIndex,
    verified_int_prefix: usize,
    key: i64,
) -> Option<usize> {
    verified_int_position(entries, verified_int_prefix, key)
        .or_else(|| int_index.get(&key).copied().map(IntIndexValue::position))
}

#[cfg(feature = "quick-loops")]
impl ExactOrderedIntLayout {
    #[inline(always)]
    pub(crate) unsafe fn positioned_value(self, key: i64) -> Option<*const Value> {
        let offset = key.checked_sub(self.position_zero_key)?;
        let position = usize::try_from(offset).ok()?;
        if position >= self.run_end {
            return None;
        }
        Some(
            self.first_value
                .as_ptr()
                .cast::<u8>()
                .add(position * std::mem::size_of::<(ArrayEntryKey, Value)>())
                .cast(),
        )
    }
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
        self.entries[..self.len()].iter().position(
            |entry| matches!(entry, Some((ArrayEntryKey::Int(found), _)) if *found == key),
        )
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
    entries
        .iter()
        .position(|entry| matches!(&entry.0, ArrayEntryKey::String(found) if found.as_ref() == key))
}

#[inline(always)]
fn set_indexed_int(
    entries: &mut Vec<(ArrayEntryKey, Value)>,
    int_index: &mut IntIndex,
    verified_int_prefix: &mut usize,
    next_int_key: &mut i64,
    key: i64,
    val: Value,
) {
    if let Some(position) = verified_int_position(entries, *verified_int_prefix, key) {
        entries[position].1 = val;
        return;
    }

    let extends_verified_prefix = *verified_int_prefix == entries.len()
        && int_index.is_empty()
        && match *verified_int_prefix {
            0 => entries.is_empty(),
            1 => matches!(entries.first(), Some((ArrayEntryKey::Int(first), _)) if *first != key),
            len => match (entries.first(), entries.get(1)) {
                (Some((ArrayEntryKey::Int(first), _)), Some((ArrayEntryKey::Int(second), _))) => {
                    second
                        .checked_sub(*first)
                        .and_then(|stride| {
                            i64::try_from(len)
                                .ok()
                                .and_then(|len| stride.checked_mul(len))
                        })
                        .and_then(|offset| first.checked_add(offset))
                        == Some(key)
                }
                _ => false,
            },
        };
    if extends_verified_prefix {
        entries.push((ArrayEntryKey::Int(key), val));
        *verified_int_prefix += 1;
        if key >= *next_int_key {
            *next_int_key = key.saturating_add(1);
        }
        return;
    }

    if *verified_int_prefix != 0 {
        materialize_int_index(entries, int_index, 1);
        *verified_int_prefix = 0;
    }
    match int_index.entry(key) {
        Entry::Occupied(mut entry) => {
            let position = entry.get().position();
            entries[position].1 = val;
            entry.insert(IntIndexValue::new(position, &entries[position].1));
        }
        Entry::Vacant(entry) => {
            let position = entries.len();
            entries.push((ArrayEntryKey::Int(key), val));
            entry.insert(IntIndexValue::new(position, &entries[position].1));
            if key >= *next_int_key {
                *next_int_key = key.saturating_add(1);
            }
        }
    }
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
            cursor: Cell::new(0),
        }
    }

    #[inline]
    pub(crate) fn cursor_reset(&self) -> Option<&Value> {
        self.cursor.set(0);
        self.iter().next().map(|(_, value)| value)
    }

    #[inline]
    pub(crate) fn cursor_end(&self) -> Option<&Value> {
        let position = self.len().saturating_sub(1);
        self.cursor.set(position);
        self.iter().nth(position).map(|(_, value)| value)
    }

    #[inline]
    pub(crate) fn cursor_current(&self) -> Option<&Value> {
        self.iter().nth(self.cursor.get()).map(|(_, value)| value)
    }

    #[inline]
    pub(crate) fn cursor_key(&self) -> Option<ArrayKey> {
        self.iter().nth(self.cursor.get()).map(|(key, _)| key)
    }

    #[inline]
    pub(crate) fn cursor_next(&self) -> Option<&Value> {
        self.cursor.set(self.cursor.get().saturating_add(1));
        self.cursor_current()
    }

    #[inline]
    pub(crate) fn cursor_prev(&self) -> Option<&Value> {
        let current = self.cursor.get();
        if current == 0 {
            self.cursor.set(self.len());
            return None;
        }
        self.cursor.set(current - 1);
        self.cursor_current()
    }

    #[inline]
    fn adjust_cursor_after_remove(&self, removed_position: usize) {
        let current = self.cursor.get();
        if removed_position < current {
            self.cursor.set(current - 1);
        }
    }

    /// PHP array union (`$left + $right`): retain every left entry and append
    /// only right-hand keys that are absent. Values stay COW-safe clones and
    /// the left array's insertion order remains authoritative.
    pub fn union(&self, right: &Self) -> Self {
        let mut result = self.clone();
        for (key, value) in right.iter() {
            let exists = match &key {
                ArrayKey::Int(key) => result.get_int(*key).is_some(),
                ArrayKey::String(key) => result.get_str(key).is_some(),
            };
            if !exists {
                result.set(key, value.clone());
            }
        }
        result
    }

    /// Create packed storage with capacity known from an array literal.
    pub fn with_packed_capacity(capacity: usize) -> Self {
        Self {
            storage: ArrayStorage::Packed(Vec::with_capacity(capacity)),
            next_int_key: 0,
            cursor: Cell::new(0),
        }
    }

    /// Create string-indexed hash storage directly when a literal string key
    /// proves that a packed representation would immediately transition.
    pub fn with_hash_capacity(capacity: usize) -> Self {
        if capacity <= SMALL_HASH_CAPACITY {
            return Self {
                storage: ArrayStorage::SmallHash(SmallHashStorage::new()),
                next_int_key: 0,
                cursor: Cell::new(0),
            };
        }
        Self {
            storage: ArrayStorage::Hash {
                entries: Vec::with_capacity(capacity),
                str_index: HashMap::with_capacity(capacity),
                int_index: int_index_with_capacity(0),
                verified_int_prefix: 0,
            },
            next_int_key: 0,
            cursor: Cell::new(0),
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
                cursor: Cell::new(0),
            };
        }
        if capacity <= LINEAR_HASH_CAPACITY {
            return Self {
                storage: ArrayStorage::LinearHash(LinearHashStorage::with_capacity(capacity)),
                next_int_key: 0,
                cursor: Cell::new(0),
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
            let int_index = int_index_with_capacity(0);
            for (i, val) in std::mem::take(values).into_iter().enumerate() {
                entries.push((ArrayEntryKey::Int(i as i64), val));
            }
            *&mut self.storage = ArrayStorage::Hash {
                entries,
                str_index: HashMap::new(),
                int_index,
                verified_int_prefix: len,
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
        let small = match std::mem::replace(&mut self.storage, ArrayStorage::Packed(Vec::new())) {
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
        let mut int_index = int_index_with_capacity(0);
        for (position, (key, _)) in entries.iter().enumerate() {
            match key {
                ArrayEntryKey::Int(_) => {}
                ArrayEntryKey::String(key) => {
                    str_index.insert(key.clone(), position);
                }
            }
        }
        let verified_int_prefix =
            rebuild_int_index(&entries, &mut int_index, additional_int_capacity);
        self.storage = ArrayStorage::Hash {
            entries,
            str_index,
            int_index,
            verified_int_prefix,
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
        let mut int_index = int_index_with_capacity(0);
        for (position, (key, _)) in entries.iter().enumerate() {
            match key {
                ArrayEntryKey::Int(_) => {}
                ArrayEntryKey::String(key) => {
                    if !str_index.contains_key(key.as_ref()) {
                        str_index.insert(key.clone(), position);
                    }
                }
            }
        }
        let verified_int_prefix =
            rebuild_int_index(&entries, &mut int_index, additional_int_capacity);
        self.storage = ArrayStorage::Hash {
            entries,
            str_index,
            int_index,
            verified_int_prefix,
        };
    }

    /// Append with PHP's next available integer key. Once `i64::MAX` is
    /// occupied, PHP cannot represent another key and the append must fail
    /// rather than wrapping into the negative range.
    #[inline]
    pub(crate) fn try_push(&mut self, val: Value) -> bool {
        let key = self.next_int_key;
        if key == i64::MAX && self.get_int(key).is_some() {
            return false;
        }
        self.next_int_key = key.saturating_add(1);
        if let ArrayStorage::Packed(values) = &mut self.storage {
            values.push(val);
            return true;
        }

        match &self.storage {
            ArrayStorage::SmallHash(small) if small.len() == SMALL_HASH_CAPACITY => {
                self.promote_small_hash(0, 1);
            }
            ArrayStorage::LinearHash(linear) if linear.entries.len() == LINEAR_HASH_CAPACITY => {
                self.promote_linear_hash(0, 1);
            }
            _ => {}
        }
        match &mut self.storage {
            ArrayStorage::Packed(_) => unreachable!("packed append returned through fast path"),
            ArrayStorage::SmallHash(small) => {
                let inserted = small.push(ArrayEntryKey::Int(key), val);
                debug_assert!(inserted);
            }
            ArrayStorage::LinearHash(linear) => {
                linear.invalidate_index();
                linear.entries.push((ArrayEntryKey::Int(key), val));
            }
            ArrayStorage::Hash {
                entries,
                int_index,
                verified_int_prefix,
                ..
            } => {
                set_indexed_int(
                    entries,
                    int_index,
                    verified_int_prefix,
                    &mut self.next_int_key,
                    key,
                    val,
                );
            }
        }
        true
    }

    /// Append with auto-incrementing key (`$a[] = $value`). Callers whose PHP
    /// surface can report overflow use `try_push`; legacy infallible internal
    /// materializers retain a no-op once the key space is exhausted.
    #[inline]
    pub fn push(&mut self, val: Value) {
        let _ = self.try_push(val);
    }

    /// Append one already validated dense Long batch without redispatching the
    /// storage enum for every member. Quick-loop callers retain a complete
    /// generic fallback when the array is not in canonical packed state.
    #[cfg(any(test, feature = "quick-loops"))]
    #[inline]
    pub(crate) fn push_packed_long_chunk(&mut self, values: &[i64]) -> bool {
        let ArrayStorage::Packed(packed) = &mut self.storage else {
            return false;
        };
        if i64::try_from(packed.len()).ok() != Some(self.next_int_key) {
            return false;
        }
        let Ok(count) = i64::try_from(values.len()) else {
            return false;
        };
        let Some(next_int_key) = self.next_int_key.checked_add(count) else {
            return false;
        };
        for value in values.iter().copied() {
            packed.push(Value::long(value));
        }
        self.next_int_key = next_int_key;
        true
    }

    /// Reserve a proven number of dense appends without changing the array's
    /// storage tier. Allocation failure keeps canonical geometric growth as
    /// the exact fallback.
    #[cfg(any(test, feature = "quick-loops"))]
    #[inline]
    pub(crate) fn reserve_packed_long_appends(&mut self, additional: usize) -> bool {
        let ArrayStorage::Packed(packed) = &mut self.storage else {
            return false;
        };
        if i64::try_from(packed.len()).ok() != Some(self.next_int_key) {
            return false;
        }
        packed.try_reserve(additional).is_ok()
    }

    /// Reserve canonical indexed-hash storage for a bounded native write
    /// estimate without changing the current storage tier.
    #[cfg(any(test, all(feature = "quick-loops", feature = "jit-prototype")))]
    pub(crate) fn reserve_indexed_int_writes(&mut self, additional: usize) -> bool {
        if additional == 0 {
            return true;
        }
        let ArrayStorage::Hash {
            entries,
            int_index,
            verified_int_prefix,
            ..
        } = &mut self.storage
        else {
            return false;
        };

        entries.reserve(additional);
        // Progression-only integer hashes intentionally keep their arithmetic
        // prefix and empty canonical index. Reserve buckets only after the
        // index has already materialized; a future irregular key remains the
        // authority for deciding whether that representation is necessary.
        if *verified_int_prefix == 0 && !int_index.is_empty() {
            int_index.reserve(additional);
        }
        true
    }

    #[cfg(all(
        feature = "quick-loops",
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    pub(crate) fn can_reserve_indexed_int_writes(&self) -> bool {
        matches!(self.storage, ArrayStorage::Hash { .. })
    }

    #[cfg(any(test, all(feature = "quick-loops", feature = "jit-prototype")))]
    fn indexed_int_write_reservation_is_unneeded(&self, key: i64) -> bool {
        let ArrayStorage::Packed(values) = &self.storage else {
            return false;
        };
        key == self.next_int_key || (key >= 0 && (key as usize) < values.len())
    }

    /// Set by integer key
    pub fn set_int(&mut self, key: i64, val: Value) {
        // Wide associative arrays are a stable hot state. Resolve a new or
        // existing integer key with one hash/probe instead of walking every
        // earlier storage tier and then performing separate get + insert.
        if let ArrayStorage::Hash {
            entries,
            int_index,
            verified_int_prefix,
            ..
        } = &mut self.storage
        {
            set_indexed_int(
                entries,
                int_index,
                verified_int_prefix,
                &mut self.next_int_key,
                key,
                val,
            );
            return;
        }

        let storage = &mut self.storage;
        if let ArrayStorage::Packed(values) = storage {
            // Can stay packed if key == next sequential
            if key == self.next_int_key {
                self.next_int_key = key.saturating_add(1);
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
        if let ArrayStorage::SmallHash(small) = &mut self.storage {
            if let Some(index) = small.find_int(key) {
                small.entries[index].as_mut().unwrap().1 = val;
                return;
            }
            if small.len() < SMALL_HASH_CAPACITY {
                let inserted = small.push(ArrayEntryKey::Int(key), val);
                debug_assert!(inserted);
                if key >= self.next_int_key {
                    self.next_int_key = key.saturating_add(1);
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
                    self.next_int_key = key.saturating_add(1);
                }
                return;
            }
        }
        self.promote_linear_hash(0, 1);
        // Now in indexed hash mode.
        let storage = &mut self.storage;
        if let ArrayStorage::Hash {
            entries,
            int_index,
            verified_int_prefix,
            ..
        } = storage
        {
            set_indexed_int(
                entries,
                int_index,
                verified_int_prefix,
                &mut self.next_int_key,
                key,
                val,
            );
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
        if let ArrayStorage::SmallHash(small) = &mut self.storage {
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
                linear
                    .entries
                    .push((ArrayEntryKey::String(SharedStringKey::new(key)), val));
                return;
            }
        }
        self.promote_linear_hash(1, 0);
        if let ArrayStorage::Hash {
            entries, str_index, ..
        } = &mut self.storage
        {
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
        if let ArrayStorage::SmallHash(small) = &mut self.storage {
            if let Some(index) = small.find_str(&key) {
                small.entries[index].as_mut().unwrap().1 = val;
                return;
            }
            if small.len() < SMALL_HASH_CAPACITY {
                let inserted =
                    small.push(ArrayEntryKey::String(SharedStringKey::from_owned(key)), val);
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
                linear
                    .entries
                    .push((ArrayEntryKey::String(SharedStringKey::from_owned(key)), val));
                return;
            }
        }
        self.promote_linear_hash(1, 0);
        if let ArrayStorage::Hash {
            entries, str_index, ..
        } = &mut self.storage
        {
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
        if let ArrayStorage::SmallHash(small) = &mut self.storage {
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
        if let ArrayStorage::Hash {
            entries, str_index, ..
        } = &mut self.storage
        {
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
            ArrayStorage::SmallHash(small) => small.get(position),
            ArrayStorage::Packed(_) => None,
        }
    }

    #[inline(always)]
    fn hash_len(&self) -> Option<usize> {
        match &self.storage {
            ArrayStorage::Hash { entries, .. } => Some(entries.len()),
            ArrayStorage::LinearHash(linear) => Some(linear.entries.len()),
            ArrayStorage::SmallHash(small) => Some(small.len()),
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
            ArrayStorage::Hash {
                entries,
                int_index,
                verified_int_prefix,
                ..
            } => {
                // Ordered PHP arrays commonly retain a contiguous integer run
                // after transitioning to hash storage. Keep its stride-one
                // candidate especially cheap, then use the exact arithmetic
                // prefix or the materialized index for other layouts.
                if let Some((ArrayEntryKey::Int(first_key), _)) = entries.first() {
                    if let Some(position) = key
                        .checked_sub(*first_key)
                        .and_then(|offset| usize::try_from(offset).ok())
                    {
                        if let Some((ArrayEntryKey::Int(found_key), value)) = entries.get(position)
                        {
                            if *found_key == key {
                                return Some(value);
                            }
                        }
                    }
                } else if key >= 0 {
                    if let Some((ArrayEntryKey::Int(found_key), value)) = entries.get(key as usize)
                    {
                        if *found_key == key {
                            return Some(value);
                        }
                    }
                }
                indexed_int_position(entries, int_index, *verified_int_prefix, key)
                    .map(|position| &entries[position].1)
            }
        }
    }

    /// Mutable lookup used only after the caller has established unique COW
    /// ownership. Replacing the returned entry cannot change array structure.
    #[inline(always)]
    pub(crate) fn get_int_mut(&mut self, key: i64) -> Option<&mut Value> {
        match &mut self.storage {
            ArrayStorage::Packed(values) if key >= 0 => values.get_mut(key as usize),
            ArrayStorage::Packed(_) => None,
            ArrayStorage::SmallHash(small) => {
                let position = small.find_int(key)?;
                small
                    .entries
                    .get_mut(position)?
                    .as_mut()
                    .map(|entry| &mut entry.1)
            }
            ArrayStorage::LinearHash(linear) => {
                let position = linear.find_int(key)?;
                linear.entries.get_mut(position).map(|entry| &mut entry.1)
            }
            ArrayStorage::Hash {
                entries,
                int_index,
                verified_int_prefix,
                ..
            } => {
                let position = indexed_int_position(entries, int_index, *verified_int_prefix, key)?;
                if let Some(indexed) = int_index.get_mut(&key) {
                    indexed.clear_cached_long();
                }
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
            (Some((ArrayEntryKey::Int(first), _)), Some((ArrayEntryKey::Int(second), _))) => {
                first.checked_add(1) == Some(*second)
            }
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
            let stride = second.checked_sub(*first).filter(|stride| *stride != 0)?;
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
            (suffix_start != 0)
                .then(|| window_hint(suffix_start))
                .flatten()
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
                offset
                    .checked_div(stride)
                    .and_then(|value| usize::try_from(value).ok())
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
            ArrayStorage::Hash {
                entries, int_index, ..
            } => int_index
                .get(&key)
                .map(|value| &entries[value.position()].1),
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
            ArrayStorage::Hash {
                entries, int_index, ..
            } => int_index
                .get(&key)
                .map(|value| &entries[value.position()].1),
            ArrayStorage::Packed(_) => None,
        }
    }

    /// Indexed lookup that also exposes the canonical insertion-order
    /// position. Guarded read-only regions use the position as a speculative
    /// cursor for a following dynamic key; the key is always revalidated.
    #[cfg(test)]
    #[inline]
    pub(crate) fn get_indexed_int_with_position(&self, key: i64) -> Option<(usize, &Value)> {
        match &self.storage {
            ArrayStorage::SmallHash(small) => {
                let position = small.find_int(key)?;
                Some((position, &small.get(position)?.1))
            }
            ArrayStorage::LinearHash(linear) => {
                let position = linear.find_int(key)?;
                Some((position, &linear.entries.get(position)?.1))
            }
            ArrayStorage::Hash {
                entries, int_index, ..
            } => {
                let position = int_index.get(&key)?.position();
                Some((position, &entries[position].1))
            }
            ArrayStorage::Packed(_) => None,
        }
    }

    /// Validate one predicted insertion-order position without probing the
    /// secondary integer index.
    #[cfg(feature = "quick-loops")]
    #[inline(always)]
    pub(crate) fn get_ordered_int_at(&self, position: usize, key: i64) -> Option<&Value> {
        match self.hash_entry_at(position) {
            Some((ArrayEntryKey::Int(found), value)) if *found == key => Some(value),
            _ => None,
        }
    }

    /// Typed integer lookup that consumes a compact cached Long payload when
    /// available and otherwise validates the canonical ordered entry.
    #[cfg(any(feature = "quick-loops", test))]
    #[inline(always)]
    pub(crate) fn get_indexed_long(&self, key: i64) -> Option<i64> {
        match &self.storage {
            ArrayStorage::SmallHash(small) => {
                let position = small.find_int(key)?;
                small.get(position)?.1.as_long()
            }
            ArrayStorage::LinearHash(linear) => {
                let position = linear.find_int(key)?;
                linear.entries.get(position)?.1.as_long()
            }
            ArrayStorage::Hash {
                entries, int_index, ..
            } => {
                let indexed = *int_index.get(&key)?;
                indexed
                    .cached_long()
                    .or_else(|| entries.get(indexed.position())?.1.as_long())
            }
            ArrayStorage::Packed(_) => None,
        }
    }

    /// Typed indexed lookup retaining the ordered position needed by the
    /// adaptive insertion-order cursor.
    #[cfg(any(feature = "quick-loops", test))]
    #[inline(always)]
    pub(crate) fn get_indexed_long_with_position(&self, key: i64) -> Option<(usize, i64)> {
        match &self.storage {
            ArrayStorage::SmallHash(small) => {
                let position = small.find_int(key)?;
                Some((position, small.get(position)?.1.as_long()?))
            }
            ArrayStorage::LinearHash(linear) => {
                let position = linear.find_int(key)?;
                Some((position, linear.entries.get(position)?.1.as_long()?))
            }
            ArrayStorage::Hash {
                entries, int_index, ..
            } => {
                let indexed = *int_index.get(&key)?;
                let position = indexed.position();
                let value = indexed
                    .cached_long()
                    .or_else(|| entries.get(position)?.1.as_long())?;
                Some((position, value))
            }
            ArrayStorage::Packed(_) => None,
        }
    }

    /// Build a stable native lookup context only for a materialized canonical
    /// integer index. Progression-only hashes intentionally keep using their
    /// arithmetic quick path instead.
    #[cfg(any(test, all(feature = "quick-loops", feature = "jit-prototype")))]
    #[inline]
    pub(crate) fn native_indexed_long_lookup_context(
        &self,
    ) -> Option<NativeIndexedLongLookupContext> {
        let ArrayStorage::Hash {
            entries,
            int_index,
            verified_int_prefix: 0,
            ..
        } = &self.storage
        else {
            return None;
        };
        (!int_index.is_empty()).then_some(NativeIndexedLongLookupContext {
            int_index,
            entries: entries.as_ptr(),
            entries_len: entries.len(),
        })
    }

    /// Get by string key — O(1), zero allocation.
    /// Uses `HashMap<String, usize>::get(&str)` via `Borrow<str>` trait.
    #[inline]
    pub fn get_str(&self, key: &str) -> Option<&Value> {
        match &self.storage {
            ArrayStorage::Hash {
                entries, str_index, ..
            } => str_index.get(key).map(|&idx| &entries[idx].1),
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
    #[inline(always)]
    pub(crate) fn get_str_mut(&mut self, key: &str) -> Option<&mut Value> {
        match &mut self.storage {
            ArrayStorage::Packed(_) => None,
            ArrayStorage::SmallHash(small) => {
                let position = small.find_str(key)?;
                small
                    .entries
                    .get_mut(position)?
                    .as_mut()
                    .map(|entry| &mut entry.1)
            }
            ArrayStorage::LinearHash(linear) => {
                let position = linear.find_str(key)?;
                linear.entries.get_mut(position).map(|entry| &mut entry.1)
            }
            ArrayStorage::Hash {
                entries, str_index, ..
            } => {
                let position = *str_index.get(key)?;
                entries.get_mut(position).map(|entry| &mut entry.1)
            }
        }
    }

    #[inline]
    pub(crate) fn get_key_mut(&mut self, key: &ArrayKey) -> Option<&mut Value> {
        match key {
            ArrayKey::Int(key) => self.get_int_mut(*key),
            ArrayKey::String(key) => self.get_str_mut(key),
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
                entries, str_index, ..
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
            ArrayStorage::SmallHash(small) => small.len() == 0,
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
            ArrayStorage::Packed(values) => values.get(pos).map(|v| (v, ArrayKey::Int(pos as i64))),
            ArrayStorage::SmallHash(small) => {
                small.get(pos).map(|(key, value)| (value, key.to_public()))
            }
            ArrayStorage::LinearHash(linear) => linear
                .entries
                .get(pos)
                .map(|(key, value)| (value, key.to_public())),
            ArrayStorage::Hash { entries, .. } => entries.get(pos).map(|(k, v)| (v, k.to_public())),
        }
    }

    /// Get value at position — for foreach when key is not needed.
    #[inline]
    pub fn get_value_at(&self, pos: usize) -> Option<&Value> {
        match &self.storage {
            ArrayStorage::Packed(values) => values.get(pos),
            ArrayStorage::SmallHash(small) => small.get(pos).map(|entry| &entry.1),
            ArrayStorage::LinearHash(linear) => linear.entries.get(pos).map(|entry| &entry.1),
            ArrayStorage::Hash { entries, .. } => entries.get(pos).map(|(_, v)| v),
        }
    }

    /// Replace an existing entry by iteration position without changing array
    /// structure. Used by the baseline by-reference foreach writeback path
    /// after the owning Value has completed copy-on-write detachment.
    pub(crate) fn set_value_at(&mut self, pos: usize, value: Value) -> bool {
        let slot = match &mut self.storage {
            ArrayStorage::Packed(values) => values.get_mut(pos),
            ArrayStorage::SmallHash(small) => small
                .entries
                .get_mut(pos)
                .and_then(Option::as_mut)
                .map(|entry| &mut entry.1),
            ArrayStorage::LinearHash(linear) => {
                linear.entries.get_mut(pos).map(|entry| &mut entry.1)
            }
            ArrayStorage::Hash { entries, .. } => entries.get_mut(pos).map(|entry| &mut entry.1),
        };
        let Some(slot) = slot else {
            return false;
        };
        *slot = value;
        true
    }

    /// Promote one array entry to a stable PHP reference cell and return a new
    /// alias for a source-level argument-unpack call. Array copy-on-write is
    /// resolved by `Value::as_array_mut()` before this method is entered.
    pub(crate) fn argument_unpack_reference_at(&mut self, pos: usize) -> Option<Value> {
        let slot = match &mut self.storage {
            ArrayStorage::Packed(values) => values.get_mut(pos),
            ArrayStorage::SmallHash(small) => small
                .entries
                .get_mut(pos)
                .and_then(Option::as_mut)
                .map(|entry| &mut entry.1),
            ArrayStorage::LinearHash(linear) => {
                linear.entries.get_mut(pos).map(|entry| &mut entry.1)
            }
            ArrayStorage::Hash { entries, .. } => entries.get_mut(pos).map(|entry| &mut entry.1),
        }?;

        if slot.is_owned_reference() {
            return Some(slot.clone_owned_reference_alias());
        }
        if slot.is_reference() {
            // SAFETY: the array retains the borrowed reference for at least as
            // long as the returned alias is consumed by the synchronous call.
            return Some(Value::reference(unsafe { slot.as_ref_ptr() }));
        }

        let value = std::mem::replace(slot, Value::undef());
        let reference = Value::owned_reference(value);
        let alias = reference.clone_owned_reference_alias();
        *slot = reference;
        Some(alias)
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
            ArrayStorage::LinearHash(linear) => PhpArrayIterInner::Hash(linear.entries.iter()),
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
            ArrayStorage::LinearHash(linear) => PhpArrayValuesInner::Hash(linear.entries.iter()),
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
            ArrayStorage::LinearHash(linear) => linear
                .entries
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
            ArrayStorage::LinearHash(linear) => linear
                .entries
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
        if let ArrayStorage::SmallHash(small) = &mut self.storage {
            let position = match key {
                ArrayKey::Int(key) => small.find_int(*key),
                ArrayKey::String(key) => small.find_str(key),
            };
            if let Some(position) = position {
                let removed = small.remove_at(position).is_some();
                if removed {
                    self.adjust_cursor_after_remove(position);
                }
                return removed;
            }
            return false;
        }
        if let ArrayStorage::LinearHash(linear) = &mut self.storage {
            let position = match key {
                ArrayKey::Int(key) => linear.find_int(*key),
                ArrayKey::String(key) => linear.find_str_for_update(key),
            };
            if let Some(position) = position {
                linear.entries.remove(position);
                linear.invalidate_index();
                self.adjust_cursor_after_remove(position);
                return true;
            }
            return false;
        }
        if let ArrayStorage::Hash {
            entries,
            int_index,
            str_index,
            verified_int_prefix,
        } = &mut self.storage
        {
            let found_idx = match key {
                ArrayKey::Int(n) => {
                    indexed_int_position(entries, int_index, *verified_int_prefix, *n)
                }
                ArrayKey::String(s) => str_index.get(s.as_str()).copied(),
            };
            if let Some(idx) = found_idx {
                let (removed_key, _) = entries.remove(idx);
                if let ArrayEntryKey::String(s) = removed_key {
                    str_index.remove(s.as_ref());
                }
                *verified_int_prefix = rebuild_int_index(entries, int_index, 0);
                Self::reindex_string_entries(entries, str_index, idx);
                self.adjust_cursor_after_remove(idx);
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
            ArrayStorage::Hash {
                entries,
                int_index,
                str_index,
                verified_int_prefix,
            } => {
                if let Some((key, val)) = entries.pop() {
                    match &key {
                        ArrayEntryKey::Int(n) => {
                            if *verified_int_prefix == entries.len() + 1 {
                                *verified_int_prefix = entries.len();
                            } else {
                                int_index.remove(n);
                            }
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
        if let ArrayStorage::SmallHash(small) = &mut self.storage {
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
        if let ArrayStorage::Hash {
            entries,
            int_index,
            str_index,
            verified_int_prefix,
        } = &mut self.storage
        {
            if entries.is_empty() {
                return None;
            }
            let (_key, val) = entries.remove(0);

            // Renumber: rebuild with int keys starting from 0, string keys preserved
            let mut new_int_counter: i64 = 0;
            str_index.clear();
            for (i, (key, _)) in entries.iter_mut().enumerate() {
                match key {
                    ArrayEntryKey::Int(n) => {
                        *n = new_int_counter;
                        new_int_counter += 1;
                    }
                    ArrayEntryKey::String(s) => {
                        str_index.insert(s.clone(), i);
                    }
                }
            }
            self.next_int_key = new_int_counter;
            *verified_int_prefix = rebuild_int_index(entries, int_index, 0);
            Some(val)
        } else {
            None
        }
    }

    /// Rebuild String positions affected by an ordered-entry removal.
    fn reindex_string_entries(
        entries: &[(ArrayEntryKey, Value)],
        str_index: &mut HashMap<SharedStringKey, usize>,
        from: usize,
    ) {
        for (i, (k, _)) in entries.iter().enumerate() {
            if i >= from {
                if let ArrayEntryKey::String(s) = k {
                    str_index.insert(s.clone(), i);
                }
            }
        }
    }

    /// Check if array is in packed mode (sequential 0..N-1 int keys).
    #[inline]
    pub fn is_packed(&self) -> bool {
        matches!(&self.storage, ArrayStorage::Packed(_))
    }

    /// PHP list semantics depend on ordered keys, not on the current internal
    /// storage tier. Explicit `0, 1, ...` keys therefore remain a list even
    /// when their array was constructed in hash mode.
    #[inline]
    pub fn is_list(&self) -> bool {
        match &self.storage {
            ArrayStorage::Packed(_) => true,
            ArrayStorage::SmallHash(small) => {
                small.entries[..small.len()]
                    .iter()
                    .enumerate()
                    .all(|(index, entry)| {
                        entry.as_ref().is_some_and(
                        |(key, _)| matches!(key, ArrayEntryKey::Int(key) if *key == index as i64),
                    )
                    })
            }
            ArrayStorage::LinearHash(linear) => linear.entries.iter().enumerate().all(
                |(index, (key, _))| matches!(key, ArrayEntryKey::Int(key) if *key == index as i64),
            ),
            ArrayStorage::Hash { entries, .. } => entries.iter().enumerate().all(
                |(index, (key, _))| matches!(key, ArrayEntryKey::Int(key) if *key == index as i64),
            ),
        }
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

    /// Return a direct value layout for an exactly maintained integer run.
    /// Structural mutations update or invalidate the run metadata, and quick
    /// regions keep the allocation stable, so keys inside this range do not
    /// need a redundant per-iteration validation load.
    #[cfg(feature = "quick-loops")]
    #[inline]
    pub(crate) fn exact_ordered_int_layout(&self) -> Option<ExactOrderedIntLayout> {
        let ArrayStorage::Hash {
            entries,
            verified_int_prefix,
            ..
        } = &self.storage
        else {
            return None;
        };
        if *verified_int_prefix < 8 {
            return None;
        }
        let (ArrayEntryKey::Int(position_zero_key), _) = entries.first()? else {
            return None;
        };
        let (ArrayEntryKey::Int(second_key), _) = entries.get(1)? else {
            return None;
        };
        let key_stride = second_key
            .checked_sub(*position_zero_key)
            .filter(|stride| *stride != 0)?;
        if key_stride != 1 {
            return None;
        }
        let first_value = std::ptr::NonNull::from(&entries.first()?.1);
        Some(ExactOrderedIntLayout {
            first_value,
            run_end: *verified_int_prefix,
            position_zero_key: *position_zero_key,
        })
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
            PhpArrayIterInner::Hash(iter) => iter.next().map(|(k, v)| (k.to_public(), v)),
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
            PhpArrayValuesInner::Small(iter) => iter.next().map(|entry| &entry.as_ref().unwrap().1),
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
            ArrayStorage::Packed(values) => {
                ArrayStorage::Packed(values.iter().map(Value::clone_for_array_cow).collect())
            }
            ArrayStorage::SmallHash(small) => {
                let mut cloned = SmallHashStorage::new();
                for (key, value) in small.entries.iter().flatten() {
                    let inserted = cloned.push(key.clone(), value.clone_for_array_cow());
                    debug_assert!(inserted);
                }
                ArrayStorage::SmallHash(cloned)
            }
            ArrayStorage::LinearHash(linear) => {
                ArrayStorage::LinearHash(LinearHashStorage::from_entries(
                    linear
                        .entries
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone_for_array_cow()))
                        .collect(),
                ))
            }
            ArrayStorage::Hash {
                entries,
                str_index,
                int_index,
                verified_int_prefix,
            } => ArrayStorage::Hash {
                entries: entries
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone_for_array_cow()))
                    .collect(),
                str_index: str_index.clone(),
                int_index: int_index.clone(),
                verified_int_prefix: *verified_int_prefix,
            },
        };
        Self {
            storage: cloned_storage,
            next_int_key: self.next_int_key,
            cursor: Cell::new(self.cursor.get()),
        }
    }
}

impl std::fmt::Debug for PhpArray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.storage {
            ArrayStorage::Packed(values) => f
                .debug_struct("PhpArray")
                .field("mode", &"packed")
                .field("len", &values.len())
                .finish(),
            ArrayStorage::SmallHash(small) => f
                .debug_struct("PhpArray")
                .field("mode", &"small-hash")
                .field("len", &small.len())
                .finish(),
            ArrayStorage::LinearHash(linear) => f
                .debug_struct("PhpArray")
                .field("mode", &"linear-hash")
                .field("len", &linear.entries.len())
                .finish(),
            ArrayStorage::Hash { entries, .. } => f
                .debug_struct("PhpArray")
                .field("mode", &"hash")
                .field("len", &entries.len())
                .finish(),
        }
    }
}

#[cfg(test)]
#[path = "php_array_tests.rs"]
mod php_array_tests;

#[cfg(test)]
mod closure_ownership_tests {
    use super::{PhpClosure, PhpObject, Value};
    use std::collections::HashMap;
    use std::rc::Rc;

    fn closure_with_capture(capture: Value) -> Value {
        Value::closure(PhpClosure {
            object_handle: 0,
            func: std::ptr::null(),
            called_scope_class_id: 0,
            is_static: true,
            bound_this: None,
            captures: vec![capture],
            static_vars: None,
            has_heap_captures: true,
        })
    }

    #[test]
    fn copied_closures_share_immutable_payload_and_capture_ownership() {
        let captured = Value::object(PhpObject::dynamic(
            "Captured".to_string(),
            0,
            HashMap::new(),
        ));
        let closure = closure_with_capture(captured.clone());
        let copy = closure.clone();

        assert!(std::ptr::eq(
            closure.as_closure().unwrap(),
            copy.as_closure().unwrap()
        ));
        assert!(
            closure
                .as_closure()
                .unwrap()
                .same_identity(copy.as_closure().unwrap())
        );
        assert_eq!(captured.object_strong_count(), Some(2));

        drop(copy);
        assert_eq!(captured.object_strong_count(), Some(2));
        drop(closure);
        assert_eq!(captured.object_strong_count(), Some(1));
    }

    #[test]
    fn capture_construction_rejects_a_published_closure() {
        let mut closure = closure_with_capture(Value::long(1));
        let _published = closure.clone();

        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            closure.push_closure_capture(Value::long(2));
        }));
        assert!(rejected.is_err());
    }
}

/// PHP closure — function pointer + captured values.
/// Stored behind `Rc` in `Value`, like strings, arrays, and objects.
pub(crate) type ClosureStaticVars = Rc<RefCell<HashMap<String, Value>>>;

pub struct PhpClosure {
    /// Request-local Zend object-store handle. Closures are PHP objects and
    /// therefore consume the same diagnostic handle sequence as instances.
    pub(crate) object_handle: u32,
    /// Direct pointer to the resolved function. No string lookup needed at call time.
    pub func: *const FunctionCommon,
    /// Late-called class captured when a class-scoped closure is created.
    /// Zero keeps ordinary closures on the existing path.
    pub called_scope_class_id: u32,
    /// True for PHP's `static function` and `static fn` forms. Retained on the
    /// value so Closure binding can enforce the language-level object rule.
    pub is_static: bool,
    /// Object bound implicitly at creation or explicitly through
    /// `Closure::bind()`. Static closures always keep this empty.
    pub bound_this: Option<Value>,
    /// Captured `use` variable values, in declaration order.
    pub captures: Vec<Value>,
    /// Function-static cells owned by this Closure object. Ordinary Value
    /// clones share the payload; an explicit Closure clone/bind snapshots it.
    pub(crate) static_vars: Option<ClosureStaticVars>,
    /// True if any captured value needs cleanup (owned heap values/resources).
    /// When false, captures are all scalars — clone is a cheap memcpy.
    pub has_heap_captures: bool,
}

impl Clone for PhpClosure {
    fn clone(&self) -> Self {
        let static_vars = self.static_vars.as_ref().map(|source| {
            let values: HashMap<String, Value> = source
                .as_ref()
                .borrow()
                .iter()
                .map(|(name, value)| {
                    let value = if value.is_owned_reference() {
                        // SAFETY: the source cell remains live throughout this
                        // borrow. Binding snapshots its value into a new cell.
                        let mut snapshot = Value::owned_reference(value.dereferenced().clone());
                        if value.is_static_initializer_in_progress() {
                            snapshot.mark_static_initializer_in_progress();
                        }
                        snapshot
                    } else {
                        value.clone()
                    };
                    (name.clone(), value)
                })
                .collect();
            Rc::new(RefCell::new(values))
        });
        Self {
            object_handle: 0,
            func: self.func,
            called_scope_class_id: self.called_scope_class_id,
            is_static: self.is_static,
            bound_this: self.bound_this.clone(),
            captures: self.clone_captures(),
            static_vars,
            has_heap_captures: self.has_heap_captures,
        }
    }
}

impl PhpClosure {
    /// Materialize one invocation/copy of the lexical environment without
    /// separating reference captures from their shared cells.
    #[inline]
    pub(crate) fn clone_captures(&self) -> Vec<Value> {
        self.captures
            .iter()
            .map(Value::clone_closure_capture)
            .collect()
    }

    /// PHP callback registries compare closures by object identity, not by
    /// function body or captures.
    #[inline]
    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }

    /// Recover the common header retained by every live Closure function.
    #[inline]
    pub(crate) fn common(&self) -> Option<&FunctionCommon> {
        (!self.func.is_null()).then(|| {
            // SAFETY: Closure construction accepts only registered function
            // allocations, which ExecutorGlobals retains for the request.
            unsafe { &*self.func }
        })
    }

    /// Recover the user function guaranteed by normal closure construction.
    ///
    /// Keeping the checked pointer cast here gives generic calls and
    /// Reflection one canonical boundary for the closure/function layout.
    #[inline]
    pub(crate) fn user_function(&self) -> Option<&UserFunction> {
        let common = self.common()?;
        (common.fn_type == FunctionType::User)
            .then(|| unsafe { &*(self.func as *const UserFunction) })
    }
}

impl std::fmt::Debug for PhpClosure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhpClosure")
            .field("func", &self.func)
            .field("called_scope_class_id", &self.called_scope_class_id)
            .field("is_static", &self.is_static)
            .field("bound_this", &self.bound_this.is_some())
            .field("captures", &self.captures.len())
            .finish()
    }
}

/// RPHP's compact 16-byte tagged value.
#[repr(C)]
pub struct Value {
    data: ValueData,
    type_info: u32,
    _not_send: PhantomData<*mut ()>,
}

struct OwnedReference {
    value: UnsafeCell<Value>,
    internal_aliases: Cell<usize>,
    property_constraints: RefCell<Vec<ReferencePropertyConstraint>>,
}

/// A typed property that currently owns one alias of an `OwnedReference`.
/// Keeping this metadata beside the shared cell preserves the compact Value
/// layout while allowing every alias write to enforce the intersection of all
/// property types holding the reference.
#[derive(Clone, Debug)]
pub(crate) struct ReferencePropertyConstraint {
    pub(crate) owner: usize,
    pub(crate) declaring_class: String,
    pub(crate) property: String,
    pub(crate) type_scope: String,
    pub(crate) called_class: String,
    pub(crate) type_hint: ParamTypeHint,
}

const _: [(); 16] = [(); std::mem::size_of::<Value>()];

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
    const OWNED_REFERENCE_FLAG: u32 = 1 << 8;
    /// Marks only the frame-local alias that created a local-static cell.
    /// A later declaration of the same static in that frame may replace the
    /// first initializer; aliases and recursive frames must not inherit it.
    const STATIC_INITIALIZER_IN_PROGRESS_FLAG: u32 = 1 << 9;
    /// Marks a compiler-only reference handle that has no PHP-visible storage
    /// location. Its Rc owner keeps the target alive but must not make
    /// `var_dump()` display a reference wrapper after the last source alias is
    /// unset.
    const INTERNAL_REFERENCE_ALIAS_FLAG: u32 = 1 << 10;

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
    /// For owned values (String/Array/Object/Resource), caller must handle refcount.
    #[inline(always)]
    pub unsafe fn raw_copy(src: *const Value, dst: *mut Value) {
        std::ptr::copy_nonoverlapping(
            src as *const u8,
            dst as *mut u8,
            std::mem::size_of::<Value>(),
        );
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
        Self::shared_string(rc)
    }

    /// Create a string value from an existing owner. Used by immutable
    /// compiled metadata whose PHP values can share the same bytes.
    #[inline]
    pub(crate) fn shared_string(rc: Rc<String>) -> Self {
        Self {
            data: ValueData {
                ptr: Rc::into_raw(rc) as *mut u8,
            },
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
        stats::inc_array_owner_allocation();
        Self {
            data: ValueData {
                ptr: Rc::into_raw(rc) as *mut u8,
            },
            type_info: ValueType::Array as u32,
            _not_send: PhantomData,
        }
    }

    /// Create an object value from a PhpObject (reference-counted).
    /// Stores Rc pointer directly — no Box wrapper. Clone = Rc increment, Drop = Rc decrement.
    #[inline]
    pub fn object(mut obj: PhpObject) -> Self {
        let is_declared = obj.class_id != 0;
        let (handle, in_request) = allocate_object_handle();
        obj.lifecycle = handle;
        let rc = Rc::new(RefCell::new(obj));
        if is_declared {
            stats::inc_declared_object_owner_allocation();
        }
        let ptr = Rc::into_raw(rc) as *mut u8;
        if !in_request {
            register_object_identity(ptr as usize);
        }
        Self {
            data: ValueData { ptr },
            type_info: ValueType::Object as u32,
            _not_send: PhantomData,
        }
    }

    /// Create a closure value from a PhpClosure.
    /// Clone = Rc refcount bump; binding creates a distinct payload and identity.
    #[inline]
    pub fn closure(mut c: PhpClosure) -> Self {
        if c.captures.capacity() != 0 {
            stats::inc_closure_capture_storage_allocation();
        }
        let (handle, in_request) = allocate_object_handle();
        c.object_handle = handle;
        let closure = Rc::new(c);
        stats::inc_closure_payload_allocation();
        let ptr = Rc::into_raw(closure) as *mut u8;
        if !in_request {
            register_object_identity(ptr as usize);
        }
        Self {
            data: ValueData { ptr },
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

    /// Add one capture while the closure payload is still uniquely owned by
    /// the compiler-created temporary.
    ///
    /// Closure bytecode fills the reserved capture vector immediately after
    /// construction and before the value can be copied or published. Keeping
    /// this transition here prevents general closure mutation after sharing.
    #[inline]
    pub(crate) fn push_closure_capture(&mut self, value: Value) {
        assert_eq!(self.value_type(), ValueType::Closure);
        let needs_cleanup = value.needs_cleanup();
        // SAFETY: the type check proves that the active union field is the raw
        // pointer stored by `Rc::into_raw` in `Value::closure`. The
        // CreateClosure/ClosureUseVar sequence retains the only strong owner
        // until all captures are appended, and no weak handles are exposed.
        let mut owner = std::mem::ManuallyDrop::new(unsafe {
            Rc::from_raw(self.data.ptr as *const PhpClosure)
        });
        let closure = Rc::get_mut(&mut owner)
            .expect("ClosureUseVar requires a uniquely owned construction temporary");
        if needs_cleanup {
            closure.has_heap_captures = true;
        }
        closure.captures.push(value);
    }

    /// Get the Rc<RefCell<PhpObject>> for shared access.
    /// Returns a temporary Rc handle without affecting the refcount.
    /// The caller must NOT drop the returned Rc (use for borrow/clone only).
    #[inline]
    pub fn as_object_rc(&self) -> Option<std::mem::ManuallyDrop<Rc<RefCell<PhpObject>>>> {
        if self.value_type() == ValueType::Object {
            Some(unsafe {
                std::mem::ManuallyDrop::new(Rc::from_raw(
                    self.data.ptr as *const RefCell<PhpObject>,
                ))
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

    /// Read the stable Rc allocation identity without taking a `RefCell`
    /// borrow or adjusting the reference count.
    /// SAFETY: Only valid when `value_type() == ValueType::Object`.
    #[inline(always)]
    pub unsafe fn object_identity_unchecked(&self) -> usize {
        debug_assert!(self.value_type() == ValueType::Object);
        self.data.ptr as usize
    }

    /// Stable allocation identity for an object value.
    #[inline]
    pub fn object_identity(&self) -> Option<usize> {
        (self.value_type() == ValueType::Object).then(|| {
            // SAFETY: the tag check above proves that the pointer union field
            // contains the live `Rc<RefCell<PhpObject>>` allocation address.
            unsafe { self.object_identity_unchecked() }
        })
    }

    /// Request-local object-store handle used by PHP-visible diagnostics.
    #[inline]
    pub fn object_handle(&self) -> Option<u32> {
        self.as_object()
            .map(|object| object.lifecycle & OBJECT_HANDLE_MASK)
            .or_else(|| self.as_closure().map(|closure| closure.object_handle))
    }

    /// Mark an Object allocation as having entered its destructor. Returns
    /// false when the same allocation was already destructed.
    #[inline]
    pub(crate) fn mark_object_destructed(&self) -> bool {
        let Some(mut object) = self.as_object_mut() else {
            return false;
        };
        if object.lifecycle & OBJECT_DESTRUCTOR_RAN != 0 {
            return false;
        }
        object.lifecycle |= OBJECT_DESTRUCTOR_RAN;
        true
    }

    /// Number of live PHP Value handles sharing this object identity.
    #[inline]
    pub(crate) fn object_strong_count(&self) -> Option<usize> {
        let object = self.as_object_rc()?;
        Some(Rc::strong_count(&object))
    }

    /// Read the immutable class name for Object values without taking a
    /// `RefCell` borrow. This is needed by boundary checks that may run while
    /// the same object's property storage is already mutably borrowed.
    /// SAFETY: Only valid when `value_type() == ValueType::Object`.
    #[inline(always)]
    pub unsafe fn object_class_name_unchecked(&self) -> &str {
        debug_assert!(self.value_type() == ValueType::Object);
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        (*refcell.as_ptr()).class_name.as_ref()
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

    /// Stable allocation identity for an array's current COW storage.
    #[inline]
    pub(crate) fn array_identity(&self) -> Option<usize> {
        self.as_array()
            .map(|array| array as *const PhpArray as usize)
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

    /// PHP's value name in diagnostics. Objects expose their concrete runtime
    /// class while all scalar names retain the allocation-free static form.
    pub fn diagnostic_type_name(&self) -> Cow<'_, str> {
        self.as_object().map_or_else(
            || Cow::Borrowed(self.type_name()),
            |object| Cow::Owned(object.class_name.to_string()),
        )
    }

    #[inline]
    pub fn as_long(&self) -> Option<i64> {
        if self.value_type() == ValueType::Long {
            Some(unsafe { self.data.long })
        } else {
            None
        }
    }

    /// Convert operands whose PHP numeric kind remains integer during
    /// arithmetic. Unlike `to_double()`, this preserves the result kind for
    /// null, booleans, resources, and integer numeric strings.
    #[inline]
    pub(crate) fn to_arithmetic_long(&self) -> Option<i64> {
        match self.value_type() {
            ValueType::Long => Some(unsafe { self.data.long }),
            ValueType::True => Some(1),
            ValueType::False | ValueType::Null | ValueType::Undef => Some(0),
            ValueType::String => self.as_str()?.trim().parse::<i64>().ok(),
            ValueType::Resource => self.as_resource_id(),
            _ => None,
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
            ValueType::True => Some(1.0),
            ValueType::False | ValueType::Null | ValueType::Undef => Some(0.0),
            ValueType::String => self.as_str().unwrap().trim().parse::<f64>().ok(),
            ValueType::Resource => Some(self.as_resource_id().unwrap() as f64),
            _ => None,
        }
    }

    /// Structural equality check for compile-time constant values.
    /// Used for trait property collision detection.
    /// Supports scalars, null, arrays (recursive), and identical object handles.
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
                a.iter()
                    .zip(b.iter())
                    .all(|((ka, va), (kb, vb))| ka == kb && va.structurally_equal(vb))
            }
            ValueType::Object => self.object_identity() == other.object_identity(),
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
                if d.is_nan() {
                    "NAN".to_string()
                } else if d == f64::INFINITY {
                    "INF".to_string()
                } else if d == f64::NEG_INFINITY {
                    "-INF".to_string()
                } else if d == d.floor() && d.abs() < 1e15 {
                    format!("{}", d as i64)
                } else {
                    format!("{}", d)
                }
            }
            ValueType::String => unsafe { &*(self.data.ptr as *const String) }.clone(),
            ValueType::Array => "Array".to_string(),
            ValueType::Object => {
                let refcell = unsafe { &*(self.data.ptr as *const RefCell<PhpObject>) };
                let obj = refcell.borrow();
                format!("{} Object", obj.class_name)
            }
            ValueType::Resource => {
                format!("Resource id #{}", self.as_resource_id().unwrap())
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
                if d.is_nan() {
                    output.push_str("NAN");
                } else if d == f64::INFINITY {
                    output.push_str("INF");
                } else if d == f64::NEG_INFINITY {
                    output.push_str("-INF");
                } else if d == d.floor() && d.abs() < 1e15 {
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
            ValueType::Resource => {
                let _ = write!(output, "Resource id #{}", self.as_resource_id().unwrap());
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
            ValueType::Resource => 22,
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
                if s.is_empty() {
                    return 0;
                }
                let bytes = s.as_bytes();
                let mut end = 0;
                if bytes[0] == b'-' || bytes[0] == b'+' {
                    end = 1;
                }
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                if end == 0 || (end == 1 && (bytes[0] == b'-' || bytes[0] == b'+')) {
                    return 0;
                }
                s[..end].parse().unwrap_or(0)
            }
            ValueType::Resource => self.as_resource_id().unwrap(),
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
            ValueType::Resource => self.as_resource_id().unwrap() as f64,
            _ => 0.0,
        }
    }

    /// Create a boolean value
    #[inline]
    pub fn bool(v: bool) -> Self {
        Self {
            data: ValueData { long: 0 },
            type_info: if v {
                ValueType::True as u32
            } else {
                ValueType::False as u32
            },
            _not_send: PhantomData,
        }
    }

    /// Create a reference value — points to another Value slot (e.g. caller's CV).
    /// SAFETY: `ptr` must remain valid for the lifetime of this Value.
    #[inline]
    pub fn reference(ptr: *mut Value) -> Self {
        Self {
            data: ValueData {
                ptr: ptr as *mut u8,
            },
            type_info: ValueType::Reference as u32,
            _not_send: PhantomData,
        }
    }

    /// Create a stable reference target shared by frame variables and array
    /// elements. Unlike a borrowed frame-slot reference, this target remains
    /// live while any owned reference handle can reach it.
    #[inline]
    pub(crate) fn owned_reference(value: Value) -> Self {
        let target = Rc::new(OwnedReference {
            value: UnsafeCell::new(value),
            internal_aliases: Cell::new(0),
            property_constraints: RefCell::new(Vec::new()),
        });
        Self {
            data: ValueData {
                ptr: Rc::into_raw(target) as *mut u8,
            },
            type_info: ValueType::Reference as u32 | Self::OWNED_REFERENCE_FLAG,
            _not_send: PhantomData,
        }
    }

    const TRAVERSABLE_UNPACK_VALUE_FLAG: u32 = 1 << 29;

    /// Retain the origin of a Traversable-expanded value until call signature
    /// resolution. By-value parameters dereference it like an ordinary PHP
    /// reference; by-reference parameters use the marker for PHP's warning and
    /// deliberately receive a detached value.
    pub(crate) fn traversable_unpack_value(value: Value) -> Self {
        let mut reference = Self::owned_reference(value);
        reference.type_info |= Self::TRAVERSABLE_UNPACK_VALUE_FLAG;
        reference
    }

    #[inline]
    pub(crate) fn is_traversable_unpack_value(&self) -> bool {
        self.is_owned_reference() && self.type_info & Self::TRAVERSABLE_UNPACK_VALUE_FLAG != 0
    }

    #[inline]
    pub(crate) fn is_owned_reference(&self) -> bool {
        self.value_type() == ValueType::Reference
            && self.type_info & Self::OWNED_REFERENCE_FLAG != 0
    }

    /// Clone an owned reference as an alias instead of reading its target.
    /// Array copy-on-write must preserve explicit PHP reference cells.
    #[inline]
    pub(crate) fn clone_owned_reference_alias(&self) -> Self {
        debug_assert!(self.is_owned_reference());
        let retained = Rc::clone(&self.owned_reference_rc());
        Self {
            data: ValueData {
                ptr: Rc::into_raw(retained) as *mut u8,
            },
            // The local-static initializer marker belongs to one frame slot,
            // not to the shared PHP reference cell or any aliases of it.
            type_info: self.type_info
                & !(Self::STATIC_INITIALIZER_IN_PROGRESS_FLAG
                    | Self::INTERNAL_REFERENCE_ALIAS_FLAG),
            _not_send: PhantomData,
        }
    }

    /// Temporarily reconstruct the existing Rc owner without consuming it.
    #[inline]
    fn owned_reference_rc(&self) -> std::mem::ManuallyDrop<Rc<OwnedReference>> {
        debug_assert!(self.is_owned_reference());
        // SAFETY: `owned_reference()` stores exactly an `Rc<OwnedReference>`
        // raw pointer, and ManuallyDrop leaves its existing strong owner intact.
        unsafe { std::mem::ManuallyDrop::new(Rc::from_raw(self.data.ptr as *const OwnedReference)) }
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn reference_property_constraints(&self) -> Vec<ReferencePropertyConstraint> {
        if !self.is_owned_reference() {
            return Vec::new();
        }
        self.owned_reference_rc()
            .property_constraints
            .borrow()
            .clone()
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn add_reference_property_constraint(
        &self,
        constraint: ReferencePropertyConstraint,
    ) {
        debug_assert!(self.is_owned_reference());
        let reference = self.owned_reference_rc();
        let mut constraints = reference.property_constraints.borrow_mut();
        constraints.retain(|existing| existing.owner != constraint.owner);
        constraints.push(constraint);
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn remove_reference_property_constraint(&self, owner: usize) {
        if !self.is_owned_reference() {
            return;
        }
        self.owned_reference_rc()
            .property_constraints
            .borrow_mut()
            .retain(|constraint| constraint.owner != owner);
    }

    /// Exclude one compiler-owned CV from PHP-visible reference cardinality.
    #[inline]
    pub(crate) fn mark_internal_reference_alias(&mut self) {
        debug_assert!(self.is_owned_reference());
        if self.type_info & Self::INTERNAL_REFERENCE_ALIAS_FLAG != 0 {
            return;
        }
        let reference = self.owned_reference_rc();
        let internal_aliases = reference.internal_aliases.get();
        debug_assert!(internal_aliases < usize::MAX);
        reference.internal_aliases.set(internal_aliases + 1);
        self.type_info |= Self::INTERNAL_REFERENCE_ALIAS_FLAG;
    }

    /// Whether this request-owned reference cell is still reachable through
    /// more than one PHP storage location.
    ///
    /// Zend makes a reference wrapper visually invisible once its last alias
    /// is rebound or unset. The stable allocation remains internally useful:
    /// in particular, a later by-reference argument unpack may acquire the
    /// array element again without losing its l-value identity.
    #[inline]
    pub(crate) fn owned_reference_is_aliased(&self) -> bool {
        if !self.is_owned_reference() {
            return false;
        }
        let reference = self.owned_reference_rc();
        let strong_count = Rc::strong_count(&reference);
        let internal_aliases = reference.internal_aliases.get();
        debug_assert!(internal_aliases <= strong_count);
        if internal_aliases > strong_count {
            return true;
        }
        strong_count - internal_aliases > 1
    }

    /// Clone a lexical capture while retaining explicit PHP reference
    /// identity. Ordinary `Value::clone()` intentionally dereferences, which
    /// is correct for by-value assignment but not for a `use (&$var)` cell.
    #[inline]
    pub(crate) fn clone_closure_capture(&self) -> Self {
        if self.is_owned_reference() {
            self.clone_owned_reference_alias()
        } else if self.is_reference() {
            // Borrowed reference captures are retained for call-frame aliases.
            // Normal closure construction promotes local CVs to owned cells.
            Value::reference(unsafe { self.as_ref_ptr() })
        } else {
            self.clone()
        }
    }

    #[inline]
    pub(crate) fn mark_static_initializer_in_progress(&mut self) {
        debug_assert!(self.is_owned_reference());
        self.type_info |= Self::STATIC_INITIALIZER_IN_PROGRESS_FLAG;
    }

    #[inline]
    pub(crate) fn is_static_initializer_in_progress(&self) -> bool {
        self.is_owned_reference() && self.type_info & Self::STATIC_INITIALIZER_IN_PROGRESS_FLAG != 0
    }

    #[inline]
    pub(crate) fn clear_static_initializer_in_progress(&mut self) {
        self.type_info &= !Self::STATIC_INITIALIZER_IN_PROGRESS_FLAG;
    }

    #[inline]
    fn clone_for_array_cow(&self) -> Self {
        if self.is_owned_reference() {
            self.clone_owned_reference_alias()
        } else {
            self.clone()
        }
    }

    /// Check if this value is a reference.
    #[inline]
    pub fn is_reference(&self) -> bool {
        self.value_type() == ValueType::Reference
    }

    /// Stable identity of the referenced PHP value cell while this handle is
    /// live. Serialization uses it to emit `R:n` for repeated aliases and to
    /// terminate self-referential arrays.
    #[inline]
    pub(crate) fn reference_identity(&self) -> Option<usize> {
        self.is_reference()
            .then(|| self.dereferenced() as *const Value as usize)
    }

    #[inline]
    #[cfg(not(feature = "resource-lifetime"))]
    pub fn needs_cleanup(&self) -> bool {
        self.is_owned_reference()
            || matches!(
                self.value_type(),
                ValueType::String | ValueType::Array | ValueType::Object | ValueType::Closure
            )
    }

    #[inline]
    #[cfg(feature = "resource-lifetime")]
    pub fn needs_cleanup(&self) -> bool {
        self.is_owned_reference()
            || matches!(
                self.value_type(),
                ValueType::String
                    | ValueType::Array
                    | ValueType::Object
                    | ValueType::Resource
                    | ValueType::Closure
            )
    }

    /// Get the target pointer of a reference value.
    /// SAFETY: only valid when is_reference() is true.
    #[inline]
    pub unsafe fn as_ref_ptr(&self) -> *mut Value {
        if self.is_owned_reference() {
            (*(self.data.ptr as *const OwnedReference)).value.get()
        } else {
            self.data.ptr as *mut Value
        }
    }

    /// Follow a PHP reference while tying the shared borrow to this value.
    #[inline]
    pub fn dereferenced(&self) -> &Value {
        if self.is_reference() {
            // SAFETY: both reference representations keep their target live
            // for at least as long as the Value through which it is reached.
            unsafe { &*self.as_ref_ptr() }
        } else {
            self
        }
    }

    /// Create a shared handle for one request-owned resource-registry entry.
    /// The handle keeps `Value` at 16 bytes while the final alias closes an
    /// entry that was not closed explicitly.
    #[inline]
    #[cfg(feature = "resource-lifetime")]
    pub(crate) fn resource(handle: ResourceHandle) -> Self {
        let handle = Rc::into_raw(Rc::new(handle));
        Self {
            data: ValueData {
                ptr: handle as *mut u8,
            },
            type_info: ValueType::Resource as u32,
            _not_send: PhantomData,
        }
    }

    /// Create the default scalar handle for one request-owned registry entry.
    #[inline]
    #[cfg(not(feature = "resource-lifetime"))]
    pub fn resource(id: i64) -> Self {
        Self {
            data: ValueData { long: id },
            type_info: ValueType::Resource as u32,
            _not_send: PhantomData,
        }
    }

    #[inline]
    pub fn as_resource_id(&self) -> Option<i64> {
        if self.value_type() == ValueType::Resource {
            #[cfg(feature = "resource-lifetime")]
            {
                let handle = unsafe { &*(self.data.ptr as *const ResourceHandle) };
                Some(handle.id())
            }
            #[cfg(not(feature = "resource-lifetime"))]
            {
                Some(unsafe { self.data.long })
            }
        } else {
            None
        }
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
                    data: ValueData {
                        ptr: unsafe { self.data.ptr },
                    },
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
                    data: ValueData {
                        ptr: unsafe { self.data.ptr },
                    },
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
                    data: ValueData {
                        ptr: unsafe { self.data.ptr },
                    },
                    type_info: self.type_info,
                    _not_send: PhantomData,
                }
            }
            #[cfg(feature = "resource-lifetime")]
            ValueType::Resource => {
                unsafe {
                    Rc::increment_strong_count(self.data.ptr as *const ResourceHandle);
                }
                Self {
                    data: ValueData {
                        ptr: unsafe { self.data.ptr },
                    },
                    type_info: self.type_info,
                    _not_send: PhantomData,
                }
            }
            ValueType::Closure => {
                // Closure payloads are immutable after construction. Copies
                // retain identity and captures through one Rc increment.
                // SAFETY: every closure pointer comes from `Rc::into_raw` in
                // `Value::closure`; this clone creates the matching new owner.
                unsafe {
                    Rc::increment_strong_count(self.data.ptr as *const PhpClosure);
                    Self {
                        data: ValueData { ptr: self.data.ptr },
                        type_info: self.type_info,
                        _not_send: PhantomData,
                    }
                }
            }
            ValueType::Reference => {
                // Clone a reference: clone the TARGET value (dereference + deep clone)
                // SAFETY: both borrowed and owned reference constructors keep
                // their target live for every Value that can reach this clone.
                let target = unsafe { &*self.as_ref_ptr() };
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
                unsafe {
                    let pointer = self.data.ptr as *const RefCell<PhpObject>;
                    let owner = std::mem::ManuallyDrop::new(Rc::from_raw(pointer));
                    if Rc::strong_count(&owner) == 1 {
                        let handle = (*(*pointer).as_ptr()).lifecycle & OBJECT_HANDLE_MASK;
                        release_object_handle(pointer as usize, handle);
                    }
                    Rc::decrement_strong_count(pointer);
                };
            }
            #[cfg(feature = "resource-lifetime")]
            ValueType::Resource => {
                unsafe { Rc::decrement_strong_count(self.data.ptr as *const ResourceHandle) };
            }
            ValueType::Closure => {
                // SAFETY: closure construction stores the raw pointer returned
                // by `Rc::into_raw`; each clone has incremented the same count.
                unsafe {
                    let pointer = self.data.ptr as *const PhpClosure;
                    let owner = std::mem::ManuallyDrop::new(Rc::from_raw(pointer));
                    if Rc::strong_count(&owner) == 1 {
                        release_object_handle(pointer as usize, (*pointer).object_handle);
                    }
                    Rc::decrement_strong_count(pointer);
                };
            }
            ValueType::Reference if self.is_owned_reference() => {
                // SAFETY: owned references store the raw pointer produced by
                // `Rc::into_raw`; each owned alias increments the same count.
                unsafe {
                    if self.type_info & Self::INTERNAL_REFERENCE_ALIAS_FLAG != 0 {
                        let reference = &*(self.data.ptr as *const OwnedReference);
                        let internal_aliases = reference.internal_aliases.get();
                        debug_assert!(internal_aliases > 0);
                        if internal_aliases > 0 {
                            reference.internal_aliases.set(internal_aliases - 1);
                        }
                    }
                    Rc::decrement_strong_count(self.data.ptr as *const OwnedReference);
                }
            }
            // Borrowed references do not own their frame-slot target.
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
            ValueType::String => write!(f, "Value(string={:?})", unsafe {
                &*(self.data.ptr as *const String)
            }),
            ValueType::Array => write!(
                f,
                "Value(array[{}])",
                unsafe { &*(self.data.ptr as *const PhpArray) }.len()
            ),
            ValueType::Object => {
                let refcell = unsafe { &*(self.data.ptr as *const RefCell<PhpObject>) };
                let obj = refcell.borrow();
                write!(f, "Value(object({}))", obj.class_name)
            }
            ValueType::Resource => {
                write!(f, "Value(resource({}))", self.as_resource_id().unwrap())
            }
            ValueType::Reference => write!(f, "Value(ref={:p})", unsafe { self.data.ptr }),
            ValueType::Closure => write!(f, "Value(Closure)"),
        }
    }
}
