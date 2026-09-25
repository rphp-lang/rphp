mod common;

use common::run_php;

#[test]
fn iterator_resume_rewind() {
    assert_eq!(
        run_php(
            r####"<?php

class ResumingIterator implements Iterator {
    private int $position = 0;
    private bool $paused = false;
    private function visit($phase) {
        echo $phase, '|';
        if (!$this->paused && $phase === 'rewind') {
            $this->paused = true;
            try { echo 'input:', Fiber::suspend($phase), '|'; }
            finally { echo 'callback-finally|'; }
        }
    }
    function rewind(): void { $this->visit('rewind'); }
    function valid(): bool { $this->visit('valid'); return $this->position < 2; }
    function current(): mixed { $this->visit('current'); return 20 + $this->position; }
    function key(): mixed { $this->visit('key'); return 'key' . $this->position; }
    function next(): void { $this->visit('next'); ++$this->position; }
}
function delegatedItems($iterator) {
    try { yield from $iterator; echo 'generator-end|'; }
    finally { echo 'generator-finally|'; }
}
$iterator = new ResumingIterator;
$generator = delegatedItems($iterator);
$fiber = new Fiber(function () use ($generator) {
    foreach ($generator as $key => $value) echo $key, ':', $value, '|';
    echo 'fiber-end|';
});
echo 'start:', $fiber->start(), '|';
echo 'paused:', (int) $fiber->isSuspended(), '|';
$fiber->resume('resume-value');
echo 'terminated:', (int) $fiber->isTerminated(), '|';
"####
        ),
        r####"start:rewind|input:rewind|paused:1|resume-value|callback-finally|valid|current|key|key0:20|next|valid|current|key|key1:21|next|valid|generator-end|generator-finally|fiber-end|terminated:1|"####,
    );
}

#[test]
fn iterator_resume_valid() {
    assert_eq!(
        run_php(
            r####"<?php

class ResumingIterator implements Iterator {
    private int $position = 0;
    private bool $paused = false;
    private function visit($phase) {
        echo $phase, '|';
        if (!$this->paused && $phase === 'valid') {
            $this->paused = true;
            try { echo 'input:', Fiber::suspend($phase), '|'; }
            finally { echo 'callback-finally|'; }
        }
    }
    function rewind(): void { $this->visit('rewind'); }
    function valid(): bool { $this->visit('valid'); return $this->position < 2; }
    function current(): mixed { $this->visit('current'); return 20 + $this->position; }
    function key(): mixed { $this->visit('key'); return 'key' . $this->position; }
    function next(): void { $this->visit('next'); ++$this->position; }
}
function delegatedItems($iterator) {
    try { yield from $iterator; echo 'generator-end|'; }
    finally { echo 'generator-finally|'; }
}
$iterator = new ResumingIterator;
$generator = delegatedItems($iterator);
$fiber = new Fiber(function () use ($generator) {
    foreach ($generator as $key => $value) echo $key, ':', $value, '|';
    echo 'fiber-end|';
});
echo 'start:', $fiber->start(), '|';
echo 'paused:', (int) $fiber->isSuspended(), '|';
$fiber->resume('resume-value');
echo 'terminated:', (int) $fiber->isTerminated(), '|';
"####
        ),
        r####"start:rewind|valid|input:valid|paused:1|resume-value|callback-finally|current|key|key0:20|next|valid|current|key|key1:21|next|valid|generator-end|generator-finally|fiber-end|terminated:1|"####,
    );
}

#[test]
fn iterator_resume_current() {
    assert_eq!(
        run_php(
            r####"<?php

class ResumingIterator implements Iterator {
    private int $position = 0;
    private bool $paused = false;
    private function visit($phase) {
        echo $phase, '|';
        if (!$this->paused && $phase === 'current') {
            $this->paused = true;
            try { echo 'input:', Fiber::suspend($phase), '|'; }
            finally { echo 'callback-finally|'; }
        }
    }
    function rewind(): void { $this->visit('rewind'); }
    function valid(): bool { $this->visit('valid'); return $this->position < 2; }
    function current(): mixed { $this->visit('current'); return 20 + $this->position; }
    function key(): mixed { $this->visit('key'); return 'key' . $this->position; }
    function next(): void { $this->visit('next'); ++$this->position; }
}
function delegatedItems($iterator) {
    try { yield from $iterator; echo 'generator-end|'; }
    finally { echo 'generator-finally|'; }
}
$iterator = new ResumingIterator;
$generator = delegatedItems($iterator);
$fiber = new Fiber(function () use ($generator) {
    foreach ($generator as $key => $value) echo $key, ':', $value, '|';
    echo 'fiber-end|';
});
echo 'start:', $fiber->start(), '|';
echo 'paused:', (int) $fiber->isSuspended(), '|';
$fiber->resume('resume-value');
echo 'terminated:', (int) $fiber->isTerminated(), '|';
"####
        ),
        r####"start:rewind|valid|current|input:current|paused:1|resume-value|callback-finally|key|key0:20|next|valid|current|key|key1:21|next|valid|generator-end|generator-finally|fiber-end|terminated:1|"####,
    );
}

#[test]
fn iterator_resume_key() {
    assert_eq!(
        run_php(
            r####"<?php

class ResumingIterator implements Iterator {
    private int $position = 0;
    private bool $paused = false;
    private function visit($phase) {
        echo $phase, '|';
        if (!$this->paused && $phase === 'key') {
            $this->paused = true;
            try { echo 'input:', Fiber::suspend($phase), '|'; }
            finally { echo 'callback-finally|'; }
        }
    }
    function rewind(): void { $this->visit('rewind'); }
    function valid(): bool { $this->visit('valid'); return $this->position < 2; }
    function current(): mixed { $this->visit('current'); return 20 + $this->position; }
    function key(): mixed { $this->visit('key'); return 'key' . $this->position; }
    function next(): void { $this->visit('next'); ++$this->position; }
}
function delegatedItems($iterator) {
    try { yield from $iterator; echo 'generator-end|'; }
    finally { echo 'generator-finally|'; }
}
$iterator = new ResumingIterator;
$generator = delegatedItems($iterator);
$fiber = new Fiber(function () use ($generator) {
    foreach ($generator as $key => $value) echo $key, ':', $value, '|';
    echo 'fiber-end|';
});
echo 'start:', $fiber->start(), '|';
echo 'paused:', (int) $fiber->isSuspended(), '|';
$fiber->resume('resume-value');
echo 'terminated:', (int) $fiber->isTerminated(), '|';
"####
        ),
        r####"start:rewind|valid|current|key|input:key|paused:1|resume-value|callback-finally|key0:20|next|valid|current|key|key1:21|next|valid|generator-end|generator-finally|fiber-end|terminated:1|"####,
    );
}

#[test]
fn iterator_resume_next() {
    assert_eq!(
        run_php(
            r####"<?php

class ResumingIterator implements Iterator {
    private int $position = 0;
    private bool $paused = false;
    private function visit($phase) {
        echo $phase, '|';
        if (!$this->paused && $phase === 'next') {
            $this->paused = true;
            try { echo 'input:', Fiber::suspend($phase), '|'; }
            finally { echo 'callback-finally|'; }
        }
    }
    function rewind(): void { $this->visit('rewind'); }
    function valid(): bool { $this->visit('valid'); return $this->position < 2; }
    function current(): mixed { $this->visit('current'); return 20 + $this->position; }
    function key(): mixed { $this->visit('key'); return 'key' . $this->position; }
    function next(): void { $this->visit('next'); ++$this->position; }
}
function delegatedItems($iterator) {
    try { yield from $iterator; echo 'generator-end|'; }
    finally { echo 'generator-finally|'; }
}
$iterator = new ResumingIterator;
$generator = delegatedItems($iterator);
$fiber = new Fiber(function () use ($generator) {
    foreach ($generator as $key => $value) echo $key, ':', $value, '|';
    echo 'fiber-end|';
});
echo 'start:', $fiber->start(), '|';
echo 'paused:', (int) $fiber->isSuspended(), '|';
$fiber->resume('resume-value');
echo 'terminated:', (int) $fiber->isTerminated(), '|';
"####
        ),
        r####"start:rewind|valid|current|key|key0:20|next|input:next|paused:1|resume-value|callback-finally|valid|current|key|key1:21|next|valid|generator-end|generator-finally|fiber-end|terminated:1|"####,
    );
}

#[test]
fn iterator_throw_and_close() {
    assert_eq!(
        run_php(
            r####"<?php

class ClosingIterator implements Iterator {
    function rewind(): void {
        try { echo 'rewind|'; Fiber::suspend('ready'); echo 'resumed|'; }
        finally { echo 'iterator-finally|'; }
    }
    function valid(): bool { return false; }
    function current(): mixed { return null; }
    function key(): mixed { return null; }
    function next(): void {}
}
function closingDelegate($iterator) {
    try { yield from $iterator; } finally { echo 'generator-finally|'; }
}
$generator = closingDelegate(new ClosingIterator);
$fiber = new Fiber(function () use ($generator) {
    try { $generator->current(); } catch (RuntimeException $error) { echo 'caught:', $error->getMessage(), '|'; }
    finally { echo 'fiber-finally|'; }
});
echo $fiber->start(), '|'; $fiber->throw(new RuntimeException('injected'));
echo (int) $fiber->isTerminated(), '|';
"####
        ),
        r####"rewind|ready|iterator-finally|generator-finally|caught:injected|fiber-finally|1|"####,
    );
}

#[test]
fn ordinary_destructor_suspension() {
    assert_eq!(
        run_php(
            r####"<?php

class PausingDestructor {
    function __destruct() {
        try { echo 'drop-start|'; echo 'drop-value:', Fiber::suspend('drop-ready'), '|'; }
        finally { echo 'drop-finally|'; }
    }
}
$fiber = new Fiber(function () { $item = new PausingDestructor; unset($item); echo 'after-unset|'; });
echo $fiber->start(), '|'; $fiber->resume('continued');
echo (int) $fiber->isTerminated(), '|';
"####
        ),
        r####"drop-start|drop-value:drop-ready|continued|drop-finally|after-unset|1|"####,
    );
}

#[test]
fn destructor_same_fiber_and_nested_resume() {
    assert_eq!(
        run_php(
            r####"<?php

class OwnedDrop {
    public static $expected;
    function __destruct() {
        echo 'same:', (int) (Fiber::getCurrent() === self::$expected), '|';
        try {
            echo 'first:', Fiber::suspend('one'), '|';
            $child = new Fiber(function () { echo 'child|'; return 9; });
            $child->start(); echo 'child-result:', $child->getReturn(), '|';
            echo 'second:', Fiber::suspend('two'), '|';
        } finally { echo 'drop-finally|'; }
    }
}
$driver = new Fiber(function () {
    OwnedDrop::$expected = Fiber::getCurrent();
    $owner = new OwnedDrop;
    unset($owner);
    echo 'after-drop|';
    return 'complete';
});
echo $driver->start(), '|';
echo $driver->resume('alpha'), '|';
$driver->resume('beta'); echo $driver->getReturn(), '|';
OwnedDrop::$expected = null;
"####
        ),
        r####"same:1|first:one|alpha|child|child-result:9|second:two|beta|drop-finally|after-drop|complete|"####,
    );
}

#[test]
fn destructor_injected_exception() {
    assert_eq!(
        run_php(
            r####"<?php

class FailingDrop {
    function __destruct() {
        try { Fiber::suspend('drop'); echo 'unreachable|'; }
        finally { echo 'drop-finally|'; }
    }
}
$fiber = new Fiber(function () {
    try { $owner = new FailingDrop; unset($owner); }
    catch (RuntimeException $error) { echo 'caught:', $error->getMessage(), '|'; }
    finally { echo 'fiber-finally|'; }
    return 31;
});
echo $fiber->start(), '|';
$fiber->throw(new RuntimeException('injected'));
echo $fiber->getReturn(), '|';
"####
        ),
        r####"drop|drop-finally|caught:injected|fiber-finally|31|"####,
    );
}

#[test]
fn destructor_force_close() {
    assert_eq!(
        run_php(
            r####"<?php

class ClosingDrop {
    function __destruct() {
        try { Fiber::suspend('pending'); echo 'unreachable|'; }
        finally { echo 'drop-finally|'; }
    }
}
$fiber = new Fiber(function () {
    try { $owner = new ClosingDrop; unset($owner); }
    finally { echo 'fiber-finally|'; }
});
echo $fiber->start(), '|';
unset($fiber); echo 'closed|';
"####
        ),
        r####"pending|drop-finally|fiber-finally|closed|"####,
    );
}

#[test]
fn destructor_receiver_resurrection() {
    assert_eq!(
        run_php(
            r####"<?php

class ResurrectingDrop {
    public $payload = 'retained';
    function __destruct() {
        $GLOBALS['saved'] = $this;
        echo 'drop:', Fiber::suspend('ready'), '|';
        echo $this->payload, '|';
    }
}
$fiber = new Fiber(function () {
    $owner = new ResurrectingDrop;
    $GLOBALS['weak'] = WeakReference::create($owner);
    unset($owner); echo 'after-drop|';
});
echo $fiber->start(), '|';
echo 'alive:', (int) ($weak->get() === $saved), '|';
$fiber->resume('continue');
echo $saved->payload, '|'; unset($saved);
echo 'released:', (int) ($weak->get() === null), '|';
"####
        ),
        r####"drop:ready|alive:1|continue|retained|after-drop|retained|released:1|"####,
    );
}

#[test]
fn destructor_no_suspend_control() {
    assert_eq!(
        run_php(
            r####"<?php

class SynchronousDrop {
    function __destruct() {
        echo 'drop|';
        $child = new Fiber(function () { echo 'child|'; });
        $child->start();
    }
}
$fiber = new Fiber(function () {
    $owner = new SynchronousDrop;
    unset($owner); echo 'after|'; return 12;
});
$fiber->start(); echo $fiber->getReturn(), '|';
"####
        ),
        r####"drop|child|after|12|"####,
    );
}

#[test]
fn iterator_force_close_retained_0() {
    assert_eq!(
        run_php(
            r####"<?php

class ParkedIterator implements Iterator {
    function rewind(): void {
        try { echo 'rewind|'; Fiber::suspend('parked'); echo 'unreachable|'; }
        finally { echo 'callback-finally|'; }
    }
    function valid(): bool { return false; }
    function current(): mixed { return 40; }
    function key(): mixed { return 'kept'; }
    function next(): void {}
}
function parkedSource() {
    try { yield from new ParkedIterator; }
    finally { echo 'generator-finally|'; }
}
$source = parkedSource();
$fiber = new Fiber(function () use ($source) {
    try { $source->current(); }
    finally { echo 'fiber-finally|'; }
});
unset($source);
echo $fiber->start(), '|';
unset($fiber); echo 'fiber-gone|';

"####
        ),
        r####"rewind|parked|callback-finally|generator-finally|fiber-finally|fiber-gone|"####,
    );
}

#[test]
fn iterator_force_close_retained_1() {
    assert_eq!(
        run_php(
            r####"<?php

class ParkedIterator implements Iterator {
    function rewind(): void {
        try { echo 'rewind|'; Fiber::suspend('parked'); echo 'unreachable|'; }
        finally { echo 'callback-finally|'; }
    }
    function valid(): bool { return false; }
    function current(): mixed { return 40; }
    function key(): mixed { return 'kept'; }
    function next(): void {}
}
function parkedSource() {
    try { yield from new ParkedIterator; }
    finally { echo 'generator-finally|'; }
}
$source = parkedSource();
$fiber = new Fiber(function () use ($source) {
    try { $source->current(); }
    finally { echo 'fiber-finally|'; }
});

echo $fiber->start(), '|';
unset($fiber); echo 'fiber-gone|';
unset($source); echo "source-gone|";
"####
        ),
        r####"rewind|parked|callback-finally|generator-finally|fiber-finally|fiber-gone|source-gone|"####,
    );
}

#[test]
fn gc_reentrant_admission() {
    assert_eq!(
        run_php(
            r####"<?php

class AdmissionAnchor { public $self; function __construct() { $this->self = $this; } }
class AdmissionDestructor {
    public $self;
    function __construct() { $this->self = $this; }
    function __destruct() {
        echo 'destructor|';
        $GLOBALS['resurrected'] = $GLOBALS['anchor'];
        unset($GLOBALS['anchor']);
        echo 'recursive:', gc_collect_cycles(), '|';
    }
}
$anchor = new AdmissionAnchor;
for ($index = 0; $index < 9998; ++$index) { $cycle = []; $cycle[] =& $cycle; unset($cycle); }
$cycle = new AdmissionDestructor; unset($cycle);
$trigger = []; $trigger[] =& $trigger; unset($trigger);
echo 'after|'; unset($resurrected, $anchor); echo 'collected:', gc_collect_cycles(), '|';
"####
        ),
        r####"destructor|recursive:0|after|collected:2|"####,
    );
}
