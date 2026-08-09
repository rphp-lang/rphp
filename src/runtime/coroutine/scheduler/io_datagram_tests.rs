use std::collections::VecDeque;
use std::time::Duration;

use super::super::{IoDirection, IoReady};
use super::*;

#[test]
fn udp_datagrams_use_the_shared_readiness_driver_and_preserve_peers() {
    let mut io = IoSet::default();
    let (first, first_address) = io
        .create_udp_socket("127.0.0.1:0".parse().unwrap())
        .unwrap();
    let (second, second_address) = io
        .create_udp_socket("127.0.0.1:0".parse().unwrap())
        .unwrap();

    assert!(io.ensure_waitable(first, IoDirection::Writable).is_ok());
    io.enqueue_waiter(first, 80, IoDirection::Writable);
    let mut ready = VecDeque::new();
    io.poll_ready(Some(Duration::from_secs(2)), &mut ready)
        .unwrap();
    assert_eq!(
        ready.pop_front(),
        Some(IoReady {
            task: 80,
            descriptor: first,
            direction: IoDirection::Writable,
        })
    );
    io.acknowledge_ready(80);

    assert!(matches!(
        io.receive_udp(second, 64).unwrap(),
        DatagramReceiveOutcome::WouldBlock
    ));
    io.enqueue_waiter(second, 81, IoDirection::Readable);
    assert!(matches!(
        io.send_udp(first, b"ping", second_address).unwrap(),
        WriteOutcome::Written(4)
    ));

    io.poll_ready(Some(Duration::from_secs(2)), &mut ready)
        .unwrap();
    assert_eq!(
        ready.pop_front(),
        Some(IoReady {
            task: 81,
            descriptor: second,
            direction: IoDirection::Readable,
        })
    );
    io.acknowledge_ready(81);
    let DatagramReceiveOutcome::Packet { bytes, peer } = io.receive_udp(second, 64).unwrap() else {
        panic!("readable UDP socket must preserve its datagram");
    };
    assert_eq!(bytes, b"ping");
    assert_eq!(peer, first_address);

    assert!(matches!(
        io.send_udp(second, b"pong", peer).unwrap(),
        WriteOutcome::Written(4)
    ));
    io.enqueue_waiter(first, 82, IoDirection::Readable);
    io.poll_ready(Some(Duration::from_secs(2)), &mut ready)
        .unwrap();
    assert_eq!(ready.pop_front().unwrap().task, 82);
    io.acknowledge_ready(82);
    let DatagramReceiveOutcome::Packet { bytes, peer } = io.receive_udp(first, 64).unwrap() else {
        panic!("reply UDP socket must preserve its datagram");
    };
    assert_eq!(bytes, b"pong");
    assert_eq!(peer, second_address);
}

#[test]
fn udp_operations_reject_other_descriptor_kinds() {
    let mut io = IoSet::default();
    let (stream, _) = io.create_pair().unwrap();
    assert!(matches!(
        io.send_udp(stream, b"invalid", "127.0.0.1:9".parse().unwrap()),
        Err(VmError::Fatal(message)) if message.contains("is not a UDP socket")
    ));

    let (socket, _) = io
        .create_udp_socket("127.0.0.1:0".parse().unwrap())
        .unwrap();
    assert!(matches!(
        io.read(socket, 1),
        Err(VmError::Fatal(message)) if message.contains("is not a byte stream")
    ));
    assert!(matches!(
        io.accept(socket),
        Err(VmError::Fatal(message)) if message.contains("is not a TCP listener")
    ));
}
