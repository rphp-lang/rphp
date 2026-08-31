mod common;

use common::run_php;

#[test]
fn restricted_properties_distinguish_nested_object_mutation_from_storage_writes() {
    assert_eq!(
        run_php(
            r#"<?php
function attempt(string $label, Closure $operation): void {
    echo $label, ':';
    try {
        $value = $operation();
        if ($value === null) {
            echo "ok\n";
        } else {
            var_dump($value);
        }
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage(), "\n";
    }
}

class CapabilityNode {
    public int $count = 1;
    public array $items = [];
}

class LockedBox {
    public private(set) int $scalar;
    public private(set) array $items;
    public private(set) CapabilityNode $node;

    public function initialize(): void {
        $this->scalar = 1;
        $this->items = [];
        $this->node = new CapabilityNode();
    }
}

$locked = new LockedBox();
$locked->initialize();
attempt('direct', static fn() => $locked->scalar = 2);
attempt('array', static fn() => $locked->items[] = 1);
attempt('nested', static function () use ($locked) {
    $locked->node->count++;
    return $locked->node->count;
});

$empty = new LockedBox();
attempt('uninitialized-write', static fn() => $empty->node->count = 2);
attempt('uninitialized-unset', static function () use ($empty) {
    unset($empty->items[0]);
});
attempt('uninitialized-nested-unset', static function () use ($empty) {
    unset($empty->node->items[0]);
});

function unset_items(LockedBox $box): void { unset($box->items[0]); }
attempt('cache-unset-initialized', static fn() => unset_items($locked));
attempt('cache-unset-uninitialized', static fn() => unset_items($empty));

function property_name(string $name): string {
    echo "name:$name\n";
    return $name;
}
attempt('ordered-uninitialized-unset', static function () use ($empty) {
    unset($empty->{property_name('node')}->{property_name('items')}[property_name('leaf')]);
});

class NullableLockedBox { public private(set) ?object $node = null; }
$nullable = new NullableLockedBox();
attempt('ordered-null-unset', static function () use ($nullable) {
    unset($nullable->{property_name('node')}->{property_name('items')}[property_name('leaf')]);
});
attempt('ordered-direct-unset', static function () use ($nullable) {
    unset($nullable->{property_name('node')}->{property_name('child')});
});

class ReadonlyBox {
    public readonly object $node;
    public readonly array $items;

    public function __construct() {
        $this->node = (object) ['count' => 1];
        $this->items = [];
    }
}

$readonly = new ReadonlyBox();
attempt('readonly-nested', static function () use ($readonly) {
    $readonly->node->count++;
    return $readonly->node->count;
});
attempt('readonly-array', static fn() => $readonly->items[] = 1);

for ($iteration = 0; $iteration < 2; $iteration++) {
    attempt("warm-$iteration", static fn() => $locked->scalar++);
}
"#,
        ),
        concat!(
            "direct:Error:Cannot modify private(set) property LockedBox::$scalar from global scope\n",
            "array:Error:Cannot indirectly modify private(set) property LockedBox::$items from global scope\n",
            "nested:int(2)\n",
            "uninitialized-write:Error:Cannot indirectly modify private(set) property LockedBox::$node from global scope\n",
            "uninitialized-unset:ok\n",
            "uninitialized-nested-unset:ok\n",
            "cache-unset-initialized:Error:Cannot indirectly modify private(set) property LockedBox::$items from global scope\n",
            "cache-unset-uninitialized:ok\n",
            "ordered-uninitialized-unset:name:node\n",
            "name:items\n",
            "name:leaf\n",
            "ok\n",
            "ordered-null-unset:name:node\n",
            "name:items\n",
            "name:leaf\n",
            "Error:Cannot indirectly modify private(set) property NullableLockedBox::$node from global scope\n",
            "ordered-direct-unset:name:node\n",
            "name:child\n",
            "Error:Cannot indirectly modify private(set) property NullableLockedBox::$node from global scope\n",
            "readonly-nested:int(2)\n",
            "readonly-array:Error:Cannot indirectly modify readonly property ReadonlyBox::$items\n",
            "warm-0:Error:Cannot modify private(set) property LockedBox::$scalar from global scope\n",
            "warm-1:Error:Cannot modify private(set) property LockedBox::$scalar from global scope\n",
        ),
    );
}

#[test]
fn restricted_incdec_preserves_uninitialized_and_object_operator_precedence() {
    assert_eq!(
        run_php(
            r#"<?php
class IncrementBox {
    public readonly int $readonlyUninitialized;
    public readonly object $readonlyObject;
    public private(set) int $privateUninitialized;
    public private(set) object $privateObject;

    public function __construct() {
        $this->readonlyObject = new stdClass();
        $this->privateObject = new stdClass();
    }
}

foreach (['readonlyUninitialized', 'readonlyObject', 'privateUninitialized', 'privateObject'] as $property) {
    $object = new IncrementBox();
    foreach (['post', 'pre', 'compound'] as $operation) {
        try {
            if ($operation === 'post') {
                $object->$property++;
            } elseif ($operation === 'pre') {
                ++$object->$property;
            } else {
                $object->$property += 1;
            }
            echo "$property:$operation:ok\n";
        } catch (Throwable $error) {
            echo "$property:$operation:", $error::class, ':', $error->getMessage(), "\n";
        }
    }
}
"#,
        ),
        concat!(
            "readonlyUninitialized:post:Error:Typed property IncrementBox::$readonlyUninitialized must not be accessed before initialization\n",
            "readonlyUninitialized:pre:Error:Typed property IncrementBox::$readonlyUninitialized must not be accessed before initialization\n",
            "readonlyUninitialized:compound:Error:Typed property IncrementBox::$readonlyUninitialized must not be accessed before initialization\n",
            "readonlyObject:post:Error:Cannot modify readonly property IncrementBox::$readonlyObject\n",
            "readonlyObject:pre:Error:Cannot modify readonly property IncrementBox::$readonlyObject\n",
            "readonlyObject:compound:TypeError:Unsupported operand types: stdClass + int\n",
            "privateUninitialized:post:Error:Typed property IncrementBox::$privateUninitialized must not be accessed before initialization\n",
            "privateUninitialized:pre:Error:Typed property IncrementBox::$privateUninitialized must not be accessed before initialization\n",
            "privateUninitialized:compound:Error:Typed property IncrementBox::$privateUninitialized must not be accessed before initialization\n",
            "privateObject:post:Error:Cannot modify private(set) property IncrementBox::$privateObject from global scope\n",
            "privateObject:pre:Error:Cannot modify private(set) property IncrementBox::$privateObject from global scope\n",
            "privateObject:compound:TypeError:Unsupported operand types: stdClass + int\n",
        ),
    );
}

#[test]
fn restricted_object_references_are_detached_but_preserve_object_identity() {
    assert_eq!(
        run_php(
            r#"<?php
class Node { public int $count = 1; }

class ReferenceBox {
    public private(set) Node $node;
    public private(set) array $items;

    public function __construct() { $this->node = new Node(); }
    public function bind(array &$items): void { $this->items =& $items; }
}

$box = new ReferenceBox();
$reference =& $box->node;
$reference->count = 2;
echo 'shared-object:', $box->node->count, "\n";
$reference = new Node();
$reference->count = 9;
echo 'detached-storage:', $box->node->count, ':', $reference->count, "\n";

function replace_view(Node &$view): void {
    $view->count = 4;
    $view = new Node();
    $view->count = 7;
}
replace_view($box->node);
echo 'argument-detached:', $box->node->count, "\n";

class ReadonlyReferenceBox {
    public readonly object $node;
    public function __construct() { $this->node = (object) ['count' => 1]; }
}

$readonly = new ReadonlyReferenceBox();
$readonlyReference =& $readonly->node;
$readonlyReference->count = 3;
$readonlyReference = (object) ['count' => 8];
echo 'readonly-detached:', $readonly->node->count, ':', $readonlyReference->count, "\n";

$items = [];
$box->bind($items);
try {
    $box->items[] = 1;
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}
$items[] = 2;
echo 'reference-storage:', count($box->items), ':', $box->items[0], "\n";

class PublicInitialization {
    public public(set) readonly int $open;
    public readonly int $closed;
}

$initialization = new PublicInitialization();
$initialization->open = 7;
echo 'public-init:', $initialization->open, "\n";
try {
    $initialization->closed = 8;
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "shared-object:2\n",
            "detached-storage:2:9\n",
            "argument-detached:4\n",
            "readonly-detached:3:8\n",
            "Cannot indirectly modify private(set) property ReferenceBox::$items from global scope\n",
            "reference-storage:1:2\n",
            "public-init:7\n",
            "Cannot modify protected(set) readonly property PublicInitialization::$closed from global scope\n",
        ),
    );
}

#[test]
fn nested_unset_preserves_magic_dispatch_and_missing_property_state() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) {
    echo "diag:$severity:$message\n";
    return true;
});

class PlainBox {}
$plain = new PlainBox();
unset($plain->missing->child);
unset($plain->items[0]);
echo 'plain-state:', (int) ($plain->missing === null), ':', (int) ($plain->items === null), "\n";

class SetterOnly {
    public function __set($name, $value) { echo "set:$name\n"; }
}
$setter = new SetterOnly();
unset($setter->missing->child);
echo json_encode($setter), "\n";

class GetterOnly {
    public function __get($name) { echo "get:$name\n"; return null; }
}
$getter = new GetterOnly();
unset($getter->missing->child);
echo json_encode($getter), "\n";

class MagicLocked {
    public private(set) int $value = 1;
    public function release(): void { unset($this->value); }
    public function __unset($name): void { echo "unset:$name\n"; }
}
$magic = new MagicLocked();
$magic->release();
unset($magic->value);

class LockedWithoutMagic {
    public private(set) int $value = 1;
    public function release(): void { unset($this->value); }
}
$locked = new LockedWithoutMagic();
$locked->release();
try {
    unset($locked->value);
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "diag:8192:Creation of dynamic property PlainBox::$missing is deprecated\n",
            "diag:8192:Creation of dynamic property PlainBox::$items is deprecated\n",
            "plain-state:1:1\n",
            "diag:8192:Creation of dynamic property SetterOnly::$missing is deprecated\n",
            "{\"missing\":null}\n",
            "get:missing\n",
            "diag:8:Indirect modification of overloaded property GetterOnly::$missing has no effect\n",
            "{}\n",
            "unset:value\n",
            "Cannot unset private(set) property LockedWithoutMagic::$value from global scope\n",
        ),
    );
}

#[test]
fn internal_property_argument_references_do_not_escape_through_object_vars() {
    assert_eq!(
        run_php(
            r#"<?php
class PropertyArgumentBox {
    public array $ordinary = ['first'];
    public $linked;
}

$box = new PropertyArgumentBox();
array_pop($box->ordinary);
$snapshot = get_object_vars($box);
$box->ordinary[] = 'later';
echo 'ordinary:', count($snapshot['ordinary']), ':', $box->ordinary[0], "\n";

$external = ['first'];
$box->linked =& $external;
$linkedSnapshot = get_object_vars($box);
$box->linked[] = 'shared';
echo 'linked:', implode(',', $linkedSnapshot['linked']), "\n";
"#,
        ),
        concat!("ordinary:0:later\n", "linked:first,shared\n"),
    );
}
