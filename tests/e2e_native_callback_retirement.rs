mod common;

use common::run_php;

// Trace origins and request shutdown belong to the CLI envelope, not the
// embedding helper, which intentionally leaves source/shutdown policy to its
// caller. Keep these checks byte-exact against the same PHP -r envelope.
fn run_php_cli(source: &str) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-d",
            "display_errors=1",
            "-d",
            "log_errors=0",
            "-d",
            "zend.exception_ignore_args=1",
            "-r",
            source.strip_prefix("<?php\n").expect("PHP source"),
        ])
        .output()
        .expect("run CLI");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 diagnostics")
}

#[test]
fn native_reporting_scope() {
    assert_eq!(
        run_php(
            r####"<?php

class ReportingIterator implements Iterator {
 function rewind(): void { error_reporting(0); Fiber::suspend('ready'); echo 'rewind:', error_reporting(), '|'; }
 function valid(): bool { echo 'valid:', error_reporting(), '|'; return false; }
 function current(): mixed { return null; } function key(): mixed { return null; } function next(): void {}
}
function reportingSource() { yield from new ReportingIterator; echo 'source:', error_reporting(), '|'; }
$f = new Fiber(function () { error_reporting(32767); $g = reportingSource(); $g->current(); echo 'fiber:', error_reporting(), '|'; });
echo $f->start(), '|'; $f->resume();
"####
        ),
        r####"ready|rewind:0|valid:0|source:0|fiber:0|"####
    );
}

#[test]
fn native_suppressed_call() {
    assert_eq!(
        run_php(
            r####"<?php

class SuppressedIterator implements Iterator {
 function rewind(): void { echo 'before:', error_reporting(), '|'; Fiber::suspend('ready'); echo 'after:', error_reporting(), '|'; error_reporting(7); }
 function valid(): bool { echo 'valid:', error_reporting(), '|'; return false; }
 function current(): mixed { return null; } function key(): mixed { return null; } function next(): void {}
}
function suppressedSource() { yield from new SuppressedIterator; }
$f = new Fiber(function () { error_reporting(32767); $g = suppressedSource(); @$g->current(); echo 'fiber:', error_reporting(), '|'; });
echo $f->start(), '|'; $f->resume();
"####
        ),
        r####"before:4437|ready|after:4437|valid:7|fiber:7|"####
    );
}

#[test]
fn native_reporting_destructor() {
    assert_eq!(
        run_php(
            r####"<?php

class ReportingDrop { function __destruct() { error_reporting(15); Fiber::suspend('ready'); echo 'drop:', error_reporting(), '|'; error_reporting(7); } }
$f = new Fiber(function () { error_reporting(32767); $x = new ReportingDrop; unset($x); echo 'fiber:', error_reporting(), '|'; });
echo $f->start(), '|'; $f->resume();
"####
        ),
        r####"ready|drop:15|fiber:7|"####
    );
}

#[test]
fn destructor_child_order() {
    assert_eq!(
        run_php(
            r####"<?php

class ParkedChild {
  function __construct(public $name) {}
  function __destruct() { try { echo $this->name, ':', Fiber::suspend($this->name), '|'; } finally { echo 'finally-', $this->name, '|'; } }
}
class ParkedRoot {
  public $left; public $right;
  function __construct() { $this->left = new ParkedChild('left'); $this->right = new ParkedChild('right'); }
  function __destruct() { echo 'root:', Fiber::suspend('root'), '|'; }
}
$fiber = new Fiber(function () { $root = new ParkedRoot; unset($root); echo 'after|'; });
echo $fiber->start(), '|'; echo $fiber->resume('R'), '|'; echo $fiber->resume('L'), '|'; $fiber->resume('X');
echo 'done:', (int) $fiber->isTerminated(), '|';
"####
        ),
        r####"root:root|R|left:left|L|finally-left|right:right|X|finally-right|after|done:1|"####
    );
}

#[test]
fn child_only_destructor() {
    assert_eq!(
        run_php(
            r####"<?php

class PausedLeaf { function __destruct() { echo 'leaf:', Fiber::suspend('leaf'), '|'; } }
$fiber = new Fiber(function () { $root = new stdClass; $root->child = new PausedLeaf; unset($root); echo 'after|'; });
echo $fiber->start(), '|'; $fiber->resume('ok'); echo 'done|';
"####
        ),
        r####"leaf:leaf|ok|after|done|"####
    );
}

#[test]
fn destructor_mutates_committed_slot() {
    assert_eq!(
        run_php(
            r####"<?php

class UpdatingDrop {
  function __construct(public $slot) {}
  function __destruct() { echo 'drop:', Fiber::suspend('paused'), '|'; $this->slot->value = 'changed'; }
}
$slot = new stdClass; $slot->value = new UpdatingDrop($slot);
$fiber = new Fiber(function () use ($slot) { $x = $slot->value; unset($slot->value); $x = 'replacement'; echo $x, ':', $slot->value, '|'; });
echo $fiber->start(), '|'; $fiber->resume('ok');
"####
        ),
        r####"drop:paused|ok|replacement:changed|"####
    );
}

#[test]
fn destructor_creates_child() {
    assert_eq!(
        run_php(
            r####"<?php

class NewChild { function __destruct() { echo 'child:', Fiber::suspend('child'), '|'; } }
class NewRoot {
  public $child;
  function __destruct() { Fiber::suspend('root'); $this->child = new NewChild; echo 'root-done|'; }
}
$fiber = new Fiber(function () { $x = new NewRoot; unset($x); echo 'after|'; });
echo $fiber->start(), '|'; echo $fiber->resume(), '|'; $fiber->resume('ok');
"####
        ),
        r####"root|root-done|child:child|ok|after|"####
    );
}

#[test]
fn destructor_nested_callback() {
    assert_eq!(
        run_php(
            r####"<?php

class InnerDrop { function __destruct() { echo 'inner:', Fiber::suspend('inner'), '|'; } }
class OuterDrop {
  function __destruct() { $inner = new InnerDrop; unset($inner); echo 'outer:', Fiber::suspend('outer'), '|'; }
}
$fiber = new Fiber(function () { $x = new OuterDrop; unset($x); echo 'after|'; });
echo $fiber->start(), '|'; echo $fiber->resume('I'), '|'; $fiber->resume('O');
"####
        ),
        r####"inner:inner|I|outer:outer|O|after|"####
    );
}

#[test]
fn destructor_child_exception() {
    assert_eq!(
        run_php(
            r####"<?php

class ThrowingLeaf { function __destruct() { try { Fiber::suspend('child'); } finally { echo 'child-finally|'; } } }
class ChildRoot { public $child; function __construct() { $this->child = new ThrowingLeaf; } function __destruct() { echo 'root|'; } }
$fiber = new Fiber(function () { try { $x = new ChildRoot; unset($x); } catch (RuntimeException $error) { echo $error->getMessage(), '|'; } finally { echo 'fiber-finally|'; } });
echo $fiber->start(), '|'; $fiber->throw(new RuntimeException('injected'));
"####
        ),
        r####"root|child|child-finally|injected|fiber-finally|"####
    );
}

#[test]
fn destructor_weakmap_retirement() {
    assert_eq!(
        run_php(
            r####"<?php

class WeakPayload { function __destruct() { echo 'weak-payload:', Fiber::suspend('weak'), '|'; } }
class WeakKey { function __destruct() { echo 'key:', Fiber::suspend('key'), '|'; } }
$map = new WeakMap;
$fiber = new Fiber(function () use ($map) { $key = new WeakKey; $map[$key] = new WeakPayload; unset($key); echo 'after:', count($map), '|'; });
echo $fiber->start(), '|'; echo 'map:', count($map), '|'; echo $fiber->resume('K'), '|'; $fiber->resume('W');
"####
        ),
        r####"key:key|map:1|K|weak-payload:weak|W|after:0|"####
    );
}

#[test]
fn destructor_same_fiber_local_force_close() {
    assert_eq!(
        run_php(
            r####"<?php

class SelfDrop { function __destruct() { $owner = Fiber::getCurrent(); try { Fiber::suspend('ready'); } finally { echo 'drop-finally|'; } } }
$fiber = new Fiber(function () { try { $x = new SelfDrop; unset($x); } finally { echo 'fiber-finally|'; } });
echo $fiber->start(), '|'; unset($fiber); echo 'after|'; gc_collect_cycles(); echo 'collected|';
"####
        ),
        r####"ready|after|drop-finally|fiber-finally|collected|"####
    );
}

#[test]
fn iterator_multiple_pause() {
    assert_eq!(
        run_php(
            r####"<?php

class DoubleIterator implements Iterator {
 public $position = 0;
 function rewind(): void { echo 'a:', Fiber::suspend('first'), '|'; echo 'b:', Fiber::suspend('second'), '|'; }
 function valid(): bool { return $this->position < 1; }
 function current(): mixed { return 42; }
 function key(): mixed { return 9; }
 function next(): void { $this->position++; }
}
function delegateTwice() { try { yield from new DoubleIterator; } finally { echo 'gen-finally|'; } }
$fiber = new Fiber(function () { foreach (delegateTwice() as $key => $value) echo $key, ':', $value, '|'; });
echo $fiber->start(), '|'; echo $fiber->resume('A'), '|'; $fiber->resume('B');
"####
        ),
        r####"a:first|A|b:second|B|9:42|gen-finally|"####
    );
}

#[test]
fn iterator_nested_generator() {
    assert_eq!(
        run_php(
            r####"<?php

class NestedIterator implements Iterator {
 function rewind(): void { echo 'inner:', Fiber::suspend('pause'), '|'; }
 function valid(): bool { return false; } function current(): mixed { return null; }
 function key(): mixed { return null; } function next(): void {}
}
function sourceLeaf() { try { yield from new NestedIterator; } finally { echo 'leaf|'; } }
function sourceRoot() { try { yield from sourceLeaf(); } finally { echo 'root|'; } }
$fiber = new Fiber(function () { foreach (sourceRoot() as $value) echo 'bad|'; echo 'done|'; });
echo $fiber->start(), '|'; $fiber->resume('ok');
"####
        ),
        r####"inner:pause|ok|leaf|root|done|"####
    );
}

#[test]
fn iterator_native_nested_generator() {
    assert_eq!(
        run_php(
            r####"<?php

function innerSource() { try { echo 'inner:', Fiber::suspend('inside'), '|'; yield 5; } finally { echo 'inner-finally|'; } }
class InnerGeneratorIterator implements Iterator {
 function rewind(): void { foreach (innerSource() as $value) echo $value, '|'; }
 function valid(): bool { return false; } function current(): mixed { return null; }
 function key(): mixed { return null; } function next(): void {}
}
function outerSource() { yield from new InnerGeneratorIterator; }
$fiber = new Fiber(function () { $g = outerSource(); $g->current(); echo 'done|'; });
echo $fiber->start(), '|'; $fiber->resume('ok');
"####
        ),
        r####"inner:inside|ok|5|inner-finally|done|"####
    );
}

#[test]
fn iterator_force_close_current_identity() {
    assert_eq!(
        run_php(
            r####"<?php

class RetainingIterator implements Iterator {
 function rewind(): void { $self = Fiber::getCurrent(); try { Fiber::suspend('ready'); } finally { echo 'iterator-finally|'; } }
 function valid(): bool { return false; } function current(): mixed { return null; }
 function key(): mixed { return null; } function next(): void {}
}
function parkedIdentity() { try { yield from new RetainingIterator; } finally { echo 'generator-finally|'; } }
$fiber = new Fiber(function () { foreach (parkedIdentity() as $x) {} });
echo $fiber->start(), '|'; unset($fiber); echo 'after|'; gc_collect_cycles(); echo 'collected|';
"####
        ),
        r####"ready|after|iterator-finally|generator-finally|collected|"####
    );
}

#[test]
fn iterator_trace() {
    assert_eq!(
        run_php_cli(
            r####"<?php

class TraceIterator implements Iterator {
 function rewind(): void { Fiber::suspend('trace'); foreach (debug_backtrace(2) as $item) echo $item['function'], '|'; }
 function valid(): bool { return false; } function current(): mixed { return null; }
 function key(): mixed { return null; } function next(): void {}
}
function traceSource() { yield from new TraceIterator; }
$fiber = new Fiber(function () { $g = traceSource(); $g->current(); });
echo $fiber->start(), '|'; $fiber->resume();
"####
        ),
        r####"trace|rewind|traceSource|current|{closure:Command line code:8}|resume|"####
    );
}

#[test]
fn parked_native_cycle_collection() {
    assert_eq!(
        run_php_cli(
            r####"<?php

class CyclicIterator implements Iterator {
 public $fiber;
 function rewind(): void { $alias = $this; try { Fiber::suspend('cycle'); } finally { echo 'iterator-finally|'; } }
 function valid(): bool { return false; } function current(): mixed { return null; }
 function key(): mixed { return null; } function next(): void {}
 function __destruct() { echo 'iterator-drop|'; }
}
function cycleSource($iterator) { try { yield from $iterator; } finally { echo 'generator-finally|'; } }
$it = new CyclicIterator; $g = cycleSource($it); $f = new Fiber(function () use ($g) { $g->current(); }); $it->fiber = $f;
$weak = WeakReference::create($f); echo $f->start(), '|'; unset($it, $g, $f); gc_collect_cycles(); echo 'gone:', (int) ($weak->get() === null), '|';
"####
        ),
        r####"cycle|gone:0|iterator-drop|iterator-finally|generator-finally|"####
    );
}

#[test]
fn parked_native_dynamic_variable() {
    assert_eq!(
        run_php(
            r####"<?php

class DynamicOwner { function __destruct() { echo 'dynamic-drop|'; } }
class DynamicIterator implements Iterator {
 function rewind(): void { $name = 'held'; $$name = new DynamicOwner; $GLOBALS['dynamicWeak'] = WeakReference::create($$name); Fiber::suspend('dynamic'); echo 'same:', (int) ($$name === $GLOBALS['dynamicWeak']->get()), '|'; }
 function valid(): bool { return false; } function current(): mixed { return null; }
 function key(): mixed { return null; } function next(): void {}
}
function dynamicSource() { yield from new DynamicIterator; }
$f = new Fiber(function () { $g = dynamicSource(); $g->current(); }); echo $f->start(), '|'; gc_collect_cycles(); $f->resume(); echo 'gone:', (int) ($dynamicWeak->get() === null), '|';
"####
        ),
        r####"dynamic|same:1|dynamic-drop|gone:1|"####
    );
}

#[test]
fn destructor_gc_inside_callback() {
    assert_eq!(
        run_php(
            r####"<?php

class CollectingDrop {
 function __destruct() { $self = $this; gc_collect_cycles(); echo 'first:', Fiber::suspend('ready'), '|'; gc_collect_cycles(); echo 'same:', (int) ($self === $this), '|'; }
}
$f = new Fiber(function () { $x = new CollectingDrop; unset($x); echo 'after|'; }); echo $f->start(), '|'; gc_collect_cycles(); $f->resume('ok');
"####
        ),
        r####"first:ready|ok|same:1|after|"####
    );
}

#[test]
fn iterator_exception_trace() {
    assert_eq!(
        run_php_cli(
            r####"<?php

class ThrowTraceIterator implements Iterator {
 function rewind(): void { Fiber::suspend('ready'); throw new RuntimeException('error'); }
 function valid(): bool { return false; } function current(): mixed { return null; }
 function key(): mixed { return null; } function next(): void {}
}
function errorSource() { yield from new ThrowTraceIterator; }
$f = new Fiber(function () { $g = errorSource(); try { $g->current(); } catch (RuntimeException $error) { foreach ($error->getTrace() as $item) echo $item['function'], '|'; } });
echo $f->start(), '|'; $f->resume();
"####
        ),
        r####"ready|rewind|errorSource|current|{closure:Command line code:8}|resume|"####
    );
}

#[test]
fn destructor_aliased_child() {
    assert_eq!(
        run_php(
            r####"<?php

class SharedLeaf { function __destruct() { echo 'leaf:', Fiber::suspend('child'), '|'; } }
class SharedRoot { public $a; public $b; function __construct() { $this->a = $this->b = new SharedLeaf; } function __destruct() { echo 'root|'; } }
$f = new Fiber(function () { $x = new SharedRoot; unset($x); echo 'after|'; }); echo $f->start(), '|'; $f->resume('ok');
"####
        ),
        r####"root|leaf:child|ok|after|"####
    );
}
