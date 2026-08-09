use std::net::SocketAddr;

use super::CoroutineScheduler;
use super::io::{DatagramReceiveOutcome, WriteOutcome};
use crate::vm::execute::VmError;

impl CoroutineScheduler {
    pub(in crate::runtime::coroutine) fn create_udp_socket(
        &mut self,
        address: SocketAddr,
    ) -> Result<(u64, SocketAddr), VmError> {
        self.io.create_udp_socket(address)
    }

    pub(in crate::runtime::coroutine) fn send_udp(
        &self,
        descriptor: u64,
        bytes: &[u8],
        peer: SocketAddr,
    ) -> Result<Option<usize>, VmError> {
        match self.io.send_udp(descriptor, bytes, peer)? {
            WriteOutcome::Written(written) => Ok(Some(written)),
            WriteOutcome::WouldBlock => Ok(None),
        }
    }

    pub(in crate::runtime::coroutine) fn receive_udp(
        &self,
        descriptor: u64,
        length: usize,
    ) -> Result<Option<(Vec<u8>, SocketAddr)>, VmError> {
        match self.io.receive_udp(descriptor, length)? {
            DatagramReceiveOutcome::Packet { bytes, peer } => Ok(Some((bytes, peer))),
            DatagramReceiveOutcome::WouldBlock => Ok(None),
        }
    }
}
