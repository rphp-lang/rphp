mod common;
use common::{
    run_php, run_php_expect_error, run_php_expect_error_with_source_context,
    run_php_with_source_context,
};

#[test]
fn non_public_magic_methods_warn_at_compile_time() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class HiddenSetter {
    protected function __set($name, $value) {}
    private function __construct() {}
    private function __clone() {}
}
trait HiddenInvoker {
    private function __invoke() {}
}
echo "ok";
"#,
            "magic-visibility.php",
            ".",
        ),
        "\nWarning: The magic method HiddenSetter::__set() must have public visibility in magic-visibility.php on line 3\n\nWarning: The magic method HiddenInvoker::__invoke() must have public visibility in magic-visibility.php on line 8\nok"
    );
}

#[test]
fn magic_dispatch_methods_reject_reference_parameters_in_every_declaration_shape() {
    for (source, file, expected) in [
        (
            "<?php\nnamespace App;\nclass InvalidGetter { public function __get(&$name) {} }",
            "/virtual/magic-class.php",
            "Method App\\InvalidGetter::__get() cannot take arguments by reference in /virtual/magic-class.php on line 3",
        ),
        (
            "<?php\ntrait InvalidSetter {\n    public function __set($name, &$value) {}\n}",
            "/virtual/magic-trait.php",
            "Method InvalidSetter::__set() cannot take arguments by reference in /virtual/magic-trait.php on line 3",
        ),
        (
            "<?php\ninterface InvalidIsset {\n    public function __isset(&$name);\n}",
            "/virtual/magic-interface.php",
            "Method InvalidIsset::__isset() cannot take arguments by reference in /virtual/magic-interface.php on line 3",
        ),
        (
            "<?php\nabstract class InvalidUnset {\n    abstract public function __unset(&$name);\n}",
            "/virtual/magic-abstract.php",
            "Method InvalidUnset::__unset() cannot take arguments by reference in /virtual/magic-abstract.php on line 3",
        ),
        (
            "<?php\nclass InvalidStaticCall {\n    public static function __callStatic($name, &$arguments) {}\n}",
            "/virtual/magic-call-static.php",
            "Method InvalidStaticCall::__callStatic() cannot take arguments by reference in /virtual/magic-call-static.php on line 3",
        ),
        (
            "<?php\nenum InvalidEnumCall {\n    case Value;\n    public function __call(&$name, $arguments) {}\n}",
            "/virtual/magic-enum.php",
            "Method InvalidEnumCall::__call() cannot take arguments by reference in /virtual/magic-enum.php on line 4",
        ),
        (
            "<?php\nclass InvalidUnserialize {\n    public function __unserialize(array &$data): void {}\n}",
            "/virtual/magic-unserialize.php",
            "Method InvalidUnserialize::__unserialize() cannot take arguments by reference in /virtual/magic-unserialize.php on line 3",
        ),
        (
            "<?php\nclass InvalidState {\n    public static function __set_state(array &$properties): object { return new self; }\n}",
            "/virtual/magic-state.php",
            "Method InvalidState::__set_state() cannot take arguments by reference in /virtual/magic-state.php on line 3",
        ),
        (
            "<?php\nclass ReferenceBeforeReturn {\n    public function __get(&$name): void {}\n}",
            "/virtual/magic-precedence.php",
            "Method ReferenceBeforeReturn::__get() cannot take arguments by reference in /virtual/magic-precedence.php on line 3",
        ),
    ] {
        assert_eq!(
            run_php_expect_error_with_source_context(source, file, "/virtual").to_string(),
            expected,
        );
    }

    assert_eq!(
        run_php(
            r#"<?php
class ReferenceParametersAllowed {
    public function __construct(&$value) { echo "construct>"; }
    public function __invoke(&$value) { echo "invoke>"; }
    public function ordinary(&$value) { echo "ordinary>"; }
}
$value = 0;
$callable = new ReferenceParametersAllowed($value);
$callable($value);
$callable->ordinary($value);
"#,
        ),
        "construct>invoke>ordinary>",
    );
}

#[test]
fn magic_method_arity_staticness_and_signature_precedence_match_php() {
    for (source, file, expected) in [
        (
            "<?php\nclass CloneArguments {\n    public function __clone($value) {}\n}",
            "/virtual/magic-clone-arity.php",
            "Method CloneArguments::__clone() cannot take arguments in /virtual/magic-clone-arity.php on line 3",
        ),
        (
            "<?php\ntrait GetterArity {\n    public function __get() {}\n}",
            "/virtual/magic-getter-arity.php",
            "Method GetterArity::__get() must take exactly 1 argument in /virtual/magic-getter-arity.php on line 3",
        ),
        (
            "<?php\nabstract class SetterArity {\n    abstract public function __set($name);\n}",
            "/virtual/magic-setter-arity.php",
            "Method SetterArity::__set() must take exactly 2 arguments in /virtual/magic-setter-arity.php on line 3",
        ),
        (
            "<?php\ninterface StaticCall {\n    public static function __call($name, $arguments);\n}",
            "/virtual/magic-call-staticness.php",
            "Method StaticCall::__call() cannot be static in /virtual/magic-call-staticness.php on line 3",
        ),
        (
            "<?php\ninterface InstanceCallStatic {\n    public function __callStatic($name, $arguments);\n}",
            "/virtual/magic-callstatic-staticness.php",
            "Method InstanceCallStatic::__callStatic() must be static in /virtual/magic-callstatic-staticness.php on line 3",
        ),
        (
            "<?php\nenum StaticInvoke {\n    case Value;\n    public static function __invoke() {}\n}",
            "/virtual/magic-enum-staticness.php",
            "Method StaticInvoke::__invoke() cannot be static in /virtual/magic-enum-staticness.php on line 4",
        ),
        (
            "<?php\nclass InstanceState {\n    public function __set_state($properties): object { return new self; }\n}",
            "/virtual/magic-state-staticness.php",
            "Method InstanceState::__set_state() must be static in /virtual/magic-state-staticness.php on line 3",
        ),
        (
            "<?php\nclass StaticConstructor {\n    public static function __construct() {}\n}",
            "/virtual/magic-constructor-staticness.php",
            "Method StaticConstructor::__construct() cannot be static in /virtual/magic-constructor-staticness.php on line 3",
        ),
        (
            "<?php\nclass ArityFirst {\n    protected static function __toString(&$first, $second): int {}\n}",
            "/virtual/magic-arity-precedence.php",
            "Method ArityFirst::__toString() cannot take arguments in /virtual/magic-arity-precedence.php on line 3",
        ),
        (
            "<?php\nclass ReferenceFirst {\n    public static function __get(&$name): void {}\n}",
            "/virtual/magic-reference-precedence.php",
            "Method ReferenceFirst::__get() cannot take arguments by reference in /virtual/magic-reference-precedence.php on line 3",
        ),
        (
            "<?php\nclass StaticFirst {\n    public static function __get($name): void {}\n}",
            "/virtual/magic-static-precedence.php",
            "Method StaticFirst::__get() cannot be static in /virtual/magic-static-precedence.php on line 3",
        ),
        (
            "<?php\nclass VariadicNeedsFixed {\n    public function __get(&...$names) {}\n}",
            "/virtual/magic-variadic-arity.php",
            "Method VariadicNeedsFixed::__get() must take exactly 1 argument in /virtual/magic-variadic-arity.php on line 3",
        ),
    ] {
        assert_eq!(
            run_php_expect_error_with_source_context(source, file, "/virtual").to_string(),
            expected,
        );
    }

    assert_eq!(
        run_php(
            r#"<?php
class VariadicMagicShapes {
    public function __get($name = null, &...$extra) { return null; }
    public function __sleep(&...$extra): array { return []; }
    public function __destruct(&...$extra) {}
    public function __invoke(&...$arguments) {}
    public static function __set_state($properties, &...$extra): object {
        return new self;
    }
}
echo "ok";
"#,
        ),
        "ok",
    );
}

#[test]
fn magic_parameter_types_accept_supertypes_and_reject_narrowing() {
    for (source, file, expected) in [
        (
            "<?php\nnamespace App;\nclass GetterType { public function __get(int $property) {} }",
            "/virtual/magic-getter-type.php",
            "App\\GetterType::__get(): Parameter #1 ($property) must be of type string when declared in /virtual/magic-getter-type.php on line 3",
        ),
        (
            "<?php\ntrait SetterType {\n    public function __set(Countable $property, $value) {}\n}",
            "/virtual/magic-setter-type.php",
            "SetterType::__set(): Parameter #1 ($property) must be of type string when declared in /virtual/magic-setter-type.php on line 3",
        ),
        (
            "<?php\ninterface CallType {\n    public function __call(string $method, object $arguments);\n}",
            "/virtual/magic-call-type.php",
            "CallType::__call(): Parameter #2 ($arguments) must be of type array when declared in /virtual/magic-call-type.php on line 3",
        ),
        (
            "<?php\nabstract class StaticCallType {\n    abstract public static function __callStatic(bool $method, array $arguments);\n}",
            "/virtual/magic-callstatic-type.php",
            "StaticCallType::__callStatic(): Parameter #1 ($method) must be of type string when declared in /virtual/magic-callstatic-type.php on line 3",
        ),
        (
            "<?php\nenum EnumCallType {\n    case Value;\n    public function __call(string $method, Traversable $arguments) {}\n}",
            "/virtual/magic-enum-type.php",
            "EnumCallType::__call(): Parameter #2 ($arguments) must be of type array when declared in /virtual/magic-enum-type.php on line 4",
        ),
        (
            "<?php\nclass UnserializeType {\n    public function __unserialize(string $payload) {}\n}",
            "/virtual/magic-unserialize-type.php",
            "UnserializeType::__unserialize(): Parameter #1 ($payload) must be of type array when declared in /virtual/magic-unserialize-type.php on line 3",
        ),
        (
            "<?php\nclass StateType {\n    public static function __set_state(object $properties) {}\n}",
            "/virtual/magic-state-type.php",
            "StateType::__set_state(): Parameter #1 ($properties) must be of type array when declared in /virtual/magic-state-type.php on line 3",
        ),
        (
            "<?php\nclass PureNullType {\n    public function __get(null $property) {}\n}",
            "/virtual/magic-null-type.php",
            "PureNullType::__get(): Parameter #1 ($property) must be of type string when declared in /virtual/magic-null-type.php on line 3",
        ),
        (
            "<?php\nclass ReferenceBeforeType {\n    public function __get(int &$property) {}\n}",
            "/virtual/magic-reference-type.php",
            "Method ReferenceBeforeType::__get() cannot take arguments by reference in /virtual/magic-reference-type.php on line 3",
        ),
        (
            "<?php\nclass TypeBeforeReturn {\n    public function __isset(int $property): int {}\n}",
            "/virtual/magic-type-return.php",
            "TypeBeforeReturn::__isset(): Parameter #1 ($property) must be of type string when declared in /virtual/magic-type-return.php on line 3",
        ),
    ] {
        assert_eq!(
            run_php_expect_error_with_source_context(source, file, "/virtual").to_string(),
            expected,
        );
    }

    assert_eq!(
        run_php(
            r#"<?php
class WideMagicParameters {
    public function __get(string|int|null $property) { return null; }
    public function __isset(?string $property): bool { return false; }
    public function __set(string|bool $property, int $value): void {}
    public function __call(string $method, iterable $arguments) {}
    public static function __callStatic(mixed $method, array|Traversable $arguments) {}
    public function __unserialize(?iterable $payload): void {}
    public static function __set_state(array|Traversable $properties): object {
        return new self;
    }
}
echo "ok";
"#,
        ),
        "ok",
    );
}

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
fn compound_property_assignment_defers_fetch_and_uses_scalar_write_errors() {
    assert_eq!(
        run_php(
            r#"<?php
function compound_value($value) { echo "rhs\n"; return $value; }
foreach ([null, true, 12, 's'] as $value) {
    try { $value->{7} += compound_value(3); } catch (Error $error) { echo $error->getMessage(), "\n"; }
    var_dump($value);
}
try { $null = null; $null->bad += compound_value([]); } catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
set_error_handler(function($code, $message) { echo "warning:$message\n"; return true; });
$object = new stdClass();
$object->missing += compound_value(3);
var_dump($object->missing);
function compound_base() { echo "base\n"; return null; }
function compound_name() { echo "name\n"; return 'inner'; }
try { compound_base()->{compound_name()}->nested += compound_value(3); } catch (Error $error) { echo $error->getMessage(), "\n"; }
$valid = (object) ['number' => 2];
var_dump($valid->number += compound_value(3));
"#
        ),
        "rhs\nAttempt to assign property \"7\" on null\nNULL\nrhs\nAttempt to assign property \"7\" on bool\nbool(true)\nrhs\nAttempt to assign property \"7\" on int\nint(12)\nrhs\nAttempt to assign property \"7\" on string\nstring(1) \"s\"\nrhs\nError:Attempt to assign property \"bad\" on null\nrhs\nwarning:Undefined property: stdClass::$missing\nint(3)\nbase\nname\nrhs\nAttempt to modify property \"inner\" on null\nrhs\nint(5)\n"
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
fn overloaded_properties_reject_reference_binding_after_magic_get_resolution() {
    assert_eq!(
        run_php(
            r#"<?php
class ScalarOverload {
    private $hidden = 'private-value';
    public function __get($name) {
        echo "get:$name>";
        if ($name === 'hidden') {
            throw new Exception('blocked');
        }
        if ($name === 'object') {
            return new stdClass;
        }
        return 17;
    }
    public function __set($name, $value) {
        echo "set:$name>";
    }
}
class ReferenceOverload {
    private $slot = 4;
    public function &__get($name) {
        echo "ref-get:$name>";
        return $this->slot;
    }
}
class AsymmetricOverload {
    public private(set) int $guarded = 1;
    public function __get($name) {
        echo "unexpected-asymmetric-get>";
        return null;
    }
}
#[AllowDynamicProperties]
class ExistingDynamic {
    public function __get($name) {
        echo "unexpected-get:$name>";
        return null;
    }
}
function &reference_source(&$slot, $label) {
    echo "source:$label>";
    return $slot;
}
function value_source() {
    echo "value-source>";
    return 5;
}

set_error_handler(function($severity, $message) {
    echo "notice:$severity:$message>";
    return true;
});
$slot = 2;
$target = new ScalarOverload;
try {
    $target->valueCall =& value_source();
} catch (Error $error) {
    echo "error:", $error->getMessage(), "\n";
}
foreach (['scalar', 'object'] as $name) {
    try {
        $target->$name =& reference_source($slot, $name);
    } catch (Error $error) {
        echo "error:", $error->getMessage(), "\n";
    }
}
try {
    $target->hidden =& reference_source($slot, 'hidden');
} catch (Exception $error) {
    echo "exception:", $error->getMessage(), "\n";
}
$referenceTarget = new ReferenceOverload;
try {
    $referenceTarget->missing =& $slot;
} catch (Error $error) {
    echo "error:", $error->getMessage(), "\n";
}
$asymmetric = new AsymmetricOverload;
try {
    $asymmetric->guarded =& $slot;
} catch (Error $error) {
    echo "asymmetric:", $error->getMessage(), "\n";
}
$existing = new ExistingDynamic;
$existing->ready = 1;
$existing->ready =& $slot;
$slot = 9;
echo "existing:", $existing->ready, "\n";
$detached =& $target->source;
$detached = 21;
echo "detached:", $detached, "\n";

set_error_handler(function() {
    echo "throwing-notice>";
    throw new RuntimeException('ignored');
});
try {
    $target->handler =& $slot;
} catch (Error $error) {
    echo "error:", $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "value-source>get:valueCall>",
            "notice:8:Indirect modification of overloaded property ",
            "ScalarOverload::$valueCall has no effect>",
            "error:Cannot assign by reference to overloaded object\n",
            "source:scalar>get:scalar>",
            "notice:8:Indirect modification of overloaded property ",
            "ScalarOverload::$scalar has no effect>",
            "error:Cannot assign by reference to overloaded object\n",
            "source:object>get:object>",
            "error:Cannot assign by reference to overloaded object\n",
            "source:hidden>get:hidden>exception:blocked\n",
            "ref-get:missing>",
            "error:Cannot assign by reference to overloaded object\n",
            "asymmetric:Cannot indirectly modify private(set) property ",
            "AsymmetricOverload::$guarded from global scope\n",
            "existing:9\n",
            "get:source>",
            "notice:8:Indirect modification of overloaded property ",
            "ScalarOverload::$source has no effect>",
            "detached:21\n",
            "get:handler>throwing-notice>",
            "error:Cannot assign by reference to overloaded object\n",
        )
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
fn detached_magic_method_errors_retain_the_callback_and_call_site_trace() {
    let error = run_php_expect_error_with_source_context(
        "<?php\nclass MagicTraceBase {\n    public function &__get($name) {\n        return $this->test;\n    }\n}\nclass MagicTraceChild extends MagicTraceBase { private $test; }\n$object = new MagicTraceChild;\nvar_dump($object->test);",
        "/fixture/magic-trace.php",
        "/fixture",
    );

    assert!(matches!(
        error,
        rphp::vm::execute::VmError::Fatal(message)
            if message == "Uncaught Error: Cannot access private property MagicTraceChild::$test in /fixture/magic-trace.php:4\nStack trace:\n#0 /fixture/magic-trace.php(9): MagicTraceBase->__get('test')\n#1 {main}\n  thrown in /fixture/magic-trace.php on line 4"
    ));
}

#[test]
fn live_traces_retain_detached_magic_property_entry_frames() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class PropertyTrace {
    private function trace(): string {
        return implode('>', array_map(
            fn($frame) => $frame['class'] . '::' . $frame['function'] . '@' . $frame['line'],
            debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS),
        ));
    }
    public function __get($name) { return $this->trace(); }
    public function __set($name, $value) { echo $this->trace(); }
    public function __isset($name) { echo $this->trace(); return false; }
    public function __unset($name) { echo $this->trace(); }
}
class ChildPropertyTrace extends PropertyTrace {
    public function __get($name) { return parent::__get($name); }
}
$property = new PropertyTrace();
echo $property->missing, '|';
$property->missing = 1;
echo '|';
isset($property->missing);
echo '|';
unset($property->missing);
echo '|', (new ChildPropertyTrace())->missing;
"#,
            "/virtual/magic-property-trace.php",
            "/virtual",
        ),
        concat!(
            "PropertyTrace::trace@9>PropertyTrace::__get@18|",
            "PropertyTrace::trace@10>PropertyTrace::__set@19|",
            "PropertyTrace::trace@11>PropertyTrace::__isset@21|",
            "PropertyTrace::trace@12>PropertyTrace::__unset@23|",
            "PropertyTrace::trace@9>PropertyTrace::__get@15>",
            "ChildPropertyTrace::__get@24",
        )
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

#[test]
fn var_dump_projects_objects_through_debug_info() {
    assert_eq!(
        run_php(
            r#"<?php
class DebugView {
    public int $hidden = 9;
    public function __debugInfo(): array { return ['shown' => 4]; }
}
var_dump(new DebugView());
"#
        ),
        "object(DebugView)#1 (1) {\n  [\"shown\"]=>\n  int(4)\n}\n"
    );
}

#[test]
fn var_dump_rejects_non_array_debug_info_results() {
    assert!(matches!(
        run_php_expect_error(
            r#"<?php
class InvalidDebugView {
    public function __debugInfo() { return 4; }
}
var_dump(new InvalidDebugView());
"#
        ),
        rphp::vm::execute::VmError::Fatal(message)
            if message.starts_with("__debuginfo() must return an array in ")
                && message.ends_with(" on line 5")
    ));
}
