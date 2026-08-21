/// Tests for __construct() constructor
mod common;
use common::run_php;
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;

fn compile_constructor_source(source: &str) -> rphp::compiler::compile::CompileResult {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    Compiler::new().compile(&statements).unwrap()
}

#[test]
fn test_constructor_basic() {
    assert_eq!(
        run_php(
            r#"<?php
class Dog {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}
$d = new Dog("Rex");
echo $d->name;
"#
        ),
        "Rex"
    );
}

#[test]
fn test_constructor_multiple_args() {
    assert_eq!(
        run_php(
            r#"<?php
class Point {
    public $x;
    public $y;
    public function __construct($x, $y) {
        $this->x = $x;
        $this->y = $y;
    }
}
$p = new Point(3, 4);
echo $p->x . "," . $p->y;
"#
        ),
        "3,4"
    );
}

#[test]
fn test_constructor_with_method() {
    assert_eq!(
        run_php(
            r#"<?php
class Greeter {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
    public function greet() {
        return "Hello " . $this->name;
    }
}
$g = new Greeter("World");
echo $g->greet();
"#
        ),
        "Hello World"
    );
}

#[test]
fn test_constructor_no_args() {
    assert_eq!(
        run_php(
            r#"<?php
class Counter {
    public $count;
    public function __construct() {
        $this->count = 0;
    }
    public function increment() {
        $this->count = $this->count + 1;
    }
}
$c = new Counter();
$c->increment();
$c->increment();
echo $c->count;
"#
        ),
        "2"
    );
}

#[test]
fn test_constructor_default_overridden() {
    assert_eq!(
        run_php(
            r#"<?php
class Config {
    public $timeout = 30;
    public function __construct($t) {
        $this->timeout = $t;
    }
}
$c = new Config(60);
echo $c->timeout;
"#
        ),
        "60"
    );
}

#[test]
fn test_no_constructor_no_args() {
    // Class without constructor — new still works
    assert_eq!(
        run_php(
            r#"<?php
class Empty2 {}
$e = new Empty2();
echo "ok";
"#
        ),
        "ok"
    );
}

#[test]
fn test_multiple_objects_different_constructor_args() {
    assert_eq!(
        run_php(
            r#"<?php
class Box {
    public $value;
    public function __construct($v) {
        $this->value = $v;
    }
}
$a = new Box(10);
$b = new Box(20);
echo $a->value . " " . $b->value;
"#
        ),
        "10 20"
    );
}

#[test]
fn test_no_constructor_with_args_silently_ignored() {
    // PHP evaluates arg expressions (side effects run) but ignores values
    // when class has no __construct
    assert_eq!(
        run_php(
            r#"<?php
class Foo {}
function side() { echo "S"; return 1; }
$f = new Foo(side());
echo "X";
"#
        ),
        "SX"
    );
}

#[test]
fn test_no_constructor_negative_cache_keeps_argument_side_effects() {
    assert_eq!(
        run_php(
            r#"<?php
class PlainBox { public $value = 7; }
$sum = 0;
for ($i = 0; $i < 5; $i++) {
    $box = new PlainBox($sum = $sum + 1);
}
echo $sum . ':' . $box->value;
"#
        ),
        "5:7"
    );
}

#[test]
fn test_declared_property_constructor_gets_init_plan() {
    let result = compile_constructor_source(
        r#"<?php
class Request {
    public $subtotal;
    public $level;
    public $region;
    public function __construct(int $subtotal, int $level, string $region) {
        $this->subtotal = $subtotal;
        $this->level = $level;
        $this->region = $region;
    }
}
"#,
    );
    let constructor = result.class_defs[0]
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name == "__construct")
        .map(|(_, _, _, _, function)| function)
        .unwrap();
    let plan = constructor
        .property_init_plan
        .as_deref()
        .expect("declared property constructor init plan");
    assert_eq!(plan.public_args, 3);
    assert_eq!(plan.assignments.len(), 3);
}

#[test]
fn test_constructor_init_plan_preserves_named_type_and_dynamic_fallbacks() {
    assert_eq!(
        run_php(
            r#"<?php
class DeclaredDto {
    public $first;
    public $second;
    public function __construct(int $first, int $second) {
        $this->first = $first;
        $this->second = $second;
    }
}
#[AllowDynamicProperties]
class DynamicDto {
    public function __construct($value) { $this->value = $value; }
}
$named = new DeclaredDto(second: 4, first: 3);
for ($i = 0; $i < 20; $i++) { $warm = new DeclaredDto($i, $i + 1); }
for ($i = 0; $i < 20; $i++) { $dynamic = new DynamicDto($i); }
echo $named->first . ':' . $named->second . '|' . $dynamic->value . '|';
try {
    new DeclaredDto('bad', 1);
} catch (TypeError $error) {
    echo 'typed';
}
"#
        ),
        "3:4|19|typed"
    );
}

#[test]
fn failed_constructor_never_reenables_destructor_after_this_escapes() {
    assert_eq!(
        run_php(
            r#"<?php
class RetryAllocation {
    public static $attempt = 0;

    public function __construct() {
        echo 'ctor:', self::$attempt, '|';
        $GLOBALS['escaped'] = $this;
        if (self::$attempt++ === 0) {
            throw new Exception('first');
        }
    }

    public function __destruct() { echo 'retry-dtor|'; }
}

try {
    new RetryAllocation();
} catch (Throwable $error) {
    echo 'caught|';
}
$GLOBALS['escaped']->__construct();
unset($GLOBALS['escaped']);
gc_collect_cycles();
echo 'done';
"#
        ),
        "ctor:0|caught|ctor:1|done"
    );
}

#[test]
fn failed_constructor_preserves_completed_temporary_destruction() {
    assert_eq!(
        run_php(
            r#"<?php
class CompletedOperand {
    public function __construct() { echo 'complete-ctor|'; }
    public function __destruct() { echo 'complete-dtor|'; }
}
class FailedOperand {
    public function __construct() {
        echo 'failed-ctor|';
        throw new Exception('stop');
    }
    public function __destruct() { echo 'failed-dtor|'; }
}
function acceptOperands($left, $right) { echo 'called|'; }

try {
    acceptOperands(new CompletedOperand(), new FailedOperand());
} catch (Throwable $error) {
    echo 'caught|';
}
echo 'done';
"#
        ),
        "complete-ctor|failed-ctor|complete-dtor|caught|done"
    );
}

#[test]
fn destructor_exception_during_constructor_frame_cleanup_keeps_owner_ineligible() {
    assert_eq!(
        run_php(
            r#"<?php
class ConstructorLocalBomb {
    public function __destruct() {
        echo 'local-dtor|';
        throw new Exception('cleanup');
    }
}
class CleanupInterrupted {
    public function __construct() {
        $GLOBALS['cleanup_escaped'] = $this;
        $local = new ConstructorLocalBomb();
        echo 'body-end|';
    }
    public function __destruct() { echo 'outer-dtor|'; }
}

try {
    new CleanupInterrupted();
    echo 'new-done|';
} catch (Throwable $error) {
    echo 'caught:', $error->getMessage(), '|';
}
unset($GLOBALS['cleanup_escaped']);
gc_collect_cycles();
echo 'done';
"#
        ),
        "body-end|local-dtor|caught:cleanup|done"
    );
}

#[test]
fn successful_and_constructorless_allocations_remain_destructor_eligible() {
    assert_eq!(
        run_php(
            r#"<?php
class PlannedComplete {
    public $value;
    public function __construct(int $value) { $this->value = $value; }
    public function __destruct() { echo 'planned:', $this->value, '|'; }
}
class NoConstructor {
    public function __destruct() { echo 'no-ctor|'; }
}
class ReflectionSkipped {
    public function __construct() { echo 'unexpected-ctor|'; }
    public function __destruct() { echo 'reflection|'; }
}

function releaseBoundaries() {
    new PlannedComplete(7);
    new NoConstructor();
    $reflection = new ReflectionClass(ReflectionSkipped::class);
    $skipped = $reflection->newInstanceWithoutConstructor();
    $skipped = null;
}
releaseBoundaries();
echo 'done';
"#
        ),
        "planned:7|no-ctor|reflection|done"
    );
}

#[test]
fn failed_owner_still_releases_destructor_bearing_properties() {
    assert_eq!(
        run_php(
            r#"<?php
class FailedChildLeaf {
    public function __destruct() { echo 'leaf-dtor|'; }
}
class FailedParentOwner {
    public $leaf;
    public function __construct() {
        $this->leaf = new FailedChildLeaf();
        throw new Exception('parent');
    }
    public function __destruct() { echo 'owner-dtor|'; }
}

try {
    new FailedParentOwner();
} catch (Throwable $error) {
    echo 'caught|';
}
echo 'done';
"#
        ),
        "leaf-dtor|caught|done"
    );
}

#[test]
fn abandoned_temporary_destructor_can_replace_constructor_exception_before_catch() {
    assert_eq!(
        run_php(
            r#"<?php
class OriginalConstructionFailure extends Exception {}
class TemporaryCleanupFailure extends Exception {}
class ThrowingTemporary {
    public function __destruct() {
        echo 'temporary-dtor|';
        throw new TemporaryCleanupFailure('cleanup');
    }
}
class FailedConstruction {
    public function __construct() {
        echo 'failed-ctor|';
        throw new OriginalConstructionFailure('original');
    }
}
function consumeTemporaries($first, $second) {}

try {
    consumeTemporaries(new ThrowingTemporary(), new FailedConstruction());
} catch (OriginalConstructionFailure $error) {
    echo 'caught-original|';
} catch (TemporaryCleanupFailure $error) {
    echo 'caught-cleanup:', $error->getPrevious()->getMessage(), '|';
}
echo 'done';
"#
        ),
        "failed-ctor|temporary-dtor|caught-cleanup:original|done"
    );
}

#[test]
fn constructor_validation_inheritance_and_unpack_share_lifecycle_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
class ValidatedFailure {
    public function __construct(int $value) {
        echo 'validated-body|';
        throw new Exception('body');
    }
    public function __destruct() { echo 'validated-dtor|'; }
}
class InheritedFailure {
    public function __construct() {
        echo 'inherited-body|';
        throw new Exception('inherited');
    }
    public function __destruct() { echo 'inherited-dtor|'; }
}
class InheritedFailureChild extends InheritedFailure {}
class SpreadComplete {
    public $value;
    public function __construct(int $value) { $this->value = $value; }
    public function __destruct() { echo 'spread-complete:', $this->value, '|'; }
}

try {
    new ValidatedFailure('wrong');
} catch (TypeError $error) {
    echo 'validated-type|';
}
try {
    new ValidatedFailure(...[4]);
} catch (Exception $error) {
    echo 'spread-failed|';
}
try {
    new InheritedFailureChild();
} catch (Exception $error) {
    echo 'inherited-caught|';
}
$complete = new SpreadComplete(...[9]);
$complete = null;
echo 'done';
"#
        ),
        concat!(
            "validated-type|validated-body|spread-failed|",
            "inherited-body|inherited-caught|spread-complete:9|done"
        )
    );
}
