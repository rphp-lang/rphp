//! Request-local weak-object sidecars.
//!
//! Ordinary objects keep their compact payload. WeakReference, WeakMap and
//! InternalIterator allocate this state only after their first observable use.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Weak;

use crate::value::{PhpObject, Value, WeakPhpObject};

use super::ExecutorGlobals;

struct WeakReferenceState {
    owner_identity: usize,
    owner: Weak<RefCell<PhpObject>>,
    target: WeakPhpObject,
    cleared: bool,
}

struct WeakMapEntry {
    key_identity: usize,
    key: WeakPhpObject,
    value: Value,
    exposed_reference: bool,
}

struct WeakMapState {
    owner: Weak<RefCell<PhpObject>>,
    entries: Vec<WeakMapEntry>,
}

struct WeakIteratorState {
    owner: Weak<RefCell<PhpObject>>,
    map: Value,
    keys: Vec<usize>,
    position: usize,
    by_reference: bool,
}

#[derive(Default)]
pub(super) struct WeakObjectRuntime {
    references: HashMap<usize, WeakReferenceState>,
    maps: HashMap<usize, WeakMapState>,
    iterators: HashMap<usize, WeakIteratorState>,
}

impl WeakObjectRuntime {
    fn reference_for_target(&mut self, target_identity: usize) -> Option<Value> {
        let state = self.references.get(&target_identity)?;
        if state.cleared || state.target.strong_count() == 0 {
            self.references.remove(&target_identity);
            return None;
        }
        let owner = state.owner.upgrade()?;
        Some(Value::from_object_owner(owner))
    }

    fn register_reference(&mut self, owner: &Value, target: &Value) -> bool {
        let (Some(owner_identity), Some(owner), Some(target_identity), Some(target)) = (
            owner.object_identity(),
            owner.object_weak(),
            target.weak_object_identity(),
            target.weak_object_owner(),
        ) else {
            return false;
        };
        self.references.insert(
            target_identity,
            WeakReferenceState {
                owner_identity,
                owner,
                target,
                cleared: false,
            },
        );
        true
    }

    fn reference_target(&self, owner_identity: usize) -> Option<Value> {
        let state = self
            .references
            .values()
            .find(|state| state.owner_identity == owner_identity)?;
        if state.cleared {
            return None;
        }
        state.target.upgrade()
    }

    fn ensure_map(&mut self, map: &Value) -> Option<usize> {
        let identity = map.object_identity()?;
        if !self.maps.contains_key(&identity) {
            self.maps.insert(
                identity,
                WeakMapState {
                    owner: map.object_weak()?,
                    entries: Vec::new(),
                },
            );
        }
        Some(identity)
    }

    fn map_value(&self, map_identity: usize, key_identity: usize) -> Option<&Value> {
        self.maps
            .get(&map_identity)?
            .entries
            .iter()
            .find(|entry| entry.key_identity == key_identity && entry.key.strong_count() != 0)
            .map(|entry| &entry.value)
    }

    fn map_set(&mut self, map: &Value, key: &Value, value: &Value) -> bool {
        let Some(map_identity) = self.ensure_map(map) else {
            return false;
        };
        let (Some(key_identity), Some(key_owner)) =
            (key.weak_object_identity(), key.weak_object_owner())
        else {
            return false;
        };
        let state = self.maps.get_mut(&map_identity).unwrap();
        if let Some(entry) = state
            .entries
            .iter_mut()
            .find(|entry| entry.key_identity == key_identity)
        {
            if value.is_owned_reference() {
                entry.value = value.clone_owned_reference_alias();
            } else {
                entry
                    .value
                    .assign_dereferenced(value.dereferenced().clone());
            }
            return true;
        }
        let value = if value.is_owned_reference() {
            value.clone_owned_reference_alias()
        } else {
            Value::owned_reference(value.dereferenced().clone())
        };
        state.entries.push(WeakMapEntry {
            key_identity,
            key: key_owner,
            value,
            exposed_reference: false,
        });
        true
    }

    fn map_remove(&mut self, map_identity: usize, key_identity: usize) -> Option<Value> {
        let entries = &mut self.maps.get_mut(&map_identity)?.entries;
        let position = entries
            .iter()
            .position(|entry| entry.key_identity == key_identity)?;
        Some(entries.remove(position).value)
    }

    fn map_entries(&self, map_identity: usize) -> Vec<(Value, Value)> {
        let Some(state) = self.maps.get(&map_identity) else {
            return Vec::new();
        };
        state
            .entries
            .iter()
            .filter_map(|entry| {
                let key = entry.key.upgrade()?;
                let value = if entry.exposed_reference || entry.value.owned_reference_is_aliased() {
                    entry.value.clone_owned_reference_alias()
                } else {
                    entry.value.dereferenced().clone()
                };
                Some((key, value))
            })
            .collect()
    }

    fn clone_map(&mut self, source: &Value, target: &Value) {
        let (Some(source_identity), Some(target_identity), Some(owner)) = (
            source.object_identity(),
            target.object_identity(),
            target.object_weak(),
        ) else {
            return;
        };
        let entries = self
            .maps
            .get(&source_identity)
            .map(|state| {
                state
                    .entries
                    .iter()
                    .filter(|entry| entry.key.strong_count() != 0)
                    .map(|entry| WeakMapEntry {
                        key_identity: entry.key_identity,
                        key: entry.key.clone(),
                        value: if entry.value.owned_reference_is_aliased() {
                            entry.value.clone_owned_reference_alias()
                        } else {
                            Value::owned_reference(entry.value.dereferenced().clone())
                        },
                        exposed_reference: entry.exposed_reference,
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.maps
            .insert(target_identity, WeakMapState { owner, entries });
    }

    fn register_iterator(&mut self, iterator: &Value, map: &Value) -> bool {
        let (Some(identity), Some(owner), Some(map_identity)) = (
            iterator.object_identity(),
            iterator.object_weak(),
            map.object_identity(),
        ) else {
            return false;
        };
        let keys = self
            .maps
            .get(&map_identity)
            .map(|state| {
                state
                    .entries
                    .iter()
                    .filter(|entry| entry.key.strong_count() != 0)
                    .map(|entry| entry.key_identity)
                    .collect()
            })
            .unwrap_or_default();
        self.iterators.insert(
            identity,
            WeakIteratorState {
                owner,
                map: map.clone(),
                keys,
                position: 0,
                by_reference: false,
            },
        );
        true
    }

    fn iterator_rewind(&mut self, iterator_identity: usize) {
        if let Some(iterator) = self.iterators.get_mut(&iterator_identity) {
            iterator.position = 0;
        }
    }

    fn iterator_next(&mut self, iterator_identity: usize) {
        if let Some(iterator) = self.iterators.get_mut(&iterator_identity) {
            iterator.position = iterator.position.saturating_add(1);
        }
    }

    fn iterator_entry(&mut self, iterator_identity: usize) -> Option<(usize, usize)> {
        loop {
            let (map_identity, key_identity) = {
                let iterator = self.iterators.get(&iterator_identity)?;
                (
                    iterator.map.object_identity()?,
                    *iterator.keys.get(iterator.position)?,
                )
            };
            if self.map_value(map_identity, key_identity).is_some() {
                return Some((map_identity, key_identity));
            }
            self.iterator_next(iterator_identity);
        }
    }

    fn iterator_key(&mut self, iterator_identity: usize) -> Option<Value> {
        let (map_identity, key_identity) = self.iterator_entry(iterator_identity)?;
        self.maps
            .get(&map_identity)?
            .entries
            .iter()
            .find(|entry| entry.key_identity == key_identity)?
            .key
            .upgrade()
    }

    fn iterator_value(&mut self, iterator_identity: usize) -> Option<Value> {
        let (map_identity, key_identity) = self.iterator_entry(iterator_identity)?;
        let by_reference = self.iterators.get(&iterator_identity)?.by_reference;
        let entry = self
            .maps
            .get_mut(&map_identity)?
            .entries
            .iter_mut()
            .find(|entry| entry.key_identity == key_identity)?;
        if by_reference {
            entry.exposed_reference = true;
        }
        Some(entry.value.clone_owned_reference_alias())
    }

    fn enable_iterator_references(&mut self, iterator_identity: usize) {
        if let Some(iterator) = self.iterators.get_mut(&iterator_identity) {
            iterator.by_reference = true;
        }
    }

    fn has_release_work(&self, identity: usize) -> bool {
        self.references
            .iter()
            .any(|(target, state)| *target == identity || state.owner_identity == identity)
            || self
                .maps
                .get(&identity)
                .is_some_and(|state| state.owner.strong_count() != 0)
            || self.maps.values().any(|state| {
                state
                    .entries
                    .iter()
                    .any(|entry| entry.key_identity == identity)
            })
            || self
                .iterators
                .get(&identity)
                .is_some_and(|state| state.owner.strong_count() != 0)
    }

    fn release_identity(&mut self, identity: usize) -> Vec<Value> {
        for (target, state) in &mut self.references {
            if *target == identity {
                state.cleared = true;
            }
        }

        let mut released = Vec::new();
        for state in self.maps.values_mut() {
            let entries = std::mem::take(&mut state.entries);
            let mut retained = Vec::with_capacity(entries.len());
            for entry in entries {
                if entry.key_identity == identity {
                    released.push(entry.value);
                } else {
                    retained.push(entry);
                }
            }
            state.entries = retained;
        }

        self.references
            .retain(|_, state| state.owner_identity != identity);
        if let Some(state) = self.maps.remove(&identity) {
            released.extend(state.entries.into_iter().map(|entry| entry.value));
        }
        if let Some(iterator) = self.iterators.remove(&identity) {
            released.push(iterator.map);
        }
        released
    }
}

impl ExecutorGlobals {
    fn weak_runtime(&mut self) -> &mut WeakObjectRuntime {
        self.weak_objects
            .get_or_insert_with(|| Box::new(WeakObjectRuntime::default()))
    }

    pub(crate) fn existing_weak_reference(&mut self, target: &Value) -> Option<Value> {
        self.weak_objects
            .as_deref_mut()?
            .reference_for_target(target.weak_object_identity()?)
    }

    pub(crate) fn register_weak_reference(&mut self, owner: &Value, target: &Value) -> bool {
        self.weak_runtime().register_reference(owner, target)
    }

    pub(crate) fn weak_reference_target(&self, owner: &Value) -> Option<Value> {
        self.weak_objects
            .as_deref()?
            .reference_target(owner.object_identity()?)
    }

    pub(crate) fn ensure_weak_map(&mut self, map: &Value) -> bool {
        self.weak_runtime().ensure_map(map).is_some()
    }

    pub(crate) fn weak_map_value(&self, map: &Value, key: &Value) -> Option<&Value> {
        self.weak_objects
            .as_deref()?
            .map_value(map.object_identity()?, key.weak_object_identity()?)
    }

    pub(crate) fn set_weak_map_value(&mut self, map: &Value, key: &Value, value: &Value) -> bool {
        self.weak_runtime().map_set(map, key, value)
    }

    pub(crate) fn remove_weak_map_value(&mut self, map: &Value, key: &Value) -> Option<Value> {
        self.weak_objects
            .as_deref_mut()?
            .map_remove(map.object_identity()?, key.weak_object_identity()?)
    }

    pub(crate) fn weak_map_entries(&self, map: &Value) -> Vec<(Value, Value)> {
        let Some(identity) = map.object_identity() else {
            return Vec::new();
        };
        self.weak_objects
            .as_deref()
            .map_or_else(Vec::new, |runtime| runtime.map_entries(identity))
    }

    pub(crate) fn clone_weak_map(&mut self, source: &Value, target: &Value) {
        if source
            .as_object()
            .is_some_and(|object| object.class_name.as_ref() == "WeakMap")
        {
            self.weak_runtime().clone_map(source, target);
        }
    }

    pub(crate) fn register_weak_map_iterator(&mut self, iterator: &Value, map: &Value) -> bool {
        self.weak_runtime().register_iterator(iterator, map)
    }

    pub(crate) fn weak_iterator_rewind(&mut self, iterator: &Value) {
        if let (Some(runtime), Some(identity)) =
            (self.weak_objects.as_deref_mut(), iterator.object_identity())
        {
            runtime.iterator_rewind(identity);
        }
    }

    pub(crate) fn weak_iterator_next(&mut self, iterator: &Value) {
        if let (Some(runtime), Some(identity)) =
            (self.weak_objects.as_deref_mut(), iterator.object_identity())
        {
            runtime.iterator_next(identity);
        }
    }

    pub(crate) fn weak_iterator_valid(&mut self, iterator: &Value) -> bool {
        let (Some(runtime), Some(identity)) =
            (self.weak_objects.as_deref_mut(), iterator.object_identity())
        else {
            return false;
        };
        runtime.iterator_entry(identity).is_some()
    }

    pub(crate) fn weak_iterator_key(&mut self, iterator: &Value) -> Option<Value> {
        self.weak_objects
            .as_deref_mut()?
            .iterator_key(iterator.object_identity()?)
    }

    pub(crate) fn weak_iterator_value(&mut self, iterator: &Value) -> Option<Value> {
        self.weak_objects
            .as_deref_mut()?
            .iterator_value(iterator.object_identity()?)
    }

    pub(crate) fn weak_iterator_allows_references(&self, iterator: &Value) -> bool {
        let Some(identity) = iterator.object_identity() else {
            return false;
        };
        self.weak_objects
            .as_deref()
            .is_some_and(|runtime| runtime.iterators.contains_key(&identity))
    }

    pub(crate) fn enable_weak_iterator_references(&mut self, iterator: &Value) {
        if let (Some(runtime), Some(identity)) =
            (self.weak_objects.as_deref_mut(), iterator.object_identity())
        {
            runtime.enable_iterator_references(identity);
        }
    }

    pub(crate) fn has_weak_object_release_work(&self, identity: usize) -> bool {
        self.weak_objects
            .as_deref()
            .is_some_and(|runtime| runtime.has_release_work(identity))
    }

    pub(crate) fn release_weak_object(&mut self, identity: usize) -> Vec<Value> {
        self.weak_objects
            .as_deref_mut()
            .map_or_else(Vec::new, |runtime| runtime.release_identity(identity))
    }
}
