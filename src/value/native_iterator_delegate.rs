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
        }
    }

    pub(crate) fn for_each_value(&self, mut visit: impl FnMut(&Value)) {
        visit(&self.current);
        visit(&self.key);
        visit(&self.iterator);
        visit(&self.inner);
    }

    pub(crate) fn append_values_reversed(self, pending: &mut Vec<Value>) {
        pending.extend([self.inner, self.iterator, self.key, self.current]);
    }
}
