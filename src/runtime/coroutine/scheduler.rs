use std::collections::HashMap;
use std::pin::Pin;

use super::state::{
    CoroutineContext, CoroutineEntry, CoroutineStackPool, CoroutineStatus, initialize_entry_frame,
};
use crate::runtime::ExecutorGlobals;
use crate::value::Value;
use crate::vm::execute::{VmError, execute_coroutine_frame};

pub(super) struct CoroutineScheduler {
    executor: *mut ExecutorGlobals,
    next_id: u64,
    pub(super) active: Option<u64>,
    contexts: HashMap<u64, Pin<Box<CoroutineContext>>>,
    pool: CoroutineStackPool,
}

impl CoroutineScheduler {
    pub(super) fn new(eg: &mut ExecutorGlobals) -> Self {
        Self {
            executor: eg,
            next_id: 1,
            active: None,
            contexts: HashMap::new(),
            pool: CoroutineStackPool::default(),
        }
    }

    pub(super) fn verify_executor(&self, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
        if std::ptr::eq(self.executor, eg) {
            Ok(())
        } else {
            Err(VmError::Fatal(
                "coroutine scope cannot migrate between executors".into(),
            ))
        }
    }

    pub(super) fn spawn(&mut self, entry: CoroutineEntry) -> Result<u64, VmError> {
        let id = self.next_id;
        if id > i64::MAX as u64 {
            return Err(VmError::Fatal(
                "coroutine identifier space exhausted".into(),
            ));
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| VmError::Fatal("coroutine identifier space exhausted".into()))?;
        let parent = self.active;
        self.contexts
            .insert(id, Box::pin(CoroutineContext::new(id, parent, entry)));
        Ok(id)
    }

    fn context_ptr(&mut self, id: u64) -> Result<*mut CoroutineContext, VmError> {
        let context = self
            .contexts
            .get_mut(&id)
            .ok_or_else(|| VmError::Fatal(format!("unknown coroutine {}", id)))?;
        Ok(unsafe { context.as_mut().get_unchecked_mut() as *mut CoroutineContext })
    }

    pub(super) unsafe fn resume(
        scheduler: *mut Self,
        id: u64,
        eg: &mut ExecutorGlobals,
    ) -> Result<bool, VmError> {
        let context = unsafe {
            let scheduler = &mut *scheduler;
            scheduler.verify_executor(eg)?;
            if scheduler.active.is_some() {
                return Err(VmError::Fatal(
                    "coroutine resume and join are only allowed from the scope root".into(),
                ));
            }

            let context = scheduler.context_ptr(id)?;
            let status = (*context).status;
            if !matches!(
                status,
                CoroutineStatus::Created | CoroutineStatus::Suspended
            ) {
                return match status {
                    CoroutineStatus::Completed
                    | CoroutineStatus::Failed
                    | CoroutineStatus::Joined => Ok(false),
                    _ => Err(VmError::Fatal(format!(
                        "coroutine {} cannot be resumed from {:?}",
                        id, status
                    ))),
                };
            }

            if (*context).state.stacks.is_none() {
                (*context).state.stacks = Some(scheduler.pool.checkout());
            }
            (*context).state.exchange(eg);
            if status == CoroutineStatus::Created {
                initialize_entry_frame(eg, context);
            }
            (*context).status = CoroutineStatus::Running;
            scheduler.active = Some(id);
            context
        };

        // No mutable scheduler reference survives this VM re-entry. A child
        // may therefore spawn another boxed context without aliasing `self`.
        let frame = eg.current_execute_data.get();
        let boundary = unsafe { (*context).boundary_execute_data };
        debug_assert!(!frame.is_null());
        debug_assert!(!boundary.is_null());
        let execution = execute_coroutine_frame(eg, frame, boundary);

        unsafe {
            let scheduler = &mut *scheduler;
            (*context).state.exchange(eg);
            scheduler.active = None;

            if super::take_suspend_request()
                && matches!(&execution, Err(VmError::Fatal(message)) if message.is_empty())
            {
                (*context).status = CoroutineStatus::Suspended;
                return Ok(true);
            }

            let exception = (*context).state.exception.take();
            (*context).state.cleanup_frames();
            (*context).boundary_execute_data = std::ptr::null_mut();
            let stacks = (*context)
                .state
                .stacks
                .take()
                .expect("completed coroutine must own checked-out storage");
            scheduler.pool.recycle(stacks);

            match execution {
                Ok(()) => {
                    if let Some(exception) = exception {
                        (*context).failure = Some(exception);
                        (*context).status = CoroutineStatus::Failed;
                    } else {
                        (*context).status = CoroutineStatus::Completed;
                    }
                    Ok(false)
                }
                Err(error) => {
                    (*context).status = CoroutineStatus::Cancelled;
                    Err(error)
                }
            }
        }
    }

    pub(super) unsafe fn join(
        scheduler: *mut Self,
        id: u64,
        eg: &mut ExecutorGlobals,
    ) -> Result<Value, VmError> {
        loop {
            let status = unsafe { (*(&mut *scheduler).context_ptr(id)?).status };
            match status {
                CoroutineStatus::Created | CoroutineStatus::Suspended => {
                    unsafe { Self::resume(scheduler, id, eg)? };
                }
                CoroutineStatus::Completed => {
                    let context = unsafe { (&mut *scheduler).context_ptr(id)? };
                    unsafe {
                        (*context).status = CoroutineStatus::Joined;
                        return Ok(std::mem::replace(&mut (*context).result, Value::null()));
                    }
                }
                CoroutineStatus::Failed => {
                    let context = unsafe { (&mut *scheduler).context_ptr(id)? };
                    unsafe {
                        (*context).status = CoroutineStatus::Joined;
                        eg.exception = (*context).failure.take();
                    }
                    return Ok(Value::null());
                }
                CoroutineStatus::Joined => {
                    return Err(VmError::Fatal(format!(
                        "coroutine {} has already been joined",
                        id
                    )));
                }
                CoroutineStatus::Cancelled => {
                    return Err(VmError::Fatal(format!("coroutine {} was cancelled", id)));
                }
                CoroutineStatus::Running => {
                    return Err(VmError::Fatal(format!(
                        "coroutine {} is already running",
                        id
                    )));
                }
            }
        }
    }

    pub(super) fn finish_scope(&mut self, eg: &mut ExecutorGlobals) {
        assert!(self.active.is_none());
        for context in self.contexts.values_mut() {
            let context = unsafe { context.as_mut().get_unchecked_mut() };
            debug_assert!(context.parent.is_none_or(|parent| parent < context.id));
            match context.status {
                CoroutineStatus::Suspended => {
                    context.state.cleanup_frames();
                    if let Some(stacks) = context.state.stacks.take() {
                        self.pool.recycle(stacks);
                    }
                    context.status = CoroutineStatus::Cancelled;
                }
                CoroutineStatus::Created => {
                    context.status = CoroutineStatus::Cancelled;
                }
                _ => {}
            }
        }

        if eg.exception.is_none() {
            let failed_id = self
                .contexts
                .iter()
                .filter_map(|(id, context)| {
                    let context = context.as_ref().get_ref();
                    (context.status == CoroutineStatus::Failed && context.failure.is_some())
                        .then_some(*id)
                })
                .min();
            if let Some(context) = failed_id.and_then(|id| self.contexts.get_mut(&id)) {
                let context = unsafe { context.as_mut().get_unchecked_mut() };
                eg.exception = context.failure.take();
                context.status = CoroutineStatus::Joined;
            }
        }
    }
}

impl Drop for CoroutineScheduler {
    fn drop(&mut self) {
        assert!(
            self.active.is_none(),
            "coroutine scheduler dropped while a child is running"
        );
        assert!(
            self.contexts.values().all(|context| !matches!(
                context.as_ref().get_ref().status,
                CoroutineStatus::Running | CoroutineStatus::Suspended
            )),
            "coroutine scope dropped without cancelling live children"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::function::FunctionCommon;

    #[test]
    fn scheduler_is_lazy_and_reuses_one_stack_pair() {
        let source = "<?php function child() { return 7; }";
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let compiled = Compiler::new().compile(&statements).unwrap();
        let (_, child) = compiled
            .functions
            .into_iter()
            .find(|(name, _)| name == "child")
            .unwrap();
        let child = Box::new(child);

        let mut eg = ExecutorGlobals::new();
        eg.register_function("child", &child.common as *const FunctionCommon)
            .unwrap();
        let mut scheduler = CoroutineScheduler::new(&mut eg);
        assert_eq!(scheduler.pool.created, 0);

        for _ in 0..64 {
            let entry = CoroutineEntry::from_value(&Value::string("child"), &eg).unwrap();
            let id = scheduler.spawn(entry).unwrap();
            assert!(!unsafe { CoroutineScheduler::resume(&mut scheduler, id, &mut eg) }.unwrap());
            assert_eq!(
                unsafe { CoroutineScheduler::join(&mut scheduler, id, &mut eg) }
                    .unwrap()
                    .as_long(),
                Some(7)
            );
        }

        assert_eq!(scheduler.pool.created, 1);
        assert_eq!(scheduler.pool.reused, 63);
        assert_eq!(scheduler.pool.idle.len(), 1);
        scheduler.finish_scope(&mut eg);
    }
}
