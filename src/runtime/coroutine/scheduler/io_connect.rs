#![allow(dead_code)] // staged internal adapter; the PHP surface is intentionally not linked yet

use std::ffi::{c_int, c_void};
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd};

use super::{ByteStream, Descriptor, DescriptorState, IoSet, os_error};
use crate::vm::execute::VmError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectOutcome {
    Connected(u64),
    InProgress(u64),
}

impl IoSet {
    fn create_tcp_connection(&mut self, address: SocketAddr) -> Result<ConnectOutcome, VmError> {
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
mod tests {
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    use super::super::{IoDirection, ReadOutcome, WriteOutcome};
    use super::*;

    #[test]
    fn outbound_tcp_connection_finishes_through_writable_readiness() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, peer) = listener.accept().unwrap();
            assert!(peer.ip().is_loopback());
            let mut request = [0; 4];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request, b"ping");
            stream.write_all(b"pong").unwrap();
        });

        let mut io = IoSet::default();
        let (client, connected) = match io.create_tcp_connection(address).unwrap() {
            ConnectOutcome::Connected(client) => (client, true),
            ConnectOutcome::InProgress(client) => (client, false),
        };
        let mut ready = VecDeque::new();
        if !connected {
            io.ensure_waitable(client, IoDirection::Writable).unwrap();
            io.enqueue_waiter(client, 51, IoDirection::Writable);
            io.poll_ready(Some(Duration::from_secs(2)), &mut ready)
                .unwrap();
            assert_eq!(ready.pop_front().unwrap().task, 51);
            io.acknowledge_ready(51);
        }
        assert!(io.finish_tcp_connection(client).unwrap());
        assert!(io.finish_tcp_connection(client).unwrap());

        assert!(matches!(
            io.write(client, b"ping").unwrap(),
            WriteOutcome::Written(4)
        ));
        io.enqueue_waiter(client, 52, IoDirection::Readable);
        io.poll_ready(Some(Duration::from_secs(2)), &mut ready)
            .unwrap();
        assert_eq!(ready.pop_front().unwrap().task, 52);
        io.acknowledge_ready(52);
        let ReadOutcome::Data(response) = io.read(client, 4).unwrap() else {
            panic!("readable outbound TCP stream must preserve response bytes");
        };
        assert_eq!(response, b"pong");
        server.join().unwrap();
    }

    #[test]
    fn outbound_tcp_connection_reports_refused_completion() {
        let address = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();

        let mut io = IoSet::default();
        let client = match io.create_tcp_connection(address) {
            Err(VmError::Fatal(message)) => {
                assert!(message.contains("connect coroutine TCP stream"));
                return;
            }
            Err(error) => panic!("unexpected outbound TCP connection error: {error:?}"),
            Ok(ConnectOutcome::InProgress(client)) => client,
            Ok(ConnectOutcome::Connected(_)) => {
                panic!("connection to a released loopback address must not complete")
            }
        };
        io.enqueue_waiter(client, 61, IoDirection::Writable);
        let mut ready = VecDeque::new();
        io.poll_ready(Some(Duration::from_secs(2)), &mut ready)
            .unwrap();
        assert_eq!(ready.pop_front().unwrap().task, 61);
        io.acknowledge_ready(61);
        assert!(matches!(
            io.finish_tcp_connection(client),
            Err(VmError::Fatal(message)) if message.contains("connect coroutine TCP stream")
        ));
    }
}
