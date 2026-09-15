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
    pub regex: Option<Box<RegexIteratorState>>,
    pub callback: Option<Box<NativeFilterCallback>>,
}

/// Only callback filters allocate this state. Preserve the original callable
/// (including Closure identity/captures), not a synthetic PHP Closure or an
/// untraced resolved descriptor. Scope strings contain no PHP ownership edges.
pub(crate) struct NativeFilterCallback {
    pub callable: Value,
    pub lexical_class: Option<String>,
    pub called_class: Option<String>,
    pub legacy_receiver: Option<Value>,
    pub legacy: bool,
}

/// Only native regex filters allocate this configuration. All PHP-owned
/// projections remain in the delegate's traced current/key/inner fields;
/// replacement stays an ordinary declared PHP property.
pub(crate) struct RegexIteratorState {
    pub pattern: String,
    pub pattern_is_binary: bool,
    pub compiled: std::rc::Rc<crate::regex::Regex>,
    pub unicode: bool,
    pub mode: i64,
    pub flags: i64,
    pub preg_flags: i64,
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
            regex: None,
            callback: None,
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
        if let Some(callback) = &self.callback {
            visit(&callback.callable);
            if let Some(receiver) = &callback.legacy_receiver {
                visit(receiver);
            }
        }
    }

    pub(crate) fn append_values_reversed(self, pending: &mut Vec<Value>) {
        // Callback ownership retires after the cached projections and inner
        // iterator, following PHP's filter destruction order.
        if let Some(callback) = self.callback {
            pending.extend(callback.legacy_receiver);
            pending.push(callback.callable);
        }
        pending.extend([self.inner, self.iterator, self.key, self.current]);
        if let Some(recursive) = self.recursive {
            pending.extend(recursive.frames.into_iter().map(|frame| frame.iterator));
        }
    }
}
