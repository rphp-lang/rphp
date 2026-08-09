#[cfg(unix)]
mod datagram;
#[cfg(unix)]
mod io;
#[cfg(unix)]
use self::datagram::{coroutine_udp_bind, coroutine_udp_recv_from, coroutine_udp_send_to};
#[cfg(any(target_vendor = "apple", target_os = "linux"))]
use self::io::coroutine_tcp_connect;
#[cfg(unix)]
use self::io::{
    coroutine_stream_pair, coroutine_stream_read, coroutine_stream_write, coroutine_tcp_accept,
    coroutine_tcp_listen, coroutine_wait_readable, coroutine_wait_writable,
};
use std::time::Duration;

use super::scheduler::CoroutineScheduler;
use super::state::{CoroutineEntry, cleanup_frame_chain, initialize_value_slot};
use super::{ScopeRegistration, SuspendKind, scheduler_ptr, suspend_signal};
use crate::compiler::make_internal_function;
use crate::runtime::ExecutorGlobals;
use crate::value::Value;
use crate::vm::execute::{VmError, execute_coroutine_frame};
use crate::vm::frame::ExecuteData;
use crate::vm::function::{InternalFunction, InternalFunctionHandler, UserFunction};

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

fn suspend(
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
    let caller = suspension_caller(execute_data)?;
    suspend_from_internal_call(caller, SuspendKind::Manual)
}

fn suspension_caller(execute_data: *mut ExecuteData) -> Result<*mut ExecuteData, VmError> {
    let caller = unsafe { (*execute_data).prev_execute_data };
    if caller.is_null() {
        return Err(VmError::Fatal(
            "coroutine suspension has no resumable caller frame".into(),
        ));
    }
    Ok(caller)
}

fn suspend_from_internal_call(caller: *mut ExecuteData, kind: SuspendKind) -> Result<(), VmError> {
    unsafe { (*caller).opline = (*caller).opline.add(1) };
    Err(suspend_signal(kind))
}

include!("api/core.rs");

fn positive_argument(value: &Value, function: &str, argument: &str) -> Result<u64, VmError> {
    value
        .as_long()
        .filter(|id| *id > 0)
        .map(|id| id as u64)
        .ok_or_else(|| VmError::Fatal(format!("{} expects a positive {}", function, argument)))
}

type ApiDefinition = (
    &'static str,
    InternalFunctionHandler,
    u32,
    u32,
    &'static [&'static str],
);

const CORE_API_DEFINITIONS: &[ApiDefinition] = &[
    ("coroutine_scope", coroutine_scope, 1, 1, &["callback"]),
    ("coroutine_spawn", coroutine_spawn, 1, 1, &["callback"]),
    ("coroutine_suspend", suspend, 0, 0, &[]),
    ("coroutine_resume", coroutine_resume, 1, 1, &["task"]),
    ("coroutine_join", coroutine_join, 1, 1, &["task"]),
    ("coroutine_channel", coroutine_channel, 1, 1, &["capacity"]),
    (
        "coroutine_send",
        coroutine_send,
        2,
        2,
        &["channel", "value"],
    ),
    ("coroutine_receive", coroutine_receive, 1, 1, &["channel"]),
    ("coroutine_sleep", coroutine_sleep, 1, 1, &["milliseconds"]),
];

#[cfg(unix)]
const PLATFORM_API_DEFINITIONS: &[ApiDefinition] = &[
    ("coroutine_stream_pair", coroutine_stream_pair, 0, 0, &[]),
    (
        "coroutine_tcp_listen",
        coroutine_tcp_listen,
        1,
        1,
        &["address"],
    ),
    (
        "coroutine_tcp_accept",
        coroutine_tcp_accept,
        1,
        1,
        &["listener"],
    ),
    #[cfg(any(target_vendor = "apple", target_os = "linux"))]
    (
        "coroutine_tcp_connect",
        coroutine_tcp_connect,
        2,
        1,
        &["address", "timeoutMilliseconds"],
    ),
    ("coroutine_udp_bind", coroutine_udp_bind, 1, 1, &["address"]),
    (
        "coroutine_udp_send_to",
        coroutine_udp_send_to,
        3,
        3,
        &["socket", "data", "address"],
    ),
    (
        "coroutine_udp_recv_from",
        coroutine_udp_recv_from,
        2,
        2,
        &["socket", "length"],
    ),
    (
        "coroutine_wait_readable",
        coroutine_wait_readable,
        1,
        1,
        &["stream"],
    ),
    (
        "coroutine_wait_writable",
        coroutine_wait_writable,
        1,
        1,
        &["stream"],
    ),
    (
        "coroutine_stream_read",
        coroutine_stream_read,
        2,
        2,
        &["stream", "length"],
    ),
    (
        "coroutine_stream_write",
        coroutine_stream_write,
        2,
        2,
        &["stream", "data"],
    ),
];

#[cfg(not(unix))]
const PLATFORM_API_DEFINITIONS: &[ApiDefinition] = &[];

/// Register the experimental PHP-facing coroutine API.
///
/// Registration itself is feature-gated, so a normal build neither links the
/// runtime nor allocates its internal-function descriptors.
pub fn register_api(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut functions =
        Vec::with_capacity(CORE_API_DEFINITIONS.len() + PLATFORM_API_DEFINITIONS.len());
    for &(name, handler, max_args, required_args, parameters) in CORE_API_DEFINITIONS
        .iter()
        .chain(PLATFORM_API_DEFINITIONS.iter())
    {
        let parameter_names = parameters
            .iter()
            .map(|parameter| (*parameter).to_string())
            .collect();
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
