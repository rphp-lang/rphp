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
