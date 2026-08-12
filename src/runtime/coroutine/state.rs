use std::collections::HashMap;
use std::marker::PhantomPinned;

use crate::runtime::ExecutorGlobals;
use crate::value::Value;
use crate::vm::execute::{VmError, cleanup_frame_slots};
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, FunctionType, UserFunction};
use crate::vm::stack::VmStack;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CoroutineStatus {
    Created,
    Ready,
    Running,
    Suspended,
    Waiting,
    Completed,
    Failed,
    Joined,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WaitReason {
    ChannelSend(u64),
    ChannelReceive(u64),
    Timer,
    #[cfg(unix)]
    IoRead(u64),
    #[cfg(unix)]
    IoWrite(u64),
    #[cfg(any(target_vendor = "apple", target_os = "linux"))]
    TcpConnect(u64),
    #[cfg(any(target_vendor = "apple", target_os = "linux"))]
    DnsResolve(u64),
}

pub(super) struct CoroutineStacks {
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

#[derive(Default)]
pub(super) struct CoroutineStackPool {
    pub(super) idle: Vec<CoroutineStacks>,
    pub(super) created: usize,
    pub(super) reused: usize,
}

impl CoroutineStackPool {
    pub(super) fn checkout(&mut self) -> CoroutineStacks {
        if let Some(stacks) = self.idle.pop() {
            self.reused += 1;
            stacks
        } else {
            self.created += 1;
            CoroutineStacks::new()
        }
    }

    pub(super) fn recycle(&mut self, stacks: CoroutineStacks) {
        self.idle.push(stacks);
    }
}

pub(super) struct CoroutineExecutionState {
    pub(super) stacks: Option<CoroutineStacks>,
    pub(super) current_execute_data: *mut ExecuteData,
    pub(super) exception: Option<Value>,
    pending_named_variadic: HashMap<usize, Vec<(String, Value)>>,
    function_arguments: HashMap<usize, Vec<Value>>,
    active_generator: Option<crate::vm::generator::GeneratorRef>,
    pending_invoke_this: Option<Value>,
}

impl CoroutineExecutionState {
    fn new() -> Self {
        Self {
            stacks: None,
            current_execute_data: std::ptr::null_mut(),
            exception: None,
            pending_named_variadic: HashMap::new(),
            function_arguments: HashMap::new(),
            active_generator: None,
            pending_invoke_this: None,
        }
    }

    #[inline]
    pub(super) fn exchange(&mut self, eg: &mut ExecutorGlobals) {
        let stacks = self
            .stacks
            .as_mut()
            .expect("coroutine storage must be checked out before activation");
        std::mem::swap(&mut stacks.vm_stack, &mut eg.vm_stack);
        std::mem::swap(&mut stacks.pending_call_stack, &mut eg.pending_call_stack);

        let current = eg.current_execute_data.replace(self.current_execute_data);
        self.current_execute_data = current;
        if self.exception.is_some() || eg.exception.is_some() {
            std::mem::swap(&mut self.exception, &mut eg.exception);
        }
        if !self.pending_named_variadic.is_empty() || !eg.pending_named_variadic.is_empty() {
            std::mem::swap(
                &mut self.pending_named_variadic,
                &mut eg.pending_named_variadic,
            );
        }
        if !self.function_arguments.is_empty() || !eg.function_arguments.is_empty() {
            std::mem::swap(&mut self.function_arguments, &mut eg.function_arguments);
        }
        if self.active_generator.is_some() || eg.active_generator.is_some() {
            std::mem::swap(&mut self.active_generator, &mut eg.active_generator);
        }
        if self.pending_invoke_this.is_some() || eg.pending_invoke_this.is_some() {
            std::mem::swap(&mut self.pending_invoke_this, &mut eg.pending_invoke_this);
        }
    }

    pub(super) fn cleanup_frames(&mut self) {
        let stacks = self
            .stacks
            .as_mut()
            .expect("a started coroutine must own checked-out storage");
        unsafe {
            cleanup_frame_chain(
                &mut stacks.vm_stack,
                &mut stacks.pending_call_stack,
                &mut self.pending_named_variadic,
                &mut self.pending_invoke_this,
                self.current_execute_data,
            );
        }
        self.current_execute_data = std::ptr::null_mut();
        self.exception = None;
        self.pending_named_variadic.clear();
        self.function_arguments.clear();
        self.active_generator = None;
        self.pending_invoke_this = None;
    }
}

unsafe fn cleanup_pending_calls(
    vm_stack: &mut VmStack,
    pending_call_stack: &mut VmStack,
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
                pending_call_stack.pop_call_frame(call);
            } else {
                vm_stack.pop_call_frame(call);
            }
            call = next;
        }
    }
}

pub(super) unsafe fn cleanup_frame_chain(
    vm_stack: &mut VmStack,
    pending_call_stack: &mut VmStack,
    pending_named_variadic: &mut HashMap<usize, Vec<(String, Value)>>,
    pending_invoke_this: &mut Option<Value>,
    mut frame: *mut ExecuteData,
) {
    unsafe {
        while !frame.is_null() {
            let previous = (*frame).prev_execute_data;
            cleanup_pending_calls(vm_stack, pending_call_stack, pending_named_variadic, frame);
            cleanup_frame_slots(frame);
            vm_stack.pop_call_frame(frame);
            frame = previous;
        }
        *pending_invoke_this = None;
    }
}

pub(super) struct CoroutineEntry {
    pub(super) function: *const FunctionCommon,
    pub(super) captures: Vec<Value>,
}

impl CoroutineEntry {
    pub(super) fn from_value(value: &Value, eg: &ExecutorGlobals) -> Result<Self, VmError> {
        let (function, captures) = if let Some(closure) = value.as_closure() {
            (closure.func, closure.captures.clone())
        } else if let Some(name) = value.as_str() {
            let function = eg.find_function(name).ok_or_else(|| {
                VmError::Fatal(format!(
                    "coroutine callback must name a defined function, {}() not found",
                    name
                ))
            })?;
            (function, Vec::new())
        } else {
            return Err(VmError::Fatal(
                "coroutine callback must be a closure or function name".into(),
            ));
        };

        let common = unsafe { &*function };
        if common.fn_type != FunctionType::User {
            return Err(VmError::Fatal(
                "coroutine callback must be a user-defined function".into(),
            ));
        }
        if common.sig.required_num_args != 0 {
            return Err(VmError::Fatal(
                "coroutine callback must accept zero required arguments".into(),
            ));
        }
        let user = unsafe { &*(function as *const UserFunction) };
        if user.op_array.is_generator {
            return Err(VmError::Fatal(
                "generator functions cannot be used as coroutine callbacks".into(),
            ));
        }
        let capture_capacity = common.frame.num_cvs.saturating_sub(common.sig.num_args) as usize;
        if captures.len() > capture_capacity {
            return Err(VmError::Fatal(
                "coroutine closure capture layout is inconsistent".into(),
            ));
        }

        Ok(Self { function, captures })
    }
}

pub(super) struct CoroutineContext {
    pub(super) id: u64,
    pub(super) parent: Option<u64>,
    pub(super) entry: CoroutineEntry,
    pub(super) state: CoroutineExecutionState,
    pub(super) status: CoroutineStatus,
    pub(super) result: Value,
    pub(super) failure: Option<Value>,
    pub(super) boundary_execute_data: *mut ExecuteData,
    pub(super) wait_reason: Option<WaitReason>,
    _pinned: PhantomPinned,
}

impl CoroutineContext {
    pub(super) fn new(id: u64, parent: Option<u64>, entry: CoroutineEntry) -> Self {
        Self {
            id,
            parent,
            entry,
            state: CoroutineExecutionState::new(),
            status: CoroutineStatus::Created,
            result: Value::null(),
            failure: None,
            boundary_execute_data: std::ptr::null_mut(),
            wait_reason: None,
            _pinned: PhantomPinned,
        }
    }
}

impl Drop for CoroutineContext {
    fn drop(&mut self) {
        assert!(
            !matches!(
                self.status,
                CoroutineStatus::Ready
                    | CoroutineStatus::Running
                    | CoroutineStatus::Suspended
                    | CoroutineStatus::Waiting
            ),
            "a live coroutine must be joined or cancelled by its owning scope"
        );
    }
}

pub(super) unsafe fn initialize_value_slot(frame: *mut ExecuteData, index: u32, value: Value) {
    unsafe {
        let slot = (*frame).cv_mut(index) as *mut Value;
        slot.write(value);
        if (*slot).needs_cleanup() {
            (*frame).has_heap_slots = true;
            let total = (*frame).num_cvs as usize + (*frame).num_temps as usize;
            if total <= 64 {
                (*frame).heap_bitmap |= 1_u64 << index;
            }
        }
    }
}

pub(super) unsafe fn initialize_entry_frame(
    eg: &mut ExecutorGlobals,
    context: *mut CoroutineContext,
) {
    unsafe {
        let entry = &(*context).entry;
        let common = &*entry.function;
        let user = &*(entry.function as *const UserFunction);
        let frame = eg.vm_stack.push_call_frame(
            entry.function,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        (*frame).return_value = &mut (*context).result;
        (*frame).opline = user.op_array.instructions.as_ptr();
        (*context).boundary_execute_data = frame;
        for (offset, capture) in entry.captures.iter().enumerate() {
            initialize_value_slot(frame, common.sig.num_args + offset as u32, capture.clone());
        }
        eg.current_execute_data.set(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_state_exchange_preserves_nonempty_side_state() {
        let mut eg = ExecutorGlobals::new();
        let mut state = CoroutineExecutionState::new();
        state.stacks = Some(CoroutineStacks::new());

        eg.exception = Some(Value::long(11));
        state.exception = Some(Value::long(22));
        eg.pending_named_variadic
            .insert(1, vec![("root".into(), Value::long(33))]);
        state
            .pending_named_variadic
            .insert(2, vec![("child".into(), Value::long(44))]);
        eg.pending_invoke_this = Some(Value::long(55));
        state.pending_invoke_this = Some(Value::long(66));

        state.exchange(&mut eg);

        assert_eq!(eg.exception.as_ref().and_then(Value::as_long), Some(22));
        assert_eq!(state.exception.as_ref().and_then(Value::as_long), Some(11));
        assert!(eg.pending_named_variadic.contains_key(&2));
        assert!(state.pending_named_variadic.contains_key(&1));
        assert_eq!(
            eg.pending_invoke_this.as_ref().and_then(Value::as_long),
            Some(66)
        );
        assert_eq!(
            state.pending_invoke_this.as_ref().and_then(Value::as_long),
            Some(55)
        );

        state.exchange(&mut eg);
        assert_eq!(eg.exception.as_ref().and_then(Value::as_long), Some(11));
        assert!(eg.pending_named_variadic.contains_key(&1));
        assert_eq!(
            eg.pending_invoke_this.as_ref().and_then(Value::as_long),
            Some(55)
        );
    }
}
