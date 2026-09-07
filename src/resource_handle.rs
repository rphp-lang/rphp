use std::cell::Cell;

pub(crate) type VmResourceRelease =
    fn(&mut crate::runtime::ExecutorGlobals, i64) -> Result<(), crate::vm::execute::VmError>;

#[derive(Clone, Copy, PartialEq)]
enum ReleaseMode {
    Ordinary,
    Suppressed,
    Deferred,
}

thread_local! {
    static PHP_RELEASE_MODE: Cell<ReleaseMode> = const { Cell::new(ReleaseMode::Ordinary) };
}

/// A pending exception suppresses resource callbacks during frame unwind.
/// User object destructors run with that exception suspended, and may enter a
/// nested ordinary scope. Neither scope stores an executor pointer in TLS.
pub(crate) struct ResourceReleaseScope(ReleaseMode);

impl ResourceReleaseScope {
    #[cold]
    pub(crate) fn new(suppress: bool) -> Self {
        Self(PHP_RELEASE_MODE.with(|state| {
            state.replace(if suppress {
                ReleaseMode::Suppressed
            } else {
                ReleaseMode::Ordinary
            })
        }))
    }

    /// Root values are released before the CLI prints an uncaught diagnostic.
    /// Leave callback resources in their request registry until that diagnostic
    /// has been reported, unlike an ordinary caught-exception unwind.
    #[cold]
    pub(crate) fn defer() -> Self {
        Self(PHP_RELEASE_MODE.with(|state| state.replace(ReleaseMode::Deferred)))
    }
}

impl Drop for ResourceReleaseScope {
    fn drop(&mut self) {
        PHP_RELEASE_MODE.with(|state| state.set(self.0));
    }
}

#[inline]
pub(crate) fn php_release_is_suppressed() -> bool {
    PHP_RELEASE_MODE.with(Cell::get) == ReleaseMode::Suppressed
}

#[inline]
pub(crate) fn php_release_is_deferred() -> bool {
    PHP_RELEASE_MODE.with(Cell::get) == ReleaseMode::Deferred
}

/// Shared identity stored behind every PHP resource `Value` alias.
///
/// The close callback keeps this ownership primitive independent of the
/// concrete registry. That prevents resource lifecycle code from becoming a
/// direct dependency of ordinary `Value` clone/drop code.
pub(crate) struct ResourceHandle {
    scope: Cell<u32>,
    id: i64,
    close: fn(u32, i64),
    vm_release: Cell<Option<VmResourceRelease>>,
}

impl ResourceHandle {
    #[inline]
    pub(crate) fn new(scope: u32, id: i64, close: fn(u32, i64)) -> Self {
        debug_assert_ne!(scope, 0);
        Self {
            scope: Cell::new(scope),
            id,
            close,
            vm_release: Cell::new(None),
        }
    }

    #[inline]
    pub(crate) fn id(&self) -> i64 {
        self.id
    }

    #[inline]
    pub(crate) fn needs_vm_release(&self) -> bool {
        self.vm_release.get().is_some() && self.scope.get() != 0
    }

    /// Explicit close and request teardown retire the shared owner once. Old
    /// aliases keep their resource identity, but their final drop does not
    /// perform a second registry lookup or dispatch a retired PHP callback.
    #[inline]
    pub(crate) fn retire(&self) {
        self.scope.set(0);
        self.vm_release.set(None);
    }

    #[cold]
    pub(crate) fn set_vm_release(&self, callback: VmResourceRelease) {
        self.vm_release.set(Some(callback));
    }

    #[cold]
    pub(crate) fn take_vm_release(&self) -> Option<VmResourceRelease> {
        self.vm_release.take()
    }
}

impl Drop for ResourceHandle {
    #[cold]
    fn drop(&mut self) {
        // Rust unwinding cannot enter PHP. An unconsumed callback therefore
        // stays request-owned for the canonical shutdown phase. Normal VM
        // release (including suppressed exception release) consumes it first.
        if self.scope.get() != 0 && self.vm_release.get().is_none() {
            (self.close)(self.scope.get(), self.id);
        }
    }
}
