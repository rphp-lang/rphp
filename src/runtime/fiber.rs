//! PHP Fiber execution state.
//!
//! Fibers exchange the executor's VM stacks and stack-owned side tables only
//! while their callback is active. Ordinary requests allocate no registry or
//! alternate stack; the state is created lazily by `Fiber::__construct()`.

use std::collections::HashMap;
use std::pin::Pin;
use std::rc::Weak;

use super::ExecutorGlobals;
use super::suspended::{CoroutineExecutionState, CoroutineStackPool};
use crate::stdlib::ResolvedCallback;
use crate::value::{PhpObject, Value, make_error_value};
use crate::vm::execute::{
    VmError, cleanup_detached_frame_chain, execute_coroutine_frame,
    initialize_suspended_callback_frame, inject_suspended_exception, write_coroutine_result,
};
use crate::vm::frame::{CALL_FRAME_SLOTS, ExecuteData};
use crate::vm::function::FunctionType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FiberStatus {
    Created,
    Running,
    Suspended,
    Terminated,
}

pub(crate) enum FiberInput {
    Start(Vec<Value>),
    Resume(Value),
    Throw(Value),
    ForceClose(Value),
}

pub(crate) struct FiberRunOutcome {
    pub(crate) value: Value,
    pub(crate) failure: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FiberReturnState {
    NotStarted,
    NotReturned,
    Threw,
}

struct FiberSuspension {
    value: Value,
    frame: *mut ExecuteData,
    return_value: *mut Value,
}

struct FiberContext {
    object: Weak<std::cell::RefCell<PhpObject>>,
    callback: ResolvedCallback,
    state: CoroutineExecutionState,
    status: FiberStatus,
    result: Value,
    threw: bool,
    force_closing: bool,
    owned_object_references: usize,
    boundary_execute_data: *mut ExecuteData,
    suspension: Option<FiberSuspension>,
    _pinned: std::marker::PhantomPinned,
}

impl FiberContext {
    fn new(object: Weak<std::cell::RefCell<PhpObject>>, callback: ResolvedCallback) -> Self {
        Self {
            object,
            callback,
            state: CoroutineExecutionState::new(),
            status: FiberStatus::Created,
            result: Value::null(),
            threw: false,
            force_closing: false,
            owned_object_references: 0,
            boundary_execute_data: std::ptr::null_mut(),
            suspension: None,
            _pinned: std::marker::PhantomPinned,
        }
    }
}

pub(crate) struct FiberRuntime {
    contexts: HashMap<usize, Pin<Box<FiberContext>>>,
    active: Vec<usize>,
    pool: CoroutineStackPool,
}

impl FiberRuntime {
    pub(crate) fn new() -> Self {
        Self {
            contexts: HashMap::new(),
            active: Vec::new(),
            pool: CoroutineStackPool::default(),
        }
    }

    pub(crate) fn register(
        &mut self,
        identity: usize,
        object: Weak<std::cell::RefCell<PhpObject>>,
        callback: ResolvedCallback,
    ) -> bool {
        if self.contexts.contains_key(&identity) {
            return false;
        }
        self.contexts
            .insert(identity, Box::pin(FiberContext::new(object, callback)));
        true
    }

    pub(crate) fn status(&self, identity: usize) -> Option<FiberStatus> {
        self.contexts.get(&identity).map(|context| context.status)
    }

    pub(crate) fn current(&self) -> Option<Value> {
        let identity = *self.active.last()?;
        let object = self.contexts.get(&identity)?.object.upgrade()?;
        Some(Value::from_object_owner(object))
    }

    pub(crate) fn has_active(&self) -> bool {
        !self.active.is_empty()
    }

    pub(crate) fn contains(&self, identity: usize) -> bool {
        self.contexts.contains_key(&identity)
    }

    pub(crate) fn active_is_force_closing(&self) -> bool {
        self.active
            .last()
            .and_then(|identity| self.contexts.get(identity))
            .is_some_and(|context| context.force_closing)
    }

    pub(crate) fn owned_object_references(&self, identity: usize) -> usize {
        self.contexts
            .get(&identity)
            .map_or(0, |context| context.owned_object_references)
    }

    pub(crate) fn release(&mut self, identity: usize) {
        self.contexts.remove(&identity);
    }

    pub(crate) fn returned(&self, identity: usize) -> Result<Value, FiberReturnState> {
        let Some(context) = self.contexts.get(&identity) else {
            return Err(FiberReturnState::NotStarted);
        };
        match context.status {
            FiberStatus::Created => Err(FiberReturnState::NotStarted),
            FiberStatus::Running | FiberStatus::Suspended => Err(FiberReturnState::NotReturned),
            FiberStatus::Terminated if context.threw => Err(FiberReturnState::Threw),
            FiberStatus::Terminated => Ok(context.result.clone()),
        }
    }

    /// Record a cooperative boundary from the active Fiber callback. The
    /// empty fatal is an internal unwind sidecar consumed only by `run()`.
    pub(crate) fn suspend(
        runtime: *mut Self,
        internal_frame: *mut ExecuteData,
        return_value: *mut Value,
        value: Value,
    ) -> Result<(), VmError> {
        // SAFETY: ExecutorGlobals supplies its live boxed registry and the VM
        // supplies the active Fiber::suspend frame. The pinned active context
        // remains registered until run() consumes this unwind sidecar.
        unsafe {
            let runtime_ref = &mut *runtime;
            let identity = *runtime_ref
                .active
                .last()
                .ok_or_else(|| VmError::Fatal("Cannot suspend outside of a fiber".to_string()))?;
            let context = runtime_ref
                .contexts
                .get_mut(&identity)
                .expect("active Fiber context must remain registered");
            let context = context.as_mut().get_unchecked_mut();
            if context.force_closing {
                return Err(VmError::Fatal(
                    "Cannot suspend in a force-closed fiber".to_string(),
                ));
            }
            let frame = (*internal_frame).prev_execute_data;
            if frame.is_null() {
                return Err(VmError::Fatal(
                    "Cannot suspend outside of a fiber".to_string(),
                ));
            }
            let mut ancestor = frame;
            while !ancestor.is_null() && ancestor != context.boundary_execute_data {
                ancestor = (*ancestor).prev_execute_data;
            }
            if ancestor.is_null() {
                return Err(VmError::Fatal(
                    "Suspending a fiber through an internal callback is not supported by this runtime"
                        .to_string(),
                ));
            }
            assert_eq!(context.status, FiberStatus::Running);
            assert!(context.suspension.is_none());
            context.suspension = Some(FiberSuspension {
                value,
                frame,
                return_value,
            });
            // The internal call is complete from the resumed callback's point
            // of view. Resume writes its input into the result slot first.
            (*frame).opline = (*frame).opline.add(1);
            Err(VmError::Fatal(String::new()))
        }
    }

    /// Run or resume one Fiber without retaining a Rust borrow across VM
    /// re-entry. Nested Fiber methods may mutate the same boxed registry, while
    /// every pinned context pointer remains stable across HashMap growth.
    pub(crate) fn run(
        runtime: *mut Self,
        eg: &mut ExecutorGlobals,
        identity: usize,
        input: FiberInput,
        logical_caller: *mut ExecuteData,
    ) -> Result<FiberRunOutcome, VmError> {
        // SAFETY: the registry is boxed and contexts are pinned. We retain raw
        // pointers only to avoid a Rust borrow across VM re-entry; nested Fiber
        // calls may mutate the registry but cannot move either allocation.
        unsafe {
            let context = {
                let runtime_ref = &mut *runtime;
                let context = runtime_ref
                    .contexts
                    .get_mut(&identity)
                    .expect("Fiber operation requires a registered receiver");
                context.as_mut().get_unchecked_mut() as *mut FiberContext
            };

            let is_start = matches!(&input, FiberInput::Start(_));
            let is_force_close = matches!(&input, FiberInput::ForceClose(_));
            let trace_callsite = {
                let caller =
                    (!logical_caller.is_null()).then(|| (*logical_caller).prev_execute_data);
                caller.and_then(|caller| {
                    if caller.is_null()
                        || (*caller).func.is_null()
                        || (*(*caller).func).fn_type != FunctionType::User
                    {
                        return None;
                    }
                    let op_array = (*caller).op_array();
                    let opline = (*caller).opline;
                    let index = opline.offset_from(op_array.instructions.as_ptr());
                    if index < 0 || index as usize >= op_array.instructions.len() {
                        return None;
                    }
                    (*caller).opline = opline.add(1);
                    Some((caller, opline))
                })
            };
            if is_start {
                (*context)
                    .state
                    .initialize_error_reporting(eg.unsuppressed_error_reporting());
                let stacks = (&mut *runtime).pool.checkout();
                (*context).state.stacks = Some(stacks);
            }

            (*context).state.exchange(eg);
            let entry = if let FiberInput::Start(arguments) = input {
                let result = &mut (*context).result as *mut Value;
                let frame = match initialize_suspended_callback_frame(
                    eg,
                    &(*context).callback,
                    &arguments,
                    result,
                    logical_caller,
                ) {
                    Ok(frame) => frame,
                    Err(error) => {
                        (*context).state.exchange(eg);
                        (*context).status = FiberStatus::Terminated;
                        (*context).threw = true;
                        (*context).state.cleanup_frames();
                        let stacks = (*context)
                            .state
                            .stacks
                            .take()
                            .expect("started Fiber must retain alternate stack storage");
                        (&mut *runtime).pool.recycle(stacks);
                        if let Some((caller, opline)) = trace_callsite {
                            (*caller).opline = opline;
                        }
                        return Err(error);
                    }
                };
                (*context).boundary_execute_data = frame;
                frame
            } else {
                let suspension = (*context)
                    .suspension
                    .take()
                    .expect("resumed Fiber must retain its suspension boundary");
                match input {
                    FiberInput::Resume(value) => {
                        write_coroutine_result(suspension.frame, suspension.return_value, value);
                        suspension.frame
                    }
                    FiberInput::Throw(exception) => {
                        inject_suspended_exception(eg, suspension.frame, exception)
                            .unwrap_or(suspension.frame)
                    }
                    FiberInput::ForceClose(exit) => {
                        (*context).force_closing = true;
                        inject_suspended_exception(eg, suspension.frame, exit)
                            .unwrap_or(suspension.frame)
                    }
                    FiberInput::Start(_) => unreachable!(),
                }
            };

            let boundary = (*context).boundary_execute_data;
            if !is_start {
                eg.publish_detached_trace_caller(boundary as usize, logical_caller as usize);
                eg.publish_detached_trace_origin(boundary as usize, "Unknown".to_string(), 0);
            }

            (*context).status = FiberStatus::Running;
            (&mut *runtime).active.push(identity);
            let execution = if eg.exception.is_some() {
                Ok(())
            } else {
                execute_coroutine_frame(eg, entry, boundary)
            };
            if (*context).suspension.is_some() {
                // The suspended stack may itself retain the Fiber object (the
                // common Fiber::getCurrent() pattern). Cache those internal
                // handles while this already-proven raw stack traversal is
                // active, so the cold last-reference planner can distinguish
                // a self-owned cycle from an unrelated external alias.
                let mut owned_references =
                    usize::from((*context).suspension.as_ref().is_some_and(|suspension| {
                        suspension.value.object_identity() == Some(identity)
                    }));
                let mut scan_frame = eg.current_execute_data.get();
                while !scan_frame.is_null() {
                    let mut scan_activation = scan_frame;
                    loop {
                        let total =
                            ((*scan_activation).num_cvs + (*scan_activation).num_temps) as usize;
                        let slots = (scan_activation as *const Value).add(CALL_FRAME_SLOTS);
                        if total <= 64 {
                            let bitmap = (*scan_activation).owned_heap_bitmap();
                            for index in 0..total {
                                if bitmap & (1_u64 << index) != 0
                                    && (*slots.add(index)).object_identity() == Some(identity)
                                {
                                    owned_references += 1;
                                }
                            }
                        } else {
                            for index in 0..total {
                                if (*slots.add(index)).object_identity() == Some(identity) {
                                    owned_references += 1;
                                }
                            }
                        }
                        scan_activation = (*scan_activation).call;
                        if scan_activation.is_null() {
                            break;
                        }
                    }
                    scan_frame = (*scan_frame).prev_execute_data;
                }
                (*context).owned_object_references = owned_references;
            }
            let cleanup = if (*context).suspension.is_none() {
                cleanup_detached_frame_chain(eg, boundary, true)
            } else {
                Ok(())
            };
            let active = (&mut *runtime).active.pop();
            assert_eq!(active, Some(identity));
            eg.discard_detached_trace_caller(boundary as usize);
            (*context).state.exchange(eg);
            if let Some((caller, opline)) = trace_callsite {
                (*caller).opline = opline;
            }

            if let Some(suspension) = (*context).suspension.as_ref() {
                assert!(!is_force_close, "force-closed Fiber must not suspend again");
                assert!(
                    matches!(&execution, Err(VmError::Fatal(message)) if message.is_empty()),
                    "Fiber suspension signal must be consumed at its owning boundary"
                );
                (*context).status = FiberStatus::Suspended;
                return Ok(FiberRunOutcome {
                    value: suspension.value.clone(),
                    failure: None,
                });
            }

            let execution_error = execution.err();
            let cleanup_error = cleanup.err();
            let failure = (*context).state.exception.take();
            (*context).threw = failure.is_some();
            (*context).status = FiberStatus::Terminated;
            (*context).owned_object_references = 0;
            (*context).state.cleanup_frames();
            (*context).boundary_execute_data = std::ptr::null_mut();
            let stacks = (*context)
                .state
                .stacks
                .take()
                .expect("started Fiber must retain alternate stack storage");
            (&mut *runtime).pool.recycle(stacks);
            if let Some(error) = execution_error {
                return Err(error);
            }
            if let Some(error) = cleanup_error {
                return Err(error);
            }
            Ok(FiberRunOutcome {
                value: Value::null(),
                failure,
            })
        }
    }

    /// Resume a suspended Fiber with an internal, uncatchable exit object.
    /// A user exception raised by `finally` replaces that sentinel and is
    /// returned to the ordinary destructor boundary.
    pub(crate) fn force_close(
        runtime: *mut Self,
        eg: &mut ExecutorGlobals,
        identity: usize,
        logical_caller: *mut ExecuteData,
    ) -> Result<Option<Value>, VmError> {
        let exit = make_error_value("\0RPHPFiberExit", "");
        let exit_identity = exit.object_identity();
        let outcome = Self::run(
            runtime,
            eg,
            identity,
            FiberInput::ForceClose(exit),
            logical_caller,
        )?;
        Ok(outcome
            .failure
            .filter(|failure| failure.object_identity() != exit_identity))
    }
}
