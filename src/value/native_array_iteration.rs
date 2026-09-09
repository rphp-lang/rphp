//! Sparse iteration metadata for native array wrappers, not ordinary arrays.
//! Bucket numbers survive compaction of the runtime's dense entry vectors.
//! No PHP values are retained here, so cursor metadata cannot extend their
//! lifetime, create reference cycles or interfere with destructor scheduling.
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Copy, Default)]
pub(crate) struct NativeArrayCursor {
    pub owner: usize,
    pub generation: usize,
    pub sorted: usize,
    pub saved: usize,
    pub live: usize,
}

pub(crate) struct NativeArrayBuckets {
    pub generation: usize,
    pub sorted: usize,
    pub positions: Vec<usize>,
    pub end: usize,
    pub object_keys: Option<Vec<String>>,
}

impl NativeArrayBuckets {
    pub fn new(len: usize) -> Self {
        Self {
            generation: 1,
            sorted: 0,
            positions: (0..len).collect(),
            end: len,
            object_keys: None,
        }
    }

    pub fn extend(&mut self, len: usize) {
        while self.positions.len() < len {
            self.positions.push(self.end);
            self.end += 1;
        }
    }

    pub fn index(&self, position: usize) -> usize {
        // No deletions is the common native traversal: direct indexing, not
        // an iterator scan or a binary search on every element.
        if self.positions.get(position) == Some(&position) {
            position
        } else {
            self.positions.partition_point(|bucket| *bucket < position)
        }
    }

    pub fn replace(&mut self, len: usize, new_table: bool) {
        self.positions.clear();
        self.positions.extend(0..len);
        self.end = len;
        self.object_keys = None;
        if new_table {
            self.generation = self
                .generation
                .checked_add(1)
                .expect("native table generation");
        } else {
            self.sorted = self.sorted.checked_add(1).expect("native sort generation");
        }
    }

    pub fn synchronize_object(&mut self, keys: Vec<String>) {
        if self.object_keys.as_ref() == Some(&keys) {
            return;
        }
        if let Some(previous) = self.object_keys.take() {
            let mut positions = Vec::with_capacity(keys.len());
            for key in &keys {
                let position = previous.iter().position(|old| old == key).map_or_else(
                    || {
                        let next = self.end;
                        self.end += 1;
                        next
                    },
                    |index| self.positions[index],
                );
                positions.push(position);
            }
            self.positions = positions;
        } else {
            self.positions = (0..keys.len()).collect();
            self.end = keys.len();
        }
        self.object_keys = Some(keys);
    }
}

#[derive(Default)]
pub(crate) struct NativeArrayIteration {
    // Native layouts are immutable; the stored value and table generation
    // are still checked on every accelerated step.
    pub storage_slot: Option<usize>,
    pub cursor: Option<Rc<RefCell<NativeArrayCursor>>>,
    pub buckets: Option<NativeArrayBuckets>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_buckets_keep_holes_and_revive_at_the_append_boundary() {
        let mut buckets = NativeArrayBuckets::new(4);
        buckets.positions.remove(1);
        assert_eq!(buckets.index(1), 1);
        assert_eq!(buckets.positions[1], 2);
        assert_eq!(buckets.index(4), 3);
        buckets.extend(4);
        assert_eq!(buckets.positions[buckets.index(4)], 4);
        buckets.replace(2, false);
        assert_eq!(buckets.index(4), 2);
        assert_eq!(buckets.generation, 1);
        buckets.replace(3, true);
        assert_eq!(buckets.generation, 2);
    }
}
