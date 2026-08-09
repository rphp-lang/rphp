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
            if (*call).deferred_scalar_call {
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
mod tests {
    use std::mem::size_of;
    use std::ptr::NonNull;
    use std::time::Instant;

    use rphp::compiler::compile::Compiler;
    use rphp::compiler::{make_internal_function, make_user_function};
    use rphp::lexer::Lexer;
    use rphp::parser::Parser;
    use rphp::runtime::ExecutorGlobals;
    use rphp::stdlib;
    use rphp::value::Value;
    use rphp::vm::execute::{self, VmError};
    use rphp::vm::frame::{CALL_FRAME_SLOTS, ExecuteData};
    use rphp::vm::function::FunctionCommon;
    use rphp::vm::generator::{Generator, new_generator_ref};

    use super::common::make_eg_with_capture;
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
    fn context_stacks_are_lazy_and_reused_after_completion() {
        let mut eg = ExecutorGlobals::new();
        let mut driver = CoroutineDriver::new(&mut eg);
        let mut first = Box::pin(CoroutineContext::new(1));
        let mut second = Box::pin(CoroutineContext::new(2));

        assert_eq!(driver.pool.created, 0);
        assert_eq!(driver.pool.reused, 0);
        assert!(first.as_ref().get_ref().state.stacks.is_none());
        assert!(second.as_ref().get_ref().state.stacks.is_none());

        driver.activate(first.as_mut()).unwrap();
        assert_eq!(driver.pool.created, 1);
        driver.switch(first.as_mut(), second.as_mut()).unwrap();
        assert_eq!(driver.pool.created, 2);

        for _ in 0..100 {
            driver.switch(second.as_mut(), first.as_mut()).unwrap();
            driver.switch(first.as_mut(), second.as_mut()).unwrap();
        }
        assert_eq!(driver.pool.created, 2);
        assert_eq!(driver.pool.reused, 0);

        driver.switch(second.as_mut(), first.as_mut()).unwrap();
        driver.complete(first.as_mut(), Value::long(1)).unwrap();
        assert_eq!(driver.pool.idle.len(), 1);

        driver.activate(second.as_mut()).unwrap();
        driver.complete(second.as_mut(), Value::long(2)).unwrap();
        assert_eq!(driver.pool.idle.len(), 2);

        let mut third = Box::pin(CoroutineContext::new(3));
        assert!(third.as_ref().get_ref().state.stacks.is_none());
        driver.activate(third.as_mut()).unwrap();
        assert_eq!(driver.pool.created, 2);
        assert_eq!(driver.pool.reused, 1);
        driver.complete(third.as_mut(), Value::long(3)).unwrap();
        assert_eq!(driver.pool.idle.len(), 2);
    }

    #[test]
    fn repeated_resume_and_discard_cleans_slots_exception_and_finally_state() {
        const ITERATIONS: usize = 64;

        let function = make_internal_function(noop_handler, 1, 1, vec!["value".to_string()]);
        let mut eg = ExecutorGlobals::new();
        let mut driver = CoroutineDriver::new(&mut eg);

        for iteration in 0..ITERATIONS {
            let mut context = Box::pin(CoroutineContext::new(iteration as u64));
            let mut witness = Value::string(format!("payload-{iteration}"));
            let mut pending_witness = Value::string(format!("pending-{iteration}"));
            let original_string_storage = witness.as_str().unwrap().as_ptr();
            let original_pending_storage = pending_witness.as_str().unwrap().as_ptr();
            assert!(context.as_ref().get_ref().state.stacks.is_none());

            driver.activate(context.as_mut()).unwrap();
            let frame = {
                let eg = driver.executor_mut();
                let frame = eg.vm_stack.push_call_frame(
                    &function.common,
                    1,
                    1,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                unsafe {
                    let slot = (*frame).cv_mut(0) as *mut Value;
                    slot.write(witness.clone());
                    (*frame).has_heap_slots = true;
                    (*frame).heap_bitmap = 1;
                    (*frame).pending_return_after_finally = true;
                }
                let pending_frame = eg.pending_call_stack.push_deferred_scalar_call(
                    &function.common,
                    1,
                    1,
                    frame,
                    std::ptr::null_mut(),
                );
                unsafe {
                    let slot = (*pending_frame).cv_mut(0) as *mut Value;
                    slot.write(pending_witness.clone());
                    (*pending_frame).has_heap_slots = true;
                    (*pending_frame).heap_bitmap = 1;
                    (*frame).call = pending_frame;
                }
                eg.current_execute_data.set(frame);
                eg.exception = Some(Value::string("suspended exception"));
                eg.pending_named_variadic.insert(
                    frame as usize,
                    vec![("named".to_string(), Value::string("pending"))],
                );
                eg.pending_named_variadic.insert(
                    pending_frame as usize,
                    vec![("pending".to_string(), Value::long(iteration as i64))],
                );
                frame
            };

            driver.suspend(context.as_mut()).unwrap();
            driver.activate(context.as_mut()).unwrap();
            {
                let eg = driver.executor_mut();
                assert_eq!(eg.current_execute_data.get(), frame);
                assert!(unsafe { (*frame).pending_return_after_finally });
                assert_eq!(
                    eg.exception.as_ref().and_then(Value::as_str),
                    Some("suspended exception")
                );
                assert_eq!(eg.pending_named_variadic.len(), 2);
            }

            driver.suspend(context.as_mut()).unwrap();
            driver.discard(context.as_mut()).unwrap();
            assert_eq!(
                context.as_ref().get_ref().status(),
                CoroutineStatus::Cancelled
            );
            assert!(context.as_ref().get_ref().state.stacks.is_none());
            assert!(
                context
                    .as_ref()
                    .get_ref()
                    .state
                    .current_execute_data
                    .is_null()
            );
            assert!(context.as_ref().get_ref().state.exception.is_none());
            assert!(
                context
                    .as_ref()
                    .get_ref()
                    .state
                    .pending_named_variadic
                    .is_empty()
            );

            unsafe { witness.as_string_mut().unwrap() }.push('!');
            assert_eq!(witness.as_str().unwrap().as_ptr(), original_string_storage);
            unsafe { pending_witness.as_string_mut().unwrap() }.push('!');
            assert_eq!(
                pending_witness.as_str().unwrap().as_ptr(),
                original_pending_storage
            );
        }

        assert_eq!(driver.pool.created, 1);
        assert_eq!(driver.pool.reused, ITERATIONS - 1);
        assert_eq!(driver.pool.idle.len(), 1);
    }

    #[test]
    fn pooled_contexts_preserve_real_php_exception_and_finally_semantics() {
        const ITERATIONS: usize = 32;
        const EXPECTED: &str = "caught finally";

        let source = r#"<?php
try {
    throw new Exception("boom");
} catch (Exception $e) {
    echo "caught";
} finally {
    echo " finally";
}
"#;
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let compiled = Compiler::new().compile(&statements).unwrap();
        let main_function = make_user_function(compiled.main);
        let functions = compiled.functions;

        let (mut eg, output) = make_eg_with_capture();
        let _stdlib = stdlib::register_stdlib(&mut eg);
        for (name, function) in &functions {
            eg.register_function(name, &function.common as *const FunctionCommon)
                .unwrap();
        }
        for class_def in compiled.class_defs {
            eg.register_class(class_def).unwrap();
        }

        let mut driver = CoroutineDriver::new(&mut eg);
        for id in 0..ITERATIONS {
            let mut context = Box::pin(CoroutineContext::new(id as u64));
            driver.activate(context.as_mut()).unwrap();
            let result = execute::execute(driver.executor_mut(), &main_function).unwrap();
            driver.complete(context.as_mut(), result).unwrap();
            assert_eq!(
                context.as_ref().get_ref().status(),
                CoroutineStatus::Completed
            );
        }

        assert_eq!(driver.pool.created, 1);
        assert_eq!(driver.pool.reused, ITERATIONS - 1);
        drop(driver);

        let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        assert_eq!(output, EXPECTED.repeat(ITERATIONS));
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
            driver.discard(first.as_mut()),
            Err(CoroutineSwitchError::ContextStillActive)
        );
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
        assert_eq!(
            driver.discard(first.as_mut()),
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
        driver.discard(first.as_mut()).unwrap();
        driver.discard(second.as_mut()).unwrap();
    }

    #[test]
    #[ignore = "run explicitly in release mode as the coroutine depth/slot scaling benchmark"]
    fn benchmark_context_handoff_is_depth_and_slot_independent() {
        const HANDOFFS: u32 = 1_000_000;

        fn measure(depth: usize, slots: u32) -> f64 {
            let function = make_internal_function(
                noop_handler,
                slots,
                0,
                (0..slots).map(|index| format!("slot{index}")).collect(),
            );
            let mut eg = ExecutorGlobals::new();
            let mut driver = CoroutineDriver::new(&mut eg);
            let mut first = Box::pin(CoroutineContext::new(1));
            let mut second = Box::pin(CoroutineContext::new(2));

            driver.activate(first.as_mut()).unwrap();
            let mut previous = std::ptr::null_mut();
            for _ in 0..depth {
                previous = driver.executor_mut().vm_stack.push_call_frame(
                    &function.common,
                    0,
                    0,
                    previous,
                    std::ptr::null_mut(),
                );
            }
            driver.executor_mut().current_execute_data.set(previous);

            driver.switch(first.as_mut(), second.as_mut()).unwrap();
            previous = std::ptr::null_mut();
            for _ in 0..depth {
                previous = driver.executor_mut().vm_stack.push_call_frame(
                    &function.common,
                    0,
                    0,
                    previous,
                    std::ptr::null_mut(),
                );
            }
            driver.executor_mut().current_execute_data.set(previous);

            let started = Instant::now();
            for _ in 0..HANDOFFS / 2 {
                driver.switch(second.as_mut(), first.as_mut()).unwrap();
                std::hint::black_box((&driver, first.as_ref(), second.as_ref()));
                driver.switch(first.as_mut(), second.as_mut()).unwrap();
                std::hint::black_box((&driver, first.as_ref(), second.as_ref()));
            }
            let ns_per_handoff = started.elapsed().as_nanos() as f64 / HANDOFFS as f64;

            for context in [&mut second, &mut first] {
                if context.as_ref().get_ref().status() == CoroutineStatus::Suspended {
                    driver.activate(context.as_mut()).unwrap();
                }
                let mut frame = driver.executor_mut().current_execute_data.get();
                while !frame.is_null() {
                    let previous = unsafe { (*frame).prev_execute_data };
                    driver.executor_mut().vm_stack.pop_call_frame(frame);
                    frame = previous;
                }
                driver
                    .executor_mut()
                    .current_execute_data
                    .set(std::ptr::null_mut());
                driver.complete(context.as_mut(), Value::null()).unwrap();
            }

            ns_per_handoff
        }

        fn median(depth: usize, slots: u32) -> f64 {
            let mut samples = [
                measure(depth, slots),
                measure(depth, slots),
                measure(depth, slots),
            ];
            samples.sort_by(f64::total_cmp);
            samples[1]
        }

        let shallow = median(1, 0);
        let deep_and_wide = median(64, 32);
        let slower = shallow.max(deep_and_wide);
        let faster = shallow.min(deep_and_wide);
        eprintln!(
            "coroutine scaling: depth=1/slots=0 {shallow:.2} ns/switch; depth=64/slots=32 {deep_and_wide:.2} ns/switch"
        );

        assert!(slower < 150.0);
        assert!(slower / faster < 1.35);
    }
}
