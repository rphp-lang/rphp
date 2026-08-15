mod common;
use common::run_php;

#[test]
fn test_tostring_echo() {
    assert_eq!(
        run_php(
            r#"<?php
class Money {
    private int $cents;
    public function __construct(int $cents) {
        $this->cents = $cents;
    }
    public function __toString(): string {
        return "USD:" . $this->cents;
    }
}
$m = new Money(1550);
echo $m;
"#
        ),
        "USD:1550"
    );
}

#[test]
fn test_tostring_concat() {
    assert_eq!(
        run_php(
            r#"<?php
class Tag {
    private string $name;
    public function __construct(string $name) { $this->name = $name; }
    public function __toString(): string { return "<" . $this->name . ">"; }
}
$t = new Tag("div");
echo "HTML: " . $t;
"#
        ),
        "HTML: <div>"
    );
}

#[test]
fn test_get_set() {
    assert_eq!(
        run_php(
            r#"<?php
class Bag {
    private $data;
    public function __construct() {
        $this->data = [];
    }
    public function __get($name) {
        return $this->data[$name] ?? "none";
    }
    public function __set($name, $value) {
        $this->data[$name] = $value;
    }
}
$b = new Bag();
$b->color = "red";
echo $b->color;
"#
        ),
        "red"
    );
}

#[test]
fn test_get_undefined_property() {
    assert_eq!(
        run_php(
            r#"<?php
class Flex {
    public function __get($name) {
        return "default_" . $name;
    }
}
$f = new Flex();
echo $f->whatever;
"#
        ),
        "default_whatever"
    );
}

#[test]
fn missing_properties_warn_after_magic_resolution_and_silent_reads_stay_silent() {
    assert_eq!(
        run_php(
            r#"<?php
class PlainPropertyBag {}
$bag = new PlainPropertyBag();
var_dump($bag->missing);
var_dump(isset($bag->missing));
var_dump(@$bag->suppressed);
set_error_handler(function($code, $message) { echo "handled:$code:$message\n"; return true; });
var_dump($bag->handled);
class MagicPropertyBag { public function __get($name) { return "magic:$name"; } }
var_dump((new MagicPropertyBag())->missing);
"#
        ),
        "\nWarning: Undefined property: PlainPropertyBag::$missing in <main> on line 4\nNULL\nbool(false)\nNULL\nhandled:2:Undefined property: PlainPropertyBag::$handled\nNULL\nstring(13) \"magic:missing\"\n"
    );
}

#[test]
fn scalar_property_reads_warn_and_return_null_outside_silent_contexts() {
    assert_eq!(
        run_php(
            r#"<?php
$null = null;
$bool = true;
$int = 1;
$string = 'value';
var_dump($null->missing);
var_dump($bool->missing);
var_dump($int->{1});
var_dump($string->missing);
var_dump(isset($null->missing));
var_dump($null?->missing);
var_dump(@$bool->suppressed);
set_error_handler(function($code, $message) { echo "handled:$code:$message\n"; return true; });
var_dump($int->handled);
"#
        ),
        "\nWarning: Attempt to read property \"missing\" on null in <main> on line 6\nNULL\n\nWarning: Attempt to read property \"missing\" on bool in <main> on line 7\nNULL\n\nWarning: Attempt to read property \"1\" on int in <main> on line 8\nNULL\n\nWarning: Attempt to read property \"missing\" on string in <main> on line 9\nNULL\nbool(false)\nNULL\nNULL\nhandled:2:Attempt to read property \"handled\" on int\nNULL\n"
    );
}

#[test]
fn scalar_property_assignment_throws_after_rhs_evaluation_without_mutating_receiver() {
    assert_eq!(
        run_php(
            r#"<?php
function assignment_value() { echo "rhs\n"; return 9; }
foreach ([null, false, 12, 's'] as $value) {
    try {
        $value->{7} = assignment_value();
    } catch (Error $error) {
        echo $error->getMessage(), "\n";
    }
    var_dump($value);
}
$object = new stdClass();
$object->stored = assignment_value();
var_dump($object->stored);
"#
        ),
        "rhs\nAttempt to assign property \"7\" on null\nNULL\nrhs\nAttempt to assign property \"7\" on bool\nbool(false)\nrhs\nAttempt to assign property \"7\" on int\nint(12)\nrhs\nAttempt to assign property \"7\" on string\nstring(1) \"s\"\nrhs\nint(9)\n"
    );
}

#[test]
fn scalar_property_modification_throws_for_references_and_nested_writes() {
    assert_eq!(
        run_php(
            r#"<?php
function modification_value() { echo "rhs\n"; return 9; }
foreach ([null, true, 12, 's'] as $value) {
    $source = 1;
    try { $value->{7} =& $source; } catch (Error $error) { echo $error->getMessage(), "\n"; }
    try { $destination =& $value->{7}; } catch (Error $error) { echo $error->getMessage(), "\n"; }
    try { $value->{7}[0] = modification_value(); } catch (Error $error) { echo $error->getMessage(), "\n"; }
    try { $value->{7}->nested = modification_value(); } catch (Error $error) { echo $error->getMessage(), "\n"; }
    var_dump($value);
}
"#
        ),
        "Attempt to modify property \"7\" on null\nAttempt to modify property \"7\" on null\nrhs\nAttempt to modify property \"7\" on null\nrhs\nAttempt to modify property \"7\" on null\nNULL\nAttempt to modify property \"7\" on bool\nAttempt to modify property \"7\" on bool\nrhs\nAttempt to modify property \"7\" on bool\nrhs\nAttempt to modify property \"7\" on bool\nbool(true)\nAttempt to modify property \"7\" on int\nAttempt to modify property \"7\" on int\nrhs\nAttempt to modify property \"7\" on int\nrhs\nAttempt to modify property \"7\" on int\nint(12)\nAttempt to modify property \"7\" on string\nAttempt to modify property \"7\" on string\nrhs\nAttempt to modify property \"7\" on string\nrhs\nAttempt to modify property \"7\" on string\nstring(1) \"s\"\n"
    );
}

#[test]
fn scalar_property_increment_and_decrement_throw_without_mutating_receiver() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([null, true, 12, 's'] as $value) {
    try { ++$value->{7}; } catch (Error $error) { echo $error->getMessage(), "\n"; }
    try { $value->{7}++; } catch (Error $error) { echo $error->getMessage(), "\n"; }
    try { --$value->{7}; } catch (Error $error) { echo $error->getMessage(), "\n"; }
    try { $value->{7}--; } catch (Error $error) { echo $error->getMessage(), "\n"; }
    var_dump($value);
}
$object = (object) ['stored' => 1];
var_dump(++$object->stored, $object->stored++, $object->stored);
"#
        ),
        "Attempt to increment/decrement property \"7\" on null\nAttempt to increment/decrement property \"7\" on null\nAttempt to increment/decrement property \"7\" on null\nAttempt to increment/decrement property \"7\" on null\nNULL\nAttempt to increment/decrement property \"7\" on bool\nAttempt to increment/decrement property \"7\" on bool\nAttempt to increment/decrement property \"7\" on bool\nAttempt to increment/decrement property \"7\" on bool\nbool(true)\nAttempt to increment/decrement property \"7\" on int\nAttempt to increment/decrement property \"7\" on int\nAttempt to increment/decrement property \"7\" on int\nAttempt to increment/decrement property \"7\" on int\nint(12)\nAttempt to increment/decrement property \"7\" on string\nAttempt to increment/decrement property \"7\" on string\nAttempt to increment/decrement property \"7\" on string\nAttempt to increment/decrement property \"7\" on string\nstring(1) \"s\"\nint(2)\nint(2)\nint(3)\n"
    );
}

#[test]
fn recursive_get_is_guarded_per_object_and_property() {
    assert_eq!(
        run_php(
            r#"<?php
class RecursiveGet {
    public function __get($name) {
        echo "get:$name\n";
        if ($name === 'first') {
            var_dump($this->{$name . ''});
            var_dump($this->second);
        }
    }
}
$object = new RecursiveGet();
var_dump($object->first);
"#
        ),
        "get:first\n\nWarning: Undefined property: RecursiveGet::$first in RecursiveGet::__get on line 6\nNULL\nget:second\nNULL\nNULL\n"
    );
}

#[test]
fn recursive_set_writes_dynamic_property_without_reentering_setter() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class RecursiveSet {
    public function __set($name, $value) {
        echo "set:$name\n";
        $this->$name = $value;
    }
}
$object = new RecursiveSet();
$object->answer = 42;
var_dump($object->answer);
"#
        ),
        "set:answer\nint(42)\n"
    );
}

#[test]
fn recursive_isset_is_guarded_without_suppressing_get() {
    assert_eq!(
        run_php(
            r#"<?php
class RecursiveIsset {
    public function __isset($name) {
        echo "isset:$name\n";
        var_dump(isset($this->$name));
        return true;
    }
    public function __get($name) {
        echo "get:$name\n";
        return 7;
    }
}
$object = new RecursiveIsset();
var_dump(isset($object->value));
"#
        ),
        "isset:value\nbool(false)\nbool(true)\n"
    );
}

#[test]
fn recursive_unset_is_guarded_per_object_and_property() {
    assert_eq!(
        run_php(
            r#"<?php
class RecursiveUnset {
    public function __unset($name) {
        echo "unset:$name\n";
        unset($this->$name);
        if ($name === 'first') {
            unset($this->second);
        }
    }
}
$object = new RecursiveUnset();
unset($object->first);
"#
        ),
        "unset:first\nunset:second\n"
    );
}

#[test]
fn magic_property_guard_is_released_after_exception() {
    assert_eq!(
        run_php(
            r#"<?php
class ThrowingGet {
    public function __get($name) {
        echo "get:$name\n";
        throw new Exception('boom');
    }
}
$object = new ThrowingGet();
for ($attempt = 0; $attempt < 2; $attempt++) {
    try { $object->value; } catch (Exception $error) {}
}
"#
        ),
        "get:value\nget:value\n"
    );
}

#[test]
fn inaccessible_declared_properties_use_magic_fallback() {
    assert_eq!(
        run_php(
            r#"<?php
class MagicVisibility {
    protected $hidden;
    public function __get($name) { echo "get:$name\n"; return $this->$name; }
    public function __set($name, $value) { echo "set:$name\n"; $this->$name = $value; }
}
$object = new MagicVisibility();
$object->hidden = 42;
var_dump($object->hidden);
"#
        ),
        "set:hidden\nget:hidden\nint(42)\n"
    );
}

#[test]
fn recursive_magic_access_to_nul_property_throws_catchable_error() {
    assert_eq!(
        run_php(
            r#"<?php
class NulProperty {
    public function __set($name, $value) { $this->$name = $value; }
    public function __get($name) { return $this->$name; }
}
$object = new NulProperty();
foreach (['write', 'read'] as $operation) {
    try {
        if ($operation === 'write') { $object->{"\0"} = 2; }
        else { $object->{"\0"}; }
    } catch (Error $error) { echo $error->getMessage(), "\n"; }
}
"#
        ),
        "Cannot access property starting with \"\\0\"\nCannot access property starting with \"\\0\"\n"
    );
}

#[test]
fn direct_nul_property_access_throws_without_magic_methods() {
    assert_eq!(
        run_php(
            r#"<?php
$object = new stdClass();
foreach (['write', 'read'] as $operation) {
    try {
        if ($operation === 'write') { $object->{"\0"} = 2; }
        else { $object->{"\0"}; }
    } catch (Error $error) { echo $error->getMessage(), "\n"; }
}
"#
        ),
        "Cannot access property starting with \"\\0\"\nCannot access property starting with \"\\0\"\n"
    );
}

#[test]
fn test_invoke() {
    assert_eq!(
        run_php(
            r#"<?php
class Multiplier {
    private int $factor;
    public function __construct(int $factor) { $this->factor = $factor; }
    public function __invoke(int $x): int { return $this->factor * $x; }
}
$double = new Multiplier(2);
echo $double(21);
"#
        ),
        "42"
    );
}

#[test]
fn test_invoke_with_closure_like_usage() {
    assert_eq!(
        run_php(
            r#"<?php
class Greeter {
    private string $greeting;
    public function __construct(string $greeting) { $this->greeting = $greeting; }
    public function __invoke(string $name): string { return $this->greeting . " " . $name; }
}
$hi = new Greeter("Hello");
echo $hi("World");
"#
        ),
        "Hello World"
    );
}

#[test]
fn wide_invokable_frame_initializes_hidden_receiver_before_overwrite_tracking() {
    assert_eq!(
        run_php(
            r#"<?php
class WideInvoker {
    public function __invoke(string $value = "default", string $suffix = ""): string {
        $v00 = 0; $v01 = 1; $v02 = 2; $v03 = 3; $v04 = 4;
        $v05 = 5; $v06 = 6; $v07 = 7; $v08 = 8; $v09 = 9;
        $v10 = 10; $v11 = 11; $v12 = 12; $v13 = 13; $v14 = 14;
        $v15 = 15; $v16 = 16; $v17 = 17; $v18 = 18; $v19 = 19;
        $v20 = 20; $v21 = 21; $v22 = 22; $v23 = 23; $v24 = 24;
        $v25 = 25; $v26 = 26; $v27 = 27; $v28 = 28; $v29 = 29;
        $v30 = 30; $v31 = 31; $v32 = 32; $v33 = 33; $v34 = 34;
        $v35 = 35; $v36 = 36; $v37 = 37; $v38 = 38; $v39 = 39;
        $v40 = 40; $v41 = 41; $v42 = 42; $v43 = 43; $v44 = 44;
        $v45 = 45; $v46 = 46; $v47 = 47; $v48 = 48; $v49 = 49;
        $v50 = 50; $v51 = 51; $v52 = 52; $v53 = 53; $v54 = 54;
        $v55 = 55; $v56 = 56; $v57 = 57; $v58 = 58; $v59 = 59;
        $v60 = 60; $v61 = 61; $v62 = 62; $v63 = 63; $v64 = 64;
        return $value . $suffix . ':' . $v64;
    }
}
$invoke = new WideInvoker();
echo $invoke(), '|', $invoke('positional'), '|', $invoke(value: 'named'), '|';
echo $invoke('mixed', suffix: '-named');
"#
        ),
        "default:64|positional:64|named:64|mixed-named:64"
    );
}
