//! Linux process-control slice required by PHPUnit's interrupt and timeout
//! integration. Native handlers publish only atomics; PHP callbacks run later
//! at an ordinary VM interrupt safepoint.

use std::sync::atomic::{AtomicI32, Ordering};

use crate::compiler::make_internal_function;
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction, ParamTypeHint};

const STATE: &str = "\0pcntl";
const ASYNC: &str = "async";
const ALARM: &str = "alarm";
const HANDLER_PREFIX: &str = "handler:";
const INSTALLED_PREFIX: &str = "installed:";

static PENDING_SIGNAL: AtomicI32 = AtomicI32::new(0);

extern "C" fn publish_signal(signal: libc::c_int) {
    PENDING_SIGNAL.store(signal, Ordering::Relaxed);
}

fn return_value(pointer: *mut Value, value: Value) -> Result<(), VmError> {
    super::write_return_value(pointer, value);
    Ok(())
}

fn state_bool(eg: &ExecutorGlobals, key: &str) -> bool {
    eg.static_vars
        .get(STATE)
        .and_then(|state| state.get(key))
        .is_some_and(Value::is_truthy)
}

fn set_state_value(eg: &mut ExecutorGlobals, key: String, value: Value) {
    eg.static_vars
        .entry(STATE.to_string())
        .or_default()
        .insert(key, value);
}

fn handler_key(signal: i32) -> String {
    format!("{HANDLER_PREFIX}{signal}")
}

fn installed_key(signal: i32) -> String {
    format!("{INSTALLED_PREFIX}{signal}")
}

fn optional_argument(ed: *mut ExecuteData, index: u32) -> Option<Value> {
    super::with_raw_argument(ed, index, |value| {
        (value.value_type() != ValueType::Undef).then(|| value.clone())
    })
}

fn fn_pcntl_alarm(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(seconds) = super::typed_internal_int_argument(ed, eg, "pcntl_alarm", 0, "seconds")?
    else {
        return Ok(());
    };
    let previous = super::native_process::pcntl_alarm(seconds as u32);
    set_state_value(eg, ALARM.to_string(), Value::bool(seconds != 0));
    return_value(rv, Value::long(i64::from(previous)))
}

fn fn_pcntl_async_signals(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let previous = state_bool(eg, ASYNC);
    let enable = match optional_argument(ed, 0) {
        None => return return_value(rv, Value::bool(previous)),
        Some(value) if value.value_type() == ValueType::Null => {
            return return_value(rv, Value::bool(previous));
        }
        Some(_) => {
            let Some(enable) =
                super::typed_internal_bool_argument(ed, eg, "pcntl_async_signals", 0, "enable")?
            else {
                return Ok(());
            };
            enable
        }
    };
    set_state_value(eg, ASYNC.to_string(), Value::bool(enable));
    if enable {
        // Keep the ordinary safepoint poll armed while async delivery is
        // enabled. The native handler itself only stores PENDING_SIGNAL, so it
        // never dereferences request memory from signal context.
        eg.vm_interrupt.store(true, Ordering::Relaxed);
    } else if !eg.timed_out.load(Ordering::Relaxed) {
        // Do not erase a timeout interrupt that raced with disabling async
        // signal polling; the shared safepoint flag must still service it.
        eg.vm_interrupt.store(false, Ordering::Relaxed);
    }
    return_value(rv, Value::bool(previous))
}

fn fn_pcntl_signal(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(signal) = super::typed_internal_int_argument(ed, eg, "pcntl_signal", 0, "signal")?
    else {
        return Ok(());
    };
    if signal < 1 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "pcntl_signal(): Argument #1 ($signal) must be greater than or equal to 1",
        ));
        return Ok(());
    }
    if signal >= 65 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "pcntl_signal(): Argument #1 ($signal) must be less than 65",
        ));
        return Ok(());
    }
    let signal = signal as i32;
    let handler = super::owned_argument(ed, 1);
    let restart_syscalls = if optional_argument(ed, 2).is_some() {
        let Some(restart) =
            super::typed_internal_bool_argument(ed, eg, "pcntl_signal", 2, "restart_syscalls")?
        else {
            return Ok(());
        };
        restart
    } else {
        true
    };

    let native_handler = if handler.value_type() == ValueType::Long {
        match handler.as_long().unwrap_or_default() {
            0 => libc::SIG_DFL,
            1 => libc::SIG_IGN,
            _ => {
                eg.exception = Some(crate::value::make_error_value(
                    "ValueError",
                    "pcntl_signal(): Argument #2 ($handler) must be either SIG_DFL or SIG_IGN when an integer value is given",
                ));
                return Ok(());
            }
        }
    } else {
        let Some(_) = super::resolve_callback_at_callsite_checked(&handler, eg, ed)? else {
            if eg.exception.is_none() {
                super::typed_internal_argument_error(
                    eg,
                    "pcntl_signal",
                    &handler,
                    2,
                    "handler",
                    "callable|int",
                );
            }
            return Ok(());
        };
        publish_signal as *const () as usize
    };

    let installed = super::native_process::pcntl_signal(signal, native_handler, restart_syscalls);
    if installed {
        set_state_value(eg, installed_key(signal), Value::bool(true));
        if native_handler == publish_signal as *const () as usize {
            set_state_value(eg, handler_key(signal), handler);
        } else if let Some(state) = eg.static_vars.get_mut(STATE) {
            state.remove(&handler_key(signal));
        }
        if state_bool(eg, ASYNC) {
            eg.vm_interrupt.store(true, Ordering::Relaxed);
        }
    }
    return_value(rv, Value::bool(installed))
}

/// Run one coalesced process signal at a VM safepoint. Returns `true` when the
/// callback left a catchable PHP exception for the interrupted frame.
pub(crate) fn dispatch_pending_signal(eg: &mut ExecutorGlobals) -> Result<bool, VmError> {
    if !state_bool(eg, ASYNC) {
        return Ok(false);
    }
    let signal = PENDING_SIGNAL.swap(0, Ordering::Relaxed);
    if signal == 0 {
        return Ok(false);
    }
    let handler = eg
        .static_vars
        .get(STATE)
        .and_then(|state| state.get(&handler_key(signal)))
        .cloned();
    let Some(handler) = handler else {
        return Ok(false);
    };
    let current = eg.current_execute_data.get();
    let lexical = crate::vm::execute::lexical_class_name_for_internal_call(eg, current);
    let Some(resolved) = super::resolve_callback(&handler, eg, lexical.as_deref()) else {
        return Ok(false);
    };
    let arguments = [
        Value::long(i64::from(signal)),
        Value::array(PhpArray::new()),
    ];
    if current.is_null() {
        let _ = super::call_resolved_with_values(eg, &resolved, &arguments)?;
    } else {
        let _ = super::call_resolved_with_values_from_internal(
            current, eg, &resolved, &arguments, true,
        )?;
    }
    Ok(eg.exception.is_some())
}

#[inline]
pub(crate) fn has_pending_async_signal(eg: &ExecutorGlobals) -> bool {
    state_bool(eg, ASYNC) && PENDING_SIGNAL.load(Ordering::Relaxed) != 0
}

#[inline]
pub(crate) fn async_signal_polling_enabled(eg: &ExecutorGlobals) -> bool {
    state_bool(eg, ASYNC)
}

/// Restore process-global dispositions before the request-owned handler values
/// and interrupt flag allocation can be released.
pub(crate) fn shutdown(eg: &ExecutorGlobals) {
    let Some(state) = eg.static_vars.get(STATE) else {
        // Signal dispositions and alarms are process-global. An unrelated
        // ExecutorGlobals may finish on another embedding thread while the
        // request that installed a PCNTL handler is still active; it must not
        // cancel or reset that request's native state.
        return;
    };
    let _ = super::native_process::pcntl_alarm(0);
    for key in state.keys() {
        if let Some(signal) = key
            .strip_prefix(INSTALLED_PREFIX)
            .and_then(|signal| signal.parse::<i32>().ok())
        {
            let _ = super::native_process::pcntl_signal(signal, libc::SIG_DFL, true);
        }
    }
    PENDING_SIGNAL.store(0, Ordering::Relaxed);
    eg.vm_interrupt.store(false, Ordering::Relaxed);
}

#[cold]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut functions = Vec::with_capacity(3);
    for (name, handler, maximum, required, parameter_names, parameter_types, return_type) in [
        (
            "pcntl_alarm",
            fn_pcntl_alarm as crate::vm::function::InternalFunctionHandler,
            1,
            1,
            &["seconds"][..],
            vec![ParamTypeHint::Int],
            ParamTypeHint::Int,
        ),
        (
            "pcntl_async_signals",
            fn_pcntl_async_signals,
            1,
            0,
            &["enable"][..],
            vec![ParamTypeHint::Nullable(Box::new(ParamTypeHint::Bool))],
            ParamTypeHint::Bool,
        ),
        (
            "pcntl_signal",
            fn_pcntl_signal,
            3,
            2,
            &["signal", "handler", "restart_syscalls"][..],
            vec![ParamTypeHint::Int, ParamTypeHint::None, ParamTypeHint::Bool],
            ParamTypeHint::Bool,
        ),
    ] {
        let mut function = Box::new(make_internal_function(
            handler,
            maximum,
            required,
            parameter_names
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        ));
        function.common.sig.param_type_hints = parameter_types;
        function.common.sig.return_type_hint = return_type;
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(name, pointer).unwrap();
        let defaults = match name {
            "pcntl_async_signals" => vec![Some(Value::null())],
            "pcntl_signal" => vec![None, None, Some(Value::bool(true))],
            _ => vec![None],
        };
        eg.register_internal_function_reflection_metadata(pointer, defaults, "pcntl");
        functions.push(function);
    }
    functions
}
