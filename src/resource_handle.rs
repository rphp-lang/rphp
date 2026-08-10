/// Shared identity stored behind every PHP resource `Value` alias.
///
/// The close callback keeps this ownership primitive independent of the
/// concrete registry. That prevents resource lifecycle code from becoming a
/// direct dependency of ordinary `Value` clone/drop code.
pub(crate) struct ResourceHandle {
    scope: u32,
    id: i64,
    close: fn(u32, i64),
}

impl ResourceHandle {
    #[inline]
    pub(crate) fn new(scope: u32, id: i64, close: fn(u32, i64)) -> Self {
        debug_assert_ne!(scope, 0);
        Self { scope, id, close }
    }

    #[inline]
    pub(crate) fn id(&self) -> i64 {
        self.id
    }
}

impl Drop for ResourceHandle {
    #[cold]
    fn drop(&mut self) {
        (self.close)(self.scope, self.id);
    }
}
