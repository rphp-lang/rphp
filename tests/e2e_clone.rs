mod common;
use common::run_php;

#[test]
fn shared_clone_site_preserves_polymorphism_reentry_and_native_state() {
    assert_eq!(
        run_php(
            r#"<?php
function copyOne($value) { return clone $value; }
class PlainClone { public $n = 1; }
class HookClone { public $n = 1; function __CLONE() { $this->n += 2; } }
class InheritedClone extends HookClone {}
trait CloneTrait { function bump() { $this->n += 4; } }
class TraitClone { use CloneTrait { bump as __clone; } public $n = 1; }
foreach ([new PlainClone, new HookClone, new InheritedClone, new TraitClone, new PlainClone] as $original) {
    for ($i = 0; $i < 2; ++$i) {
        $copy = copyOne($original);
        echo get_class($copy), ':', $original->n, ':', $copy->n, "\n";
    }
}
class RecursiveClone {
    public $n = 1;
    function __clone() { $plain = copyOne(new PlainClone); $this->n = $plain->n + 6; }
}
class ThrowClone {
    public $n = 1;
    function __clone() { copyOne(new PlainClone); throw new Exception('clone stopped'); }
}
for ($i = 0; $i < 2; ++$i) {
    echo 'nested:', copyOne(new RecursiveClone)->n, "\n";
    try { copyOne(new ThrowClone); } catch (Exception $error) { echo $error->getMessage(), "\n"; }
    echo 'after:', copyOne(new PlainClone)->n, "\n";
}
class InfoClone extends SplFileInfo { function __clone() { parent::__construct('changed.bin'); } }
foreach ([new SplFileInfo('item.txt'), new InfoClone('item.txt'), new SplFileInfo('item.txt')] as $original) {
    $copy = copyOne($original);
    echo get_class($copy), ':', $original->getPathname(), ':', $copy->getPathname(), "\n";
}
"#
        ),
        "PlainClone:1:1\nPlainClone:1:1\nHookClone:1:3\nHookClone:1:3\nInheritedClone:1:3\nInheritedClone:1:3\nTraitClone:1:5\nTraitClone:1:5\nPlainClone:1:1\nPlainClone:1:1\nnested:7\nclone stopped\nafter:1\nnested:7\nclone stopped\nafter:1\nSplFileInfo:item.txt:item.txt\nInfoClone:item.txt:changed.bin\nSplFileInfo:item.txt:item.txt\n"
    );
}

#[test]
fn test_clone_basic_properties_independent() {
    let output = run_php(
        r#"<?php
class Foo {
    public $x = 1;
    public $y = 2;
}
$a = new Foo();
$b = clone $a;
$b->x = 10;
echo $a->x . "\n";
echo $b->x . "\n";
echo $a->y . "\n";
echo $b->y . "\n";
"#,
    );
    assert_eq!(output, "1\n10\n2\n2\n");
}

#[test]
fn test_clone_does_not_affect_original() {
    let output = run_php(
        r#"<?php
class Point {
    public $x;
    public $y;
    public function __construct($x, $y) {
        $this->x = $x;
        $this->y = $y;
    }
}
$p1 = new Point(3, 4);
$p2 = clone $p1;
$p2->x = 99;
$p2->y = 100;
echo $p1->x . "," . $p1->y . "\n";
echo $p2->x . "," . $p2->y . "\n";
"#,
    );
    assert_eq!(output, "3,4\n99,100\n");
}

#[test]
fn test_clone_magic_method_called() {
    let output = run_php(
        r#"<?php
class WithClone {
    public $name;
    public $cloned = false;
    public function __construct($name) {
        $this->name = $name;
    }
    public function __clone() {
        $this->cloned = true;
        $this->name = $this->name . "_copy";
    }
}
$a = new WithClone("original");
$b = clone $a;
echo $a->name . "\n";
echo $a->cloned . "\n";
echo $b->name . "\n";
echo $b->cloned . "\n";
"#,
    );
    // $a should be unaffected; $b should have __clone modifications
    assert_eq!(output, "original\n\noriginal_copy\n1\n");
}

#[test]
fn test_clone_shallow_nested_objects_shared() {
    let output = run_php(
        r#"<?php
class Inner {
    public $val = 10;
}
class Outer {
    public $inner;
    public function __construct() {
        $this->inner = new Inner();
    }
}
$a = new Outer();
$b = clone $a;
// Shallow clone: $b->inner is the same object as $a->inner
$b->inner->val = 42;
echo $a->inner->val . "\n";
echo $b->inner->val . "\n";
"#,
    );
    // Both should be 42 because the inner object is shared (shallow clone)
    assert_eq!(output, "42\n42\n");
}

#[test]
fn test_clone_with_deep_copy_via_clone_method() {
    let output = run_php(
        r#"<?php
class Inner {
    public $val;
    public function __construct($v) {
        $this->val = $v;
    }
}
class Outer {
    public $inner;
    public function __construct($v) {
        $this->inner = new Inner($v);
    }
    public function __clone() {
        $this->inner = clone $this->inner;
    }
}
$a = new Outer(5);
$b = clone $a;
$b->inner->val = 99;
echo $a->inner->val . "\n";
echo $b->inner->val . "\n";
"#,
    );
    // Deep clone via __clone — inner objects should be independent
    assert_eq!(output, "5\n99\n");
}

#[test]
fn test_clone_exception_in_clone_method() {
    let output = run_php(
        r#"<?php
class NoClone {
    public function __clone() {
        throw new Exception("Clone not allowed");
    }
}
$a = new NoClone();
try {
    $b = clone $a;
    echo "no_exception";
} catch (Exception $e) {
    echo "caught:" . $e->getMessage();
}
"#,
    );
    assert_eq!(output, "caught:Clone not allowed");
}

#[test]
fn clone_results_are_initialized_after_mixed_stack_frame_reuse() {
    let output = run_php(
        r#"<?php
class CloneTarget {
    public int $value = 7;
}
function churn_stack($value) {
    $first = [$value, $value];
    $second = $first;
    return $second[0];
}
function copy_target(CloneTarget $target): CloneTarget {
    return clone $target;
}
$target = new CloneTarget();
for ($index = 0; $index < 64; ++$index) {
    churn_stack((string) $index);
    echo copy_target($target)->value;
}
"#,
    );
    assert_eq!(output, "7".repeat(64));
}

#[test]
fn nested_clone_wraps_assignment_before_each_clone() {
    let output = run_php(
        r#"<?php
class CloneValue {
    public int $value = 1;
}
$outer = clone clone $source = new CloneValue;
$source->value = 2;
echo $outer->value, "|", $source->value;
"#,
    );
    assert_eq!(output, "1|2");
}

#[test]
fn cloning_a_non_object_throws_a_catchable_error() {
    let output = run_php(
        r#"<?php
try {
    clone 42;
} catch (Error $error) {
    echo get_class($error), "|", $error->getMessage();
}
"#,
    );
    assert_eq!(
        output,
        "TypeError|clone(): Argument #1 ($object) must be of type object, int given"
    );
}

#[test]
fn automatic_clone_allows_one_successful_readonly_reinitialization() {
    assert_eq!(
        run_php(
            r#"<?php
class ReadonlyCloneWindow {
    public function __construct(public readonly int $value) {}
    public function __clone() {
        try { $this->value = 'bad'; } catch (TypeError $error) { echo "type:"; }
        $this->value += 1;
        try { $this->value = 99; } catch (Error $error) { echo "twice:"; }
    }
}
$original = new ReadonlyCloneWindow(1);
$copy = clone $original;
echo $original->value, ':', $copy->value, "\n";
try { $original->__clone(); } catch (Error $error) { echo "manual\n"; }
"#,
        ),
        "type:twice:1:2\nmanual\n"
    );
}

#[test]
fn automatic_clone_allows_readonly_increment_writeback() {
    assert_eq!(
        run_php(
            r#"<?php
class ReadonlyCloneIncrement {
    public function __construct(public readonly int $value) {}
    public function __clone() {
        $this->value++;
    }
}
$original = new ReadonlyCloneIncrement(1);
$copy = clone $original;
echo $original->value, ':', $copy->value;
"#,
        ),
        "1:2"
    );
}

#[test]
fn readonly_clone_rejects_indirect_updates_but_clone_with_replaces_directly() {
    assert_eq!(
        run_php(
            r#"<?php
class ReadonlyCloneArray {
    public function __construct(public public(set) readonly array $items) {}
    public function __clone() {
        try { $this->items[] = 'bad'; } catch (Error $error) { echo $error->getMessage(), "\n"; }
    }
}
$original = new ReadonlyCloneArray(['old']);
$copy = clone($original, ['items' => ['new']]);
echo $original->items[0], ':', $copy->items[0], "\n";
"#,
        ),
        concat!(
            "Cannot indirectly modify readonly property ReadonlyCloneArray::$items\n",
            "old:new\n",
        )
    );
}

#[test]
fn dynamic_new_accepts_an_object_and_rejects_other_non_strings() {
    let output = run_php(
        r#"<?php
class DynamicClass {}
$prototype = new DynamicClass;
$copy = new $prototype;
echo get_class($copy), "|";
try {
    $invalid = 42;
    new $invalid;
} catch (Error $error) {
    echo $error->getMessage();
}
"#,
    );
    assert_eq!(
        output,
        "DynamicClass|Class name must be a valid object or a string"
    );
}
