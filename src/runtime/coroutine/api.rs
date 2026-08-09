#[cfg(unix)]
use std::borrow::Cow;
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

fn coroutine_resume(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let id = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_resume",
        "task id",
    )?;
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
    let id = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_join",
        "task id",
    )?;
    let scheduler = scheduler_ptr(eg)?;
    let result = unsafe { CoroutineScheduler::join(scheduler, id, eg)? };
    write_result(return_value, result);
    Ok(())
}

fn coroutine_channel(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let capacity = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_channel",
        "capacity",
    )?;
    let capacity = usize::try_from(capacity).map_err(|_| {
        VmError::Fatal("coroutine_channel capacity exceeds the platform limit".into())
    })?;
    let scheduler = scheduler_ptr(eg)?;
    let id = unsafe { (&mut *scheduler).create_channel(capacity)? };
    write_result(return_value, Value::long(id as i64));
    Ok(())
}

fn coroutine_send(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let channel = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_send",
        "channel id",
    )?;
    let value = unsafe { argument(execute_data, 1) }.clone();
    let caller = suspension_caller(execute_data)?;
    let scheduler = scheduler_ptr(eg)?;
    write_result(return_value, Value::null());
    if unsafe { (&mut *scheduler).send(channel, value)? } {
        suspend_from_internal_call(caller, SuspendKind::Waiting)
    } else {
        Ok(())
    }
}

fn coroutine_receive(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let channel = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_receive",
        "channel id",
    )?;
    let caller = suspension_caller(execute_data)?;
    let scheduler = scheduler_ptr(eg)?;
    match unsafe { (&mut *scheduler).receive(channel, caller, return_value)? } {
        Some(value) => {
            write_result(return_value, value);
            Ok(())
        }
        None => {
            write_result(return_value, Value::null());
            suspend_from_internal_call(caller, SuspendKind::Waiting)
        }
    }
}

fn coroutine_sleep(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let milliseconds = unsafe { argument(execute_data, 0) }
        .as_long()
        .filter(|milliseconds| *milliseconds >= 0)
        .map(|milliseconds| milliseconds as u64)
        .ok_or_else(|| {
            VmError::Fatal("coroutine_sleep expects non-negative milliseconds".into())
        })?;
    let caller = suspension_caller(execute_data)?;
    let scheduler = scheduler_ptr(eg)?;
    write_result(return_value, Value::null());
    unsafe { (&mut *scheduler).sleep(Duration::from_millis(milliseconds))? };
    suspend_from_internal_call(caller, SuspendKind::Waiting)
}

#[cfg(unix)]
fn coroutine_stream_pair(
    _execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let scheduler = scheduler_ptr(eg)?;
    let (first, second) = unsafe { (&mut *scheduler).create_stream_pair()? };
    let mut streams = crate::value::PhpArray::with_packed_capacity(2);
    streams.push(Value::long(first as i64));
    streams.push(Value::long(second as i64));
    write_result(return_value, Value::array(streams));
    Ok(())
}

#[cfg(unix)]
fn coroutine_wait_readable(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let stream = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_wait_readable",
        "stream id",
    )?;
    let caller = suspension_caller(execute_data)?;
    let scheduler = scheduler_ptr(eg)?;
    unsafe { (&mut *scheduler).wait_readable(stream)? };
    write_result(return_value, Value::null());
    suspend_from_internal_call(caller, SuspendKind::Waiting)
}

#[cfg(unix)]
fn coroutine_wait_writable(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let stream = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_wait_writable",
        "stream id",
    )?;
    let caller = suspension_caller(execute_data)?;
    let scheduler = scheduler_ptr(eg)?;
    unsafe { (&mut *scheduler).wait_writable(stream)? };
    write_result(return_value, Value::null());
    suspend_from_internal_call(caller, SuspendKind::Waiting)
}

#[cfg(unix)]
fn coroutine_stream_read(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const MAX_READ_LENGTH: u64 = 8 * 1024 * 1024;

    let stream = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_stream_read",
        "stream id",
    )?;
    let length = positive_argument(
        unsafe { argument(execute_data, 1) },
        "coroutine_stream_read",
        "length",
    )?;
    if length > MAX_READ_LENGTH {
        return Err(VmError::Fatal(format!(
            "coroutine_stream_read length exceeds {} bytes",
            MAX_READ_LENGTH
        )));
    }
    let length = usize::try_from(length).map_err(|_| {
        VmError::Fatal("coroutine_stream_read length exceeds the platform limit".into())
    })?;
    let scheduler = scheduler_ptr(eg)?;
    let result = unsafe { (&mut *scheduler).read_stream(stream, length)? };
    write_result(
        return_value,
        result.map_or_else(
            || Value::bool(false),
            |bytes| Value::string(bytes_to_php_string(&bytes)),
        ),
    );
    Ok(())
}

#[cfg(unix)]
fn coroutine_stream_write(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let stream = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_stream_write",
        "stream id",
    )?;
    let data = unsafe { argument(execute_data, 1) }
        .as_str()
        .ok_or_else(|| VmError::Fatal("coroutine_stream_write expects string data".into()))?;
    let bytes = php_string_to_bytes(data);
    let scheduler = scheduler_ptr(eg)?;
    let result = unsafe { (&mut *scheduler).write_stream(stream, bytes.as_ref())? };
    write_result(
        return_value,
        result.map_or_else(|| Value::bool(false), |written| Value::long(written as i64)),
    );
    Ok(())
}

#[cfg(unix)]
fn bytes_to_php_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| *byte as char).collect()
}

#[cfg(unix)]
fn php_string_to_bytes(value: &str) -> Cow<'_, [u8]> {
    if value.is_ascii() {
        Cow::Borrowed(value.as_bytes())
    } else {
        Cow::Owned(value.chars().map(|character| character as u8).collect())
    }
}

fn positive_argument(value: &Value, function: &str, argument: &str) -> Result<u64, VmError> {
    value
        .as_long()
        .filter(|id| *id > 0)
        .map(|id| id as u64)
        .ok_or_else(|| VmError::Fatal(format!("{} expects a positive {}", function, argument)))
}

/// Register the experimental PHP-facing coroutine API.
///
/// Registration itself is feature-gated, so a normal build neither links the
/// runtime nor allocates its internal-function descriptors.
pub fn register_api(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    type ApiDefinition = (
        &'static str,
        InternalFunctionHandler,
        u32,
        u32,
        &'static [&'static str],
    );

    let mut definitions: Vec<ApiDefinition> = vec![
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
    let io_definitions: [ApiDefinition; 5] = [
        ("coroutine_stream_pair", coroutine_stream_pair, 0, 0, &[]),
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
    #[cfg(unix)]
    definitions.extend(io_definitions);
    let mut functions = Vec::with_capacity(definitions.len());
    for (name, handler, max_args, required_args, parameters) in definitions {
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
