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
