//! Executable prototype of the internal coroutine context-switching substrate.
//!
//! This is deliberately not a PHP-facing API yet. The first milestone proves
//! that all frame-bound executor state can be detached and restored in O(1)
//! without changing the ordinary execution path.
//!
//! Keeping the prototype in its own integration target is intentional:
//! production-linked variants failed the ordinary-runtime performance gate.
//! Milestone three will reconsider promotion together with the first
//! structured parent/child caller instead of linking unused runtime code.

use std::collections::HashMap;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::ptr::NonNull;

use rphp::runtime::ExecutorGlobals;
use rphp::value::Value;
use rphp::vm::frame::{CALL_FRAME_SLOTS, ExecuteData};
use rphp::vm::generator::GeneratorRef;
use rphp::vm::stack::VmStack;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoroutineStatus {
    Created,
    Running,
    Suspended,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoroutineSwitchError {
    ExecutorBusy,
    ContextNotActive,
    ContextNotResumable(CoroutineStatus),
    ContextHasActiveFrame,
    ContextStillActive,
    SameContext,
}

/// Executor fields whose ownership follows a suspended frame chain.
///
/// Globals, classes, functions, output and interrupt state remain shared in
/// `ExecutorGlobals`. Everything here either owns stack storage or contains a
/// pointer/value whose lifetime is tied to that storage.
struct CoroutineStacks {
    vm_stack: VmStack,
    pending_call_stack: VmStack,
}

impl CoroutineStacks {
    fn new() -> Self {
        Self {
            vm_stack: VmStack::new(),
            pending_call_stack: VmStack::new_pending(),
        }
    }
}

struct CoroutineStackPool {
    idle: Vec<CoroutineStacks>,
    created: usize,
    reused: usize,
}

impl CoroutineStackPool {
    fn new() -> Self {
        Self {
            idle: Vec::with_capacity(2),
            created: 0,
            reused: 0,
        }
    }

    fn checkout(&mut self) -> CoroutineStacks {
        if let Some(stacks) = self.idle.pop() {
            self.reused += 1;
            stacks
        } else {
            self.created += 1;
            CoroutineStacks::new()
        }
    }

    fn recycle(&mut self, stacks: CoroutineStacks) {
        self.idle.push(stacks);
    }
}

struct CoroutineExecutionState {
    stacks: Option<CoroutineStacks>,
    current_execute_data: *mut ExecuteData,
    exception: Option<Value>,
    pending_named_variadic: HashMap<usize, Vec<(String, Value)>>,
    active_generator: Option<GeneratorRef>,
    pending_invoke_this: Option<Value>,
}

impl CoroutineExecutionState {
    fn new() -> Self {
        Self {
            stacks: None,
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
        let stacks = self
            .stacks
            .as_mut()
            .expect("coroutine storage must be checked out before attachment");
        std::mem::swap(&mut stacks.vm_stack, &mut eg.vm_stack);
        std::mem::swap(&mut stacks.pending_call_stack, &mut eg.pending_call_stack);

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

    fn cleanup_suspended(&mut self) {
        let stacks = self
            .stacks
            .as_mut()
            .expect("a suspended coroutine must own checked-out storage");
        let mut frame = self.current_execute_data;
        while !frame.is_null() {
            let previous = unsafe { (*frame).prev_execute_data };
            unsafe {
                cleanup_pending_calls(stacks, &mut self.pending_named_variadic, frame);
                cleanup_frame_slots(frame);
            }
            stacks.vm_stack.pop_call_frame(frame);
            frame = previous;
        }

        self.current_execute_data = std::ptr::null_mut();
        self.exception = None;
        self.pending_named_variadic.clear();
        self.active_generator = None;
        self.pending_invoke_this = None;
    }
}

unsafe fn cleanup_frame_slots(frame: *mut ExecuteData) {
    unsafe {
        let total = (*frame).num_cvs as usize + (*frame).num_temps as usize;
        if !(*frame).has_heap_slots {
            return;
        }

        let slots = (frame as *mut Value).add(CALL_FRAME_SLOTS);
        if total <= 64 {
            let mut bitmap = (*frame).heap_bitmap;
            while bitmap != 0 {
                let index = bitmap.trailing_zeros() as usize;
                let slot = slots.add(index);
                std::ptr::drop_in_place(slot);
                slot.write(Value::undef());
                bitmap &= bitmap - 1;
            }
        } else {
            for index in 0..total {
                let slot = slots.add(index);
                if (*slot).needs_cleanup() {
                    std::ptr::drop_in_place(slot);
                    slot.write(Value::undef());
                }
            }
        }

        (*frame).has_heap_slots = false;
        (*frame).heap_bitmap = 0;
    }
}

unsafe fn cleanup_pending_calls(
    stacks: &mut CoroutineStacks,
    pending_named_variadic: &mut HashMap<usize, Vec<(String, Value)>>,
    frame: *mut ExecuteData,
) {
    unsafe {
        let mut call = (*frame).call;
        (*frame).call = std::ptr::null_mut();
        while !call.is_null() {
            let next = (*call).call;
            pending_named_variadic.remove(&(call as usize));
            cleanup_frame_slots(call);
            if (*call).is_deferred_scalar_call() {
                stacks.pending_call_stack.pop_call_frame(call);
            } else {
                stacks.vm_stack.pop_call_frame(call);
            }
            call = next;
        }
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
    pool: CoroutineStackPool,
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
        assert!(
            !matches!(
                self.status,
                CoroutineStatus::Running | CoroutineStatus::Suspended
            ),
            "a live coroutine context must be completed or explicitly discarded before drop"
        );
    }
}

impl<'eg> CoroutineDriver<'eg> {
    pub(crate) fn new(executor: &'eg mut ExecutorGlobals) -> Self {
        Self {
            executor,
            active: None,
            pool: CoroutineStackPool::new(),
        }
    }

    pub(crate) fn has_active_context(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn executor_mut(&mut self) -> &mut ExecutorGlobals {
        self.executor
    }

    fn ensure_storage(&mut self, context: &mut CoroutineContext) {
        if context.state.stacks.is_none() {
            context.state.stacks = Some(self.pool.checkout());
        }
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
        self.ensure_storage(context);
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
        self.ensure_storage(incoming);

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
        self.pool.recycle(
            context
                .state
                .stacks
                .take()
                .expect("completed coroutine must own checked-out storage"),
        );
        self.active = None;
        Ok(())
    }

    pub(crate) fn discard(
        &mut self,
        mut context: Pin<&mut CoroutineContext>,
    ) -> Result<(), CoroutineSwitchError> {
        let context_ptr = NonNull::from(context.as_ref().get_ref());
        if self.active == Some(context_ptr)
            || context.as_ref().get_ref().status == CoroutineStatus::Running
        {
            return Err(CoroutineSwitchError::ContextStillActive);
        }
        if matches!(
            context.as_ref().get_ref().status,
            CoroutineStatus::Completed | CoroutineStatus::Cancelled
        ) {
            return Err(CoroutineSwitchError::ContextNotResumable(
                context.as_ref().get_ref().status,
            ));
        }

        let context = unsafe { context.as_mut().get_unchecked_mut() };
        if context.status == CoroutineStatus::Suspended {
            context.state.cleanup_suspended();
        }
        if let Some(stacks) = context.state.stacks.take() {
            self.pool.recycle(stacks);
        }
        context.status = CoroutineStatus::Cancelled;
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
mod common;

#[cfg(test)]
#[path = "e2e_coroutine_context/tests.rs"]
mod tests;
