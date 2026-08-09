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
