//! GC destructor activations use ordinary detached Fiber VM stacks. A paused
//! destructor therefore retains its values without retaining a Rust callback
//! stack or pausing the collector which must visit the remaining objects.

use super::*;
use crate::compiler::make_internal_function;
use crate::vm::execute::{append_replaced_exception, run_value_destructors};

fn gc_trace_only_handler(
    _frame: *mut ExecuteData,
    _result: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    unreachable!("the GC coordinator is entered by cycle collection, not PHP dispatch")
}

impl FiberRuntime {
    /// A suspended engine destructor remains unfinished even when user code
    /// retained its public Fiber. Request-final object-store cleanup must
    /// close it; waiting for its public handle to become unreachable would
    /// silently discard the pending finally block with the executor itself.
    pub(crate) fn pending_gc_destructor_roots(&self) -> Vec<Value> {
        let mut roots: Vec<_> = self
            .contexts
            .values()
            .filter_map(|context| {
                (context.gc_trace_frame.is_some() && context.status == FiberStatus::Suspended)
                    .then(|| context.object.upgrade().map(Value::from_object_owner))
                    .flatten()
            })
            .collect();
        roots.sort_by_key(Value::object_handle);
        roots
    }

    fn prepare_gc_callback(
        &mut self,
        receiver: &Value,
        callback: ResolvedCallback,
    ) -> *const crate::vm::function::FunctionCommon {
        let function = self.gc_trace_function.get_or_insert_with(|| {
            Box::new(make_internal_function(
                gc_trace_only_handler,
                0,
                0,
                Vec::new(),
            ))
        });
        let function = &function.common as *const _;
        let identity = receiver.object_identity().expect("GC worker is an object");
        let mut replacement = FiberContext::new(
            receiver
                .object_weak()
                .expect("GC worker has a weak identity"),
            callback,
        );
        replacement.gc_trace_frame = Some(gc_frame(function));
        if let Some(context) = self.contexts.get_mut(&identity) {
            // A completed GC worker is reused for the next destructor in the
            // same collection pass. Pin::set drops its completed state in
            // place; there are no live frame pointers into that state.
            assert_eq!(context.status, FiberStatus::Terminated);
            context.set(replacement);
        } else {
            self.contexts.insert(identity, Box::pin(replacement));
        }
        function
    }
}

fn gc_frame(function: *const crate::vm::function::FunctionCommon) -> Box<ExecuteData> {
    Box::new(ExecuteData {
        opline: std::ptr::null(),
        call: std::ptr::null_mut(),
        return_value: std::ptr::null_mut(),
        func: function,
        prev_execute_data: std::ptr::null_mut(),
        num_args: 0,
        num_cvs: 0,
        num_temps: 0,
        pending_return_after_finally: false,
        has_heap_slots: false,
        named_args_used: false,
        call_kind_flags: 0,
        heap_bitmap: 0,
    })
}

impl ExecutorGlobals {
    /// Drop only the collector's private ownership. Public references and
    /// self-owned suspended stacks follow the normal Fiber release planner.
    pub(crate) fn release_gc_destructor_owner(
        &mut self,
        caller: *mut ExecuteData,
    ) -> Result<(), VmError> {
        let owner = self
            .fiber_runtime
            .as_deref_mut()
            .and_then(|runtime| runtime.gc_current.take());
        if let Some(owner) = owner {
            run_value_destructors(self, std::slice::from_ref(&owner), caller)?;
        }
        Ok(())
    }

    /// Return false for ordinary/non-user destructors so their existing
    /// synchronous dispatch and visibility behavior remain authoritative.
    pub(crate) fn run_gc_destructor_in_fiber(&mut self, owner: &Value) -> Result<bool, VmError> {
        if !self.has_active_fiber() {
            return Ok(false);
        }
        let Some(object) = owner.as_object() else {
            return Ok(false);
        };
        let class_name = object.class_name.to_string();
        let class_id = object.class_id;
        drop(object);
        let Some((_, _, declaring)) = self.find_method_info(&class_name, "__destruct") else {
            return Ok(false);
        };
        let Some(func_ptr) = self.find_function(&format!("{declaring}::__destruct")) else {
            return Ok(false);
        };
        let callback = ResolvedCallback {
            func_ptr,
            prepend_args: vec![owner.clone()],
            use_vars: Vec::new(),
            called_scope_class_id: class_id,
            closure_scope_class_id: None,
            bound_this: None,
            closure_static_vars: None,
            is_magic_call: false,
        };
        if !callback.supports_suspended_root() {
            return Ok(false);
        }
        if !owner.mark_object_destructed() {
            return Ok(true);
        }

        let caller = self.current_execute_data.get();
        let reusable = self
            .fiber_runtime
            .as_deref()
            .and_then(|runtime| runtime.gc_current.as_ref())
            .and_then(Value::object_identity)
            .is_some_and(|identity| self.fiber_status(identity) == Some(FiberStatus::Terminated));
        let worker = if reusable {
            self.fiber_runtime
                .as_deref_mut()
                .unwrap()
                .gc_current
                .take()
                .unwrap()
        } else {
            self.release_gc_destructor_owner(std::ptr::null_mut())?;
            let class = self.find_class("Fiber").expect("registered Fiber class");
            Value::object(PhpObject::with_layout_from_defaults(
                class.class_id,
                class.property_layout.clone(),
                class.property_defaults.as_ref(),
            ))
        };
        let displaced = self.exception.take();
        let identity = worker.object_identity().expect("GC worker identity");
        let function = self
            .fiber_runtime
            .as_deref_mut()
            .expect("active Fiber has a registry")
            .prepare_gc_callback(&worker, callback);
        self.register_internal_activation_display_name(function, "gc_destructor_fiber");
        let outcome = self.run_fiber(identity, FiberInput::Start(Vec::new()), caller);
        self.fiber_runtime.as_deref_mut().unwrap().gc_current = Some(worker);
        let outcome = outcome?;
        self.exception = outcome.failure;
        if let Some(displaced) = displaced {
            if let Some(replacement) = self.exception.as_ref() {
                append_replaced_exception(replacement, &displaced, self);
            } else {
                self.exception = Some(displaced);
            }
        }
        Ok(true)
    }
}
