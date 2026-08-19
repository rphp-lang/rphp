mod common;

use common::run_php;

#[test]
fn fiber_lifecycle_exchanges_values_and_injected_exceptions() {
    assert_eq!(
        run_php(
            r#"<?php
$fiber = null;
$fiber = new Fiber(function (int $seed) use (&$fiber): int {
    echo Fiber::getCurrent() === $fiber ? "current\n" : "wrong\n";
    $next = Fiber::suspend($seed + 1);
    try {
        Fiber::suspend($next + 1);
    } catch (RuntimeException $error) {
        echo "caught:", $error->getMessage(), "\n";
        return 19;
    }
});

echo (int) $fiber->isStarted(), (int) $fiber->isRunning(),
     (int) $fiber->isSuspended(), (int) $fiber->isTerminated(), "\n";
var_dump($fiber->start(4));
echo (int) $fiber->isStarted(), (int) $fiber->isRunning(),
     (int) $fiber->isSuspended(), (int) $fiber->isTerminated(), "\n";
var_dump($fiber->resume(8));
var_dump($fiber->throw(new RuntimeException('stop')));
echo (int) $fiber->isStarted(), (int) $fiber->isRunning(),
     (int) $fiber->isSuspended(), (int) $fiber->isTerminated(), "\n";
var_dump($fiber->getReturn());
"#,
        ),
        concat!(
            "0000\n",
            "current\n",
            "int(5)\n",
            "1010\n",
            "int(9)\n",
            "caught:stop\n",
            "NULL\n",
            "1001\n",
            "int(19)\n",
        )
    );
}

#[test]
fn nested_fibers_keep_current_identity_and_running_status() {
    assert_eq!(
        run_php(
            r#"<?php
$outer = null;
$outer = new Fiber(function () use (&$outer): string {
    echo $outer->isRunning() ? "outer-running\n" : "outer-stopped\n";
    $inner = new Fiber(function (): string {
        echo Fiber::getCurrent()->isRunning() ? "inner-running\n" : "inner-stopped\n";
        return Fiber::suspend('inner-ready');
    });
    var_dump($inner->start());
    echo $outer->isRunning() ? "outer-running\n" : "outer-stopped\n";
    var_dump($inner->resume('done'));
    var_dump($inner->getReturn());
    return 'outer-done';
});

var_dump($outer->start());
var_dump($outer->getReturn());
"#,
        ),
        concat!(
            "outer-running\n",
            "inner-running\n",
            "string(11) \"inner-ready\"\n",
            "outer-running\n",
            "NULL\n",
            "string(4) \"done\"\n",
            "NULL\n",
            "string(10) \"outer-done\"\n",
        )
    );
}

#[test]
fn fiber_start_uses_weak_coercion_and_terminates_after_type_error() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
$weak = new Fiber(function (int $number): int { return $number; });
$weak->start('12');
var_dump($weak->getReturn());

$invalid = new Fiber(function (int $number): int { return $number; });
try {
    $invalid->start([]);
} catch (TypeError $error) {
    echo get_class($error), ':', $invalid->isTerminated() ? 'terminated' : 'live', "\n";
}
try {
    $invalid->getReturn();
} catch (FiberError $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "int(12)\n",
            "TypeError:terminated\n",
            "FiberError:Cannot get fiber return value: The fiber threw an exception\n",
        )
    );
}

#[test]
fn generator_crossing_is_rejected_without_retaining_a_popped_frame() {
    assert_eq!(
        run_php(
            r#"<?php
$generator = (function () {
    yield from (function () {
        echo "generator\n";
        Fiber::suspend();
        yield;
    })();
})();
$fiber = new Fiber(function () use ($generator): void {
    $generator->current();
});
try {
    $fiber->start();
} catch (FiberError $error) {
    echo get_class($error), "\n";
}
echo $fiber->isTerminated() ? "terminated\n" : "live\n";
"#,
        ),
        "generator\nFiberError\nterminated\n"
    );
}

#[test]
fn error_reporting_and_silence_frames_are_local_to_each_fiber_context() {
    assert_eq!(
        run_php(
            r#"<?php
function pause_under_silence(): void {
    echo "fiber-suppressed-a:", error_reporting(), "\n";
    Fiber::suspend('one');
    echo "fiber-suppressed-b:", error_reporting(), "\n";
}

error_reporting(11111);
$fiber = new Fiber(function (): void {
    echo "fiber-start:", error_reporting(), "\n";
    @pause_under_silence();
    echo "fiber-after-own-silence:", error_reporting(), "\n";
    Fiber::suspend('two');
    echo "fiber-after-external-silence:", error_reporting(), "\n";
});

echo "main-start:", error_reporting(), "\n";
var_dump($fiber->start());
echo "main-after-start:", error_reporting(), "\n";
error_reporting(22222);
var_dump(@$fiber->resume());
echo "main-after-external-silence:", error_reporting(), "\n";
error_reporting(33333);
var_dump($fiber->resume());
echo "main-end:", error_reporting(), "\n";
"#,
        ),
        concat!(
            "main-start:11111\n",
            "fiber-start:11111\n",
            "fiber-suppressed-a:325\n",
            "string(3) \"one\"\n",
            "main-after-start:11111\n",
            "fiber-suppressed-b:325\n",
            "fiber-after-own-silence:11111\n",
            "string(3) \"two\"\n",
            "main-after-external-silence:22222\n",
            "fiber-after-external-silence:11111\n",
            "NULL\n",
            "main-end:33333\n",
        )
    );
}

#[test]
fn force_close_bypasses_catches_runs_finally_and_rejects_resuspension() {
    assert_eq!(
        run_php(
            r#"<?php
$fiber = new Fiber(function (): void {
    try {
        try {
            echo "body\n";
            Fiber::suspend();
        } catch (Throwable $error) {
            echo "exit-caught\n";
        }
    } finally {
        echo "finally\n";
        try {
            Fiber::suspend();
        } catch (FiberError $error) {
            echo $error->getMessage(), "\n";
        }
    }
    echo "unreached\n";
});

$fiber->start();
unset($fiber);
echo "done\n";
"#,
        ),
        concat!(
            "body\n",
            "finally\n",
            "Cannot suspend in a force-closed fiber\n",
            "done\n",
        )
    );
}

#[test]
fn final_external_fiber_release_cleans_self_owned_stack_objects() {
    assert_eq!(
        run_php(
            r#"<?php
class FiberLocalDestructor {
    public function __destruct() {
        echo "local-destructor\n";
    }
}

$fiber = new Fiber(function (): void {
    $local = new FiberLocalDestructor();
    $self = Fiber::getCurrent();
    Fiber::suspend();
});
$fiber->start();
echo "before-release\n";
$fiber = null;
gc_collect_cycles();
echo "after-release\n";
"#,
        ),
        "before-release\nlocal-destructor\nafter-release\n"
    );
}

#[test]
fn force_close_chains_multiple_finally_exceptions_in_release_order() {
    assert_eq!(
        run_php(
            r#"<?php
$make = function (string $message): Fiber {
    $fiber = new Fiber(function () use ($message): void {
        try {
            Fiber::suspend();
        } finally {
            throw new Exception($message);
        }
    });
    $fiber->start();
    return $fiber;
};
$outer = new Fiber(function () use ($make): void {
    $first = $make('first');
    $second = $make('second');
    Fiber::suspend();
});
$outer->start();
try {
    unset($outer);
} catch (Exception $error) {
    echo $error->getMessage(), '|', $error->getPrevious()?->getMessage(), "\n";
}
"#,
        ),
        "second|first\n"
    );
}

#[test]
fn shutdown_callbacks_retain_arguments_scope_and_fifo_registration_order() {
    assert_eq!(
        run_php(
            r#"<?php
class ShutdownOwner {
    public function arm(): void {
        register_shutdown_function('ShutdownOwner::finish', 'scoped');
    }

    public function finish(string $label): void {
        echo $label, ':', get_class($this), "\n";
        $fiber = new Fiber(function (): int {
            Fiber::suspend(1);
            return 2;
        });
        var_dump($fiber->start());
        $fiber->resume();
        var_dump($fiber->getReturn());
        register_shutdown_function(function (): void {
            echo "late\n";
        });
    }
}

(new ShutdownOwner())->arm();
register_shutdown_function(function (string $label): void {
    echo $label, "\n";
}, 'second');
echo "body\n";
"#,
        ),
        concat!(
            "body\n",
            "scoped:ShutdownOwner\n",
            "int(1)\n",
            "int(2)\n",
            "second\n",
            "late\n",
        )
    );
}

#[test]
fn shutdown_exceptions_use_the_active_exception_handler_and_continue() {
    assert_eq!(
        run_php(
            r#"<?php
set_exception_handler(function (Throwable $error): void {
    echo 'caught:', $error->getMessage(), "\n";
});
register_shutdown_function(function (): void {
    echo "throwing\n";
    throw new Exception('shutdown');
});
register_shutdown_function(function (): void {
    echo "after\n";
});
"#,
        ),
        "throwing\ncaught:shutdown\nafter\n"
    );
}

#[test]
fn fatal_fiber_shutdown_observes_the_bailout_return_state() {
    let source = r#"register_shutdown_function(function () use (&$fiber): void {
    try {
        $fiber->getReturn();
    } catch (FiberError $error) {
        echo 'shutdown:', $error->getMessage(), "\n";
    }
});
$fiber = new Fiber(function (): void {
    trigger_error('boom', E_USER_ERROR);
});
$fiber->start();"#;
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-r", source])
        .output()
        .expect("RPHP CLI must execute the fatal request specimen");

    assert_eq!(output.status.code(), Some(255));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "\nDeprecated: Passing E_USER_ERROR to trigger_error() is deprecated since 8.4, ",
            "throw an exception or call exit with a string message instead in Command line code on line 9\n",
            "shutdown:Cannot get fiber return value: The fiber exited with a fatal error\n",
        )
    );
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "\nFatal error: boom in Command line code on line 9\n"
    );
}
