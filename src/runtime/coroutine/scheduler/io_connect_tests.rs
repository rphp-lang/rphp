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

#[test]
fn cancelling_connect_continuation_closes_its_private_descriptor() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let mut io = IoSet::default();
    let client = match io.create_tcp_connection(address).unwrap() {
        ConnectOutcome::Connected(client) | ConnectOutcome::InProgress(client) => client,
    };
    io.enqueue_tcp_connect(client, 71, std::ptr::null_mut(), std::ptr::null_mut());

    io.cancel_tcp_connect(client, 71);

    assert!(!io.connect_waiters.contains_key(&client));
    assert!(!io.descriptors.contains_key(&client));
    assert!(!io.in_flight.contains_key(&71));
}
