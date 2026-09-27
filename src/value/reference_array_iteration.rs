//! Private by-reference foreach cursors. No PHP values or array owners are
//! retained: frame-owned cursor cells and array retirement delimit every entry.
//! COW copies inherit a position until the iterator selects one of them; an
//! unrelated replacement instead starts at its own internal array pointer.
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;

use super::IntKeyHasher;

type IdentityMap<V> = HashMap<usize, V, BuildHasherDefault<IntKeyHasher>>;

struct Cursor {
    array: Option<usize>,
    position: usize,
    /// Positions remembered for copies of the iterated array, keyed by copy
    /// identity.
    copies: IdentityMap<usize>,
}

/// Live cursors plus the reverse index from array identity to the cursors
/// that reference it (as the iterated array or as a remembered copy). Array
/// copies and releases consult the index instead of scanning every cursor:
/// a flagged array with no interested cursor costs one lookup.
#[derive(Default)]
struct Registry {
    cursors: IdentityMap<Cursor>,
    by_array: IdentityMap<Vec<usize>>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

fn link(by_array: &mut IdentityMap<Vec<usize>>, array: usize, key: usize) {
    by_array.entry(array).or_default().push(key);
}

fn unlink_one(by_array: &mut IdentityMap<Vec<usize>>, array: usize, key: usize) {
    if let Some(keys) = by_array.get_mut(&array) {
        keys.retain(|candidate| *candidate != key);
        if keys.is_empty() {
            by_array.remove(&array);
        }
    }
}

fn unlink(by_array: &mut IdentityMap<Vec<usize>>, key: usize, cursor: &Cursor) {
    if let Some(array) = cursor.array {
        unlink_one(by_array, array, key);
    }
    for array in cursor.copies.keys() {
        unlink_one(by_array, *array, key);
    }
}

pub(super) fn register(key: usize, array: usize) {
    REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let cursor = Cursor {
            array: Some(array),
            position: 0,
            copies: IdentityMap::default(),
        };
        if let Some(previous) = registry.cursors.insert(key, cursor) {
            unlink(&mut registry.by_array, key, &previous);
        }
        link(&mut registry.by_array, array, key);
    });
}

pub(super) fn resolve(key: usize, array: usize, fallback: usize) -> usize {
    REGISTRY.with(|registry| {
        let registry = &mut *registry.borrow_mut();
        let cursor = registry
            .cursors
            .get_mut(&key)
            .expect("live reference foreach cursor");
        if cursor.array != Some(array) {
            let position = cursor.copies.get(&array).copied().unwrap_or(fallback);
            let previous = std::mem::replace(
                cursor,
                Cursor {
                    array: Some(array),
                    position,
                    copies: IdentityMap::default(),
                },
            );
            unlink(&mut registry.by_array, key, &previous);
            link(&mut registry.by_array, array, key);
        }
        cursor.position
    })
}

pub(super) fn advance(key: usize, position: usize) {
    REGISTRY.with(|registry| {
        if let Some(cursor) = registry.borrow_mut().cursors.get_mut(&key) {
            cursor.position = position;
        }
    });
}

/// Records `target` as a copy of `source` for every cursor interested in
/// `source`. Returns whether any cursor was interested, so callers can stop
/// propagating the reference-foreach flag to copies nobody iterates.
pub(super) fn copied(source: usize, target: usize) -> bool {
    REGISTRY.with(|registry| {
        let registry = &mut *registry.borrow_mut();
        let Some(keys) = registry.by_array.get(&source) else {
            return false;
        };
        let mut linked = Vec::with_capacity(keys.len());
        for key in keys {
            let Some(cursor) = registry.cursors.get_mut(key) else {
                continue;
            };
            let position = if cursor.array == Some(source) {
                Some(cursor.position)
            } else {
                cursor.copies.get(&source).copied()
            };
            if let Some(position) = position
                && cursor.copies.insert(target, position).is_none()
            {
                linked.push(*key);
            }
        }
        if !linked.is_empty() {
            registry.by_array.entry(target).or_default().extend(linked);
        }
        true
    })
}

pub(super) fn release_array(array: usize) {
    let _ = REGISTRY.try_with(|registry| {
        let registry = &mut *registry.borrow_mut();
        let Some(keys) = registry.by_array.remove(&array) else {
            return;
        };
        for key in keys {
            if let Some(cursor) = registry.cursors.get_mut(&key) {
                if cursor.array == Some(array) {
                    cursor.array = None;
                }
                cursor.copies.remove(&array);
            }
        }
    });
}

pub(super) fn splice(array: usize, start: usize, removed: usize, inserted: usize) {
    let adjust = |position: &mut usize| {
        if start < *position {
            let removed_before = start.saturating_add(removed).min(*position) - start;
            *position = position
                .saturating_sub(removed_before)
                .saturating_add(inserted);
        }
    };
    REGISTRY.with(|registry| {
        let registry = &mut *registry.borrow_mut();
        let Some(keys) = registry.by_array.get(&array) else {
            return;
        };
        for key in keys {
            let Some(cursor) = registry.cursors.get_mut(key) else {
                continue;
            };
            if cursor.array == Some(array) {
                adjust(&mut cursor.position);
            }
            if let Some(position) = cursor.copies.get_mut(&array) {
                adjust(position);
            }
        }
    });
}

pub(super) fn release_cursor(key: usize) {
    let _ = REGISTRY.try_with(|registry| {
        let registry = &mut *registry.borrow_mut();
        if let Some(cursor) = registry.cursors.remove(&key) {
            unlink(&mut registry.by_array, key, &cursor);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{PhpArray, Value};

    #[test]
    fn reference_cursor_does_not_retain_arrays_and_last_snapshot_retires_it() {
        let mut array = PhpArray::new();
        array.push(Value::long(17));
        let source = Value::array(array);
        let cursor = Value::reference_foreach_cursor(&source);
        let key = cursor.reference_identity().unwrap();
        assert_eq!(source.cycle_strong_count(), Some(1));
        let snapshot = cursor.clone_closure_capture();
        drop(source);
        REGISTRY.with(|registry| assert!(registry.borrow().cursors[&key].array.is_none()));
        drop(cursor);
        REGISTRY.with(|registry| assert!(registry.borrow().cursors.contains_key(&key)));
        drop(snapshot);
        REGISTRY.with(|registry| {
            let registry = registry.borrow();
            assert!(!registry.cursors.contains_key(&key));
            assert!(registry.by_array.is_empty());
        });
    }

    #[test]
    fn reference_cursor_copies_are_selected_once_and_array_retirement_invalidates_them() {
        let mut array = PhpArray::new();
        array.push(Value::long(17));
        array.push(Value::long(23));
        let source = Value::array(array);
        let mut cursor = Value::reference_foreach_cursor(&source);
        cursor.set_reference_foreach_position(1);
        let mut copy = source.clone();
        copy.as_array_mut().unwrap().push(Value::long(31));
        assert_eq!(cursor.reference_foreach_position(&copy), Some(1));
        cursor.set_reference_foreach_position(2);
        assert_eq!(cursor.reference_foreach_position(&source), Some(0));
        let key = cursor.reference_identity().unwrap();
        drop(source);
        drop(copy);
        REGISTRY.with(|registry| {
            let registry = registry.borrow();
            assert!(registry.cursors[&key].array.is_none());
            assert!(registry.cursors[&key].copies.is_empty());
            assert!(registry.by_array.is_empty());
        });
        drop(cursor);
        REGISTRY.with(|registry| {
            let registry = registry.borrow();
            assert!(!registry.cursors.contains_key(&key));
            assert!(registry.by_array.is_empty());
        });
    }
}
