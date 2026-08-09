#![cfg(unix)]

use std::collections::BTreeSet;
use std::ffi::{c_int, c_short};
use std::hint::black_box;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const READABLE: c_short = 0x0001;
const DEFAULT_WORKERS: usize = 2;
const DEFAULT_QUEUE_CAPACITY: usize = 64;

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: c_short,
    revents: c_short,
}

#[cfg(any(
    target_vendor = "apple",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
type PollCount = u32;

#[cfg(not(any(
    target_vendor = "apple",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
)))]
type PollCount = usize;

unsafe extern "C" {
    #[link_name = "poll"]
    fn os_poll(fds: *mut PollFd, count: PollCount, timeout: c_int) -> c_int;
}

struct ResolveRequest {
    id: u64,
    host: String,
    port: u16,
}

#[derive(Debug)]
struct ResolveCompletion {
    id: u64,
    result: io::Result<Vec<SocketAddr>>,
}

type ResolveFn = dyn Fn(&str, u16) -> io::Result<Vec<SocketAddr>> + Send + Sync;

struct ResolverWorker {
    requests: mpsc::SyncSender<Option<ResolveRequest>>,
    completions: mpsc::Receiver<ResolveCompletion>,
    wake_reader: UnixStream,
    workers: Vec<thread::JoinHandle<()>>,
    next_id: u64,
    pending: BTreeSet<u64>,
}

impl ResolverWorker {
    fn new() -> io::Result<Self> {
        Self::with_resolver(
            DEFAULT_WORKERS,
            DEFAULT_QUEUE_CAPACITY,
            Arc::new(system_resolve),
        )
    }

    fn with_resolver(
        worker_count: usize,
        queue_capacity: usize,
        resolve: Arc<ResolveFn>,
    ) -> io::Result<Self> {
        if worker_count == 0 || queue_capacity == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "resolver workers and queue capacity must be positive",
            ));
        }
        let (wake_reader, wake_writer) = UnixStream::pair()?;
        wake_reader.set_nonblocking(true)?;
        wake_writer.set_nonblocking(true)?;

        let (request_sender, request_receiver) = mpsc::sync_channel(queue_capacity);
        let request_receiver = Arc::new(Mutex::new(request_receiver));
        let (completion_sender, completion_receiver) = mpsc::channel();
        let mut workers = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            let requests = Arc::clone(&request_receiver);
            let completions = completion_sender.clone();
            let wake = wake_writer.try_clone()?;
            let resolve = Arc::clone(&resolve);
            workers.push(
                thread::Builder::new()
                    .name(format!("rphp-resolver-{index}"))
                    .spawn(move || resolver_loop(requests, completions, wake, resolve))?,
            );
        }
        drop(completion_sender);
        drop(wake_writer);

        Ok(Self {
            requests: request_sender,
            completions: completion_receiver,
            wake_reader,
            workers,
            next_id: 1,
            pending: BTreeSet::new(),
        })
    }

    fn submit(&mut self, host: impl Into<String>, port: u16) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("resolver job identifier space exhausted"))?;
        assert!(self.pending.insert(id));
        let request = ResolveRequest {
            id,
            host: host.into(),
            port,
        };
        match self.requests.try_send(Some(request)) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                self.pending.remove(&id);
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "resolver queue is full",
                ));
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.pending.remove(&id);
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "resolver workers stopped",
                ));
            }
        }
        Ok(id)
    }

    fn cancel(&mut self, id: u64) -> bool {
        self.pending.remove(&id)
    }

    fn wait_for(&mut self, count: usize, timeout: Duration) -> io::Result<Vec<ResolveCompletion>> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| io::Error::other("resolver timeout is too large"))?;
        let mut ready = Vec::with_capacity(count);
        while ready.len() < count {
            let now = Instant::now();
            if now >= deadline || !poll_readable(self.wake_reader.as_raw_fd(), deadline - now)? {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "resolver completion timed out",
                ));
            }
            ready.extend(self.drain_ready()?);
        }
        Ok(ready)
    }

    fn drain_ready(&mut self) -> io::Result<Vec<ResolveCompletion>> {
        let mut bytes = [0_u8; 256];
        loop {
            match self.wake_reader.read(&mut bytes) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }

        Ok(self
            .completions
            .try_iter()
            .filter(|completion| self.pending.remove(&completion.id))
            .collect())
    }

    fn pending_count(&self) -> usize {
        self.pending.len()
    }

    fn shutdown(mut self) -> thread::Result<()> {
        self.stop_workers()
    }

    fn stop_workers(&mut self) -> thread::Result<()> {
        for _ in 0..self.workers.len() {
            let _ = self.requests.send(None);
        }
        while let Some(worker) = self.workers.pop() {
            worker.join()?;
        }
        Ok(())
    }
}

impl Drop for ResolverWorker {
    fn drop(&mut self) {
        let _ = self.stop_workers();
    }
}

fn resolver_loop(
    requests: Arc<Mutex<mpsc::Receiver<Option<ResolveRequest>>>>,
    completions: mpsc::Sender<ResolveCompletion>,
    mut wake_writer: UnixStream,
    resolve: Arc<ResolveFn>,
) {
    loop {
        let request = requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .recv();
        let Ok(Some(request)) = request else {
            break;
        };
        let result = resolve(&request.host, request.port);
        if completions
            .send(ResolveCompletion {
                id: request.id,
                result,
            })
            .is_err()
        {
            break;
        }

        loop {
            match wake_writer.write(&[1]) {
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => return,
            }
        }
    }
}

fn system_resolve(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    (host, port)
        .to_socket_addrs()
        .map(|addresses| addresses.collect())
}

fn poll_readable(fd: c_int, timeout: Duration) -> io::Result<bool> {
    let mut poll_fd = PollFd {
        fd,
        events: READABLE,
        revents: 0,
    };
    let timeout = timeout.as_millis().max(1).min(c_int::MAX as u128) as c_int;
    loop {
        let result = unsafe { os_poll(&mut poll_fd, 1 as PollCount, timeout) };
        if result >= 0 {
            return Ok(result > 0 && poll_fd.revents != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[test]
fn resolver_worker_resolves_localhost_off_the_caller_thread() {
    let caller = thread::current().id();
    let mut resolver = ResolverWorker::new().unwrap();
    let id = resolver.submit("localhost", 43210).unwrap();
    let mut ready = resolver.wait_for(1, Duration::from_secs(2)).unwrap();
    let completion = ready.pop().unwrap();

    assert_eq!(completion.id, id);
    let addresses = completion.result.unwrap();
    assert!(!addresses.is_empty());
    assert!(addresses.iter().all(|address| address.port() == 43210));
    assert!(
        resolver
            .workers
            .iter()
            .all(|worker| caller != worker.thread().id())
    );
    assert_eq!(resolver.pending_count(), 0);
}

#[test]
fn resolver_worker_preserves_unique_job_ids_and_numeric_addresses() {
    let mut resolver = ResolverWorker::new().unwrap();
    let ids: Vec<_> = (0..64)
        .map(|port| resolver.submit("127.0.0.1", port).unwrap())
        .collect();
    let ready = resolver
        .wait_for(ids.len(), Duration::from_secs(2))
        .unwrap();
    let ready_ids: BTreeSet<_> = ready.iter().map(|completion| completion.id).collect();

    assert_eq!(ready_ids, ids.into_iter().collect());
    assert!(ready.iter().all(|completion| completion.result.is_ok()));
    assert_eq!(resolver.pending_count(), 0);
}

#[test]
fn cancelled_resolver_job_is_filtered_without_hiding_later_completion() {
    let mut resolver = ResolverWorker::new().unwrap();
    let cancelled = resolver.submit("127.0.0.1", 10001).unwrap();
    let retained = resolver.submit("127.0.0.1", 10002).unwrap();
    assert!(resolver.cancel(cancelled));

    let ready = resolver.wait_for(1, Duration::from_secs(2)).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, retained);
    assert_eq!(ready[0].result.as_ref().unwrap()[0].port(), 10002);
    assert_eq!(resolver.pending_count(), 0);
}

#[test]
fn second_worker_completes_a_fast_job_while_the_first_is_blocked() {
    let (started_sender, started_receiver) = mpsc::sync_channel(1);
    let (release_sender, release_receiver) = mpsc::sync_channel(1);
    let release_receiver = Arc::new(Mutex::new(release_receiver));
    let resolve = Arc::new(move |host: &str, port: u16| {
        if host == "slow" {
            started_sender.send(()).unwrap();
            release_receiver.lock().unwrap().recv().unwrap();
        }
        Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))])
    });
    let mut resolver = ResolverWorker::with_resolver(2, 4, resolve).unwrap();

    let slow = resolver.submit("slow", 10001).unwrap();
    started_receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let fast = resolver.submit("fast", 10002).unwrap();
    let ready = resolver.wait_for(1, Duration::from_secs(2)).unwrap();
    assert_eq!(ready[0].id, fast);

    release_sender.send(()).unwrap();
    let ready = resolver.wait_for(1, Duration::from_secs(2)).unwrap();
    assert_eq!(ready[0].id, slow);
    resolver.shutdown().unwrap();
}

#[test]
fn full_resolver_queue_reports_backpressure_without_blocking_submitter() {
    let (started_sender, started_receiver) = mpsc::sync_channel(1);
    let (release_sender, release_receiver) = mpsc::sync_channel(1);
    let release_receiver = Arc::new(Mutex::new(release_receiver));
    let resolve = Arc::new(move |host: &str, port: u16| {
        if host == "slow" {
            started_sender.send(()).unwrap();
            release_receiver.lock().unwrap().recv().unwrap();
        }
        Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))])
    });
    let mut resolver = ResolverWorker::with_resolver(1, 1, resolve).unwrap();

    resolver.submit("slow", 10001).unwrap();
    started_receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    resolver.submit("queued", 10002).unwrap();
    let error = resolver.submit("overflow", 10003).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);

    release_sender.send(()).unwrap();
    let ready = resolver.wait_for(2, Duration::from_secs(2)).unwrap();
    assert_eq!(ready.len(), 2);
    assert_eq!(resolver.pending_count(), 0);
}

#[test]
fn resolver_worker_reports_invalid_host_input_and_shuts_down_cleanly() {
    let mut resolver = ResolverWorker::new().unwrap();
    let id = resolver.submit("\0", 80).unwrap();
    let ready = resolver.wait_for(1, Duration::from_secs(2)).unwrap();

    assert_eq!(ready[0].id, id);
    assert!(ready[0].result.is_err());
    resolver.shutdown().unwrap();
}

#[test]
#[ignore = "run explicitly in release mode as the resolver transport benchmark"]
fn benchmark_numeric_resolver_worker_round_trips() {
    const JOBS: usize = 10_000;

    let direct_started = Instant::now();
    for port in 0..JOBS {
        let address = ("127.0.0.1", port as u16)
            .to_socket_addrs()
            .unwrap()
            .next()
            .unwrap();
        black_box(address);
    }
    let direct_elapsed = direct_started.elapsed();

    let mut resolver =
        ResolverWorker::with_resolver(DEFAULT_WORKERS, JOBS, Arc::new(system_resolve)).unwrap();
    let worker_started = Instant::now();
    for port in 0..JOBS {
        black_box(resolver.submit("127.0.0.1", port as u16).unwrap());
    }
    let ready = resolver.wait_for(JOBS, Duration::from_secs(10)).unwrap();
    let worker_elapsed = worker_started.elapsed();
    assert!(ready.iter().all(|completion| completion.result.is_ok()));

    let direct_ns = direct_elapsed.as_nanos() as f64 / JOBS as f64;
    let worker_ns = worker_elapsed.as_nanos() as f64 / JOBS as f64;
    eprintln!(
        "numeric resolver transport: direct {direct_ns:.2} ns/job, worker {worker_ns:.2} ns/job"
    );
    assert!(worker_ns < 100_000.0);
}
