use std::collections::{BTreeMap, VecDeque};
use std::ffi::{c_int, c_short};
use std::io::{self, Read, Write};
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
    pub(super) stream: u64,
    pub(super) direction: IoDirection,
}

pub(super) enum ReadOutcome {
    Data(Vec<u8>),
    WouldBlock,
}

pub(super) enum WriteOutcome {
    Written(usize),
    WouldBlock,
}

struct StreamState {
    stream: UnixStream,
    readers: VecDeque<u64>,
    writers: VecDeque<u64>,
    reader_in_flight: bool,
    writer_in_flight: bool,
}

impl StreamState {
    fn new(stream: UnixStream) -> Self {
        Self {
            stream,
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
    streams: BTreeMap<u64, StreamState>,
    poll_fds: Vec<PollFd>,
    poll_streams: Vec<u64>,
    in_flight: BTreeMap<u64, (u64, IoDirection)>,
}

impl Default for IoSet {
    fn default() -> Self {
        Self {
            next_id: 1,
            streams: BTreeMap::new(),
            poll_fds: Vec::new(),
            poll_streams: Vec::new(),
            in_flight: BTreeMap::new(),
        }
    }
}

impl IoSet {
    pub(super) fn create_pair(&mut self) -> Result<(u64, u64), VmError> {
        let first_id = self.next_id;
        let second_id = first_id
            .checked_add(1)
            .ok_or_else(|| VmError::Fatal("coroutine stream identifier space exhausted".into()))?;
        if second_id > i64::MAX as u64 {
            return Err(VmError::Fatal(
                "coroutine stream identifier space exhausted".into(),
            ));
        }
        let next_id = second_id
            .checked_add(1)
            .ok_or_else(|| VmError::Fatal("coroutine stream identifier space exhausted".into()))?;

        let (first, second) =
            UnixStream::pair().map_err(|error| os_error("create coroutine stream pair", error))?;
        first
            .set_nonblocking(true)
            .map_err(|error| os_error("make coroutine stream non-blocking", error))?;
        second
            .set_nonblocking(true)
            .map_err(|error| os_error("make coroutine stream non-blocking", error))?;

        self.next_id = next_id;
        self.streams.insert(first_id, StreamState::new(first));
        self.streams.insert(second_id, StreamState::new(second));
        Ok((first_id, second_id))
    }

    pub(super) fn ensure_stream(&self, stream: u64) -> Result<(), VmError> {
        if self.streams.contains_key(&stream) {
            Ok(())
        } else {
            Err(VmError::Fatal(format!(
                "unknown coroutine stream {}",
                stream
            )))
        }
    }

    pub(super) fn enqueue_waiter(&mut self, stream: u64, task: u64, direction: IoDirection) {
        let state = self
            .streams
            .get_mut(&stream)
            .expect("validated coroutine stream must remain registered");
        match direction {
            IoDirection::Readable => state.readers.push_back(task),
            IoDirection::Writable => state.writers.push_back(task),
        }
    }

    pub(super) fn read(&mut self, stream: u64, length: usize) -> Result<ReadOutcome, VmError> {
        let state = self.stream_mut(stream)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| VmError::Fatal("failed to reserve coroutine stream read buffer".into()))?;
        bytes.resize(length, 0);
        match state.stream.read(&mut bytes) {
            Ok(read) => {
                bytes.truncate(read);
                Ok(ReadOutcome::Data(bytes))
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(ReadOutcome::WouldBlock),
            Err(error) => Err(os_error("read coroutine stream", error)),
        }
    }

    pub(super) fn write(&mut self, stream: u64, bytes: &[u8]) -> Result<WriteOutcome, VmError> {
        let state = self.stream_mut(stream)?;
        match state.stream.write(bytes) {
            Ok(written) => Ok(WriteOutcome::Written(written)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(WriteOutcome::WouldBlock),
            Err(error) => Err(os_error("write coroutine stream", error)),
        }
    }

    pub(super) fn has_waiters(&self) -> bool {
        self.streams
            .values()
            .any(|stream| !stream.readers.is_empty() || !stream.writers.is_empty())
    }

    pub(super) fn acknowledge_ready(&mut self, task: u64) {
        let Some((stream, direction)) = self.in_flight.remove(&task) else {
            return;
        };
        let state = self
            .streams
            .get_mut(&stream)
            .expect("ready coroutine stream must remain registered");
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
            let stream = self.poll_streams[index];
            let state = self
                .streams
                .get_mut(&stream)
                .expect("polled coroutine stream must remain registered");
            let terminal = poll_fd.revents & TERMINAL_EVENTS != 0;
            if (poll_fd.revents & READ_EVENTS != 0 || terminal)
                && let Some(task) = state.readers.pop_front()
            {
                assert!(!state.reader_in_flight);
                state.reader_in_flight = true;
                assert!(
                    self.in_flight
                        .insert(task, (stream, IoDirection::Readable))
                        .is_none()
                );
                ready.push_back(IoReady {
                    task,
                    stream,
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
                        .insert(task, (stream, IoDirection::Writable))
                        .is_none()
                );
                ready.push_back(IoReady {
                    task,
                    stream,
                    direction: IoDirection::Writable,
                });
            }
        }
        Ok(())
    }

    fn prepare_poll_set(&mut self) {
        self.poll_fds.clear();
        self.poll_streams.clear();
        for (id, state) in &self.streams {
            let events = state.events();
            if events == 0 {
                continue;
            }
            self.poll_fds.push(PollFd {
                fd: state.stream.as_raw_fd(),
                events,
                revents: 0,
            });
            self.poll_streams.push(*id);
        }
    }

    fn stream_mut(&mut self, stream: u64) -> Result<&mut StreamState, VmError> {
        self.streams
            .get_mut(&stream)
            .ok_or_else(|| VmError::Fatal(format!("unknown coroutine stream {}", stream)))
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
mod tests {
    use super::*;

    #[test]
    fn non_blocking_pair_reports_readiness_and_preserves_bytes() {
        let mut io = IoSet::default();
        let (reader, writer) = io.create_pair().unwrap();
        assert!(matches!(
            io.read(reader, 16).unwrap(),
            ReadOutcome::WouldBlock
        ));

        io.enqueue_waiter(reader, 7, IoDirection::Readable);
        assert!(matches!(
            io.write(writer, b"ready").unwrap(),
            WriteOutcome::Written(5)
        ));
        let mut ready = VecDeque::new();
        io.poll_ready(Some(Duration::ZERO), &mut ready).unwrap();
        assert_eq!(
            ready.pop_front(),
            Some(IoReady {
                task: 7,
                stream: reader,
                direction: IoDirection::Readable,
            })
        );

        let ReadOutcome::Data(bytes) = io.read(reader, 16).unwrap() else {
            panic!("readable stream must return the queued bytes");
        };
        assert_eq!(bytes, b"ready");
    }

    #[test]
    fn one_readiness_edge_has_only_one_in_flight_waiter() {
        let mut io = IoSet::default();
        let (reader, writer) = io.create_pair().unwrap();
        io.enqueue_waiter(reader, 1, IoDirection::Readable);
        io.enqueue_waiter(reader, 2, IoDirection::Readable);
        assert!(matches!(
            io.write(writer, b"x").unwrap(),
            WriteOutcome::Written(1)
        ));

        let mut ready = VecDeque::new();
        io.poll_ready(Some(Duration::ZERO), &mut ready).unwrap();
        io.poll_ready(Some(Duration::ZERO), &mut ready).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready.front().unwrap().task, 1);

        io.acknowledge_ready(1);
        io.poll_ready(Some(Duration::ZERO), &mut ready).unwrap();
        assert_eq!(ready.len(), 2);
        assert_eq!(ready.back().unwrap().task, 2);
    }
}
