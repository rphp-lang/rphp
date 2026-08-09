use std::io;
use std::net::{SocketAddr, TcpListener};
use std::os::unix::net::UnixStream;

use super::{
    AcceptOutcome, ByteStream, Descriptor, DescriptorState, IoDirection, IoSet, ReadOutcome,
    WriteOutcome, os_error,
};
use crate::vm::execute::VmError;

impl IoSet {
    pub(in crate::runtime::coroutine::scheduler) fn create_pair(
        &mut self,
    ) -> Result<(u64, u64), VmError> {
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

    pub(in crate::runtime::coroutine::scheduler) fn create_tcp_listener(
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

    pub(in crate::runtime::coroutine::scheduler) fn accept(
        &mut self,
        listener: u64,
    ) -> Result<AcceptOutcome, VmError> {
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

    pub(in crate::runtime::coroutine::scheduler) fn ensure_waitable(
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

    pub(in crate::runtime::coroutine::scheduler) fn enqueue_waiter(
        &mut self,
        descriptor: u64,
        task: u64,
        direction: IoDirection,
    ) {
        let state = self
            .descriptors
            .get_mut(&descriptor)
            .expect("validated coroutine descriptor must remain registered");
        match direction {
            IoDirection::Readable => state.readers.push_back(task),
            IoDirection::Writable => state.writers.push_back(task),
        }
    }

    pub(in crate::runtime::coroutine::scheduler) fn read(
        &mut self,
        stream: u64,
        length: usize,
    ) -> Result<ReadOutcome, VmError> {
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

    pub(in crate::runtime::coroutine::scheduler) fn write(
        &mut self,
        stream: u64,
        bytes: &[u8],
    ) -> Result<WriteOutcome, VmError> {
        let stream = self.byte_stream_mut(stream)?;
        match stream.write(bytes) {
            Ok(written) => Ok(WriteOutcome::Written(written)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(WriteOutcome::WouldBlock),
            Err(error) => Err(os_error("write coroutine stream", error)),
        }
    }
}
