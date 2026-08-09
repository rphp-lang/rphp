#![cfg(feature = "coroutines")]

mod common;

#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::net::{SocketAddr, TcpListener, TcpStream};
#[cfg(unix)]
use std::thread;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::runtime::coroutine;
use rphp::stdlib;
use rphp::vm::execute;
use rphp::vm::function::FunctionCommon;

fn run(source: &str) -> Result<String, execute::VmError> {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();
    let main_function = make_user_function(compiled.main);
    let (mut eg, output) = common::make_eg_with_capture();
    let _stdlib = stdlib::register_stdlib(&mut eg);
    let _coroutines = coroutine::register_api(&mut eg);
    for (name, function) in &compiled.functions {
        eg.register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class in compiled.class_defs {
        eg.register_class(class).unwrap();
    }

    execute::execute(&mut eg, &main_function)?;
    let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    Ok(output)
}

#[cfg(unix)]
fn reserve_loopback_address() -> SocketAddr {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

#[cfg(unix)]
fn spawn_loopback_client(address: SocketAddr) -> thread::JoinHandle<[u8; 4]> {
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut stream = loop {
            match TcpStream::connect(address) {
                Ok(stream) => break stream,
                Err(_) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("failed to connect loopback coroutine client: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(b"ping").unwrap();
        let mut response = [0; 4];
        stream.read_exact(&mut response).unwrap();
        response
    })
}

#[test]
fn php_api_resumes_and_joins_a_suspended_closure() {
    let output = run(r#"<?php
$result = coroutine_scope(function () {
    $task = coroutine_spawn(function () {
        echo "A";
        coroutine_suspend();
        echo "B";
        return 42;
    });

    echo "R";
    echo coroutine_resume($task) ? "S" : "D";
    echo "M";
    $value = coroutine_join($task);
    echo $value;
    return $value;
});
echo ":";
echo $result;
"#)
    .unwrap();

    assert_eq!(output, "RASM B42:42".replace(' ', ""));
}

#[test]
fn join_rethrows_child_exception_through_parent_catch_and_finally() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $task = coroutine_spawn(function () {
        try {
            coroutine_suspend();
            throw new Exception("boom");
        } finally {
            echo "F";
        }
    });

    coroutine_resume($task);
    try {
        coroutine_join($task);
    } catch (Exception $error) {
        echo $error->getMessage();
    } finally {
        echo "P";
    }
});
"#)
    .unwrap();

    assert_eq!(output, "FboomP");
}

#[test]
fn leaving_scope_cancels_an_unjoined_suspended_child() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $task = coroutine_spawn(function () {
        $payload = "owned";
        echo $payload;
        coroutine_suspend();
        echo "unreachable";
    });
    coroutine_resume($task);
    echo ":root";
});
echo ":done";
"#)
    .unwrap();

    assert_eq!(output, "owned:root:done");
}

#[test]
fn scope_propagates_an_unjoined_child_failure() {
    let output = run(r#"<?php
try {
    coroutine_scope(function () {
        $task = coroutine_spawn(function () {
            throw new Exception("unjoined");
        });
        coroutine_resume($task);
        echo "root";
    });
} catch (Exception $error) {
    echo ":";
    echo $error->getMessage();
}
"#)
    .unwrap();

    assert_eq!(output, "root:unjoined");
}

#[test]
fn scope_propagates_the_oldest_unjoined_failure_deterministically() {
    let output = run(r#"<?php
try {
    coroutine_scope(function () {
        $first = coroutine_spawn(function () {
            throw new Exception("first");
        });
        $second = coroutine_spawn(function () {
            throw new Exception("second");
        });
        coroutine_resume($first);
        coroutine_resume($second);
    });
} catch (Exception $error) {
    echo $error->getMessage();
}
"#)
    .unwrap();

    assert_eq!(output, "first");
}

#[test]
fn child_spawn_is_owned_by_the_same_scope() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $parent = coroutine_spawn(function () {
        $nested = coroutine_spawn(function () {
            echo "C";
            return 7;
        });
        echo "P";
        coroutine_suspend();
        echo "Q";
        return $nested;
    });

    coroutine_resume($parent);
    $nested = coroutine_join($parent);
    echo coroutine_join($nested);
});
"#)
    .unwrap();

    assert_eq!(output, "PQC7");
}

#[test]
fn suspension_preserves_a_multi_frame_php_call_chain() {
    let output = run(r#"<?php
function deepest() {
    echo "D";
    coroutine_suspend();
    echo "R";
    return 3;
}
function middle() {
    return deepest() + 4;
}

coroutine_scope(function () {
    $task = coroutine_spawn(function () {
        return middle();
    });
    coroutine_resume($task);
    echo ":";
    echo coroutine_join($task);
});
"#)
    .unwrap();

    assert_eq!(output, "D:R7");
}

#[test]
fn bounded_channel_applies_backpressure_and_preserves_fifo_values() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $channel = coroutine_channel(1);
    $producer = coroutine_spawn(function () use ($channel) {
        echo "P";
        coroutine_send($channel, "A");
        echo "1";
        coroutine_send($channel, "B");
        echo "2";
        return "producer";
    });
    $consumer = coroutine_spawn(function () use ($channel) {
        echo coroutine_receive($channel);
        echo coroutine_receive($channel);
        return "consumer";
    });

    echo coroutine_join($producer);
    echo coroutine_join($consumer);
});
"#)
    .unwrap();

    assert_eq!(output, "P1AB2producerconsumer");
}

#[test]
fn channel_can_deliver_a_heap_value_to_an_already_waiting_receiver() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $channel = coroutine_channel(1);
    $receiver = coroutine_spawn(function () use ($channel) {
        $value = coroutine_receive($channel);
        echo $value["message"];
        return $value["message"];
    });
    $sender = coroutine_spawn(function () use ($channel) {
        coroutine_send($channel, ["message" => "ready"]);
        echo "S";
    });

    echo ":" . coroutine_join($receiver);
    coroutine_join($sender);
});
"#)
    .unwrap();

    assert_eq!(output, "Sready:ready");
}

#[test]
fn timer_wait_runs_ready_tasks_before_sleeping_the_executor_thread() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $sleeper = coroutine_spawn(function () {
        echo "A";
        coroutine_sleep(5);
        echo "C";
    });
    $ready = coroutine_spawn(function () {
        echo "B";
    });

    coroutine_join($sleeper);
    coroutine_join($ready);
});
"#)
    .unwrap();

    assert_eq!(output, "ABC");
}

#[test]
fn joining_an_unresolvable_channel_wait_reports_deadlock() {
    let error = run(r#"<?php
coroutine_scope(function () {
    $channel = coroutine_channel(1);
    $receiver = coroutine_spawn(function () use ($channel) {
        return coroutine_receive($channel);
    });
    coroutine_join($receiver);
});
"#)
    .unwrap_err();

    assert!(matches!(
        error,
        execute::VmError::Fatal(message)
            if message == "coroutine deadlock while joining task 1"
    ));
}

#[test]
fn leaving_scope_cancels_an_unjoined_channel_waiter() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $channel = coroutine_channel(1);
    $receiver = coroutine_spawn(function () use ($channel) {
        coroutine_receive($channel);
        echo "unreachable";
    });
    coroutine_resume($receiver);
    echo "root";
});
"#)
    .unwrap();

    assert_eq!(output, "root");
}

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
fn tcp_connect_reports_refusal_and_rejects_dns_names() {
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
    coroutine_tcp_connect("localhost:8080");
});
"#)
    .unwrap_err();
    assert!(matches!(
        address_error,
        execute::VmError::Fatal(message)
            if message.starts_with("coroutine_tcp_connect expects a numeric IP address")
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

#[test]
#[ignore = "run explicitly in release mode as the PHP coroutine API benchmark"]
fn benchmark_one_million_php_suspend_resume_cycles() {
    const ITERATIONS: u64 = 1_000_000;

    let output = run(&format!(
        r#"<?php
coroutine_scope(function () {{
    $task = coroutine_spawn(function () {{
        for ($i = 0; $i < {ITERATIONS}; $i++) {{
            coroutine_suspend();
        }}
        return {ITERATIONS};
    }});

    $started = hrtime(true);
    for ($i = 0; $i < {ITERATIONS}; $i++) {{
        coroutine_resume($task);
    }}
    $result = coroutine_join($task);
    echo (hrtime(true) - $started) . ":" . $result;
}});
"#
    ))
    .unwrap();
    let (elapsed, result) = output.split_once(':').unwrap();
    let elapsed = Duration::from_nanos(elapsed.parse().unwrap());
    let ns_per_cycle = elapsed.as_nanos() as f64 / ITERATIONS as f64;
    eprintln!(
        "PHP coroutine API: {ITERATIONS} suspend/resume cycles in {elapsed:?} ({ns_per_cycle:.2} ns/cycle)"
    );

    assert_eq!(result, ITERATIONS.to_string());
    assert!(ns_per_cycle < 5_000.0);
}

#[test]
#[ignore = "run explicitly in release mode as the bounded-channel benchmark"]
fn benchmark_one_million_bounded_channel_values() {
    const ITERATIONS: u64 = 1_000_000;
    let expected_sum = ITERATIONS * (ITERATIONS - 1) / 2;

    let output = run(&format!(
        r#"<?php
coroutine_scope(function () {{
    $channel = coroutine_channel(1);
    $producer = coroutine_spawn(function () use ($channel) {{
        for ($i = 0; $i < {ITERATIONS}; $i++) {{
            coroutine_send($channel, $i);
        }}
        return {ITERATIONS};
    }});
    $consumer = coroutine_spawn(function () use ($channel) {{
        $sum = 0;
        for ($i = 0; $i < {ITERATIONS}; $i++) {{
            $sum += coroutine_receive($channel);
        }}
        return $sum;
    }});

    $started = hrtime(true);
    $produced = coroutine_join($producer);
    $sum = coroutine_join($consumer);
    echo (hrtime(true) - $started) . ":" . $produced . ":" . $sum;
}});
"#
    ))
    .unwrap();
    let mut parts = output.split(':');
    let elapsed = Duration::from_nanos(parts.next().unwrap().parse().unwrap());
    let produced = parts.next().unwrap();
    let sum = parts.next().unwrap();
    let ns_per_value = elapsed.as_nanos() as f64 / ITERATIONS as f64;
    eprintln!(
        "PHP bounded channel: {ITERATIONS} values in {elapsed:?} ({ns_per_value:.2} ns/value)"
    );

    assert_eq!(produced, ITERATIONS.to_string());
    assert_eq!(sum, expected_sum.to_string());
    assert!(ns_per_value < 20_000.0);
}

#[cfg(unix)]
#[test]
#[ignore = "run explicitly in release mode as the non-blocking I/O benchmark"]
fn benchmark_stream_readiness_ping_pong() {
    const ITERATIONS: u64 = 100_000;

    let output = run(&format!(
        r#"<?php
coroutine_scope(function () {{
    $streams = coroutine_stream_pair();
    $pingStream = $streams[0];
    $pongStream = $streams[1];
    $ping = coroutine_spawn(function () use ($pingStream) {{
        coroutine_stream_write($pingStream, "x");
        for ($i = 0; $i < {ITERATIONS}; $i++) {{
            coroutine_wait_readable($pingStream);
            coroutine_stream_read($pingStream, 1);
            if ($i + 1 < {ITERATIONS}) {{
                coroutine_stream_write($pingStream, "x");
            }}
        }}
        return {ITERATIONS};
    }});
    $pong = coroutine_spawn(function () use ($pongStream) {{
        for ($i = 0; $i < {ITERATIONS}; $i++) {{
            coroutine_wait_readable($pongStream);
            coroutine_stream_read($pongStream, 1);
            coroutine_stream_write($pongStream, "x");
        }}
        return {ITERATIONS};
    }});

    $started = hrtime(true);
    $pingResult = coroutine_join($ping);
    $pongResult = coroutine_join($pong);
    echo (hrtime(true) - $started) . ":" . $pingResult . ":" . $pongResult;
}});
"#
    ))
    .unwrap();
    let mut parts = output.split(':');
    let elapsed = Duration::from_nanos(parts.next().unwrap().parse().unwrap());
    let ping = parts.next().unwrap();
    let pong = parts.next().unwrap();
    let ns_per_round_trip = elapsed.as_nanos() as f64 / ITERATIONS as f64;
    eprintln!(
        "PHP stream readiness: {ITERATIONS} round trips in {elapsed:?} ({ns_per_round_trip:.2} ns/round trip)"
    );

    assert_eq!(ping, ITERATIONS.to_string());
    assert_eq!(pong, ITERATIONS.to_string());
    assert!(ns_per_round_trip < 100_000.0);
}
