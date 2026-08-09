use std::io;
use std::net::{SocketAddr, UdpSocket};

use super::{Descriptor, DescriptorState, IoSet, WriteOutcome, os_error};
use crate::vm::execute::VmError;

#[derive(Debug)]
pub(in crate::runtime::coroutine::scheduler) enum DatagramReceiveOutcome {
    Packet { bytes: Vec<u8>, peer: SocketAddr },
    WouldBlock,
}

impl IoSet {
    pub(in crate::runtime::coroutine::scheduler) fn create_udp_socket(
        &mut self,
        address: SocketAddr,
    ) -> Result<(u64, SocketAddr), VmError> {
        let socket = UdpSocket::bind(address)
            .map_err(|error| os_error("bind coroutine UDP socket", error))?;
        socket
            .set_nonblocking(true)
            .map_err(|error| os_error("make coroutine UDP socket non-blocking", error))?;
        let local = socket
            .local_addr()
            .map_err(|error| os_error("read coroutine UDP socket address", error))?;
        let descriptor = self.allocate_id()?;
        self.descriptors.insert(
            descriptor,
            DescriptorState::new(Descriptor::Datagram(socket)),
        );
        Ok((descriptor, local))
    }

    pub(in crate::runtime::coroutine::scheduler) fn send_udp(
        &self,
        descriptor: u64,
        bytes: &[u8],
        peer: SocketAddr,
    ) -> Result<WriteOutcome, VmError> {
        let socket = self.datagram(descriptor)?;
        match socket.send_to(bytes, peer) {
            Ok(written) => Ok(WriteOutcome::Written(written)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(WriteOutcome::WouldBlock),
            Err(error) => Err(os_error("send coroutine UDP datagram", error)),
        }
    }

    pub(in crate::runtime::coroutine::scheduler) fn receive_udp(
        &self,
        descriptor: u64,
        length: usize,
    ) -> Result<DatagramReceiveOutcome, VmError> {
        let socket = self.datagram(descriptor)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| VmError::Fatal("failed to reserve coroutine UDP receive buffer".into()))?;
        bytes.resize(length, 0);
        match socket.recv_from(&mut bytes) {
            Ok((read, peer)) => {
                bytes.truncate(read);
                Ok(DatagramReceiveOutcome::Packet { bytes, peer })
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                Ok(DatagramReceiveOutcome::WouldBlock)
            }
            Err(error) => Err(os_error("receive coroutine UDP datagram", error)),
        }
    }

    fn datagram(&self, descriptor: u64) -> Result<&UdpSocket, VmError> {
        let state = self.descriptor(descriptor)?;
        let Descriptor::Datagram(socket) = &state.descriptor else {
            return Err(VmError::Fatal(format!(
                "coroutine descriptor {} is not a UDP socket",
                descriptor
            )));
        };
        Ok(socket)
    }
}

#[cfg(test)]
#[path = "io_datagram_tests.rs"]
mod tests;
