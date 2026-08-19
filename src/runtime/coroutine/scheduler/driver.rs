use std::time::{Duration, Instant};

use super::CoroutineScheduler;
#[cfg(unix)]
use super::io::IoDirection;
use crate::runtime::suspended::{CoroutineStatus, WaitReason};
use crate::vm::execute::VmError;

impl CoroutineScheduler {
    pub(super) fn next_runnable(&mut self) -> Result<Option<u64>, VmError> {
        loop {
            self.promote_due_timers()?;
            #[cfg(unix)]
            self.promote_io(Some(Duration::ZERO))?;
            while let Some(task) = self.readiness.pop_ready() {
                let Some(context) = self.contexts.get(&task) else {
                    continue;
                };
                if matches!(
                    context.as_ref().get_ref().status,
                    CoroutineStatus::Created | CoroutineStatus::Ready
                ) {
                    return Ok(Some(task));
                }
            }

            let deadline = self.readiness.next_deadline();
            #[cfg(unix)]
            if self.io.has_waiters() {
                let timeout =
                    deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
                self.promote_io(timeout)?;
                continue;
            }

            if let Some(deadline) = deadline {
                std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            } else {
                return Ok(None);
            }
        }
    }

    #[cfg(unix)]
    fn promote_io(&mut self, timeout: Option<Duration>) -> Result<(), VmError> {
        self.io.poll_ready(timeout, &mut self.io_ready)?;
        while let Some(event) = self.io_ready.pop_front() {
            #[cfg(any(target_vendor = "apple", target_os = "linux"))]
            if event.task == super::io::RESOLVER_WAKE_TASK {
                debug_assert_eq!(event.direction, IoDirection::Readable);
                self.promote_resolver(event.descriptor)?;
                continue;
            }
            #[cfg(any(target_vendor = "apple", target_os = "linux"))]
            if event.direction == IoDirection::Writable
                && self.contexts.get(&event.task).is_some_and(|context| {
                    context.as_ref().get_ref().wait_reason
                        == Some(WaitReason::TcpConnect(event.descriptor))
                })
            {
                self.promote_tcp_connect(event.descriptor, event.task)?;
                continue;
            }
            let expected = match event.direction {
                IoDirection::Readable => WaitReason::IoRead(event.descriptor),
                IoDirection::Writable => WaitReason::IoWrite(event.descriptor),
            };
            self.wake_task(event.task, expected)?;
        }
        Ok(())
    }

    fn promote_due_timers(&mut self) -> Result<(), VmError> {
        let mut due = Vec::new();
        self.readiness.drain_due(Instant::now(), &mut due);
        for task in due {
            let wait_reason = self.contexts.get(&task).and_then(|context| {
                let context = context.as_ref().get_ref();
                (context.status == CoroutineStatus::Waiting)
                    .then_some(context.wait_reason)
                    .flatten()
            });
            match wait_reason {
                Some(WaitReason::Timer) => self.wake_task(task, WaitReason::Timer)?,
                #[cfg(any(target_vendor = "apple", target_os = "linux"))]
                Some(WaitReason::TcpConnect(descriptor)) => {
                    self.expire_tcp_connect(descriptor, task)?
                }
                #[cfg(any(target_vendor = "apple", target_os = "linux"))]
                Some(WaitReason::DnsResolve(job)) => self.expire_dns_resolve(job, task)?,
                _ => {}
            }
        }
        Ok(())
    }

    #[cfg(any(target_vendor = "apple", target_os = "linux"))]
    fn expire_tcp_connect(&mut self, descriptor: u64, task: u64) -> Result<(), VmError> {
        self.io.cancel_tcp_connect(descriptor, task);
        Err(VmError::Fatal(
            "failed to connect coroutine TCP stream: timed out".into(),
        ))
    }

    #[cfg(any(target_vendor = "apple", target_os = "linux"))]
    fn expire_dns_resolve(&mut self, job: u64, task: u64) -> Result<(), VmError> {
        self.cancel_dns_resolve(job, task);
        Err(VmError::Fatal(
            "failed to connect coroutine TCP stream: timed out".into(),
        ))
    }
}

#[cfg(all(test, any(target_vendor = "apple", target_os = "linux")))]
#[path = "driver_tests.rs"]
mod tests;
