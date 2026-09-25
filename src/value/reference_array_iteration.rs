//! Private by-reference foreach cursors. No PHP values or array owners are
//! retained: frame-owned cursor cells and array retirement delimit every entry.
//! COW copies inherit a position until the iterator selects one of them; an
//! unrelated replacement instead starts at its own internal array pointer.
use std::cell::RefCell;
use std::collections::HashMap;

struct Cursor {
    array: Option<usize>,
    position: usize,
    copies: Vec<(usize, usize)>,
}

thread_local! {
    static CURSORS: RefCell<HashMap<usize, Cursor>> = RefCell::new(HashMap::new());
}

pub(super) fn register(key: usize, array: usize) {
    CURSORS.with(|cursors| {
        cursors.borrow_mut().insert(
            key,
            Cursor {
                array: Some(array),
                position: 0,
                copies: Vec::new(),
            },
        );
    });
}

pub(super) fn resolve(key: usize, array: usize, fallback: usize) -> usize {
    CURSORS.with(|cursors| {
        let mut cursors = cursors.borrow_mut();
        let cursor = cursors
            .get_mut(&key)
            .expect("live reference foreach cursor");
        if cursor.array != Some(array) {
            cursor.position = cursor
                .copies
                .iter()
                .find_map(|&(copy, position)| (copy == array).then_some(position))
                .unwrap_or(fallback);
            cursor.array = Some(array);
            cursor.copies.clear();
        }
        cursor.position
    })
}

pub(super) fn advance(key: usize, position: usize) {
    CURSORS.with(|cursors| {
        if let Some(cursor) = cursors.borrow_mut().get_mut(&key) {
            cursor.position = position;
        }
    });
}

pub(super) fn copied(source: usize, target: usize) {
    CURSORS.with(|cursors| {
        for cursor in cursors.borrow_mut().values_mut() {
            let position = if cursor.array == Some(source) {
                Some(cursor.position)
            } else {
                cursor
                    .copies
                    .iter()
                    .find_map(|&(copy, position)| (copy == source).then_some(position))
            };
            if let Some(position) = position {
                cursor.copies.push((target, position));
            }
        }
    });
}

pub(super) fn release_array(array: usize) {
    let _ = CURSORS.try_with(|cursors| {
        for cursor in cursors.borrow_mut().values_mut() {
            if cursor.array == Some(array) {
                cursor.array = None;
            }
            cursor.copies.retain(|&(copy, _)| copy != array);
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
    CURSORS.with(|cursors| {
        for cursor in cursors.borrow_mut().values_mut() {
            if cursor.array == Some(array) {
                adjust(&mut cursor.position);
            }
            for (copy, position) in &mut cursor.copies {
                if *copy == array {
                    adjust(position);
                }
            }
        }
    });
}

pub(super) fn release_cursor(key: usize) {
    let _ = CURSORS.try_with(|cursors| {
        cursors.borrow_mut().remove(&key);
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
        CURSORS.with(|cursors| assert!(cursors.borrow()[&key].array.is_none()));
        drop(cursor);
        CURSORS.with(|cursors| assert!(cursors.borrow().contains_key(&key)));
        drop(snapshot);
        CURSORS.with(|cursors| assert!(!cursors.borrow().contains_key(&key)));
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
        CURSORS.with(|cursors| {
            let cursors = cursors.borrow();
            assert!(cursors[&key].array.is_none());
            assert!(cursors[&key].copies.is_empty());
        });
        drop(cursor);
        CURSORS.with(|cursors| assert!(!cursors.borrow().contains_key(&key)));
    }
}
