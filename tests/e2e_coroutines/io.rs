#[cfg(unix)]
#[test]
fn non_blocking_stream_read_waits_while_a_writer_task_makes_progress() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $streams = coroutine_stream_pair();
    $reader = $streams[0];
    $writer = $streams[1];

    $readTask = coroutine_spawn(function () use ($reader) {
        echo "A";
        coroutine_wait_readable($reader);
        echo coroutine_stream_read($reader, 64);
        return "R";
    });
    $writeTask = coroutine_spawn(function () use ($writer) {
        echo "B";
        echo "C" . coroutine_stream_write($writer, "ready");
    });

    echo coroutine_join($readTask);
    coroutine_join($writeTask);
});
"#)
    .unwrap();

    assert_eq!(output, "ABC5readyR");
}

#[cfg(unix)]
#[test]
fn writable_readiness_queues_behind_work_that_was_already_runnable() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $streams = coroutine_stream_pair();
    $writer = $streams[1];
    $waiter = coroutine_spawn(function () use ($writer) {
        echo "A";
        coroutine_wait_writable($writer);
        echo "B";
    });
    $ready = coroutine_spawn(function () {
        echo "C";
    });

    coroutine_join($waiter);
    coroutine_join($ready);
});
"#)
    .unwrap();

    assert_eq!(output, "ACB");
}

#[cfg(unix)]
#[test]
fn timer_and_io_readiness_share_one_progress_loop() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $streams = coroutine_stream_pair();
    $reader = $streams[0];
    $writer = $streams[1];
    $readTask = coroutine_spawn(function () use ($reader) {
        echo "A";
        coroutine_wait_readable($reader);
        echo coroutine_stream_read($reader, 1);
    });
    $writeTask = coroutine_spawn(function () use ($writer) {
        echo "B";
        coroutine_sleep(5);
        echo "C";
        coroutine_stream_write($writer, "D");
    });

    coroutine_join($readTask);
    coroutine_join($writeTask);
});
"#)
    .unwrap();

    assert_eq!(output, "ABCD");
}

#[cfg(unix)]
#[test]
fn leaving_scope_cancels_an_unjoined_io_waiter() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $streams = coroutine_stream_pair();
    $reader = $streams[0];
    $waiter = coroutine_spawn(function () use ($reader) {
        coroutine_wait_readable($reader);
        echo "unreachable";
    });
    coroutine_resume($waiter);
    echo "root";
});
"#)
    .unwrap();

    assert_eq!(output, "root");
}

#[cfg(unix)]
#[test]
fn tcp_listener_accepts_without_blocking_other_logical_tasks() {
    let address = reserve_loopback_address();
    let client = spawn_loopback_client(address);
    let output = run(&format!(
        r#"<?php
coroutine_scope(function () {{
    $bound = coroutine_tcp_listen("{address}");
    $listener = $bound[0];
    echo $bound[1] . ":";
    $server = coroutine_spawn(function () use ($listener) {{
        coroutine_wait_readable($listener);
        $accepted = coroutine_tcp_accept($listener);
        $stream = $accepted[0];
        coroutine_wait_readable($stream);
        echo coroutine_stream_read($stream, 4);
        coroutine_stream_write($stream, "pong");
        return "done";
    }});
    coroutine_resume($server);
    $ready = coroutine_spawn(function () {{
        echo "R";
    }});

    echo ":" . coroutine_join($server);
    coroutine_join($ready);
}});
"#
    ));
    let response = client.join().unwrap();

    assert_eq!(output.unwrap(), format!("{address}:Rping:done"));
    assert_eq!(&response, b"pong");
}

#[cfg(any(target_vendor = "apple", target_os = "linux"))]
#[test]
fn tcp_connect_completes_without_blocking_other_logical_tasks() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, peer) = listener.accept().unwrap();
        assert!(peer.ip().is_loopback());
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 4];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"ping");
        stream.write_all(b"pong").unwrap();
    });
    let output = run(&format!(
        r#"<?php
coroutine_scope(function () {{
    $client = coroutine_spawn(function () {{
        $stream = coroutine_tcp_connect("{address}", 2000);
        echo $stream . ":";
        coroutine_stream_write($stream, "ping");
        coroutine_wait_readable($stream);
        echo coroutine_stream_read($stream, 4) . ":";
        return "done";
    }});
    coroutine_resume($client);
    $ready = coroutine_spawn(function () {{
        echo "R";
    }});
    echo coroutine_join($client);
    coroutine_join($ready);
}});
"#
    ));
    server.join().unwrap();

    assert_eq!(output.unwrap(), "R1:pong:done");
}

#[cfg(any(target_vendor = "apple", target_os = "linux"))]
#[test]
fn tcp_connect_resolves_hostnames_without_blocking_other_logical_tasks() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, peer) = listener.accept().unwrap();
        assert!(peer.ip().is_loopback());
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 4];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"ping");
        stream.write_all(b"pong").unwrap();
    });
    let output = run(&format!(
        r#"<?php
coroutine_scope(function () {{
    $client = coroutine_spawn(function () {{
        $stream = coroutine_tcp_connect("localhost:{port}", 2000);
        echo "connected:";
        coroutine_stream_write($stream, "ping");
        coroutine_wait_readable($stream);
        echo coroutine_stream_read($stream, 4) . ":";
        return "done";
    }});
    coroutine_resume($client);
    $ready = coroutine_spawn(function () {{
        echo "R";
    }});
    echo coroutine_join($client);
    coroutine_join($ready);
}});
"#
    ));
    server.join().unwrap();

    assert_eq!(output.unwrap(), "Rconnected:pong:done");
}

#[cfg(any(target_vendor = "apple", target_os = "linux"))]
#[test]
fn tcp_connect_reports_refusal_and_rejects_malformed_addresses() {
    let address = reserve_loopback_address();
    let refused = run(&format!(
        r#"<?php
coroutine_scope(function () {{
    $client = coroutine_spawn(function () {{
        coroutine_tcp_connect("{address}");
    }});
    coroutine_join($client);
}});
"#
    ))
    .unwrap_err();
    assert!(matches!(
        refused,
        execute::VmError::Fatal(message)
            if message.contains("connect coroutine TCP stream")
    ));

    let address_error = run(r#"<?php
coroutine_scope(function () {
    coroutine_tcp_connect("localhost");
});
"#)
    .unwrap_err();
    assert!(matches!(
        address_error,
        execute::VmError::Fatal(message)
            if message == "coroutine_tcp_connect expects an IP address or hostname followed by a port"
    ));
}
#[cfg(any(target_vendor = "apple", target_os = "linux"))]
#[test]
fn tcp_connect_rejects_invalid_timeout() {
    let timeout_error = run(r#"<?php
coroutine_scope(function () {
    coroutine_tcp_connect("127.0.0.1:1", -1);
});
"#)
    .unwrap_err();
    assert!(matches!(
        timeout_error,
        execute::VmError::Fatal(message)
            if message == "coroutine_tcp_connect expects non-negative timeout milliseconds"
    ));
}

#[cfg(unix)]
#[test]
fn udp_round_trip_uses_shared_readiness_without_blocking_ready_tasks() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $first = coroutine_udp_bind("127.0.0.1:0");
    $second = coroutine_udp_bind("127.0.0.1:0");
    $receiver = coroutine_spawn(function () use ($second) {
        coroutine_wait_readable($second[0]);
        $packet = coroutine_udp_recv_from($second[0], 64);
        echo "P" . $packet[0] . ":";
        coroutine_udp_send_to($second[0], "pong", $packet[1]);
        return "receiver";
    });
    coroutine_resume($receiver);
    $sender = coroutine_spawn(function () use ($first, $second) {
        coroutine_udp_send_to($first[0], "ping", $second[1]);
        coroutine_wait_readable($first[0]);
        $packet = coroutine_udp_recv_from($first[0], 64);
        echo "Q" . $packet[0] . ":";
        return "sender";
    });
    coroutine_resume($sender);
    $ready = coroutine_spawn(function () {
        echo "R";
    });

    echo coroutine_join($receiver) . ":";
    echo coroutine_join($sender);
    coroutine_join($ready);
});
"#)
    .unwrap();

    assert_eq!(output, "RPping:receiver:Qpong:sender");
}

#[cfg(unix)]
#[test]
fn udp_rejects_dns_bind_names_and_oversized_reads() {
    let address_error = run(r#"<?php
coroutine_scope(function () {
    coroutine_udp_bind("localhost:0");
});
"#)
    .unwrap_err();
    assert!(matches!(
        address_error,
        execute::VmError::Fatal(message)
            if message == "coroutine_udp_bind expects a numeric IP address and port"
    ));

    let length_error = run(r#"<?php
coroutine_scope(function () {
    $socket = coroutine_udp_bind("127.0.0.1:0");
    coroutine_udp_recv_from($socket[0], 65536);
});
"#)
    .unwrap_err();
    assert!(matches!(
        length_error,
        execute::VmError::Fatal(message)
            if message == "coroutine_udp_recv_from length exceeds 65535 bytes"
    ));
}

#[cfg(unix)]
#[test]
fn tcp_accept_reports_would_block_before_a_connection_arrives() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $bound = coroutine_tcp_listen("127.0.0.1:0");
    echo coroutine_tcp_accept($bound[0]) ? "accepted" : "waiting";
});
"#)
    .unwrap();

    assert_eq!(output, "waiting");
}

#[cfg(unix)]
#[test]
fn leaving_scope_cancels_an_unjoined_tcp_accept_waiter() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $bound = coroutine_tcp_listen("127.0.0.1:0");
    $waiter = coroutine_spawn(function () use ($bound) {
        coroutine_wait_readable($bound[0]);
        echo "unreachable";
    });
    coroutine_resume($waiter);
    echo "root";
});
"#)
    .unwrap();

    assert_eq!(output, "root");
}

#[cfg(unix)]
#[test]
fn tcp_listener_rejects_dns_names_and_writable_waits() {
    let address_error = run(r#"<?php
coroutine_scope(function () {
    coroutine_tcp_listen("localhost:8080");
});
"#)
    .unwrap_err();
    assert!(matches!(
        address_error,
        execute::VmError::Fatal(message)
            if message.starts_with("coroutine_tcp_listen expects a numeric IP address")
    ));

    let writable_error = run(r#"<?php
coroutine_scope(function () {
    $bound = coroutine_tcp_listen("127.0.0.1:0");
    $waiter = coroutine_spawn(function () use ($bound) {
        coroutine_wait_writable($bound[0]);
    });
    coroutine_join($waiter);
});
"#)
    .unwrap_err();
    assert!(matches!(
        writable_error,
        execute::VmError::Fatal(message)
            if message.contains("does not support writable readiness")
    ));
}
