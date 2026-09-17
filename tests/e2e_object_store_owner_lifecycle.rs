mod common;

use common::run_php;

#[test]
fn bounded_consumer_proof_falls_back_for_deep_and_wide_callback_graphs() {
    assert_eq!(
        run_php(
            r#"<?php
class Tail { function __destruct() { echo "tail\n"; } }
function nestedSource($mode) {
    if ($mode === 'wide') {
        $payload = array_fill(0, 40, 1);
        $payload[] = new Tail;
    } else {
        $payload = new Tail;
        for ($i = 0; $i < 6; ++$i) $payload = (object) ['next' => $payload];
    }
    return new ArrayIterator([$payload]);
}
foreach (['deep', 'wide'] as $mode) {
    echo $mode, "\n";
    foreach (nestedSource($mode) as $value) { unset($value); break; }
    echo "after\n";
}
"#
        ),
        "deep\ntail\nafter\nwide\ntail\nafter\n"
    );
}

#[test]
fn simultaneously_retired_consumers_do_not_hide_the_last_iterator_owner() {
    assert_eq!(
        run_php(
            r#"<?php
class Walk extends ArrayIterator {
    function __destruct() { echo "destroyed\n"; }
}
function visit($mode) {
    $source = new Walk([1]);
    foreach ($source as $a) {
        foreach ($source as $b) {
            unset($source);
            if ($mode === 'return') return;
            throw new Exception('stop');
        }
    }
}
foreach (['return','throw'] as $mode) {
    echo $mode, "\n";
    try { visit($mode); } catch (Throwable $e) { echo "caught\n"; }
    echo "after\n";
}
"#
        ),
        "return\ndestroyed\nafter\nthrow\ndestroyed\ncaught\nafter\n"
    );
}

#[test]
fn last_shared_cursor_destructor_replaces_the_exception_after_retiring_siblings() {
    assert_eq!(
        run_php(
            r#"<?php
class Walk extends ArrayIterator {
    function __destruct() {
        $probe = new stdClass;
        echo 'destructor:', spl_object_id($probe), "\n";
        throw new RuntimeException('cleanup');
    }
}
function visit() {
    $source = new Walk([1]);
    foreach ($source as $a) {
        foreach ($source as $b) { unset($source); throw new Exception('body'); }
    }
}
try { visit(); }
catch (Throwable $error) {
    echo $error->getMessage(), ':', $error->getPrevious()->getMessage(), "\n";
}
"#
        ),
        "destructor:2\ncleanup:body\n"
    );
}

#[test]
fn temporary_cursor_children_with_internal_aliases_still_run_destructors() {
    assert_eq!(
        run_php(
            r#"<?php
class Child { function __destruct() { echo "child\n"; } }
class Cursor implements Iterator {
    public $left;
    public $right;
    private $at = 0;
    function __construct($mode) {
        $this->left = new Child;
        if ($mode === 'alias') $this->right = $this->left;
        elseif ($mode === 'reference') $this->right =& $this->left;
        else $this->right = [$this->left];
    }
    function rewind(): void { $this->at = 0; }
    function valid(): bool { return $this->at === 0; }
    function current(): mixed { return 4; }
    function key(): mixed { return 0; }
    function next(): void { ++$this->at; }
}
class Provider implements IteratorAggregate {
    function __construct(private $mode) {}
    function getIterator(): Traversable { return new Cursor($this->mode); }
}
foreach (['alias', 'reference', 'array'] as $mode) {
    echo $mode, "\n";
    foreach (new Provider($mode) as $v) { echo $v, "\n"; }
    echo "after\n";
}
"#
        ),
        "alias\n4\nchild\nafter\nreference\n4\nchild\nafter\narray\n4\nchild\nafter\n"
    );
}

#[test]
fn intrinsic_weak_map_cursor_does_not_publish_a_second_consumer() {
    assert_eq!(
        run_php(
            r#"<?php
function probe($tag) { $v = new stdClass; echo $tag, ':', spl_object_id($v), "\n"; }
$map = new WeakMap;
probe('before');
foreach ($map as $key => $value) {}
probe('empty');
$object = new stdClass;
$map[$object] = 4;
foreach ($map as $key => $value) { probe('body'); }
probe('after');
foreach ($map as &$value) { ++$value; }
echo $map[$object], "\n";
"#
        ),
        "before:2\nempty:2\nbody:4\nafter:3\n5\n"
    );
}

#[test]
fn closure_retires_after_its_owned_receiver_and_captures() {
    for source in [
        r#"<?php class Owner { function work() {} }
$closure = Closure::fromCallable([new Owner, 'work']);
echo spl_object_id($closure), "\n";
unset($closure);
$first = new stdClass; $second = new stdClass;
echo spl_object_id($first), ':', spl_object_id($second), "\n";"#,
        r#"<?php $child = new stdClass;
$closure = static function () use ($child) {};
unset($child);
echo spl_object_id($closure), "\n";
unset($closure);
$first = new stdClass; $second = new stdClass;
echo spl_object_id($first), ':', spl_object_id($second), "\n";"#,
    ] {
        assert_eq!(run_php(source), "2\n2:1\n");
    }
}

#[test]
fn shared_closure_does_not_return_either_handle_early() {
    assert_eq!(
        run_php(
            r#"<?php
$child = new stdClass;
$closure = static function () use ($child) {};
$alias = $closure;
unset($child, $closure);
$during = new stdClass;
echo spl_object_id($during), "\n";
unset($alias);
$after = new stdClass;
echo spl_object_id($after), "\n";
"#
        ),
        "3\n2\n"
    );
}

#[test]
fn native_consumer_has_an_independent_scoped_owner() {
    assert_eq!(
        run_php(
            r#"<?php
function probe($tag) { $object = new stdClass; echo $tag, ':', spl_object_id($object), "\n"; }
$source = new ArrayObject([9]);
probe('before');
foreach ($source as $value) { probe('body'); }
probe('after');
foreach ($source as $value) { probe('break'); break; }
probe('after-break');
unset($source);
probe('released');
"#
        ),
        "before:2\nbody:4\nafter:3\nbreak:4\nafter-break:2\nreleased:1\n"
    );
}

#[test]
fn materialized_native_cursor_does_not_publish_a_second_private_owner() {
    assert_eq!(
        run_php(
            r#"<?php
$source = new SplDoublyLinkedList;
foreach (['a', 'b', 'c'] as $value) $source->push($value);
$objects = [];
foreach ($source as $value) {
    $object = new stdClass;
    $objects[] = $object;
    echo spl_object_id($object), ';';
}
echo "\n";
unset($objects, $object);
$object = new stdClass;
echo spl_object_id($object), "\n";
"#
        ),
        "3;4;5;\n5\n"
    );
}

#[test]
fn public_iterator_callbacks_observe_the_consumer_before_rewind() {
    assert_eq!(
        run_php(
            r#"<?php
function probe($tag) { $object = new stdClass; echo $tag, ':', spl_object_id($object), "\n"; }
class Walk implements Iterator {
    private $position = 0;
    function rewind(): void { $this->position = 0; probe('rewind'); }
    function valid(): bool { return $this->position < 1; }
    function current(): mixed { probe('current'); return 9; }
    function key(): mixed { return 0; }
    function next(): void { ++$this->position; }
    function __destruct() { probe('destructor'); }
}
$source = new Walk;
probe('before');
foreach ($source as $value) { probe('body'); }
probe('after');
unset($source);
probe('released');
"#
        ),
        "before:2\nrewind:3\ncurrent:3\nbody:3\nafter:2\ndestructor:2\nreleased:1\n"
    );
}

#[test]
fn nested_consumers_retire_in_lifo_order_without_replacing_the_iterator() {
    assert_eq!(
        run_php(
            r#"<?php
function probe($tag) { $object = new stdClass; echo $tag, ':', spl_object_id($object), "\n"; }
$source = new ArrayObject([1]);
foreach ($source as $value) {
    probe('outer');
    foreach ($source as $inner) { probe('inner'); break; }
    probe('restored'); break;
}
probe('after');
"#
        ),
        "outer:4\ninner:6\nrestored:5\nafter:3\n"
    );
}

#[test]
fn throwing_rewind_retires_consumer_before_entering_the_catch() {
    assert_eq!(
        run_php(
            r#"<?php
function probe($tag) { $object = new stdClass; echo $tag, ':', spl_object_id($object), "\n"; }
class BrokenWalk extends ArrayIterator {
    function rewind(): void { probe('rewind'); throw new Exception('stop'); }
}
$source = new BrokenWalk([1]);
try { foreach ($source as $value) {} }
catch (Exception $error) { probe('caught'); echo spl_object_id($error), "\n"; }
unset($error);
probe('released');
"#
        ),
        "rewind:3\ncaught:2\n3\nreleased:3\n"
    );
}

#[test]
fn ordinary_property_and_array_iteration_do_not_allocate_consumer_handles() {
    for (source, expected) in [("(object)['item' => 9]", "2:2:1\n"), ("[9]", "1:1:1\n")] {
        assert_eq!(
            run_php(&format!(
                r#"<?php
function probe() {{ $object = new stdClass; echo spl_object_id($object); }}
$source = {source};
foreach ($source as $value) {{ probe(); }}
echo ':'; probe(); unset($source); echo ':'; probe(); echo "\n";
"#
            )),
            expected
        );
    }
}

#[test]
fn internal_enum_cases_publish_per_request_and_only_when_materialized() {
    for fetch in [
        "PropertyHookType::Get",
        "PropertyHookType::from('get')",
        "PropertyHookType::tryFrom('get')",
    ] {
        assert_eq!(
            run_php(&format!(
                r#"<?php
$before = new stdClass;
$kind = {fetch};
$after = new stdClass;
echo spl_object_id($before), ':', spl_object_id($kind), ':', spl_object_id($after), ':';
echo spl_object_id(PropertyHookType::Set), "\n";
"#
            )),
            "1:2:3:4\n"
        );
    }
}

#[test]
fn rejected_property_temporary_retires_before_the_previous_catch_owner() {
    assert_eq!(
        run_php(
            r#"<?php
class Payload {}
class Slots { public Payload $first; public ?Payload $second = null; }
$slots = new Slots;
try { $slots->first = new stdClass; }
catch (TypeError $caught) { echo 'first-error:', spl_object_id($caught), "\n"; }
$slots->first = new Payload;
echo 'stored:', spl_object_id($slots->first), "\n";
$reference =& $slots->second;
try { $slots->second = new stdClass; }
catch (TypeError $caught) { echo 'second-error:', spl_object_id($caught), "\n"; }
$reference = new Payload;
echo 'replacement:', spl_object_id($reference), "\n";
$next = new stdClass;
echo 'next:', spl_object_id($next), "\n";
"#
        ),
        "first-error:3\nstored:2\nsecond-error:5\nreplacement:3\nnext:4\n"
    );
}

#[test]
fn self_backing_is_a_member_view_not_an_owning_cycle() {
    for base in ["ArrayObject", "ArrayIterator"] {
        assert_eq!(
            run_php(&format!(
                r#"<?php
set_error_handler(static fn() => true);
class SelfBox extends {base} {{
    function __construct() {{ parent::__construct($this); $this['key'] = 4; }}
}}
$source = new SelfBox;
$weak = WeakReference::create($source);
$copy = clone $source;
$copy['key'] = 8;
$copy['extra'] = 3;
echo $source['key'], ':', $copy['key'], ':', count($source), ':', count($copy), "\n";
$id = spl_object_id($source);
unset($source);
$next = new stdClass;
echo (int)($weak->get() === null), ':', (int)(spl_object_id($next) === $id), "\n";
$wire = serialize($copy);
$id = spl_object_id($copy);
unset($copy);
$copy = unserialize($wire);
echo $copy['key'], ':', $copy['extra'], ':', (int)(spl_object_id($copy) === $id), "\n";
"#
            )),
            "4:8:1:2\n1:1\n8:3:1\n",
            "{base}"
        );
    }
}

#[test]
fn exception_in_current_retires_temporary_iterator_before_catch() {
    assert_eq!(
        run_php(
            r#"<?php
function probe($tag) { $object = new stdClass; echo $tag, ':', spl_object_id($object), "\n"; }
class BrokenCurrent extends ArrayIterator {
    function current(): mixed { throw new RuntimeException('body'); }
    function __destruct() { probe('destructor'); }
}
try { foreach (new BrokenCurrent([3]) as $value) {} }
catch (Throwable $error) { probe('catch'); }
unset($error);
probe('after');
"#
        ),
        "destructor:4\ncatch:2\nafter:3\n"
    );
}

#[test]
fn temporary_consumers_retire_on_every_exit_without_masking_destructor_errors() {
    for (mode, expected) in [
        ("empty", "rewind:3\ndestructor:3\nfinished:2\nafter:2\n"),
        (
            "rewind",
            "rewind:3\ndestructor:2\nrewind:3\ncaught:1\nafter:3\n",
        ),
        ("return", "rewind:3\nbody:3\ndestructor:3\nafter:2\n"),
        (
            "goto",
            "rewind:3\nbody:3\ndestructor:3\nfinished:2\nafter:2\n",
        ),
        (
            "destructor",
            "rewind:3\nbody:3\ndestructor:3\ndestructor:3\ncaught:2\nafter:3\n",
        ),
    ] {
        let source = r#"<?php
const MODE = 'EXIT_MODE';
function probe($tag) { $v = new stdClass; echo $tag, ':', spl_object_id($v), "\n"; }
class Walk extends ArrayIterator {
    function rewind(): void {
        probe('rewind');
        if (MODE === 'rewind') throw new Exception('rewind');
        parent::rewind();
    }
    function __destruct() {
        probe('destructor');
        if (MODE === 'destructor') throw new Exception('destructor');
    }
}
function visit() {
    foreach (new Walk(MODE === 'empty' ? [] : [1,2]) as $v) {
        probe('body');
        if (MODE === 'return') return;
        if (MODE === 'goto') goto finished;
        break;
    }
    finished: probe('finished');
}
try { visit(); }
catch (Throwable $e) { echo $e->getMessage(), ':', spl_object_id($e), "\n"; probe('caught'); }
unset($e);
probe('after');
"#
        .replace("EXIT_MODE", mode);
        assert_eq!(run_php(&source), expected, "{mode}");
    }
}
