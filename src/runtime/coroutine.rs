//! Opt-in, single-threaded coroutine runtime.
//!
//! The module is compiled only with the `coroutines` feature. Ordinary builds
//! retain the exact `ExecutorGlobals`, frame and dispatch layouts. Coroutine
//! state lives in a lexical `coroutine_scope()` and borrows the active executor
//! only for individual operations; no coroutine pointer is stored in the VM.

mod api;
mod scheduler;
mod state;

use std::cell::Cell;

pub use api::register_api;
use scheduler::CoroutineScheduler;

use crate::runtime::ExecutorGlobals;
use crate::vm::execute::VmError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SuspendKind {
    Manual,
    Waiting,
}

thread_local! {
    static ACTIVE_SCHEDULER: Cell<*mut CoroutineScheduler> = const {
        Cell::new(std::ptr::null_mut())
    };
    static SUSPEND_REQUESTED: Cell<Option<SuspendKind>> = const { Cell::new(None) };
}

fn suspend_signal(kind: SuspendKind) -> VmError {
    SUSPEND_REQUESTED.with(|requested| {
        assert!(
            requested.replace(Some(kind)).is_none(),
            "nested coroutine suspend signal"
        );
    });
    VmError::Fatal(String::new())
}

fn take_suspend_request() -> Option<SuspendKind> {
    SUSPEND_REQUESTED.with(|requested| requested.replace(None))
}

struct ScopeRegistration {
    scheduler: *mut CoroutineScheduler,
}

impl ScopeRegistration {
    fn install(scheduler: &mut CoroutineScheduler) -> Result<Self, VmError> {
        ACTIVE_SCHEDULER.with(|active| {
            if !active.get().is_null() {
                return Err(VmError::Fatal(
                    "nested coroutine_scope calls are not supported".into(),
                ));
            }
            let scheduler = scheduler as *mut CoroutineScheduler;
            active.set(scheduler);
            Ok(Self { scheduler })
        })
    }
}

impl Drop for ScopeRegistration {
    fn drop(&mut self) {
        ACTIVE_SCHEDULER.with(|active| {
            assert_eq!(active.get(), self.scheduler);
            active.set(std::ptr::null_mut());
        });
    }
}

fn scheduler_ptr(eg: &mut ExecutorGlobals) -> Result<*mut CoroutineScheduler, VmError> {
    ACTIVE_SCHEDULER.with(|active| {
        let scheduler = active.get();
        if scheduler.is_null() {
            return Err(VmError::Fatal(
                "coroutine operation requires an active coroutine_scope".into(),
            ));
        }
        unsafe { (&*scheduler).verify_executor(eg)? };
        Ok(scheduler)
    })
}
