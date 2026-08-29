mod common;

use common::run_php;

#[test]
fn unset_typed_magic_reads_coerce_or_reject_before_exposing_the_result() {
    assert_eq!(
        run_php(
            r#"<?php
class AccessProbe {
    public int $plain;
    public int $weakMagic;
    public int $badMagic;
    public $source = "41";

    public function &__get(string $name): mixed {
        echo "get:$name\n";
        if ($name === 'badMagic') {
            $this->source = 'bad';
        }
        return $this->source;
    }
}

$probe = new AccessProbe();
try { var_dump($probe->plain); }
catch (Error $error) { echo "plain:", $error->getMessage(), "\n"; }
try { $alias =& $probe->plain; }
catch (Error $error) { echo "plain-ref:", $error->getMessage(), "\n"; }

unset($probe->weakMagic, $probe->badMagic);
var_dump($probe->weakMagic);
var_dump($probe->source);
try { var_dump($probe->badMagic); }
catch (TypeError $error) { echo "bad:", $error->getMessage(), "\n"; }
var_dump($probe);
"#,
        ),
        concat!(
            "plain:Typed property AccessProbe::$plain must not be accessed before initialization\n",
            "plain-ref:Cannot access uninitialized non-nullable property AccessProbe::$plain by reference\n",
            "get:weakMagic\nint(41)\nint(41)\nget:badMagic\n",
            "bad:Value of type string returned from AccessProbe::__get() must be compatible with unset property AccessProbe::$badMagic of type int\n",
            "object(AccessProbe)#1 (1) {\n",
            "  [\"plain\"]=>\n  uninitialized(int)\n",
            "  [\"weakMagic\"]=>\n  uninitialized(int)\n",
            "  [\"badMagic\"]=>\n  uninitialized(int)\n",
            "  [\"source\"]=>\n  string(3) \"bad\"\n}\n",
        )
    );
}

#[test]
fn stringable_typed_assignment_is_transactional_across_reentrant_writes() {
    assert_eq!(
        run_php(
            r#"<?php
class StringSlot {
    public string $value = 'initial';
}
class ReentrantText {
    public function __construct(private StringSlot $owner) {}
    public function __toString(): string {
        echo "convert\n";
        $this->owner->value = 'inner';
        return 'outer';
    }
}
class InvalidText {}
class ThrowingText {
    public function __toString(): string {
        echo "throw\n";
        throw new RuntimeException('string-failed');
    }
}

$slot = new StringSlot();
$slot->value = new ReentrantText($slot);
var_dump($slot);
$alias =& $slot->value;
$alias = new ReentrantText($slot);
var_dump($slot, $alias);
try { $slot->value = new InvalidText(); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
var_dump($slot);
try { $alias = new ThrowingText(); }
catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(),
        '|prev=', $error->getPrevious()?->getMessage() ?? 'none', "\n";
}
var_dump($slot, $alias);
"#,
        ),
        concat!(
            "convert\nobject(StringSlot)#1 (1) {\n  [\"value\"]=>\n  string(5) \"outer\"\n}\n",
            "convert\nobject(StringSlot)#1 (1) {\n  [\"value\"]=>\n  &string(5) \"outer\"\n}\n",
            "string(5) \"outer\"\n",
            "Cannot assign InvalidText to property StringSlot::$value of type string\n",
            "object(StringSlot)#1 (1) {\n  [\"value\"]=>\n  &string(5) \"outer\"\n}\n",
            "throw\nTypeError:Cannot assign ThrowingText to reference held by property StringSlot::$value of type string|prev=string-failed\n",
            "object(StringSlot)#1 (1) {\n  [\"value\"]=>\n  &string(5) \"outer\"\n}\n",
            "string(5) \"outer\"\n",
        )
    );
}

#[test]
fn stringable_conversion_reports_a_released_property_receiver_without_panicking() {
    assert_eq!(
        run_php(
            r#"<?php
class ReleasedSlot {
    public string $value;
}
class ReleasingText {
    public function __toString(): string {
        global $slot;
        $slot = null;
        return 'converted';
    }
}

$slot = new ReleasedSlot();
try { $slot->value = new ReleasingText(); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
var_dump($slot);
"#,
        ),
        concat!(
            "Object was released while assigning to property ReleasedSlot::$value\n",
            "NULL\n",
        )
    );
}

#[test]
fn shared_typed_references_require_one_consistent_canonical_conversion() {
    assert_eq!(
        run_php(
            r#"<?php
class ConstraintPair {
    public int|string $left;
    public float|string $right;
}

$pair = new ConstraintPair();
$shared = 'stable';
$pair->left =& $shared;
$pair->right =& $shared;
foreach ([42, 42.5, 'ok'] as $candidate) {
    try { $shared = $candidate; }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
    var_dump($shared, $candidate);
}
var_dump($pair);

$pair2 = new ConstraintPair();
$pair2->left = 42;
try { $pair2->right =& $pair2->left; }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
var_dump(isset($pair2->right), $pair2->left);
"#,
        ),
        concat!(
            "Cannot assign int to reference held by property ConstraintPair::$left of type string|int and property ConstraintPair::$right of type string|float, as this would result in an inconsistent type conversion\n",
            "string(6) \"stable\"\nint(42)\n",
            "Cannot assign float to reference held by property ConstraintPair::$left of type string|int and property ConstraintPair::$right of type string|float, as this would result in an inconsistent type conversion\n",
            "string(6) \"stable\"\nfloat(42.5)\n",
            "string(2) \"ok\"\nstring(2) \"ok\"\n",
            "object(ConstraintPair)#1 (2) {\n",
            "  [\"left\"]=>\n  &string(2) \"ok\"\n",
            "  [\"right\"]=>\n  &string(2) \"ok\"\n}\n",
            "Reference with value of type int held by property ConstraintPair::$left of type string|int is not compatible with property ConstraintPair::$right of type string|float\n",
            "bool(false)\nint(42)\n",
        )
    );
}

#[test]
fn unset_declared_compound_write_and_class_constraints_preserve_state_order() {
    assert_eq!(
        run_php(
            r#"<?php
class StateProbe {
    public $count = 1;
    public MissingContract $object;
}

spl_autoload_register(function (string $class): void {
    echo "autoload:$class\n";
});

$probe = new StateProbe();
unset($probe->count);
set_error_handler(function (int $severity, string $message): bool {
    echo "warning:$message\n";
    return true;
});
$probe->count += 3;
restore_error_handler();
var_dump($probe->count);

try { $probe->object = new stdClass(); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
var_dump($probe);
"#,
        ),
        concat!(
            "warning:Undefined property: StateProbe::$count\nint(3)\n",
            "Cannot assign stdClass to property StateProbe::$object of type MissingContract\n",
            "object(StateProbe)#2 (1) {\n",
            "  [\"count\"]=>\n  int(3)\n",
            "  [\"object\"]=>\n  uninitialized(MissingContract)\n}\n",
        )
    );
}
