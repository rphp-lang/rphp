use super::*;
use std::thread;

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
            descriptor: reader,
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

#[test]
fn tcp_listener_accepts_a_non_blocking_scope_owned_stream() {
    let mut io = IoSet::default();
    let (listener, address) = io
        .create_tcp_listener("127.0.0.1:0".parse().unwrap())
        .unwrap();
    io.enqueue_waiter(listener, 11, IoDirection::Readable);

    let client = thread::spawn(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(b"ping").unwrap();
        let mut response = [0; 4];
        stream.read_exact(&mut response).unwrap();
        response
    });

    let mut ready = VecDeque::new();
    io.poll_ready(Some(Duration::from_secs(2)), &mut ready)
        .unwrap();
    assert_eq!(ready.pop_front().unwrap().task, 11);
    io.acknowledge_ready(11);

    let AcceptOutcome::Accepted { stream, peer } = io.accept(listener).unwrap() else {
        panic!("readable TCP listener must accept the queued connection");
    };
    assert!(peer.ip().is_loopback());
    io.enqueue_waiter(stream, 12, IoDirection::Readable);
    io.poll_ready(Some(Duration::from_secs(2)), &mut ready)
        .unwrap();
    assert_eq!(ready.pop_front().unwrap().task, 12);
    io.acknowledge_ready(12);

    let ReadOutcome::Data(bytes) = io.read(stream, 4).unwrap() else {
        panic!("readable accepted stream must preserve client bytes");
    };
    assert_eq!(bytes, b"ping");
    assert!(matches!(
        io.write(stream, b"pong").unwrap(),
        WriteOutcome::Written(4)
    ));
    assert_eq!(&client.join().unwrap(), b"pong");
}

#[test]
fn listener_rejects_stream_only_operations() {
    let mut io = IoSet::default();
    let (listener, _) = io
        .create_tcp_listener("127.0.0.1:0".parse().unwrap())
        .unwrap();

    assert!(io.ensure_waitable(listener, IoDirection::Readable).is_ok());
    assert!(matches!(
        io.ensure_waitable(listener, IoDirection::Writable),
        Err(VmError::Fatal(message)) if message.contains("does not support writable readiness")
    ));
    assert!(matches!(
        io.read(listener, 1),
        Err(VmError::Fatal(message)) if message.contains("is not a byte stream")
    ));
    assert!(matches!(
        io.write(listener, b"invalid"),
        Err(VmError::Fatal(message)) if message.contains("is not a byte stream")
    ));

    let (stream, _) = io.create_pair().unwrap();
    assert!(matches!(
        io.accept(stream),
        Err(VmError::Fatal(message)) if message.contains("is not a TCP listener")
    ));
}
