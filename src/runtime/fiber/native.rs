//! Owned user activations entered by a resumable native operation.
//!
//! Unlike an ordinary detached call, this activation never points its return
//! value at a Rust local. Its VM stacks and result live in a pinned owner until
//! the operation completes. The native caller retains its own explicit phase.

use std::pin::Pin;

use super::{FiberInput, FiberSuspension};
use crate::runtime::ExecutorGlobals;
use crate::runtime::suspended::CoroutineExecutionState;
use crate::stdlib::ResolvedCallback;
use crate::value::Value;
use crate::vm::execute::{
    VmError, cleanup_detached_frame_chain, execute_coroutine_frame,
    initialize_suspended_callback_frame, inject_suspended_exception,
    resume_suspended_generator_call, write_coroutine_result,
};
use crate::vm::frame::ExecuteData;

pub(crate) enum NativeCallbackOutcome {
    Complete(Value),
    Suspended(Value),
}

/// A native builtin may retain explicit progress, never a suspended Rust
/// stack. Every PHP-owned edge must be exposed to the Fiber collector.
pub(crate) trait NativeOperation {
    fn run(
        &mut self,
        eg: &mut ExecutorGlobals,
        caller: *mut ExecuteData,
        input: Option<FiberInput>,
    ) -> Result<NativeCallbackOutcome, VmError>;
    fn cycle_snapshot(&self) -> (Vec<Value>, Vec<usize>);
}

/// An Iterator-backed yield-from advances each protocol method exactly once,
/// including when the current method has parked its detached user activation.
pub(crate) struct IteratorContinuation {
    phase: usize,
    value: Value,
    callback: Option<Pin<Box<NativeCallback>>>,
    pub(crate) input: Option<FiberInput>,
}

pub(crate) enum IteratorStep {
    Item(Value, Value),
    End,
    Suspended(Value),
}

impl IteratorContinuation {
    pub(crate) fn new(first: bool) -> Self {
        Self {
            phase: if first { 0 } else { 4 },
            value: Value::null(),
            callback: None,
            input: None,
        }
    }

    pub(crate) fn is_suspended(&self) -> bool {
        self.callback.is_some()
    }

    pub(crate) fn shutdown_roots(&self) -> Vec<Value> {
        self.callback
            .as_ref()
            .map_or_else(Vec::new, |callback| callback.shutdown_roots())
    }

    pub(crate) fn cycle_snapshot(&self) -> Vec<Value> {
        let mut values = self
            .callback
            .as_ref()
            .map_or_else(Vec::new, |callback| callback.cycle_snapshot().0);
        values.extend(self.value.clone_cycle_handle());
        if let Some(
            FiberInput::Resume(value) | FiberInput::Throw(value) | FiberInput::ForceClose(value),
        ) = &self.input
        {
            values.extend(value.clone_cycle_handle());
        }
        values
    }

    pub(crate) fn step(
        &mut self,
        eg: &mut ExecutorGlobals,
        iterator: &Value,
    ) -> Result<IteratorStep, VmError> {
        loop {
            let method = ["rewind", "valid", "current", "key", "next"][self.phase];
            if self.callback.is_none()
                && eg.has_active_fiber()
                && let Some(callback) =
                    crate::stdlib::resolve_object_public_method(eg, iterator, method)
                && callback.common().fn_type == crate::vm::function::FunctionType::User
            {
                self.callback = Some(NativeCallback::new(callback));
            }
            let result = if let Some(callback) = self.callback.as_mut() {
                let caller = eg.current_execute_data.get();
                match NativeCallback::run(callback.as_mut(), eg, self.input.take(), caller)? {
                    NativeCallbackOutcome::Suspended(value) => {
                        return Ok(IteratorStep::Suspended(value));
                    }
                    NativeCallbackOutcome::Complete(value) => {
                        self.callback = None;
                        value
                    }
                }
            } else {
                crate::stdlib::call_object_protocol_method(eg, iterator, "Iterator", method, &[])?
                    .unwrap_or_else(Value::null)
            };
            if eg.exception.is_some() {
                return Ok(IteratorStep::End);
            }
            match self.phase {
                0 | 4 => self.phase = 1,
                1 if !result.is_truthy() => return Ok(IteratorStep::End),
                1 => self.phase = 2,
                2 => {
                    self.value = result;
                    self.phase = 3;
                }
                3 => {
                    self.phase = 4;
                    return Ok(IteratorStep::Item(
                        result,
                        std::mem::replace(&mut self.value, Value::null()),
                    ));
                }
                _ => unreachable!(),
            }
        }
    }
}

pub(crate) struct NativeCallback {
    callback: ResolvedCallback,
    arguments: Vec<Value>,
    state: CoroutineExecutionState,
    pub(super) boundary: *mut ExecuteData,
    pub(super) suspension: Option<FiberSuspension>,
    result: Value,
    complete: bool,
    _pinned: std::marker::PhantomPinned,
}

impl NativeCallback {
    pub(crate) fn shutdown_roots(&self) -> Vec<Value> {
        self.callback
            .prepend_args
            .iter()
            .filter_map(Value::clone_cycle_handle)
            .collect()
    }
    pub(crate) fn new(callback: ResolvedCallback) -> Pin<Box<Self>> {
        Self::with_arguments(callback, Vec::new())
    }
    pub(crate) fn with_arguments(
        callback: ResolvedCallback,
        arguments: Vec<Value>,
    ) -> Pin<Box<Self>> {
        Box::pin(Self {
            callback,
            arguments,
            state: CoroutineExecutionState::new(),
            boundary: std::ptr::null_mut(),
            suspension: None,
            result: Value::null(),
            complete: false,
            _pinned: std::marker::PhantomPinned,
        })
    }

    pub(crate) fn cycle_snapshot(&self) -> (Vec<Value>, Vec<usize>) {
        let (mut values, mut frames) = self.state.cycle_snapshot();
        values.extend(self.arguments.iter().filter_map(Value::clone_cycle_handle));
        // The native invocation retains its receiver independently of the PHP
        // frame. This engine keepalive is not a collectable Fiber edge: PHP
        // keeps a parked Iterator -> Fiber cycle alive through that native
        // call until request force-close, even after gc_collect_cycles().
        // Publish frame locals below, but leave the descriptor as a root.
        values.extend(
            self.callback
                .use_vars
                .iter()
                .filter_map(Value::clone_cycle_handle),
        );
        if let Some(value) = &self.callback.bound_this {
            values.extend(value.clone_cycle_handle());
        }
        values.extend(self.result.clone_cycle_handle());
        if let Some(suspension) = &self.suspension {
            values.extend(suspension.value().clone_cycle_handle());
            if let FiberSuspension::Release { release, .. } = suspension {
                let (children, release_frames) = release.cycle_snapshot();
                values.extend(children);
                frames.extend(release_frames);
            }
            if let FiberSuspension::Call { call, .. } = suspension {
                let (children, call_frames) = call.cycle_snapshot();
                values.extend(children);
                frames.extend(call_frames);
            }
            if let FiberSuspension::GeneratorIteration { generator, .. } = suspension {
                values.extend(generator.clone_cycle_handle());
            }
            if let FiberSuspension::Generator {
                generator,
                arguments,
                ..
            } = suspension
            {
                values.extend(generator.clone_cycle_handle());
                values.extend(arguments.iter().filter_map(Value::clone_cycle_handle));
            }
        }
        (values, frames)
    }

    pub(crate) fn run(
        this: Pin<&mut Self>,
        eg: &mut ExecutorGlobals,
        input: Option<FiberInput>,
        logical_caller: *mut ExecuteData,
    ) -> Result<NativeCallbackOutcome, VmError> {
        // SAFETY: the caller owns the pinned activation across VM re-entry.
        // All raw frame/result pointers refer to its checked-out stacks or
        // pinned result. No Rust field borrow survives execution; the runtime
        // registry holds only a scoped raw pointer for recording suspension.
        unsafe {
            let this = this.get_unchecked_mut() as *mut Self;
            assert!(!(*this).complete);
            let runtime = eg.fiber_runtime_ptr();
            let owner = *(*runtime)
                .active
                .last()
                .expect("native callback requires an active Fiber");
            let first = (*this).boundary.is_null();
            if first {
                assert!(input.is_none());
                (*this).state.initialize_error_reporting(eg.error_reporting);
                (*this).state.stacks = Some((*runtime).pool.checkout());
            }
            (*this).state.exchange(eg);

            let mut generator_call = None;
            let mut generator_iteration = None;
            let mut native_release = None;
            let mut native_call = None;
            let entry_result = if first {
                crate::vm::execute::catch_memory_exhaustion(eg, |eg| {
                    initialize_suspended_callback_frame(
                        eg,
                        &(*this).callback,
                        &(*this).arguments,
                        &mut (*this).result,
                        logical_caller,
                    )
                })
                .inspect(|frame| (*this).boundary = *frame)
            } else {
                let suspension = (*this)
                    .suspension
                    .take()
                    .expect("native callback must retain its suspension");
                let input = input.expect("resuming a native callback requires Fiber input");
                match (suspension, input) {
                    (
                        FiberSuspension::Call {
                            frame,
                            call,
                            return_value,
                            ..
                        },
                        input,
                    ) => {
                        native_call = Some((call, return_value, input));
                        Ok(frame)
                    }
                    (FiberSuspension::Release { frame, release, .. }, input) => {
                        native_release = Some((release, input));
                        Ok(frame)
                    }
                    (
                        FiberSuspension::GeneratorIteration {
                            frame, generator, ..
                        },
                        input,
                    ) => {
                        generator_iteration = Some((generator, input));
                        Ok(frame)
                    }
                    (
                        FiberSuspension::Direct {
                            frame,
                            return_value,
                            ..
                        },
                        FiberInput::Resume(value),
                    ) => {
                        write_coroutine_result(frame, return_value, value);
                        Ok(frame)
                    }
                    (
                        FiberSuspension::Direct { frame, .. },
                        FiberInput::Throw(exception) | FiberInput::ForceClose(exception),
                    ) => inject_suspended_exception(eg, frame, exception)
                        .map(|entry| entry.unwrap_or(frame)),
                    (
                        FiberSuspension::Generator {
                            frame,
                            generator,
                            function,
                            public_num_args,
                            arguments,
                            return_value,
                            ..
                        },
                        input,
                    ) => {
                        let generator = generator
                            .as_object()
                            .and_then(|object| object.generator.clone())
                            .expect("native generator suspension retains its receiver");
                        generator.borrow_mut().fiber_resume_input = Some(match input {
                            FiberInput::Resume(value) => {
                                crate::vm::generator::GeneratorFiberInput::Resume(value)
                            }
                            FiberInput::Throw(value) => {
                                crate::vm::generator::GeneratorFiberInput::Throw(value)
                            }
                            FiberInput::ForceClose(value) => {
                                crate::vm::generator::GeneratorFiberInput::ForceClose(value)
                            }
                            FiberInput::Start(_) => unreachable!(),
                        });
                        generator_call = Some((function, public_num_args, arguments, return_value));
                        Ok(frame)
                    }
                    (_, FiberInput::Start(_)) => unreachable!(),
                }
            };
            let boundary = (*this).boundary;
            if !boundary.is_null() {
                eg.publish_detached_trace_caller_at_current_site(
                    boundary as usize,
                    logical_caller as usize,
                );
                eg.discard_detached_trace_origin(boundary as usize);
            }
            (*runtime).active_native.push((owner, this));
            let execution = match entry_result {
                Err(error) => Err(error),
                Ok(_) if eg.exception.is_some() => Ok(()),
                Ok(entry) => {
                    if let Some((call, return_value, input)) = native_call {
                        match crate::vm::execute::resume_suspended_native_call(
                            eg,
                            entry,
                            call,
                            return_value,
                            input,
                        ) {
                            Ok(Some(entry)) if eg.exception.is_none() => {
                                execute_coroutine_frame(eg, entry, boundary)
                            }
                            Ok(_) => Ok(()),
                            Err(error) => Err(error),
                        }
                    } else if let Some((release, input)) = native_release {
                        match crate::vm::execute::resume_suspended_native_release(
                            eg, entry, release, input,
                        ) {
                            Ok(Some(entry)) if eg.exception.is_none() => {
                                execute_coroutine_frame(eg, entry, boundary)
                            }
                            Ok(_) => Ok(()),
                            Err(error) => Err(error),
                        }
                    } else if let Some((generator, input)) = generator_iteration {
                        match crate::vm::execute::resume_suspended_generator_iteration(
                            eg, entry, generator, input,
                        ) {
                            Ok(Some(entry)) if eg.exception.is_none() => {
                                execute_coroutine_frame(eg, entry, boundary)
                            }
                            Ok(_) => Ok(()),
                            Err(error) => Err(error),
                        }
                    } else if let Some((function, public_num_args, arguments, return_value)) =
                        generator_call
                    {
                        match resume_suspended_generator_call(
                            eg,
                            entry,
                            function,
                            public_num_args,
                            arguments,
                            return_value,
                        ) {
                            Ok(Some(entry)) if eg.exception.is_none() => {
                                execute_coroutine_frame(eg, entry, boundary)
                            }
                            Ok(_) => Ok(()),
                            Err(error) => Err(error),
                        }
                    } else {
                        execute_coroutine_frame(eg, entry, boundary)
                    }
                }
            };
            let cleanup = if (*this).suspension.is_none() && !boundary.is_null() {
                cleanup_detached_frame_chain(eg, boundary, true, true)
            } else {
                Ok(())
            };
            assert_eq!((*runtime).active_native.pop(), Some((owner, this)));
            if !boundary.is_null() {
                eg.discard_detached_trace_caller(boundary as usize);
            }
            let escaped = eg.exception.take();
            let reporting = eg.error_reporting;
            (*this).state.exchange(eg);
            // Native callbacks stay in the same Fiber: unlike a new Fiber,
            // they inherit @ and propagate error_reporting() to their caller.
            eg.set_error_reporting(reporting);
            if let Some(suspension) = &(*this).suspension {
                assert!(
                    matches!(&execution, Err(VmError::Fatal(message)) if message.is_empty())
                        || matches!(&cleanup, Err(VmError::Fatal(message)) if message.is_empty())
                );
                return Ok(NativeCallbackOutcome::Suspended(suspension.value().clone()));
            }
            (*this).complete = true;
            (*this).state.cleanup_frames();
            (*runtime).pool.recycle(
                (*this)
                    .state
                    .stacks
                    .take()
                    .expect("native activation owns its stacks"),
            );
            if escaped.is_some() {
                eg.exception = escaped;
            }
            execution?;
            cleanup?;
            Ok(NativeCallbackOutcome::Complete(std::mem::replace(
                &mut (*this).result,
                Value::null(),
            )))
        }
    }
}

impl Drop for NativeCallback {
    fn drop(&mut self) {
        if self.state.stacks.is_some() {
            self.state.cleanup_frames();
        }
        // These descriptor copies are engine roots, not additional PHP
        // variables. The actual callback frame was retired by the VM above.
        let _snapshot = crate::value::suppress_cycle_snapshot_roots();
        self.callback.prepend_args.clear();
        self.callback.use_vars.clear();
        self.callback.bound_this = None;
        self.callback.closure_static_vars = None;
    }
}
