use std::time::{Duration, Instant};

use super::CoroutineScheduler;
#[cfg(unix)]
use super::io::IoDirection;
use crate::runtime::coroutine::state::{CoroutineStatus, WaitReason};
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
            let is_waiting = self.contexts.get(&task).is_some_and(|context| {
                let context = context.as_ref().get_ref();
                context.status == CoroutineStatus::Waiting
                    && context.wait_reason == Some(WaitReason::Timer)
            });
            if is_waiting {
                self.wake_task(task, WaitReason::Timer)?;
            }
        }
        Ok(())
    }
}
