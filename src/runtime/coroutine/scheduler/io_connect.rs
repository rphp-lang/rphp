use std::collections::VecDeque;
use std::ffi::{c_int, c_void};
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd};

use super::{ByteStream, Descriptor, DescriptorState, IoSet, os_error};
use crate::value::Value;
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime::coroutine::scheduler) enum ConnectOutcome {
    Connected(u64),
    InProgress(u64),
}

#[derive(Clone)]
pub(in crate::runtime::coroutine::scheduler) struct ConnectWaiter {
    pub(in crate::runtime::coroutine::scheduler) task: u64,
    pub(in crate::runtime::coroutine::scheduler) frame: *mut ExecuteData,
    pub(in crate::runtime::coroutine::scheduler) return_value: *mut Value,
    pub(in crate::runtime::coroutine::scheduler) remaining: VecDeque<SocketAddr>,
}

pub(in crate::runtime::coroutine::scheduler) enum ConnectCompletion {
    Connected(ConnectWaiter),
    Pending,
    Failed {
        waiter: ConnectWaiter,
        error: VmError,
    },
}

impl IoSet {
    pub(in crate::runtime::coroutine::scheduler) fn create_tcp_connection(
        &mut self,
        address: SocketAddr,
    ) -> Result<ConnectOutcome, VmError> {
        let (stream, connected) = start(address)?;
        let id = self.allocate_id()?;
        self.descriptors.insert(
            id,
            DescriptorState::new(Descriptor::Stream(ByteStream::Tcp(stream))),
        );
        Ok(if connected {
            ConnectOutcome::Connected(id)
        } else {
            ConnectOutcome::InProgress(id)
        })
    }

    pub(in crate::runtime::coroutine::scheduler) fn enqueue_tcp_connect(
        &mut self,
        descriptor: u64,
        task: u64,
        frame: *mut ExecuteData,
        return_value: *mut Value,
        remaining: VecDeque<SocketAddr>,
    ) {
        self.enqueue_tcp_connect_waiter(
            descriptor,
            ConnectWaiter {
                task,
                frame,
                return_value,
                remaining,
            },
        );
    }

    pub(in crate::runtime::coroutine::scheduler) fn enqueue_tcp_connect_waiter(
        &mut self,
        descriptor: u64,
        waiter: ConnectWaiter,
    ) {
        let task = waiter.task;
        assert!(
            self.connect_waiters.insert(descriptor, waiter).is_none(),
            "a coroutine TCP descriptor can have only one connect continuation"
        );
        self.enqueue_waiter(descriptor, task, super::IoDirection::Writable);
    }

    pub(in crate::runtime::coroutine::scheduler) fn complete_tcp_connect(
        &mut self,
        descriptor: u64,
        task: u64,
    ) -> Result<ConnectCompletion, VmError> {
        let waiter = self.connect_waiter(descriptor, task)?;
        match self.finish_tcp_connection(descriptor) {
            Ok(true) => {
                self.connect_waiters.remove(&descriptor);
                Ok(ConnectCompletion::Connected(waiter))
            }
            Ok(false) => {
                self.acknowledge_ready(task);
                self.enqueue_waiter(descriptor, task, super::IoDirection::Writable);
                Ok(ConnectCompletion::Pending)
            }
            Err(error) => {
                self.cancel_tcp_connect(descriptor, task);
                Ok(ConnectCompletion::Failed { waiter, error })
            }
        }
    }

    pub(in crate::runtime::coroutine::scheduler) fn cancel_tcp_connect(
        &mut self,
        descriptor: u64,
        task: u64,
    ) {
        if let Some(waiter) = self.connect_waiters.remove(&descriptor) {
            assert_eq!(waiter.task, task);
        }
        self.in_flight.remove(&task);
        if let Some(state) = self.descriptors.get_mut(&descriptor) {
            state.writers.retain(|waiter| *waiter != task);
        }
        self.descriptors.remove(&descriptor);
    }

    fn connect_waiter(&self, descriptor: u64, task: u64) -> Result<ConnectWaiter, VmError> {
        let waiter = self.connect_waiters.get(&descriptor).ok_or_else(|| {
            VmError::Fatal(format!(
                "coroutine TCP connection {} has no pending continuation",
                descriptor
            ))
        })?;
        if waiter.task != task {
            return Err(VmError::Fatal(format!(
                "coroutine TCP connection {} belongs to task {}, not {}",
                descriptor, waiter.task, task
            )));
        }
        Ok(waiter.clone())
    }

    fn finish_tcp_connection(&mut self, descriptor: u64) -> Result<bool, VmError> {
        let state = self.descriptors.get_mut(&descriptor).ok_or_else(|| {
            VmError::Fatal(format!("unknown coroutine TCP connection {}", descriptor))
        })?;
        let Descriptor::Stream(ByteStream::Tcp(socket)) = &mut state.descriptor else {
            return Err(VmError::Fatal(format!(
                "coroutine descriptor {} is not a TCP connection",
                descriptor
            )));
        };
        if let Some(error) = socket
            .take_error()
            .map_err(|error| os_error("inspect coroutine TCP connection", error))?
        {
            return Err(os_error("connect coroutine TCP stream", error));
        }
        match socket.peer_addr() {
            Ok(_) => Ok(true),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotConnected | io::ErrorKind::WouldBlock
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(os_error("finish coroutine TCP connection", error)),
        }
    }
}

const AF_INET: c_int = 2;
#[cfg(target_os = "linux")]
const AF_INET6: c_int = 10;
#[cfg(target_vendor = "apple")]
const AF_INET6: c_int = 30;
const SOCK_STREAM: c_int = 1;
#[cfg(target_os = "linux")]
const SOCK_CLOEXEC: c_int = 0x80000;
#[cfg(target_vendor = "apple")]
const F_SETFD: c_int = 2;
#[cfg(target_vendor = "apple")]
const FD_CLOEXEC: c_int = 1;
#[cfg(target_vendor = "apple")]
const SOL_SOCKET: c_int = 0xffff;
#[cfg(target_vendor = "apple")]
const SO_NOSIGPIPE: c_int = 0x1022;
#[cfg(target_os = "linux")]
const EINPROGRESS: c_int = 115;
#[cfg(target_os = "linux")]
const EALREADY: c_int = 114;
#[cfg(target_os = "linux")]
const EISCONN: c_int = 106;
#[cfg(target_vendor = "apple")]
const EINPROGRESS: c_int = 36;
#[cfg(target_vendor = "apple")]
const EALREADY: c_int = 37;
#[cfg(target_vendor = "apple")]
const EISCONN: c_int = 56;

#[cfg(target_os = "linux")]
#[repr(C)]
struct SockAddrV4 {
    family: u16,
    port: u16,
    address: [u8; 4],
    zero: [u8; 8],
}

#[cfg(target_vendor = "apple")]
#[repr(C)]
struct SockAddrV4 {
    length: u8,
    family: u8,
    port: u16,
    address: [u8; 4],
    zero: [u8; 8],
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct SockAddrV6 {
    family: u16,
    port: u16,
    flow_info: u32,
    address: [u8; 16],
    scope_id: u32,
}

const _: () = assert!(size_of::<SockAddrV4>() == 16);
const _: () = assert!(size_of::<SockAddrV6>() == 28);

#[cfg(target_vendor = "apple")]
#[repr(C)]
struct SockAddrV6 {
    length: u8,
    family: u8,
    port: u16,
    flow_info: u32,
    address: [u8; 16],
    scope_id: u32,
}

unsafe extern "C" {
    #[link_name = "socket"]
    fn os_socket(domain: c_int, socket_type: c_int, protocol: c_int) -> c_int;
    #[link_name = "connect"]
    fn os_connect(socket: c_int, address: *const c_void, length: u32) -> c_int;
    #[cfg(target_vendor = "apple")]
    #[link_name = "fcntl"]
    fn os_fcntl(descriptor: c_int, command: c_int, ...) -> c_int;
    #[cfg(target_vendor = "apple")]
    #[link_name = "setsockopt"]
    fn os_setsockopt(
        socket: c_int,
        level: c_int,
        option: c_int,
        value: *const c_void,
        length: u32,
    ) -> c_int;
}

fn start(address: SocketAddr) -> Result<(TcpStream, bool), VmError> {
    let domain = if address.is_ipv4() { AF_INET } else { AF_INET6 };
    #[cfg(target_os = "linux")]
    let socket_type = SOCK_STREAM | SOCK_CLOEXEC;
    #[cfg(target_vendor = "apple")]
    let socket_type = SOCK_STREAM;
    // SAFETY: the selected domain/type constants belong to the active ABI and
    // this call passes no borrowed pointers.
    let descriptor = unsafe { os_socket(domain, socket_type, 0) };
    if descriptor < 0 {
        return Err(os_error(
            "create coroutine TCP connection socket",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: `socket` returned a fresh owned descriptor and this is its only
    // ownership transfer; every later error therefore closes it through RAII.
    let stream = unsafe { TcpStream::from_raw_fd(descriptor) };
    #[cfg(target_vendor = "apple")]
    if unsafe { os_fcntl(stream.as_raw_fd(), F_SETFD, FD_CLOEXEC) } < 0 {
        return Err(os_error(
            "mark coroutine TCP connection close-on-exec",
            io::Error::last_os_error(),
        ));
    }
    #[cfg(target_vendor = "apple")]
    {
        let enabled: c_int = 1;
        if unsafe {
            os_setsockopt(
                stream.as_raw_fd(),
                SOL_SOCKET,
                SO_NOSIGPIPE,
                (&raw const enabled).cast(),
                size_of::<c_int>() as u32,
            )
        } < 0
        {
            return Err(os_error(
                "disable SIGPIPE for coroutine TCP connection",
                io::Error::last_os_error(),
            ));
        }
    }
    stream
        .set_nonblocking(true)
        .map_err(|error| os_error("make coroutine TCP connection non-blocking", error))?;

    let connected = loop {
        let result = match address {
            SocketAddr::V4(address) => {
                #[cfg(target_os = "linux")]
                let native = SockAddrV4 {
                    family: AF_INET as u16,
                    port: address.port().to_be(),
                    address: address.ip().octets(),
                    zero: [0; 8],
                };
                #[cfg(target_vendor = "apple")]
                let native = SockAddrV4 {
                    length: size_of::<SockAddrV4>() as u8,
                    family: AF_INET as u8,
                    port: address.port().to_be(),
                    address: address.ip().octets(),
                    zero: [0; 8],
                };
                // SAFETY: `native` has the compile-time checked platform
                // layout and remains alive for the complete FFI call.
                unsafe {
                    os_connect(
                        stream.as_raw_fd(),
                        (&raw const native).cast(),
                        size_of::<SockAddrV4>() as u32,
                    )
                }
            }
            SocketAddr::V6(address) => {
                #[cfg(target_os = "linux")]
                let native = SockAddrV6 {
                    family: AF_INET6 as u16,
                    port: address.port().to_be(),
                    flow_info: address.flowinfo(),
                    address: address.ip().octets(),
                    scope_id: address.scope_id(),
                };
                #[cfg(target_vendor = "apple")]
                let native = SockAddrV6 {
                    length: size_of::<SockAddrV6>() as u8,
                    family: AF_INET6 as u8,
                    port: address.port().to_be(),
                    flow_info: address.flowinfo(),
                    address: address.ip().octets(),
                    scope_id: address.scope_id(),
                };
                // SAFETY: `native` has the compile-time checked platform
                // layout and remains alive for the complete FFI call.
                unsafe {
                    os_connect(
                        stream.as_raw_fd(),
                        (&raw const native).cast(),
                        size_of::<SockAddrV6>() as u32,
                    )
                }
            }
        };
        if result == 0 {
            break true;
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        match error.raw_os_error() {
            Some(EISCONN) => break true,
            Some(code) if code == EINPROGRESS || code == EALREADY => break false,
            _ => return Err(os_error("connect coroutine TCP stream", error)),
        }
    };
    Ok((stream, connected))
}

#[cfg(test)]
#[path = "io_connect_tests.rs"]
mod tests;
