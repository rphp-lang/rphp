//! Lexically enabled ticks and their sparse, request-owned callback roots.
//! No state is polled or allocated by ordinary bytecode execution.

use super::*;
use crate::value::ArrayKey;

const STATE: &str = "\0tick_callbacks";
const ENTRIES: &str = "entries";
const ACTIVE: &str = "active";
const COUNTER: &str = "counter";
const NEXT: &str = "next";

pub(super) fn is_executing(eg: &ExecutorGlobals) -> bool {
    eg.static_vars
        .get(STATE)
        .and_then(|state| state.get(ACTIVE))
        .is_some_and(|active| active.to_long_val() != 0)
}

fn state(eg: &mut ExecutorGlobals) -> &mut std::collections::HashMap<String, Value> {
    if !eg.static_vars.contains_key(STATE) {
        eg.static_vars.insert(
            STATE.into(),
            [
                (COUNTER.into(), Value::long(0)),
                (ACTIVE.into(), Value::long(0)),
                (NEXT.into(), Value::long(1)),
            ]
            .into_iter()
            .collect(),
        );
    }
    eg.static_vars
        .get_mut(STATE)
        .expect("initialized tick state")
}

fn resolve(
    callback: &Value,
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<Option<ResolvedCallback>, VmError> {
    let resolved = resolve_callback_at_callsite_checked(callback, eg, ed)?;
    if resolved.is_none() && eg.exception.is_none() {
        let detail = ordinary_callback_invalid_reason(callback, eg);
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("{function}(): Argument #1 ($callback) must be a valid callback, {detail}"),
        ));
    }
    Ok(resolved)
}

pub(super) fn register(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 0).clone();
    let Some(resolved) = resolve(&callback, ed, eg, "register_tick_function")? else {
        return Ok(());
    };
    let identity = autoload::canonical_callback(callback.clone(), &resolved, eg);
    // Magic methods need the missing member spelling and argument repacking.
    // Ordinary resolved closures retain registration-time private scope and
    // reference captures rather than resolving them from the future caller.
    let invocation = if resolved.is_magic_call {
        callback
    } else {
        resolved_callback_into_closure(resolved, eg)
    };
    let arguments = arg_opt!(ed, 1)
        .cloned()
        .unwrap_or_else(|| Value::array(PhpArray::new()));
    let mut entry = PhpArray::with_packed_capacity(3);
    entry.push(identity);
    entry.push(invocation);
    entry.push(arguments);
    let entry = Value::array(entry);
    eg.note_request_static_value(&entry);
    let state = state(eg);
    let next = state.get_mut(NEXT).unwrap();
    let id = next.to_long_val();
    *next = Value::long(id + 1);
    state
        .entry(ENTRIES.into())
        .or_insert_with(|| Value::array(PhpArray::new()))
        .as_array_mut()
        .expect("tick registry is an array")
        .set_int(id, entry);
    ret!(rv, Value::bool(true));
}

pub(super) fn unregister(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 0).clone();
    let Some(resolved) = resolve(&callback, ed, eg, "unregister_tick_function")? else {
        return Ok(());
    };
    let identity = autoload::canonical_callback(callback, &resolved, eg);
    let mut active_match = false;
    let found = eg.static_vars.get(STATE).and_then(|state| {
        let active = state.get(ACTIVE).map_or(0, Value::to_long_val);
        state
            .get(ENTRIES)?
            .as_array()?
            .iter()
            .find_map(|(key, value)| {
                let ArrayKey::Int(id) = key else {
                    return None;
                };
                if !autoload::callback_equal(value.as_array()?.get_int(0)?, &identity) {
                    return None;
                }
                if id == active {
                    // Keep the active descriptor rooted, but remove the first
                    // later duplicate before propagating the callback Error.
                    active_match = true;
                    None
                } else {
                    Some(id)
                }
            })
    });
    if active_match {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "Registered tick function cannot be unregistered while it is being executed",
        ));
    }
    if let Some(id) = found {
        let state = eg.static_vars.get_mut(STATE).expect("existing registry");
        let entries = state.get_mut(ENTRIES).unwrap().as_array_mut().unwrap();
        let removed = entries.get_int(id).cloned().unwrap();
        entries.remove(&ArrayKey::Int(id));
        crate::vm::execute::run_value_destructors(eg, &[removed], ed)?;
    }
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
pub(crate) fn dispatch(
    eg: &mut ExecutorGlobals,
    caller: *mut ExecuteData,
    interval: u32,
    file: &str,
    line: usize,
) -> Result<(), VmError> {
    let state = state(eg);
    let counter = state.get_mut(COUNTER).unwrap();
    let count = counter.to_long_val() + 1;
    *counter = Value::long(if count >= interval as i64 { 0 } else { count });
    if count < interval as i64
        || state
            .get(ACTIVE)
            .is_some_and(|active| active.to_long_val() != 0)
    {
        return Ok(());
    }
    let mut last = 0;
    loop {
        // IDs are monotonic. Read the live registry after every invocation:
        // removed future callbacks are skipped and newly appended ones run.
        let next = eg.static_vars.get(STATE).and_then(|state| {
            state
                .get(ENTRIES)?
                .as_array()?
                .iter()
                .find_map(|(key, entry)| {
                    let ArrayKey::Int(id) = key else {
                        return None;
                    };
                    (id > last).then(|| (id, entry.clone()))
                })
        });
        let Some((id, entry)) = next else {
            return Ok(());
        };
        last = id;
        *eg.static_vars
            .get_mut(STATE)
            .unwrap()
            .get_mut(ACTIVE)
            .unwrap() = Value::long(id);
        let result = invoke(eg, caller, &entry, file, line);
        *eg.static_vars
            .get_mut(STATE)
            .unwrap()
            .get_mut(ACTIVE)
            .unwrap() = Value::long(0);
        result?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
}

fn invoke(
    eg: &mut ExecutorGlobals,
    caller: *mut ExecuteData,
    entry: &Value,
    file: &str,
    line: usize,
) -> Result<(), VmError> {
    let fields = entry.as_array().expect("tick descriptor");
    let invocation = fields.get_int(1).unwrap();
    let arguments = fields
        .get_int(2)
        .and_then(Value::as_array)
        .expect("tick arguments");
    match resolve_callback(invocation, eg, None) {
        Some(resolved) => {
            if callback_has_hard_reference_parameters(&resolved) {
                let name = callable_display_name(fields.get_int(0).unwrap(), eg);
                for message in
                    callback_reference_warning_messages(&resolved, arguments, true, &name)
                {
                    report_diagnostic_from(eg, caller, file, line, 2, "Warning", &message)?;
                    if eg.exception.is_some() {
                        return Ok(());
                    }
                }
            }
            let arguments: Vec<_> = arguments.values().cloned().collect();
            call_resolved_with_values_from(eg, &resolved, &arguments, caller, file, line, true)
                .map(|_| ())
        }
        None => Ok(()),
    }
}
