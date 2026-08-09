use super::CoroutineScheduler;
use crate::runtime::ExecutorGlobals;
use crate::runtime::coroutine::state::{CoroutineStatus, WaitReason};

impl CoroutineScheduler {
    pub(in crate::runtime::coroutine) fn finish_scope(&mut self, eg: &mut ExecutorGlobals) {
        assert!(self.active.is_none());
        for context in self.contexts.values_mut() {
            let context = unsafe { context.as_mut().get_unchecked_mut() };
            debug_assert!(context.parent.is_none_or(|parent| parent < context.id));
            #[cfg(any(target_vendor = "apple", target_os = "linux"))]
            if let Some(WaitReason::TcpConnect(descriptor)) = context.wait_reason {
                self.io.cancel_tcp_connect(descriptor, context.id);
            }
            #[cfg(any(target_vendor = "apple", target_os = "linux"))]
            if let Some(WaitReason::DnsResolve(job)) = context.wait_reason {
                if let Some(resolver) = self.resolver.as_mut() {
                    resolver.cancel(job, context.id);
                    if !resolver.has_waiters() {
                        self.io.disarm_resolver_wake(resolver.wake_descriptor);
                    }
                }
            }
            match context.status {
                CoroutineStatus::Ready | CoroutineStatus::Suspended | CoroutineStatus::Waiting => {
                    context.state.cleanup_frames();
                    if let Some(stacks) = context.state.stacks.take() {
                        self.pool.recycle(stacks);
                    }
                    context.wait_reason = None;
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
                CoroutineStatus::Ready
                    | CoroutineStatus::Running
                    | CoroutineStatus::Suspended
                    | CoroutineStatus::Waiting
            )),
            "coroutine scope dropped without cancelling live children"
        );
    }
}
