mod common;

use common::run_php;

#[test]
fn asymmetric_writes_and_recursive_magic_guards_keep_declared_visibility() {
    assert_eq!(
        run_php(
            r#"<?php
class LockedLedger {
    public private(set) int $count;

    public function initialize(int $value) { $this->count = $value; }
    public function release() { unset($this->count); }
    public function __set($name, $value) { echo "magic-set:$name:$value\n"; }
}

$ledger = new LockedLedger;
foreach ([1, 2] as $value) {
    try { $ledger->count = $value; }
    catch (Error $error) { echo $error->getMessage(), "\n"; }
    if ($value === 1) { $ledger->initialize(10); }
}
$ledger->release();
$ledger->count = 3;

function guarded_set($object) { $object->payload = 1; }
function guarded_get($object) { return $object->payload; }
function guarded_unset($object) { unset($object->payload); }

class GuardedBox {
    private int $payload;
    public function __set($name, $value) { guarded_set($this); }
    public function __get($name) { return guarded_get($this); }
    public function __unset($name) { guarded_unset($this); }
}

$box = new GuardedBox;
foreach ([
    function () use ($box) { $box->payload = 2; },
    function () use ($box) { return $box->payload; },
    function () use ($box) { unset($box->payload); },
] as $operation) {
    try { $operation(); }
    catch (Error $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "Cannot modify private(set) property LockedLedger::$count from global scope\n",
            "Cannot modify private(set) property LockedLedger::$count from global scope\n",
            "magic-set:count:3\n",
            "Cannot access private property GuardedBox::$payload\n",
            "Cannot access private property GuardedBox::$payload\n",
            "Cannot access private property GuardedBox::$payload\n",
        ),
    );
}

#[test]
fn missing_ordinary_property_lvalue_defers_creation_without_read_warning() {
    assert_eq!(
        run_php(
            r#"<?php
class Plain {}

set_error_handler(function($severity, $message) {
    echo "diag:$severity:$message\n";
    return true;
});

$plain = new Plain;
$plain->items[] = 1;
$plain->nested['key'] = 2;
echo json_encode($plain), "\n";
"#,
        ),
        concat!(
            "diag:8192:Creation of dynamic property Plain::$items is deprecated\n",
            "diag:8192:Creation of dynamic property Plain::$nested is deprecated\n",
            "{\"items\":[1],\"nested\":{\"key\":2}}\n",
        )
    );
}

#[test]
fn clone_scope_can_unset_and_reinitialize_a_readonly_property() {
    assert_eq!(
        run_php(
            r#"<?php
class CloneableValue {
    public function __construct(public readonly int $value) {}

    public function __clone() {
        unset($this->value);
        var_dump(isset($this->value));
        $this->value = 2;
    }
}

$original = new CloneableValue(1);
$copy = clone $original;
var_dump($original->value, $copy->value);
"#,
        ),
        "bool(false)\nint(1)\nint(2)\n"
    );
}

#[test]
fn indirect_magic_property_write_uses_a_temporary_without_writeback() {
    assert_eq!(
        run_php(
            r#"<?php
class Box {
    public array $events = [];
    public array $storage = ['items' => [1]];

    public function __get($name) {
        $this->events[] = "get:$name";
        return $this->storage[$name];
    }

    public function __set($name, $value) {
        $this->events[] = "set:$name";
        $this->storage[$name] = $value;
    }
}

set_error_handler(function($severity, $message) {
    echo "notice:$severity:$message\n";
    return true;
});

$box = new Box;
$box->items[] = 2;
echo json_encode($box->events), "\n";
echo json_encode($box->storage), "\n";
"#,
        ),
        concat!(
            "notice:8:Indirect modification of overloaded property Box::$items has no effect\n",
            "[\"get:items\"]\n",
            "{\"items\":[1]}\n",
        )
    );
}

#[test]
fn indirect_magic_property_write_preserves_rhs_key_getter_order() {
    assert_eq!(
        run_php(
            r#"<?php
class Box {
    public function __get($name) {
        echo "get:$name\n";
        return [];
    }
}

function make_key() { echo "key\n"; return 'slot'; }
function rhs() { echo "rhs\n"; return 3; }
set_error_handler(function($severity, $message) {
    echo "notice:$severity:$message\n";
    return true;
});

$box = new Box;
$box->items[make_key()] = rhs();
echo "done\n";
"#,
        ),
        concat!(
            "key\n",
            "rhs\n",
            "get:items\n",
            "notice:8:Indirect modification of overloaded property Box::$items has no effect\n",
            "done\n",
        )
    );
}

#[test]
fn indirect_magic_property_reference_mutates_exposed_storage_once() {
    assert_eq!(
        run_php(
            r#"<?php
class Box {
    public array $events = [];
    public array $storage = ['items' => [1]];

    public function &__get($name) {
        $this->events[] = "get:$name";
        return $this->storage[$name];
    }

    public function __set($name, $value) {
        $this->events[] = "set:$name";
    }
}

$box = new Box;
$box->items[] = 2;
$box->items['tail'] = 3;
echo json_encode($box->events), "\n";
echo json_encode($box->storage), "\n";
"#,
        ),
        concat!(
            "[\"get:items\",\"get:items\"]\n",
            "{\"items\":{\"0\":1,\"1\":2,\"tail\":3}}\n",
        )
    );
}

#[test]
fn explicitly_unset_declared_property_dispatches_magic_once_and_survives_clone() {
    assert_eq!(
        run_php(
            r#"<?php
class Ledger {
    public int $count;

    public function release() { unset($this->count); }
    public function __isset($name) { echo "isset:$name\n"; return true; }
    public function __get($name) { echo "get:$name\n"; return 7; }
    public function __set($name, $value) {
        echo "set:$name:$value\n";
        $this->$name = $value;
    }
    public function __unset($name) { echo "unset:$name\n"; }
}

$ledger = new Ledger;
var_dump(isset($ledger->count));
try { var_dump($ledger->count); } catch (Error $error) { echo $error->getMessage(), "\n"; }
$ledger->count = 1;
$ledger->release();
var_dump(isset($ledger->count));
var_dump($ledger->count);
$ledger->count = 2;
var_dump($ledger->count);
$ledger->release();
$copy = clone $ledger;
$copy->count = 3;
var_dump($copy->count);
unset($ledger->count);
"#,
        ),
        concat!(
            "bool(false)\n",
            "Typed property Ledger::$count must not be accessed before initialization\n",
            "isset:count\n",
            "bool(true)\n",
            "get:count\n",
            "int(7)\n",
            "set:count:2\n",
            "int(2)\n",
            "set:count:3\n",
            "int(3)\n",
            "unset:count\n",
        )
    );
}

#[test]
fn failed_typed_write_preserves_explicit_unset_magic_state() {
    assert_eq!(
        run_php(
            r#"<?php
class TypedFallback {
    public int $count;
    public function release() { unset($this->count); }
    public function __get($name) { echo "get:$name\n"; return 9; }
}

$value = new TypedFallback;
$value->release();
$copy = clone $value;
try { $copy->count = []; } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
var_dump($copy->count);
var_dump($value->count);
"#,
        ),
        concat!(
            "Cannot assign array to property TypedFallback::$count of type int\n",
            "get:count\n",
            "int(9)\n",
            "get:count\n",
            "int(9)\n",
        )
    );
}

#[test]
fn explicitly_unset_typed_magic_reference_coerces_the_exposed_alias() {
    assert_eq!(
        run_php(
            r#"<?php
class TypedMagicAlias {
    public $source = '42';
    public int $target;

    public function release() { unset($this->target); }
    public function &__get($name) { return $this->source; }
}

$object = new TypedMagicAlias;
$object->release();
$alias =& $object->target;
var_dump($alias);
$object->source = 'later';
var_dump($alias, isset($object->target));
"#,
        ),
        "int(42)\nstring(5) \"later\"\nbool(false)\n"
    );
}

#[test]
fn recursive_reference_getter_probe_stays_silent_before_parent_fallback() {
    assert_eq!(
        run_php(
            r#"<?php
class ReferenceStore {
    public object $storage;

    public function __construct() { $this->storage = new stdClass; }
    public function &__get($name) {
        if (isset($this->storage->{$name})) {
            $result =& $this->storage->{$name};
            return $result;
        }
        static $missing;
        return $missing;
    }
    public function __set($name, $value) { $this->storage->{$name} = $value; }
    public function __isset($name) { return isset($this->storage->{$name}); }
}

class LayeredReferenceStore extends ReferenceStore {
    public function &__get($name) {
        if (isset($this->settings) && isset($this->settings[$name])) {
            $result =& $this->settings[$name];
            return $result;
        }
        return parent::__get($name);
    }
}

$store = new LayeredReferenceStore;
$store->settings = ['name' => 'Ada'];
var_dump($store->name);
var_dump($store->settings);
"#,
        ),
        concat!(
            "string(3) \"Ada\"\n",
            "array(1) {\n  [\"name\"]=>\n  string(3) \"Ada\"\n}\n",
        )
    );
}
