//! Opt-in, single-threaded coroutine runtime.
//!
//! The module is compiled only with the `coroutines` feature. Ordinary builds
//! retain the exact `ExecutorGlobals`, frame and dispatch layouts. Coroutine
//! state lives in a lexical `coroutine_scope()` and borrows the active executor
//! only for individual operations; no coroutine pointer is stored in the VM.

mod scheduler;
mod state;

use std::cell::Cell;

use scheduler::CoroutineScheduler;
use state::{CoroutineEntry, cleanup_frame_chain, initialize_value_slot};

use crate::compiler::make_internal_function;
use crate::runtime::ExecutorGlobals;
use crate::value::Value;
use crate::vm::execute::{VmError, execute_coroutine_frame};
use crate::vm::frame::ExecuteData;
use crate::vm::function::{InternalFunction, UserFunction};

fn invoke_scope_root(eg: &mut ExecutorGlobals, entry: &CoroutineEntry) -> Result<Value, VmError> {
    let saved_execute_data = eg.current_execute_data.get();
    let common = unsafe { &*entry.function };
    let user = unsafe { &*(entry.function as *const UserFunction) };
    let frame = eg.vm_stack.push_call_frame(
        entry.function,
        0,
        0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    let mut result = Value::null();
    unsafe {
        (*frame).return_value = &mut result;
        (*frame).opline = user.op_array.instructions.as_ptr();
        for (offset, capture) in entry.captures.iter().enumerate() {
            initialize_value_slot(frame, common.sig.num_args + offset as u32, capture.clone());
        }
    }
    eg.current_execute_data.set(frame);

    let execution = execute_coroutine_frame(eg, frame, frame);
    let current = eg.current_execute_data.get();
    unsafe {
        cleanup_frame_chain(
            &mut eg.vm_stack,
            &mut eg.pending_call_stack,
            &mut eg.pending_named_variadic,
            current,
        );
    }
    eg.current_execute_data.set(saved_execute_data);

    execution.map(|()| result)
}

thread_local! {
    static ACTIVE_SCHEDULER: Cell<*mut CoroutineScheduler> = const {
        Cell::new(std::ptr::null_mut())
    };
    static SUSPEND_REQUESTED: Cell<bool> = const { Cell::new(false) };
}

fn suspend_signal() -> VmError {
    SUSPEND_REQUESTED.with(|requested| {
        assert!(!requested.replace(true), "nested coroutine suspend signal");
    });
    VmError::Fatal(String::new())
}

fn take_suspend_request() -> bool {
    SUSPEND_REQUESTED.with(|requested| requested.replace(false))
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

unsafe fn argument<'a>(execute_data: *mut ExecuteData, index: u32) -> &'a Value {
    unsafe {
        let value = (*execute_data).cv(index);
        if value.is_reference() {
            &*value.as_ref_ptr()
        } else {
            &*(value as *const Value)
        }
    }
}

fn write_result(return_value: *mut Value, value: Value) {
    if !return_value.is_null() {
        unsafe { return_value.write(value) };
    }
}

fn coroutine_scope(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let entry = CoroutineEntry::from_value(unsafe { argument(execute_data, 0) }, eg)?;
    let mut scheduler = CoroutineScheduler::new(eg);
    let registration = ScopeRegistration::install(&mut scheduler)?;
    let result = invoke_scope_root(eg, &entry);
    scheduler.finish_scope(eg);
    drop(registration);
    write_result(return_value, result?);
    Ok(())
}

fn coroutine_spawn(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let entry = CoroutineEntry::from_value(unsafe { argument(execute_data, 0) }, eg)?;
    let scheduler = scheduler_ptr(eg)?;
    let id = unsafe { (&mut *scheduler).spawn(entry)? };
    write_result(return_value, Value::long(id as i64));
    Ok(())
}

fn coroutine_suspend(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let scheduler = scheduler_ptr(eg)?;
    if unsafe { (*scheduler).active.is_none() } {
        return Err(VmError::Fatal(
            "coroutine_suspend can only be called by a running child".into(),
        ));
    }

    write_result(return_value, Value::null());
    let caller = unsafe { (*execute_data).prev_execute_data };
    if caller.is_null() {
        return Err(VmError::Fatal(
            "coroutine suspension has no resumable caller frame".into(),
        ));
    }
    unsafe { (*caller).opline = (*caller).opline.add(1) };
    Err(suspend_signal())
}

fn coroutine_resume(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let id = task_id(unsafe { argument(execute_data, 0) }, "coroutine_resume")?;
    let scheduler = scheduler_ptr(eg)?;
    let suspended = unsafe { CoroutineScheduler::resume(scheduler, id, eg)? };
    write_result(return_value, Value::bool(suspended));
    Ok(())
}

fn coroutine_join(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let id = task_id(unsafe { argument(execute_data, 0) }, "coroutine_join")?;
    let scheduler = scheduler_ptr(eg)?;
    let result = unsafe { CoroutineScheduler::join(scheduler, id, eg)? };
    write_result(return_value, result);
    Ok(())
}

fn task_id(value: &Value, function: &str) -> Result<u64, VmError> {
    value
        .as_long()
        .filter(|id| *id > 0)
        .map(|id| id as u64)
        .ok_or_else(|| VmError::Fatal(format!("{} expects a positive task id", function)))
}

/// Register the experimental PHP-facing coroutine API.
///
/// Registration itself is feature-gated, so a normal build neither links the
/// runtime nor allocates its five internal-function descriptors.
pub fn register_api(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let definitions = [
        ("coroutine_scope", coroutine_scope as _, 1, 1, "callback"),
        ("coroutine_spawn", coroutine_spawn as _, 1, 1, "callback"),
        ("coroutine_suspend", coroutine_suspend as _, 0, 0, ""),
        ("coroutine_resume", coroutine_resume as _, 1, 1, "task"),
        ("coroutine_join", coroutine_join as _, 1, 1, "task"),
    ];
    let mut functions = Vec::with_capacity(definitions.len());
    for (name, handler, max_args, required_args, parameter) in definitions {
        let parameter_names = if parameter.is_empty() {
            Vec::new()
        } else {
            vec![parameter.to_string()]
        };
        let function = Box::new(make_internal_function(
            handler,
            max_args,
            required_args,
            parameter_names,
        ));
        eg.register_function(name, &function.common)
            .unwrap_or_else(|error| panic!("failed to register {}: {}", name, error));
        functions.push(function);
    }
    functions
}
