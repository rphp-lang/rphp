mod common;

use common::run_php;

#[test]
fn gc_internal_frames_are_skipped_when_nested_callbacks_publish_globals() {
    assert_eq!(
        run_php(
            r#"<?php
class InternalGcDispatch {
    public $cycle;
    public function __construct() { $this->cycle = $this; }
    public function __destruct() {
        global $events;
        $events[] = 'drop';
        (new Fiber(function () {
            global $events;
            $events[] = 'child';
        }))->start();
    }
}
$events = [];
new InternalGcDispatch;
new InternalGcDispatch;
gc_collect_cycles();
echo implode(',', $events);
"#,
        ),
        "drop,child,drop,child"
    );
}

#[cfg(feature = "stream-registry")]
#[test]
fn closing_a_native_owner_retires_the_wrapper_without_a_temporary_php_root() {
    assert_eq!(
        run_php(
            r#"<?php
class ClosingNativeOwner {
    public $context;
    public function stream_open($path, $mode, $options, &$opened): bool { return true; }
    public function stream_close(): void { echo 'close|'; }
    public function __destruct() { echo 'destroy|'; }
}
stream_wrapper_register('closeprobe', ClosingNativeOwner::class);
$handle = fopen('closeprobe://item', 'r');
echo 'opened|';
fclose($handle);
echo 'finished';
"#,
        ),
        "opened|close|destroy|finished"
    );
}

#[test]
fn abandoned_gc_worker_runs_finally_and_rejects_another_suspension() {
    assert_eq!(
        run_php(
            r#"<?php
class AbandonedCollectedCycle {
    public $self;
    public function __construct() { $this->self = $this; }
    public function __destruct() {
        try { echo 'park|'; Fiber::suspend(); }
        finally { echo 'finally|'; Fiber::suspend(); }
    }
}
$owner = new Fiber(function () {
    new AbandonedCollectedCycle;
    gc_collect_cycles();
    echo 'owner-continues|';
});
try { $owner->start(); }
catch (FiberError $error) {
    foreach ($error->getTrace() as $frame) { echo $frame['function'], '|'; }
    echo $error->getMessage(), '|';
}
gc_collect_cycles();
echo (int) $owner->isTerminated(), '|done';
"#,
        ),
        concat!(
            "park|finally|",
            "suspend|__destruct|gc_destructor_fiber|",
            "Cannot suspend in a force-closed fiber|1|done",
        )
    );
}

#[test]
fn resurrected_gc_receiver_remains_live_until_the_resumed_destructor_releases_it() {
    assert_eq!(
        run_php(
            r#"<?php
class SavedCollectedCycle {
    public $self;
    public function __construct() { $this->self = $this; }
    public function __destruct() {
        global $saved, $worker;
        $saved = $this;
        $worker = Fiber::getCurrent();
        Fiber::suspend();
        $saved = null;
        echo 'released|';
    }
}
$owner = new Fiber(function () {
    global $weak;
    $weak = WeakReference::create(new SavedCollectedCycle);
    gc_collect_cycles();
});
$owner->start();
gc_collect_cycles();
echo (int) ($weak->get() === $saved), '|';
$worker->resume();
gc_collect_cycles();
echo (int) ($weak->get() === null), (int) ($saved === null);
"#,
        ),
        "1|released|11"
    );
}

#[test]
fn append_results_do_not_retain_their_already_consumed_call_arguments() {
    assert_eq!(
        run_php(
            r#"<?php
class AppendOperand {
    public function __construct(public string $label) {}
    public function __destruct() { echo 'drop:', $this->label, '|'; }
}
$items = [];
$items[] = WeakReference::create(new AppendOperand('local'));
echo (int) ($items[0]->get() === null), '|';
$holder = new stdClass;
$holder->items = [];
$holder->items[] = WeakReference::create(new AppendOperand('property'));
echo (int) ($holder->items[0]->get() === null), '|';
$nested = [[]];
$nested[0][] = WeakReference::create(new AppendOperand('nested'));
echo (int) ($nested[0][0]->get() === null);
"#,
        ),
        "drop:local|1|drop:property|1|drop:nested|1"
    );
}

#[test]
fn gc_continues_after_destructor_exceptions_and_chains_them_before_resume() {
    assert_eq!(
        run_php(
            r#"<?php
class FailingCollectedCycle {
    public $self;
    public static $calls = 0;
    public function __construct() { $this->self = $this; }
    public function __destruct() {
        $call = ++self::$calls;
        echo 'enter', $call, '|';
        if ($call === 1) {
            global $parked;
            $parked = Fiber::getCurrent();
            Fiber::suspend();
        }
        throw new Exception('failure' . $call);
    }
}
$owner = new Fiber(function () {
    new FailingCollectedCycle;
    new FailingCollectedCycle;
    new FailingCollectedCycle;
    try { gc_collect_cycles(); }
    catch (Exception $error) {
        do { echo $error->getMessage(), '|'; }
        while ($error = $error->getPrevious());
    }
    echo 'collector-done|';
});
$owner->start();
try { $parked->resume(); }
catch (Exception $error) { echo $error->getMessage(), '|'; }
echo (int) $owner->isTerminated(), (int) $parked->isTerminated();
"#,
        ),
        "enter1|enter2|enter3|failure3|failure2|collector-done|failure1|11"
    );
}

#[test]
fn consumed_comparison_operands_retire_before_the_result_is_observed() {
    assert_eq!(
        run_php(
            r#"<?php
class ComparisonLifetime {
    public function __construct(public $name) {}
    public function __destruct() { echo 'drop:', $this->name, '|'; }
}
function comparisonOperand($name) {
    echo 'make:', $name, '|';
    return new ComparisonLifetime($name);
}
echo (int) (comparisonOperand('left') !== comparisonOperand('right')), '|after|';
class ThrowingComparisonLifetime {
    public function __destruct() { throw new Exception('retire'); }
}
function throwingOperand() { return new ThrowingComparisonLifetime; }
try { echo (int) (throwingOperand() === null), '|unreachable|'; }
catch (Exception $error) { echo $error->getMessage(), '|'; }
echo 'done';
"#,
        ),
        "make:left|make:right|drop:left|drop:right|1|after|retire|done"
    );
}

#[test]
fn intermediate_global_bindings_preserve_new_names_for_the_main_caller() {
    assert_eq!(
        run_php(
            r#"<?php
function publishLateGlobal(): void { global $late; $late = 'ordinary'; }
function partialGlobalCaller(): void { global $anchor; publishLateGlobal(); }
$anchor = 'kept';
partialGlobalCaller();
echo $late, '|', $anchor, '|';
unset($late);
$outer = new Fiber(function () {
    global $anchor;
    $inner = new Fiber(function () {
        global $late;
        $late = 'suspended';
        Fiber::suspend();
    });
    $inner->start();
});
$outer->start();
echo $late, '|', $anchor;
"#,
        ),
        "ordinary|kept|suspended|kept"
    );
}

#[test]
fn gc_destructor_suspends_its_own_context_and_retains_the_receiver_until_resume() {
    assert_eq!(
        run_php(
            r#"<?php
class PausedCycleDestructor {
    public $cycle;
    public function __construct() { $this->cycle = $this; }
    public function __destruct() {
        global $worker;
        foreach (debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 3) as $frame) {
            echo $frame['function'], '|';
        }
        $worker = Fiber::getCurrent();
        echo 'open|';
        echo Fiber::suspend('parked'), '|close|';
    }
}
$owner = new Fiber(function () {
    global $weak;
    $weak = WeakReference::create(new PausedCycleDestructor);
    gc_collect_cycles();
    echo 'collector-returned|';
});
$owner->start();
echo (int) $owner->isTerminated(), (int) $worker->isSuspended(), (int) ($weak->get() !== null), '|';
$worker->resume('resumed');
gc_collect_cycles();
echo (int) $worker->isTerminated(), (int) ($weak->get() === null);
"#,
        ),
        concat!(
            "__destruct|gc_destructor_fiber|gc_collect_cycles|open|",
            "collector-returned|111|resumed|close|11",
        )
    );
}

#[test]
#[cfg(unix)]
fn rejected_small_stack_leaves_the_fiber_unstarted_and_reusable() {
    assert_eq!(
        run_php(
            r#"<?php
$fiber = new Fiber(function () { echo 'body|'; });
foreach (['', '0', '1'] as $size) {
    ini_set('fiber.stack_size', $size);
    try { $fiber->start(); }
    catch (Throwable $error) {
        echo get_class($error), ':', explode(', ', $error->getMessage())[0], '|';
    }
    echo (int) $fiber->isStarted(), (int) $fiber->isTerminated(), "\n";
}
ini_set('fiber.stack_size', '2097152');
$fiber->start();
echo (int) $fiber->isTerminated();
"#,
        ),
        concat!(
            "Exception:Fiber stack size is too small|00\n",
            "Exception:Fiber stack size is too small|00\n",
            "Exception:Fiber stack size is too small|00\n",
            "body|1",
        )
    );
}

#[test]
fn tick_switch_guard_precedes_state_errors_without_consuming_suspension() {
    assert_eq!(
        run_php(
            r#"<?php
declare(ticks=1);
$armed = null;
$created = new Fiber(function () { echo 'new-body|'; });
$ended = new Fiber(function () {});
$ended->start();
$suspended = new Fiber(function () {
    try { Fiber::suspend(); }
    catch (Exception $error) { echo 'injected:', $error->getMessage(), '|'; }
});
$suspended->start();
$tick = function () use (&$armed) {
    if ($armed === null) return;
    $operation = $armed;
    $armed = null;
    try { $operation(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
};
register_tick_function($tick);
$armed = function () use ($created) { $created->start(); };
$armed = function () use ($ended) { $ended->start(); };
$armed = function () use ($created) { $created->resume(); };
$armed = function () use ($suspended) { $suspended->resume(); };
$armed = function () use ($suspended) { $suspended->throw(new Exception('blocked')); };
$armed = function () use ($suspended) { $suspended->throw('invalid'); };
$armed = function () { Fiber::suspend(); };
$worker = new Fiber(function () use (&$armed) {
    $armed = function () { Fiber::suspend('blocked'); };
    echo 'worker-continues|';
});
$worker->start();
unregister_tick_function($tick);
echo (int) $created->isStarted(), (int) $suspended->isSuspended(), '|';
$created->start();
$suspended->throw(new Exception('allowed'));
"#,
        ),
        concat!(
            "FiberError:Cannot switch fibers in current execution context\n",
            "FiberError:Cannot switch fibers in current execution context\n",
            "FiberError:Cannot switch fibers in current execution context\n",
            "FiberError:Cannot switch fibers in current execution context\n",
            "FiberError:Cannot switch fibers in current execution context\n",
            "TypeError:Fiber::throw(): Argument #1 ($exception) must be of type Throwable, string given\n",
            "FiberError:Cannot suspend outside of a fiber\n",
            "FiberError:Cannot switch fibers in current execution context\n",
            "worker-continues|01|new-body|injected:allowed|",
        )
    );
}
