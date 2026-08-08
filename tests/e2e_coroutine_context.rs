//! Executable prototype of the internal coroutine context-switching substrate.
//!
//! This is deliberately not a PHP-facing API yet. The first milestone proves
//! that all frame-bound executor state can be detached and restored in O(1)
//! without changing the ordinary execution path.
//!
//! Keeping the prototype in its own integration target is intentional:
//! production-linked variants failed the ordinary-runtime performance gate.
//! Milestone two will reconsider promotion together with lazy pooled stacks.

use std::collections::HashMap;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::ptr::NonNull;

use rphp::runtime::ExecutorGlobals;
use rphp::value::Value;
use rphp::vm::frame::ExecuteData;
use rphp::vm::generator::GeneratorRef;
use rphp::vm::stack::VmStack;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoroutineStatus {
    Created,
    Running,
    Suspended,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoroutineSwitchError {
    ExecutorBusy,
    ContextNotActive,
    ContextNotResumable(CoroutineStatus),
    ContextHasActiveFrame,
    SameContext,
}

/// Executor fields whose ownership follows a suspended frame chain.
///
/// Globals, classes, functions, output and interrupt state remain shared in
/// `ExecutorGlobals`. Everything here either owns stack storage or contains a
/// pointer/value whose lifetime is tied to that storage.
struct CoroutineExecutionState {
    vm_stack: VmStack,
    pending_call_stack: VmStack,
    current_execute_data: *mut ExecuteData,
    exception: Option<Value>,
    pending_named_variadic: HashMap<usize, Vec<(String, Value)>>,
    active_generator: Option<GeneratorRef>,
    pending_invoke_this: Option<Value>,
}

impl CoroutineExecutionState {
    fn new() -> Self {
        Self {
            vm_stack: VmStack::new(),
            pending_call_stack: VmStack::new_pending(),
            current_execute_data: std::ptr::null_mut(),
            exception: None,
            pending_named_variadic: HashMap::new(),
            active_generator: None,
            pending_invoke_this: None,
        }
    }

    /// Exchange the complete frame-bound state with the active executor.
    /// This performs a fixed number of pointer/container-header swaps and does
    /// not inspect a frame, stack page, Value or map entry.
    #[inline]
    fn exchange(&mut self, eg: &mut ExecutorGlobals) {
        std::mem::swap(&mut self.vm_stack, &mut eg.vm_stack);
        std::mem::swap(&mut self.pending_call_stack, &mut eg.pending_call_stack);

        let current = eg.current_execute_data.replace(self.current_execute_data);
        self.current_execute_data = current;

        std::mem::swap(&mut self.exception, &mut eg.exception);
        std::mem::swap(
            &mut self.pending_named_variadic,
            &mut eg.pending_named_variadic,
        );
        std::mem::swap(&mut self.active_generator, &mut eg.active_generator);
        std::mem::swap(&mut self.pending_invoke_this, &mut eg.pending_invoke_this);
    }
}

/// One opt-in logical execution context.
///
/// The value must remain pinned while active because `CoroutineDriver` keeps a
/// non-owning pointer for state-machine validation. A future scheduler will own
/// pinned contexts and provide structured lifetime management.
pub(crate) struct CoroutineContext {
    id: u64,
    state: CoroutineExecutionState,
    status: CoroutineStatus,
    result: Option<Value>,
    _pinned: PhantomPinned,
}

/// Opt-in owner of the active-context pointer for one executor.
///
/// Keeping this state outside `ExecutorGlobals` preserves its ordinary layout
/// and makes the complete coroutine substrate pay-for-use. The cooperative
/// scheduler will own this driver once it is introduced.
pub(crate) struct CoroutineDriver<'eg> {
    executor: &'eg mut ExecutorGlobals,
    active: Option<NonNull<CoroutineContext>>,
}

impl CoroutineContext {
    pub(crate) fn new(id: u64) -> Self {
        Self {
            id,
            state: CoroutineExecutionState::new(),
            status: CoroutineStatus::Created,
            result: None,
            _pinned: PhantomPinned,
        }
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn status(&self) -> CoroutineStatus {
        self.status
    }

    pub(crate) fn result(&self) -> Option<&Value> {
        self.result.as_ref()
    }

    fn is_resumable(&self) -> bool {
        matches!(
            self.status,
            CoroutineStatus::Created | CoroutineStatus::Suspended
        )
    }
}

impl Drop for CoroutineContext {
    fn drop(&mut self) {
        assert_ne!(
            self.status,
            CoroutineStatus::Running,
            "an active coroutine context must be suspended or completed before drop"
        );
    }
}

impl<'eg> CoroutineDriver<'eg> {
    pub(crate) fn new(executor: &'eg mut ExecutorGlobals) -> Self {
        Self {
            executor,
            active: None,
        }
    }

    pub(crate) fn has_active_context(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn executor_mut(&mut self) -> &mut ExecutorGlobals {
        self.executor
    }

    pub(crate) fn activate(
        &mut self,
        mut context: Pin<&mut CoroutineContext>,
    ) -> Result<(), CoroutineSwitchError> {
        if self.active.is_some() {
            return Err(CoroutineSwitchError::ExecutorBusy);
        }
        if !context.as_ref().get_ref().is_resumable() {
            return Err(CoroutineSwitchError::ContextNotResumable(
                context.as_ref().get_ref().status,
            ));
        }

        let context_ptr = NonNull::from(context.as_ref().get_ref());
        let context = unsafe { context.as_mut().get_unchecked_mut() };
        context.state.exchange(self.executor);
        context.status = CoroutineStatus::Running;
        self.active = Some(context_ptr);
        Ok(())
    }

    pub(crate) fn suspend(
        &mut self,
        mut context: Pin<&mut CoroutineContext>,
    ) -> Result<(), CoroutineSwitchError> {
        let context_ptr = NonNull::from(context.as_ref().get_ref());
        if self.active != Some(context_ptr)
            || context.as_ref().get_ref().status != CoroutineStatus::Running
        {
            return Err(CoroutineSwitchError::ContextNotActive);
        }

        let context = unsafe { context.as_mut().get_unchecked_mut() };
        context.state.exchange(self.executor);
        context.status = CoroutineStatus::Suspended;
        self.active = None;
        Ok(())
    }

    pub(crate) fn switch(
        &mut self,
        mut outgoing: Pin<&mut CoroutineContext>,
        mut incoming: Pin<&mut CoroutineContext>,
    ) -> Result<(), CoroutineSwitchError> {
        let outgoing_ptr = NonNull::from(outgoing.as_ref().get_ref());
        let incoming_ptr = NonNull::from(incoming.as_ref().get_ref());
        if outgoing_ptr == incoming_ptr {
            return Err(CoroutineSwitchError::SameContext);
        }
        if self.active != Some(outgoing_ptr)
            || outgoing.as_ref().get_ref().status != CoroutineStatus::Running
        {
            return Err(CoroutineSwitchError::ContextNotActive);
        }
        if !incoming.as_ref().get_ref().is_resumable() {
            return Err(CoroutineSwitchError::ContextNotResumable(
                incoming.as_ref().get_ref().status,
            ));
        }

        let outgoing = unsafe { outgoing.as_mut().get_unchecked_mut() };
        let incoming = unsafe { incoming.as_mut().get_unchecked_mut() };

        outgoing.state.exchange(self.executor);
        incoming.state.exchange(self.executor);
        outgoing.status = CoroutineStatus::Suspended;
        incoming.status = CoroutineStatus::Running;
        self.active = Some(incoming_ptr);
        Ok(())
    }

    pub(crate) fn complete(
        &mut self,
        mut context: Pin<&mut CoroutineContext>,
        result: Value,
    ) -> Result<(), CoroutineSwitchError> {
        let context_ptr = NonNull::from(context.as_ref().get_ref());
        if self.active != Some(context_ptr)
            || context.as_ref().get_ref().status != CoroutineStatus::Running
        {
            return Err(CoroutineSwitchError::ContextNotActive);
        }
        if !self.executor.current_execute_data.get().is_null() {
            return Err(CoroutineSwitchError::ContextHasActiveFrame);
        }
        let context = unsafe { context.as_mut().get_unchecked_mut() };
        context.state.exchange(self.executor);
        context.status = CoroutineStatus::Completed;
        context.result = Some(result);
        self.active = None;
        Ok(())
    }
}

impl Drop for CoroutineDriver<'_> {
    fn drop(&mut self) {
        assert!(
            self.active.is_none(),
            "a coroutine driver must not be dropped with an attached context"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;
    use std::ptr::NonNull;
    use std::time::Instant;

    use rphp::compiler::make_internal_function;
    use rphp::runtime::ExecutorGlobals;
    use rphp::value::Value;
    use rphp::vm::execute::VmError;
    use rphp::vm::frame::{CALL_FRAME_SLOTS, ExecuteData};
    use rphp::vm::generator::{Generator, new_generator_ref};

    use super::{CoroutineContext, CoroutineDriver, CoroutineStatus, CoroutineSwitchError};

    fn noop_handler(
        _execute_data: *mut ExecuteData,
        _return_value: *mut Value,
        _eg: &mut ExecutorGlobals,
    ) -> Result<(), VmError> {
        Ok(())
    }

    #[test]
    fn executor_has_no_coroutine_until_opt_in() {
        let mut eg = ExecutorGlobals::new();
        let driver = CoroutineDriver::new(&mut eg);
        assert!(!driver.has_active_context());
    }

    #[test]
    fn two_contexts_restore_isolated_frame_bound_state() {
        let function = make_internal_function(noop_handler, 0, 0, vec![]);
        let mut eg = ExecutorGlobals::new();
        let mut driver = CoroutineDriver::new(&mut eg);

        let mut first = Box::pin(CoroutineContext::new(1));
        let mut second = Box::pin(CoroutineContext::new(2));
        let first_generator = new_generator_ref(Generator::new(&function.common, Vec::new(), 0, 0));
        let second_generator =
            new_generator_ref(Generator::new(&function.common, Vec::new(), 0, 0));

        driver.activate(first.as_mut()).unwrap();
        assert_eq!(first.as_ref().get_ref().id(), 1);
        assert_eq!(first.as_ref().get_ref().status(), CoroutineStatus::Running);
        let (first_frame, first_pending_frame) = {
            let eg = driver.executor_mut();
            let frame = eg.vm_stack.push_call_frame(
                &function.common,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            eg.current_execute_data.set(frame);
            eg.exception = Some(Value::string("first"));
            eg.pending_named_variadic
                .insert(frame as usize, vec![("first".to_string(), Value::long(11))]);
            let pending_frame = eg.pending_call_stack.push_deferred_scalar_call(
                &function.common,
                0,
                0,
                frame,
                std::ptr::null_mut(),
            );
            eg.active_generator = Some(first_generator.clone());
            eg.pending_invoke_this = Some(Value::string("first-this"));
            (frame, pending_frame)
        };

        driver.switch(first.as_mut(), second.as_mut()).unwrap();
        assert_eq!(
            first.as_ref().get_ref().status(),
            CoroutineStatus::Suspended
        );
        assert_eq!(second.as_ref().get_ref().status(), CoroutineStatus::Running);
        let (second_frame, second_pending_frame) = {
            let eg = driver.executor_mut();
            assert!(eg.current_execute_data.get().is_null());
            assert!(eg.exception.is_none());
            assert!(eg.pending_named_variadic.is_empty());
            assert!(eg.active_generator.is_none());
            assert!(eg.pending_invoke_this.is_none());

            let frame = eg.vm_stack.push_call_frame(
                &function.common,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            eg.current_execute_data.set(frame);
            eg.exception = Some(Value::string("second"));
            let pending_frame = eg.pending_call_stack.push_deferred_scalar_call(
                &function.common,
                0,
                0,
                frame,
                std::ptr::null_mut(),
            );
            eg.active_generator = Some(second_generator.clone());
            eg.pending_invoke_this = Some(Value::string("second-this"));
            (frame, pending_frame)
        };
        assert_ne!(first_frame, second_frame);
        assert_ne!(first_pending_frame, second_pending_frame);

        driver.switch(second.as_mut(), first.as_mut()).unwrap();
        {
            let eg = driver.executor_mut();
            assert_eq!(eg.current_execute_data.get(), first_frame);
            assert_eq!(eg.exception.as_ref().and_then(Value::as_str), Some("first"));
            assert_eq!(eg.pending_named_variadic.len(), 1);
            let next_pending = eg.pending_call_stack.push_deferred_scalar_call(
                &function.common,
                0,
                0,
                first_frame,
                std::ptr::null_mut(),
            );
            assert_eq!(
                next_pending as usize,
                first_pending_frame as usize + CALL_FRAME_SLOTS * size_of::<Value>()
            );
            assert!(std::rc::Rc::ptr_eq(
                eg.active_generator.as_ref().unwrap(),
                &first_generator
            ));
            assert_eq!(
                eg.pending_invoke_this.as_ref().and_then(Value::as_str),
                Some("first-this")
            );

            eg.exception = None;
            eg.pending_named_variadic.clear();
            eg.active_generator = None;
            eg.pending_invoke_this = None;
            eg.current_execute_data.set(std::ptr::null_mut());
            eg.pending_call_stack.pop_call_frame(first_pending_frame);
            eg.vm_stack.pop_call_frame(first_frame);
        }
        driver.complete(first.as_mut(), Value::long(11)).unwrap();
        {
            let eg = driver.executor_mut();
            assert!(eg.current_execute_data.get().is_null());
            assert!(eg.exception.is_none());
            assert!(eg.pending_named_variadic.is_empty());
            assert!(eg.active_generator.is_none());
            assert!(eg.pending_invoke_this.is_none());
        }
        assert_eq!(
            first.as_ref().get_ref().status(),
            CoroutineStatus::Completed
        );
        assert_eq!(
            first.as_ref().get_ref().result().and_then(Value::as_long),
            Some(11)
        );

        driver.activate(second.as_mut()).unwrap();
        {
            let eg = driver.executor_mut();
            assert_eq!(eg.current_execute_data.get(), second_frame);
            assert_eq!(
                eg.exception.as_ref().and_then(Value::as_str),
                Some("second")
            );
            let next_pending = eg.pending_call_stack.push_deferred_scalar_call(
                &function.common,
                0,
                0,
                second_frame,
                std::ptr::null_mut(),
            );
            assert_eq!(
                next_pending as usize,
                second_pending_frame as usize + CALL_FRAME_SLOTS * size_of::<Value>()
            );
            assert!(std::rc::Rc::ptr_eq(
                eg.active_generator.as_ref().unwrap(),
                &second_generator
            ));
            assert_eq!(
                eg.pending_invoke_this.as_ref().and_then(Value::as_str),
                Some("second-this")
            );

            eg.exception = None;
            eg.active_generator = None;
            eg.pending_invoke_this = None;
            eg.current_execute_data.set(std::ptr::null_mut());
            eg.pending_call_stack.pop_call_frame(second_pending_frame);
            eg.vm_stack.pop_call_frame(second_frame);
        }
        driver.complete(second.as_mut(), Value::long(22)).unwrap();
        assert!(driver.executor_mut().current_execute_data.get().is_null());
        assert_eq!(
            second.as_ref().get_ref().result().and_then(Value::as_long),
            Some(22)
        );
    }

    #[test]
    fn state_machine_rejects_invalid_context_transitions() {
        let mut eg = ExecutorGlobals::new();
        let mut driver = CoroutineDriver::new(&mut eg);
        let mut first = Box::pin(CoroutineContext::new(1));
        let mut second = Box::pin(CoroutineContext::new(2));

        assert_eq!(
            driver.suspend(first.as_mut()),
            Err(CoroutineSwitchError::ContextNotActive)
        );
        driver.activate(first.as_mut()).unwrap();
        assert_eq!(
            driver.activate(second.as_mut()),
            Err(CoroutineSwitchError::ExecutorBusy)
        );
        driver.suspend(first.as_mut()).unwrap();
        driver.activate(first.as_mut()).unwrap();
        driver
            .executor_mut()
            .current_execute_data
            .set(NonNull::<ExecuteData>::dangling().as_ptr());
        assert_eq!(
            driver.complete(first.as_mut(), Value::null()),
            Err(CoroutineSwitchError::ContextHasActiveFrame)
        );
        driver
            .executor_mut()
            .current_execute_data
            .set(std::ptr::null_mut());
        driver.complete(first.as_mut(), Value::null()).unwrap();
        assert_eq!(
            driver.activate(first.as_mut()),
            Err(CoroutineSwitchError::ContextNotResumable(
                CoroutineStatus::Completed
            ))
        );
    }

    #[test]
    #[ignore = "run explicitly in release mode as the coroutine hand-off microbenchmark"]
    fn benchmark_one_million_context_handoffs() {
        const HANDOFFS: u32 = 1_000_000;

        let mut eg = ExecutorGlobals::new();
        let mut driver = CoroutineDriver::new(&mut eg);
        let mut first = Box::pin(CoroutineContext::new(1));
        let mut second = Box::pin(CoroutineContext::new(2));
        driver.activate(first.as_mut()).unwrap();

        let started = Instant::now();
        for _ in 0..HANDOFFS / 2 {
            driver.switch(first.as_mut(), second.as_mut()).unwrap();
            std::hint::black_box((&driver, first.as_ref(), second.as_ref()));
            driver.switch(second.as_mut(), first.as_mut()).unwrap();
            std::hint::black_box((&driver, first.as_ref(), second.as_ref()));
        }
        let elapsed = started.elapsed();
        let ns_per_handoff = elapsed.as_nanos() as f64 / HANDOFFS as f64;
        eprintln!(
            "coroutine context hand-off: {HANDOFFS} switches in {elapsed:?} ({ns_per_handoff:.2} ns/switch)"
        );

        driver.suspend(first.as_mut()).unwrap();
        assert_eq!(
            first.as_ref().get_ref().status(),
            CoroutineStatus::Suspended
        );
        assert_eq!(
            second.as_ref().get_ref().status(),
            CoroutineStatus::Suspended
        );
    }
}
