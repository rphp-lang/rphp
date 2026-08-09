mod channel;
#[cfg(unix)]
mod datagram;
mod driver;
#[cfg(unix)]
mod io;
mod lifecycle;
mod readiness;
#[cfg(any(target_vendor = "apple", target_os = "linux"))]
mod resolver;

#[cfg(unix)]
use std::collections::VecDeque;
#[cfg(unix)]
use std::net::SocketAddr;
use std::pin::Pin;
use std::time::{Duration, Instant};

use self::channel::{ChannelSet, ReceiveOutcome, SendOutcome};
#[cfg(any(target_vendor = "apple", target_os = "linux"))]
use self::io::ConnectOutcome;
#[cfg(unix)]
use self::io::{AcceptOutcome, IoDirection, IoReady, IoSet, ReadOutcome, WriteOutcome};
use self::readiness::Readiness;
#[cfg(any(target_vendor = "apple", target_os = "linux"))]
use self::resolver::ResolverSet;
use super::state::{
    CoroutineContext, CoroutineEntry, CoroutineStackPool, CoroutineStatus, WaitReason,
    initialize_entry_frame,
};
use crate::runtime::ExecutorGlobals;
use crate::value::Value;
use crate::vm::execute::{VmError, execute_coroutine_frame, write_coroutine_result};

struct ContextSet {
    entries: Vec<Pin<Box<CoroutineContext>>>,
    // Match the former HashMap field width so later scheduler fields retain
    // their established offsets while dense IDs avoid hashing on every switch.
    _layout_reserve: [usize; 3],
}

impl ContextSet {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            _layout_reserve: [0; 3],
        }
    }

    fn insert(&mut self, id: u64, context: Pin<Box<CoroutineContext>>) {
        debug_assert_eq!(usize::try_from(id).ok(), self.entries.len().checked_add(1));
        self.entries.push(context);
    }

    #[inline]
    fn get(&self, id: &u64) -> Option<&Pin<Box<CoroutineContext>>> {
        self.entries.get(context_index(*id)?)
    }

    #[inline]
    fn get_mut(&mut self, id: &u64) -> Option<&mut Pin<Box<CoroutineContext>>> {
        self.entries.get_mut(context_index(*id)?)
    }

    fn values(&self) -> impl Iterator<Item = &Pin<Box<CoroutineContext>>> {
        self.entries.iter()
    }

    fn values_mut(&mut self) -> impl Iterator<Item = &mut Pin<Box<CoroutineContext>>> {
        self.entries.iter_mut()
    }

    fn iter(&self) -> impl Iterator<Item = (u64, &Pin<Box<CoroutineContext>>)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, context)| (index as u64 + 1, context))
    }
}

#[inline]
fn context_index(id: u64) -> Option<usize> {
    id.checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
}

pub(super) struct CoroutineScheduler {
    executor: *mut ExecutorGlobals,
    next_id: u64,
    pub(super) active: Option<u64>,
    contexts: ContextSet,
    pool: CoroutineStackPool,
    channels: ChannelSet,
    readiness: Readiness,
    #[cfg(unix)]
    io: IoSet,
    #[cfg(unix)]
    io_ready: VecDeque<IoReady>,
    #[cfg(any(target_vendor = "apple", target_os = "linux"))]
    resolver: Option<Box<ResolverSet>>,
}

impl CoroutineScheduler {
    pub(super) fn new(eg: &mut ExecutorGlobals) -> Self {
        Self {
            executor: eg,
            next_id: 1,
            active: None,
            contexts: ContextSet::new(),
            pool: CoroutineStackPool::default(),
            channels: ChannelSet::default(),
            readiness: Readiness::default(),
            #[cfg(unix)]
            io: IoSet::default(),
            #[cfg(unix)]
            io_ready: VecDeque::new(),
            #[cfg(any(target_vendor = "apple", target_os = "linux"))]
            resolver: None,
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
        self.readiness.enqueue(id);
        Ok(id)
    }

    pub(super) fn create_channel(&mut self, capacity: usize) -> Result<u64, VmError> {
        self.channels.create(capacity)
    }

    pub(super) fn send(&mut self, channel: u64, value: Value) -> Result<bool, VmError> {
        let task = self.active_task("coroutine_send")?;
        match self.channels.send(channel, task, value)? {
            SendOutcome::Complete => Ok(false),
            SendOutcome::Blocked => {
                self.block_task(task, WaitReason::ChannelSend(channel))?;
                Ok(true)
            }
            SendOutcome::WakeReceiver { waiter, value } => {
                unsafe { write_coroutine_result(waiter.frame, waiter.return_value, value) };
                self.wake_task(waiter.task, WaitReason::ChannelReceive(channel))?;
                Ok(false)
            }
        }
    }

    pub(super) fn receive(
        &mut self,
        channel: u64,
        frame: *mut crate::vm::frame::ExecuteData,
        return_value: *mut Value,
    ) -> Result<Option<Value>, VmError> {
        let task = self.active_task("coroutine_receive")?;
        match self.channels.receive(channel, task, frame, return_value)? {
            ReceiveOutcome::Ready { value, wake_sender } => {
                if let Some(sender) = wake_sender {
                    self.wake_task(sender, WaitReason::ChannelSend(channel))?;
                }
                Ok(Some(value))
            }
            ReceiveOutcome::Blocked => {
                self.block_task(task, WaitReason::ChannelReceive(channel))?;
                Ok(None)
            }
        }
    }

    pub(super) fn sleep(&mut self, duration: Duration) -> Result<(), VmError> {
        let task = self.active_task("coroutine_sleep")?;
        let deadline = Instant::now()
            .checked_add(duration)
            .ok_or_else(|| VmError::Fatal("coroutine_sleep duration is too large".into()))?;
        self.block_active(WaitReason::Timer)?;
        self.readiness.schedule_timer(task, deadline);
        Ok(())
    }

    #[cfg(unix)]
    pub(super) fn create_stream_pair(&mut self) -> Result<(u64, u64), VmError> {
        self.io.create_pair()
    }

    #[cfg(unix)]
    pub(super) fn create_tcp_listener(
        &mut self,
        address: SocketAddr,
    ) -> Result<(u64, SocketAddr), VmError> {
        self.io.create_tcp_listener(address)
    }

    #[cfg(any(target_vendor = "apple", target_os = "linux"))]
    pub(super) fn connect_tcp(
        &mut self,
        address: SocketAddr,
        timeout: Option<Duration>,
        frame: *mut crate::vm::frame::ExecuteData,
        return_value: *mut Value,
    ) -> Result<Option<u64>, VmError> {
        let task = self.active_task("coroutine_tcp_connect")?;
        let deadline = timeout
            .map(|duration| {
                Instant::now().checked_add(duration).ok_or_else(|| {
                    VmError::Fatal("coroutine TCP connect timeout is too large".into())
                })
            })
            .transpose()?;
        match self.io.create_tcp_connection(address)? {
            ConnectOutcome::Connected(stream) => Ok(Some(stream)),
            ConnectOutcome::InProgress(stream) => {
                self.block_active(WaitReason::TcpConnect(stream))?;
                self.io.enqueue_tcp_connect(
                    stream,
                    task,
                    frame,
                    return_value,
                    std::collections::VecDeque::new(),
                );
                if let Some(deadline) = deadline {
                    self.readiness.schedule_timer(task, deadline);
                }
                Ok(None)
            }
        }
    }

    #[cfg(unix)]
    pub(super) fn accept_tcp(
        &mut self,
        listener: u64,
    ) -> Result<Option<(u64, SocketAddr)>, VmError> {
        match self.io.accept(listener)? {
            AcceptOutcome::Accepted { stream, peer } => Ok(Some((stream, peer))),
            AcceptOutcome::WouldBlock => Ok(None),
        }
    }

    #[cfg(unix)]
    pub(super) fn wait_readable(&mut self, descriptor: u64) -> Result<(), VmError> {
        self.wait_descriptor(descriptor, IoDirection::Readable)
    }

    #[cfg(unix)]
    pub(super) fn wait_writable(&mut self, descriptor: u64) -> Result<(), VmError> {
        self.wait_descriptor(descriptor, IoDirection::Writable)
    }

    #[cfg(unix)]
    fn wait_descriptor(&mut self, descriptor: u64, direction: IoDirection) -> Result<(), VmError> {
        let task = self.active_task(match direction {
            IoDirection::Readable => "coroutine_wait_readable",
            IoDirection::Writable => "coroutine_wait_writable",
        })?;
        self.io.ensure_waitable(descriptor, direction)?;
        let reason = match direction {
            IoDirection::Readable => WaitReason::IoRead(descriptor),
            IoDirection::Writable => WaitReason::IoWrite(descriptor),
        };
        self.block_active(reason)?;
        self.io.enqueue_waiter(descriptor, task, direction);
        Ok(())
    }

    #[cfg(unix)]
    pub(super) fn read_stream(
        &mut self,
        stream: u64,
        length: usize,
    ) -> Result<Option<Vec<u8>>, VmError> {
        match self.io.read(stream, length)? {
            ReadOutcome::Data(bytes) => Ok(Some(bytes)),
            ReadOutcome::WouldBlock => Ok(None),
        }
    }

    #[cfg(unix)]
    pub(super) fn write_stream(
        &mut self,
        stream: u64,
        bytes: &[u8],
    ) -> Result<Option<usize>, VmError> {
        match self.io.write(stream, bytes)? {
            WriteOutcome::Written(written) => Ok(Some(written)),
            WriteOutcome::WouldBlock => Ok(None),
        }
    }

    fn active_task(&self, operation: &str) -> Result<u64, VmError> {
        self.active.ok_or_else(|| {
            VmError::Fatal(format!(
                "{} can only be called by a running child",
                operation
            ))
        })
    }

    fn block_active(&mut self, reason: WaitReason) -> Result<(), VmError> {
        let task = self.active_task("coroutine wait")?;
        self.block_task(task, reason)
    }

    fn block_task(&mut self, task: u64, reason: WaitReason) -> Result<(), VmError> {
        let context = self.context_ptr(task)?;
        unsafe {
            if (*context).status != CoroutineStatus::Running || (*context).wait_reason.is_some() {
                return Err(VmError::Fatal(format!(
                    "coroutine {} cannot enter wait state from {:?}",
                    task,
                    (*context).status
                )));
            }
            (*context).wait_reason = Some(reason);
        }
        Ok(())
    }

    fn wake_task(&mut self, task: u64, expected: WaitReason) -> Result<(), VmError> {
        let context = self.context_ptr(task)?;
        unsafe {
            if (*context).status != CoroutineStatus::Waiting
                || (*context).wait_reason != Some(expected)
            {
                return Err(VmError::Fatal(format!(
                    "coroutine {} has inconsistent readiness state",
                    task
                )));
            }
            (*context).wait_reason = None;
            (*context).status = CoroutineStatus::Ready;
        }
        self.readiness.enqueue(task);
        Ok(())
    }

    fn context_ptr(&mut self, id: u64) -> Result<*mut CoroutineContext, VmError> {
        let context = self
            .contexts
            .get_mut(&id)
            .ok_or_else(|| VmError::Fatal(format!("unknown coroutine {}", id)))?;
        Ok(unsafe { context.as_mut().get_unchecked_mut() as *mut CoroutineContext })
    }

    /// Resume a task on an executor already validated against this scheduler.
    /// PHP API entry points obtain the scheduler through `scheduler_ptr`, and
    /// `join` retains that same validated scheduler/executor pair.
    pub(super) unsafe fn resume(
        scheduler: *mut Self,
        id: u64,
        eg: &mut ExecutorGlobals,
    ) -> Result<bool, VmError> {
        let context = unsafe {
            let scheduler = &mut *scheduler;
            if scheduler.active.is_some() {
                return Err(VmError::Fatal(
                    "coroutine resume and join are only allowed from the scope root".into(),
                ));
            }

            let context = scheduler.context_ptr(id)?;
            let status = (*context).status;
            if !matches!(
                status,
                CoroutineStatus::Created | CoroutineStatus::Ready | CoroutineStatus::Suspended
            ) {
                return match status {
                    CoroutineStatus::Waiting => Ok(true),
                    CoroutineStatus::Completed
                    | CoroutineStatus::Failed
                    | CoroutineStatus::Joined => Ok(false),
                    _ => Err(VmError::Fatal(format!(
                        "coroutine {} cannot be resumed from {:?}",
                        id, status
                    ))),
                };
            }

            if matches!(status, CoroutineStatus::Created | CoroutineStatus::Ready) {
                scheduler.readiness.remove_ready(id);
            }
            #[cfg(unix)]
            if status == CoroutineStatus::Ready {
                scheduler.io.acknowledge_ready(id);
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

            if let Some(kind) = super::take_suspend_request() {
                assert!(
                    matches!(&execution, Err(VmError::Fatal(message)) if message.is_empty()),
                    "coroutine suspend signal must be the empty internal fatal sidecar"
                );
                (*context).status = match kind {
                    super::SuspendKind::Manual => {
                        assert!((*context).wait_reason.is_none());
                        CoroutineStatus::Suspended
                    }
                    super::SuspendKind::Waiting => {
                        assert!((*context).wait_reason.is_some());
                        CoroutineStatus::Waiting
                    }
                };
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
                CoroutineStatus::Created | CoroutineStatus::Ready | CoroutineStatus::Suspended => {
                    unsafe { Self::resume(scheduler, id, eg)? };
                }
                CoroutineStatus::Waiting => {
                    let next = unsafe { (&mut *scheduler).next_runnable()? };
                    let Some(next) = next else {
                        return Err(VmError::Fatal(format!(
                            "coroutine deadlock while joining task {}",
                            id
                        )));
                    };
                    unsafe { Self::resume(scheduler, next, eg)? };
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::function::FunctionCommon;

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn dense_context_registry_preserves_established_scheduler_field_width() {
        assert_eq!(std::mem::size_of::<ContextSet>(), 48);
    }

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
