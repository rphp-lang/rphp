//! Sparse, object-owned delegation edges. These are not PHP properties, but
//! are traced and released by the same ownership walkers as property values.
use super::Value;

pub(crate) struct NativeIteratorDelegate {
    // PHP retires cached projections before the delegated iterator.
    pub current: Value,
    pub key: Value,
    pub iterator: Value,
    pub inner: Value,
    pub position: i64,
    pub offset: i64,
    pub limit: i64,
    pub recursive: Option<Box<RecursiveTraversal>>,
}

#[derive(Clone, Copy)]
pub(crate) enum RecursivePhase {
    Check,
    Descend,
    AfterChildren,
    Advance,
}

pub(crate) struct RecursiveFrame {
    pub iterator: Value,
    pub phase: RecursivePhase,
}

/// Allocated only for recursive drivers. Every iterator is an ordinary traced
/// ownership edge; the reusable depth stack is independent of the Rust stack.
pub(crate) struct RecursiveTraversal {
    pub frames: Vec<RecursiveFrame>,
    pub mode: i64,
    pub flags: i64,
    pub max_depth: i64,
    pub in_iteration: bool,
    pub generation: u64,
}

impl NativeIteratorDelegate {
    pub(crate) fn new(inner: Value, iterator: Value, offset: i64, limit: i64) -> Self {
        Self {
            current: Value::undef(),
            key: Value::undef(),
            iterator,
            inner,
            position: 0,
            offset,
            limit,
            recursive: None,
        }
    }

    pub(crate) fn for_each_value(&self, mut visit: impl FnMut(&Value)) {
        if let Some(recursive) = &self.recursive {
            for frame in &recursive.frames {
                visit(&frame.iterator);
            }
        }
        visit(&self.current);
        visit(&self.key);
        visit(&self.iterator);
        visit(&self.inner);
    }

    pub(crate) fn append_values_reversed(self, pending: &mut Vec<Value>) {
        pending.extend([self.inner, self.iterator, self.key, self.current]);
        if let Some(recursive) = self.recursive {
            pending.extend(recursive.frames.into_iter().map(|frame| frame.iterator));
        }
    }
}
