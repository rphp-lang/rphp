/// A parked builtin owns its arguments and progress independently of the
/// temporary internal ExecuteData frame, which the dispatcher always retires.
pub(crate) struct NativeCall {
    function: *const FunctionCommon,
    public_num_args: u32,
    arguments: Vec<Value>,
    operation: Box<dyn crate::runtime::fiber::native::NativeOperation>,
}

impl NativeCall {
    pub(crate) fn cycle_snapshot(&self) -> (Vec<Value>, Vec<usize>) {
        let (mut values, frames) = self.operation.cycle_snapshot();
        values.extend(self.arguments.iter().filter_map(Value::clone_cycle_handle));
        (values, frames)
    }
}

#[cold]
pub(crate) fn start_native_call(
    eg: &mut ExecutorGlobals,
    internal: *mut ExecuteData,
    return_value: *mut Value,
    operation: Box<dyn crate::runtime::fiber::native::NativeOperation>,
) -> Result<(), VmError> {
    // SAFETY: an internal handler supplies its live, arity-checked activation.
    // Copy every CV before re-entry. Only the request-owned function pointer
    // and caller-owned result slot survive the internal frame's retirement.
    unsafe {
        let mut call = Box::new(NativeCall {
            function: (*internal).func,
            public_num_args: (*internal).num_args,
            arguments: (0..(*internal).num_cvs)
                .map(|i| (*internal).cv(i).clone_closure_capture())
                .collect(),
            operation,
        });
        match call.operation.run(eg, internal, None)? {
            crate::runtime::fiber::native::NativeCallbackOutcome::Complete(value) => {
                if !return_value.is_null() {
                    *return_value = value;
                }
                Ok(())
            }
            crate::runtime::fiber::native::NativeCallbackOutcome::Suspended(value) => {
                eg.current_execute_data.set((*internal).prev_execute_data);
                eg.suspend_native_call(call, return_value, value)
            }
        }
    }
}

#[cold]
pub(crate) fn resume_suspended_native_call(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    mut continuation: Box<NativeCall>,
    return_value: *mut Value,
    input: crate::runtime::fiber::FiberInput,
) -> Result<Option<*mut ExecuteData>, VmError> {
    // SAFETY: Fiber retains `frame`, its current DoFcall, and the result slot
    // on its inactive VM stack. Registered function metadata is request-owned.
    // The temporary internal frame exists only during run(); callback owners
    // keep separate stacks and remove trace links before this frame is popped.
    unsafe {
        let internal = eg.vm_stack.push_call_frame(
            continuation.function,
            continuation.arguments.len() as u32,
            continuation.public_num_args,
            frame,
            std::ptr::null_mut(),
        );
        for (index, value) in continuation.arguments.iter().enumerate() {
            frame_slot_init(
                internal,
                (*internal).cv_mut(index as u32),
                value.clone_closure_capture(),
            );
        }
        eg.current_execute_data.set(frame);
        let result = continuation.operation.run(eg, internal, Some(input));
        eg.current_execute_data.set(frame);
        if let Some(exception) = eg.exception.as_ref() {
            attach_internal_call_trace_if_missing(exception, internal, frame, eg);
        }
        cleanup_frame_slots(internal);
        pop_vm_call_frame(eg, internal);
        match result? {
            crate::runtime::fiber::native::NativeCallbackOutcome::Suspended(value) => {
                eg.suspend_native_call(continuation, return_value, value)?;
                unreachable!("suspension unwinds to the Fiber owner");
            }
            crate::runtime::fiber::native::NativeCallbackOutcome::Complete(value) => {
                drop(continuation);
                if let Some(exception) = eg.exception.take() {
                    return inject_suspended_exception(eg, frame, exception);
                }
                write_coroutine_result(frame, return_value, value);
                (*frame).opline = (*frame).opline.add(1);
                Ok(Some(frame))
            }
        }
    }
}
