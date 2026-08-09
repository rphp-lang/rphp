use std::collections::BTreeMap;
use std::io::{self, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use super::CoroutineScheduler;
use super::io::{ConnectCompletion, ConnectOutcome, ConnectWaiter, IoSet, RESOLVER_WAKE_TASK};
use crate::runtime::coroutine::state::{CoroutineStatus, WaitReason};
use crate::value::Value;
use crate::vm::execute::{VmError, write_coroutine_result};
use crate::vm::frame::ExecuteData;

const WORKERS: usize = 2;
const QUEUE_CAPACITY: usize = 64;

struct ResolveRequest {
    id: u64,
    host: String,
    port: u16,
    completions: mpsc::Sender<ResolveCompletion>,
    wake_writer: UnixStream,
}

pub(super) struct ResolveCompletion {
    pub(super) id: u64,
    pub(super) result: io::Result<Vec<SocketAddr>>,
}

#[derive(Clone, Copy)]
pub(super) struct ResolveWaiter {
    pub(super) task: u64,
    pub(super) frame: *mut ExecuteData,
    pub(super) return_value: *mut Value,
}

pub(super) struct ResolverSet {
    completion_sender: mpsc::Sender<ResolveCompletion>,
    completions: mpsc::Receiver<ResolveCompletion>,
    wake_writer: UnixStream,
    pub(super) wake_descriptor: u64,
    next_id: u64,
    waiters: BTreeMap<u64, ResolveWaiter>,
}

struct ResolverPool {
    requests: mpsc::SyncSender<ResolveRequest>,
}

static RESOLVER_POOL: OnceLock<Result<ResolverPool, String>> = OnceLock::new();

impl CoroutineScheduler {
    pub(in crate::runtime::coroutine) fn resolve_and_connect_tcp(
        &mut self,
        host: String,
        port: u16,
        timeout: Option<Duration>,
        frame: *mut ExecuteData,
        return_value: *mut Value,
    ) -> Result<(), VmError> {
        let task = self.active_task("coroutine_tcp_connect")?;
        let deadline = timeout
            .map(|duration| {
                Instant::now().checked_add(duration).ok_or_else(|| {
                    VmError::Fatal("coroutine TCP connect timeout is too large".into())
                })
            })
            .transpose()?;
        if self.resolver.is_none() {
            self.resolver = Some(Box::new(ResolverSet::new(&mut self.io)?));
        }
        let resolver = self.resolver.as_mut().unwrap();
        let job = resolver.submit(host, port, task, frame, return_value)?;
        if let Err(error) = self.block_active(WaitReason::DnsResolve(job)) {
            self.resolver.as_mut().unwrap().cancel(job, task);
            return Err(error);
        }
        let wake_descriptor = self.resolver.as_ref().unwrap().wake_descriptor;
        self.io.arm_resolver_wake(wake_descriptor);
        if let Some(deadline) = deadline {
            self.readiness.schedule_timer(task, deadline);
        }
        Ok(())
    }

    pub(super) fn cancel_dns_resolve(&mut self, job: u64, task: u64) {
        let Some(resolver) = self.resolver.as_mut() else {
            return;
        };
        resolver.cancel(job, task);
        if !resolver.has_waiters() {
            self.io.disarm_resolver_wake(resolver.wake_descriptor);
        }
    }

    #[cold]
    pub(super) fn promote_resolver(&mut self, descriptor: u64) -> Result<(), VmError> {
        if descriptor != self.resolver.as_ref().unwrap().wake_descriptor {
            return Err(VmError::Fatal(
                "coroutine resolver signalled an inconsistent wake descriptor".into(),
            ));
        }
        self.io.acknowledge_ready(RESOLVER_WAKE_TASK);
        self.io.drain_resolver_wake(descriptor)?;
        let completions = self.resolver.as_mut().unwrap().drain();
        for completion in completions {
            let Some(waiter) = self.resolver.as_mut().unwrap().take_waiter(completion.id) else {
                continue;
            };
            match completion.result {
                Ok(addresses) => self.start_connect_candidates(
                    ConnectWaiter {
                        task: waiter.task,
                        frame: waiter.frame,
                        return_value: waiter.return_value,
                        remaining: addresses.into(),
                    },
                    WaitReason::DnsResolve(completion.id),
                    None,
                )?,
                Err(error) => {
                    self.readiness.cancel_timer(waiter.task);
                    return Err(VmError::Fatal(format!(
                        "failed to resolve coroutine TCP host: {error}"
                    )));
                }
            }
        }
        if self.resolver.as_ref().unwrap().has_waiters() {
            self.io.arm_resolver_wake(descriptor);
        }
        Ok(())
    }

    #[cold]
    pub(super) fn promote_tcp_connect(
        &mut self,
        descriptor: u64,
        task: u64,
    ) -> Result<(), VmError> {
        match self.io.complete_tcp_connect(descriptor, task)? {
            ConnectCompletion::Connected(waiter) => {
                self.readiness.cancel_timer(task);
                unsafe {
                    write_coroutine_result(
                        waiter.frame,
                        waiter.return_value,
                        Value::long(descriptor as i64),
                    );
                }
                self.wake_task(task, WaitReason::TcpConnect(descriptor))
            }
            ConnectCompletion::Pending => Ok(()),
            ConnectCompletion::Failed { waiter, error } => self.start_connect_candidates(
                waiter,
                WaitReason::TcpConnect(descriptor),
                Some(error),
            ),
        }
    }

    fn start_connect_candidates(
        &mut self,
        mut waiter: ConnectWaiter,
        expected: WaitReason,
        mut last_error: Option<VmError>,
    ) -> Result<(), VmError> {
        while let Some(address) = waiter.remaining.pop_front() {
            match self.io.create_tcp_connection(address) {
                Ok(ConnectOutcome::Connected(descriptor)) => {
                    self.readiness.cancel_timer(waiter.task);
                    unsafe {
                        write_coroutine_result(
                            waiter.frame,
                            waiter.return_value,
                            Value::long(descriptor as i64),
                        );
                    }
                    self.wake_task(waiter.task, expected)?;
                    return Ok(());
                }
                Ok(ConnectOutcome::InProgress(descriptor)) => {
                    self.replace_wait_reason(
                        waiter.task,
                        expected,
                        WaitReason::TcpConnect(descriptor),
                    )?;
                    self.io.enqueue_tcp_connect_waiter(descriptor, waiter);
                    return Ok(());
                }
                Err(error) => last_error = Some(error),
            }
        }
        self.readiness.cancel_timer(waiter.task);
        Err(last_error.unwrap_or_else(|| {
            VmError::Fatal("coroutine TCP host resolved to no addresses".into())
        }))
    }

    pub(super) fn replace_wait_reason(
        &mut self,
        task: u64,
        expected: WaitReason,
        replacement: WaitReason,
    ) -> Result<(), VmError> {
        let context = self.context_ptr(task)?;
        unsafe {
            if (*context).status != CoroutineStatus::Waiting
                || (*context).wait_reason != Some(expected)
            {
                return Err(VmError::Fatal(format!(
                    "coroutine {} has inconsistent wait transition",
                    task
                )));
            }
            (*context).wait_reason = Some(replacement);
        }
        Ok(())
    }
}

impl ResolverSet {
    pub(super) fn new(io: &mut IoSet) -> Result<Self, VmError> {
        let (wake_reader, wake_writer) =
            UnixStream::pair().map_err(|error| resolver_error("create wake stream", error))?;
        wake_reader
            .set_nonblocking(true)
            .map_err(|error| resolver_error("make wake reader non-blocking", error))?;
        wake_writer
            .set_nonblocking(true)
            .map_err(|error| resolver_error("make wake writer non-blocking", error))?;

        let (completion_sender, completion_receiver) = mpsc::channel();
        let wake_descriptor = io.register_resolver_wake(wake_reader)?;

        Ok(Self {
            completion_sender,
            completions: completion_receiver,
            wake_writer,
            wake_descriptor,
            next_id: 1,
            waiters: BTreeMap::new(),
        })
    }

    pub(super) fn submit(
        &mut self,
        host: String,
        port: u16,
        task: u64,
        frame: *mut ExecuteData,
        return_value: *mut Value,
    ) -> Result<u64, VmError> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| VmError::Fatal("coroutine resolver job space exhausted".into()))?;
        let request = ResolveRequest {
            id,
            host,
            port,
            completions: self.completion_sender.clone(),
            wake_writer: self
                .wake_writer
                .try_clone()
                .map_err(|error| resolver_error("clone wake writer", error))?,
        };
        match resolver_pool()?.requests.try_send(request) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                return Err(VmError::Fatal("coroutine resolver queue is full".into()));
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(VmError::Fatal("coroutine resolver workers stopped".into()));
            }
        }
        assert!(
            self.waiters
                .insert(
                    id,
                    ResolveWaiter {
                        task,
                        frame,
                        return_value,
                    },
                )
                .is_none()
        );
        Ok(id)
    }

    pub(super) fn has_waiters(&self) -> bool {
        !self.waiters.is_empty()
    }

    pub(super) fn drain(&mut self) -> Vec<ResolveCompletion> {
        self.completions.try_iter().collect()
    }

    pub(super) fn take_waiter(&mut self, id: u64) -> Option<ResolveWaiter> {
        self.waiters.remove(&id)
    }

    pub(super) fn cancel(&mut self, id: u64, task: u64) {
        if let Some(waiter) = self.waiters.remove(&id) {
            assert_eq!(waiter.task, task);
        }
    }
}

impl ResolverPool {
    fn new() -> Result<Self, String> {
        let (request_sender, request_receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let request_receiver = Arc::new(Mutex::new(request_receiver));
        for index in 0..WORKERS {
            let requests = Arc::clone(&request_receiver);
            thread::Builder::new()
                .name(format!("rphp-resolver-{index}"))
                .spawn(move || resolver_loop(requests))
                .map_err(|error| {
                    format!("failed to start worker for coroutine resolver: {error}")
                })?;
        }
        Ok(Self {
            requests: request_sender,
        })
    }
}

fn resolver_pool() -> Result<&'static ResolverPool, VmError> {
    RESOLVER_POOL
        .get_or_init(ResolverPool::new)
        .as_ref()
        .map_err(|error| VmError::Fatal(error.clone()))
}

fn resolver_loop(requests: Arc<Mutex<mpsc::Receiver<ResolveRequest>>>) {
    loop {
        let request = requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .recv();
        let Ok(mut request) = request else {
            break;
        };
        let result = (request.host.as_str(), request.port)
            .to_socket_addrs()
            .map(|addresses| addresses.collect());
        if request
            .completions
            .send(ResolveCompletion {
                id: request.id,
                result,
            })
            .is_err()
        {
            continue;
        }
        loop {
            match request.wake_writer.write(&[1]) {
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }
}

fn resolver_error(operation: &str, error: io::Error) -> VmError {
    VmError::Fatal(format!(
        "failed to {operation} for coroutine resolver: {error}"
    ))
}
