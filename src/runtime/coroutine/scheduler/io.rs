use std::collections::{BTreeMap, VecDeque};
use std::ffi::{c_int, c_short};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::vm::execute::VmError;

// POSIX pollfd layout and event bits are identical on the supported Darwin
// and Linux targets. Keeping this tiny binding local avoids a dependency for
// one system call; both ABIs are exercised in the two-host test matrix.
const READ_EVENTS: c_short = 0x0001;
const WRITE_EVENTS: c_short = 0x0004;
const TERMINAL_EVENTS: c_short = 0x0008 | 0x0010 | 0x0020;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IoDirection {
    Readable,
    Writable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct IoReady {
    pub(super) task: u64,
    pub(super) descriptor: u64,
    pub(super) direction: IoDirection,
}

#[derive(Debug)]
pub(super) enum ReadOutcome {
    Data(Vec<u8>),
    WouldBlock,
}

#[derive(Debug)]
pub(super) enum WriteOutcome {
    Written(usize),
    WouldBlock,
}

#[derive(Debug)]
pub(super) enum AcceptOutcome {
    Accepted { stream: u64, peer: SocketAddr },
    WouldBlock,
}

enum ByteStream {
    Unix(UnixStream),
    Tcp(TcpStream),
}

impl ByteStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Unix(stream) => stream.read(bytes),
            Self::Tcp(stream) => stream.read(bytes),
        }
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Unix(stream) => stream.write(bytes),
            Self::Tcp(stream) => stream.write(bytes),
        }
    }

    fn raw_fd(&self) -> c_int {
        match self {
            Self::Unix(stream) => stream.as_raw_fd(),
            Self::Tcp(stream) => stream.as_raw_fd(),
        }
    }
}

enum Descriptor {
    Stream(ByteStream),
    Listener(TcpListener),
}

impl Descriptor {
    fn raw_fd(&self) -> c_int {
        match self {
            Self::Stream(stream) => stream.raw_fd(),
            Self::Listener(listener) => listener.as_raw_fd(),
        }
    }

    fn stream_mut(&mut self) -> Option<&mut ByteStream> {
        match self {
            Self::Stream(stream) => Some(stream),
            Self::Listener(_) => None,
        }
    }

    fn listener(&self) -> Option<&TcpListener> {
        match self {
            Self::Listener(listener) => Some(listener),
            Self::Stream(_) => None,
        }
    }
}

struct DescriptorState {
    descriptor: Descriptor,
    readers: VecDeque<u64>,
    writers: VecDeque<u64>,
    reader_in_flight: bool,
    writer_in_flight: bool,
}

impl DescriptorState {
    fn new(descriptor: Descriptor) -> Self {
        Self {
            descriptor,
            readers: VecDeque::new(),
            writers: VecDeque::new(),
            reader_in_flight: false,
            writer_in_flight: false,
        }
    }

    fn events(&self) -> c_short {
        let mut events = 0;
        if !self.reader_in_flight && !self.readers.is_empty() {
            events |= READ_EVENTS;
        }
        if !self.writer_in_flight && !self.writers.is_empty() {
            events |= WRITE_EVENTS;
        }
        events
    }
}

pub(super) struct IoSet {
    next_id: u64,
    descriptors: BTreeMap<u64, DescriptorState>,
    poll_fds: Vec<PollFd>,
    poll_streams: Vec<u64>,
    in_flight: BTreeMap<u64, (u64, IoDirection)>,
}

impl Default for IoSet {
    fn default() -> Self {
        Self {
            next_id: 1,
            descriptors: BTreeMap::new(),
            poll_fds: Vec::new(),
            poll_streams: Vec::new(),
            in_flight: BTreeMap::new(),
        }
    }
}

impl IoSet {
    pub(super) fn create_pair(&mut self) -> Result<(u64, u64), VmError> {
        let (first, second) =
            UnixStream::pair().map_err(|error| os_error("create coroutine stream pair", error))?;
        first
            .set_nonblocking(true)
            .map_err(|error| os_error("make coroutine stream non-blocking", error))?;
        second
            .set_nonblocking(true)
            .map_err(|error| os_error("make coroutine stream non-blocking", error))?;

        let first_id = self.allocate_id()?;
        let second_id = self.allocate_id()?;
        self.descriptors.insert(
            first_id,
            DescriptorState::new(Descriptor::Stream(ByteStream::Unix(first))),
        );
        self.descriptors.insert(
            second_id,
            DescriptorState::new(Descriptor::Stream(ByteStream::Unix(second))),
        );
        Ok((first_id, second_id))
    }

    pub(super) fn create_tcp_listener(
        &mut self,
        address: SocketAddr,
    ) -> Result<(u64, SocketAddr), VmError> {
        let listener = TcpListener::bind(address)
            .map_err(|error| os_error("bind coroutine TCP listener", error))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| os_error("make coroutine TCP listener non-blocking", error))?;
        let local = listener
            .local_addr()
            .map_err(|error| os_error("read coroutine TCP listener address", error))?;
        let id = self.allocate_id()?;
        self.descriptors
            .insert(id, DescriptorState::new(Descriptor::Listener(listener)));
        Ok((id, local))
    }

    pub(super) fn accept(&mut self, listener: u64) -> Result<AcceptOutcome, VmError> {
        let accepted = {
            let state = self.descriptor(listener)?;
            let listener = state.descriptor.listener().ok_or_else(|| {
                VmError::Fatal(format!(
                    "coroutine descriptor {} is not a TCP listener",
                    listener
                ))
            })?;
            match listener.accept() {
                Ok(accepted) => accepted,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(AcceptOutcome::WouldBlock);
                }
                Err(error) => return Err(os_error("accept coroutine TCP connection", error)),
            }
        };
        accepted
            .0
            .set_nonblocking(true)
            .map_err(|error| os_error("make accepted coroutine TCP stream non-blocking", error))?;
        let stream = self.allocate_id()?;
        self.descriptors.insert(
            stream,
            DescriptorState::new(Descriptor::Stream(ByteStream::Tcp(accepted.0))),
        );
        Ok(AcceptOutcome::Accepted {
            stream,
            peer: accepted.1,
        })
    }

    pub(super) fn ensure_waitable(
        &self,
        descriptor: u64,
        direction: IoDirection,
    ) -> Result<(), VmError> {
        let state = self.descriptor(descriptor)?;
        if direction == IoDirection::Writable && matches!(state.descriptor, Descriptor::Listener(_))
        {
            return Err(VmError::Fatal(format!(
                "coroutine TCP listener {} does not support writable readiness",
                descriptor
            )));
        }
        Ok(())
    }

    pub(super) fn enqueue_waiter(&mut self, descriptor: u64, task: u64, direction: IoDirection) {
        let state = self
            .descriptors
            .get_mut(&descriptor)
            .expect("validated coroutine descriptor must remain registered");
        match direction {
            IoDirection::Readable => state.readers.push_back(task),
            IoDirection::Writable => state.writers.push_back(task),
        }
    }

    pub(super) fn read(&mut self, stream: u64, length: usize) -> Result<ReadOutcome, VmError> {
        let stream = self.byte_stream_mut(stream)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| VmError::Fatal("failed to reserve coroutine stream read buffer".into()))?;
        bytes.resize(length, 0);
        match stream.read(&mut bytes) {
            Ok(read) => {
                bytes.truncate(read);
                Ok(ReadOutcome::Data(bytes))
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(ReadOutcome::WouldBlock),
            Err(error) => Err(os_error("read coroutine stream", error)),
        }
    }

    pub(super) fn write(&mut self, stream: u64, bytes: &[u8]) -> Result<WriteOutcome, VmError> {
        let stream = self.byte_stream_mut(stream)?;
        match stream.write(bytes) {
            Ok(written) => Ok(WriteOutcome::Written(written)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(WriteOutcome::WouldBlock),
            Err(error) => Err(os_error("write coroutine stream", error)),
        }
    }

    pub(super) fn has_waiters(&self) -> bool {
        self.descriptors
            .values()
            .any(|descriptor| !descriptor.readers.is_empty() || !descriptor.writers.is_empty())
    }

    pub(super) fn acknowledge_ready(&mut self, task: u64) {
        let Some((stream, direction)) = self.in_flight.remove(&task) else {
            return;
        };
        let state = self
            .descriptors
            .get_mut(&stream)
            .expect("ready coroutine descriptor must remain registered");
        match direction {
            IoDirection::Readable => {
                assert!(state.reader_in_flight);
                state.reader_in_flight = false;
            }
            IoDirection::Writable => {
                assert!(state.writer_in_flight);
                state.writer_in_flight = false;
            }
        }
    }

    pub(super) fn poll_ready(
        &mut self,
        timeout: Option<Duration>,
        ready: &mut VecDeque<IoReady>,
    ) -> Result<(), VmError> {
        self.prepare_poll_set();
        if self.poll_fds.is_empty() {
            return Ok(());
        }

        let timeout = poll_timeout(timeout);
        loop {
            let result = unsafe {
                os_poll(
                    self.poll_fds.as_mut_ptr(),
                    self.poll_fds.len() as PollCount,
                    timeout,
                )
            };
            if result >= 0 {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(());
            }
            return Err(os_error("poll coroutine streams", error));
        }

        for (index, poll_fd) in self.poll_fds.iter().enumerate() {
            if poll_fd.revents == 0 {
                continue;
            }
            let descriptor = self.poll_streams[index];
            let state = self
                .descriptors
                .get_mut(&descriptor)
                .expect("polled coroutine descriptor must remain registered");
            let terminal = poll_fd.revents & TERMINAL_EVENTS != 0;
            if (poll_fd.revents & READ_EVENTS != 0 || terminal)
                && let Some(task) = state.readers.pop_front()
            {
                assert!(!state.reader_in_flight);
                state.reader_in_flight = true;
                assert!(
                    self.in_flight
                        .insert(task, (descriptor, IoDirection::Readable))
                        .is_none()
                );
                ready.push_back(IoReady {
                    task,
                    descriptor,
                    direction: IoDirection::Readable,
                });
            }
            if (poll_fd.revents & WRITE_EVENTS != 0 || terminal)
                && let Some(task) = state.writers.pop_front()
            {
                assert!(!state.writer_in_flight);
                state.writer_in_flight = true;
                assert!(
                    self.in_flight
                        .insert(task, (descriptor, IoDirection::Writable))
                        .is_none()
                );
                ready.push_back(IoReady {
                    task,
                    descriptor,
                    direction: IoDirection::Writable,
                });
            }
        }
        Ok(())
    }

    fn prepare_poll_set(&mut self) {
        self.poll_fds.clear();
        self.poll_streams.clear();
        for (id, state) in &self.descriptors {
            let events = state.events();
            if events == 0 {
                continue;
            }
            self.poll_fds.push(PollFd {
                fd: state.descriptor.raw_fd(),
                events,
                revents: 0,
            });
            self.poll_streams.push(*id);
        }
    }

    fn allocate_id(&mut self) -> Result<u64, VmError> {
        let id = self.next_id;
        if id > i64::MAX as u64 {
            return Err(VmError::Fatal(
                "coroutine descriptor identifier space exhausted".into(),
            ));
        }
        self.next_id = id.checked_add(1).ok_or_else(|| {
            VmError::Fatal("coroutine descriptor identifier space exhausted".into())
        })?;
        Ok(id)
    }

    fn descriptor(&self, descriptor: u64) -> Result<&DescriptorState, VmError> {
        self.descriptors
            .get(&descriptor)
            .ok_or_else(|| VmError::Fatal(format!("unknown coroutine descriptor {}", descriptor)))
    }

    fn byte_stream_mut(&mut self, stream: u64) -> Result<&mut ByteStream, VmError> {
        self.descriptors
            .get_mut(&stream)
            .ok_or_else(|| VmError::Fatal(format!("unknown coroutine stream {}", stream)))?
            .descriptor
            .stream_mut()
            .ok_or_else(|| {
                VmError::Fatal(format!(
                    "coroutine descriptor {} is not a byte stream",
                    stream
                ))
            })
    }
}

fn poll_timeout(timeout: Option<Duration>) -> c_int {
    match timeout {
        None => -1,
        Some(timeout) if timeout.is_zero() => 0,
        Some(timeout) => timeout.as_millis().max(1).min(c_int::MAX as u128) as c_int,
    }
}

fn os_error(operation: &str, error: io::Error) -> VmError {
    VmError::Fatal(format!("failed to {}: {}", operation, error))
}

#[cfg(test)]
#[path = "io_tests.rs"]
mod tests;
