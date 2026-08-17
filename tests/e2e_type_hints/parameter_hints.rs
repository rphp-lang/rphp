// ── Basic scalar type hints ──

#[test]
fn test_int_type_hint_pass() {
    assert_eq!(
        run_php(
            r#"<?php
function add(int $a, int $b) { echo $a + $b; }
add(3, 4);
"#
        ),
        "7"
    );
}

#[test]
fn test_int_type_hint_fail() {
    assert_eq!(
        run_php(
            r#"<?php
function add(int $a, int $b) { echo $a + $b; }
try {
    add("hello", 4);
} catch (TypeError $e) {
    echo $e->getMessage();
}
"#
        )
        .contains("must be of type int"),
        true
    );
}

#[test]
fn argument_type_errors_keep_declaration_origin_and_pending_call_trace() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function acceptInt(int $value): void {}
class Gate { public static function acceptInt(int $value): void {} }
$closure = function(int $value): void {};
try { acceptInt([]); } catch (TypeError $error) { $trace = $error->getTrace(); echo $error->getFile(), ':', $error->getLine(), '|', $trace[0]['function'], ':', $trace[0]['line'], ':', gettype($trace[0]['args'][0]), "\n"; }
try { Gate::acceptInt([]); } catch (TypeError $error) { echo $error->getFile(), ':', $error->getLine(), "\n"; }
try { $closure([]); } catch (TypeError $error) { echo $error->getFile(), ':', $error->getLine(), "\n"; }
"#,
            "argument_origin.php",
            ".",
        ),
        "argument_origin.php:2|acceptInt:5:array\nargument_origin.php:3\nargument_origin.php:4\n"
    );
}

#[test]
fn test_string_type_hint_pass() {
    assert_eq!(
        run_php(
            r#"<?php
function greet(string $name) { echo "Hello $name"; }
greet("world");
"#
        ),
        "Hello world"
    );
}

#[test]
fn test_string_type_hint_fail() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function greet(string $name) { echo "Hello $name"; }
try {
    greet(42);
} catch (TypeError $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

#[test]
fn test_bool_type_hint_pass() {
    assert_eq!(
        run_php(
            r#"<?php
function check(bool $flag) { echo $flag ? "yes" : "no"; }
check(true);
"#
        ),
        "yes"
    );
}

#[test]
fn test_bool_type_hint_fail() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function check(bool $flag) { echo $flag ? "yes" : "no"; }
try {
    check(1);
} catch (TypeError $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

#[test]
fn test_float_type_hint_pass() {
    assert_eq!(
        run_php(
            r#"<?php
function half(float $x) { echo $x / 2; }
half(10.0);
"#
        ),
        "5"
    );
}

#[test]
fn test_float_type_hint_accepts_int() {
    // PHP: float type hint accepts int values (widening)
    assert_eq!(
        run_php(
            r#"<?php
function half(float $x) { echo $x / 2; }
half(10);
"#
        ),
        "5"
    );
}

#[test]
fn test_float_type_hint_fail() {
    assert_eq!(
        run_php(
            r#"<?php
function half(float $x) { echo $x; }
try {
    half("abc");
} catch (TypeError $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

#[test]
fn test_array_type_hint_pass() {
    assert_eq!(
        run_php(
            r#"<?php
function first(array $arr) { echo $arr[0]; }
first([10, 20, 30]);
"#
        ),
        "10"
    );
}

#[test]
fn test_array_type_hint_fail() {
    assert_eq!(
        run_php(
            r#"<?php
function first(array $arr) { echo $arr[0]; }
try {
    first("not array");
} catch (TypeError $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

// ── Nullable type hints ──

#[test]
fn test_nullable_int_pass_int() {
    assert_eq!(
        run_php(
            r#"<?php
function show(?int $x) { echo $x ?? "null"; }
show(42);
"#
        ),
        "42"
    );
}

#[test]
fn test_nullable_int_pass_null() {
    assert_eq!(
        run_php(
            r#"<?php
function show(?int $x) { echo $x ?? "null"; }
show(null);
"#
        ),
        "null"
    );
}

#[test]
fn test_nullable_int_fail() {
    assert_eq!(
        run_php(
            r#"<?php
function show(?int $x) { echo $x; }
try {
    show("hello");
} catch (TypeError $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

#[test]
fn test_nullable_string_pass() {
    assert_eq!(
        run_php(
            r#"<?php
function show(?string $x) { echo $x ?? "empty"; }
show(null);
echo " ";
show("hi");
"#
        ),
        "empty hi"
    );
}

// ── Class type hints ──

#[test]
fn test_class_type_hint_pass() {
    assert_eq!(
        run_php(
            r#"<?php
class Foo {
    public $val;
    public function __construct($v) { $this->val = $v; }
}
function show(Foo $f) { echo $f->val; }
show(new Foo("ok"));
"#
        ),
        "ok"
    );
}

#[test]
fn test_class_type_hint_fail() {
    assert_eq!(
        run_php(
            r#"<?php
class Foo {}
class Bar {}
function show(Foo $f) { echo "ok"; }
try {
    show(new Bar());
} catch (TypeError $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

#[test]
fn test_class_type_hint_accepts_child() {
    assert_eq!(
        run_php(
            r#"<?php
class Animal {}
class Dog extends Animal {}
function show(Animal $a) { echo "ok"; }
show(new Dog());
"#
        ),
        "ok"
    );
}

#[test]
fn test_interface_type_hint() {
    assert_eq!(
        run_php(
            r#"<?php
interface Printable {
    public function display();
}
class Doc implements Printable {
    public function display() { echo "doc"; }
}
function show(Printable $p) { $p->display(); }
show(new Doc());
"#
        ),
        "doc"
    );
}

#[test]
fn test_intersection_parameter_return_and_typed_reference() {
    compile_types(
        "<?php interface ReferenceContract {} function replace(ReferenceContract &$value): void {}",
    );
    assert_eq!(
        run_php(
            r#"<?php
interface LeftContract {}
interface RightContract {}
class BothContracts implements LeftContract, RightContract {}
class LeftOnly implements LeftContract {}
function acceptBoth(LeftContract&RightContract $value): LeftContract&RightContract {
    return $value;
}
function badReturn(): LeftContract&RightContract { return new LeftOnly(); }
$both = new BothContracts();
echo (acceptBoth($both) instanceof RightContract) ? "both:" : "missing:";
try { acceptBoth(new LeftOnly()); } catch (TypeError $error) { echo "parameter:"; }
try {
    badReturn();
} catch (TypeError $error) { echo "return:"; }
"#,
        ),
        "both:parameter:return:"
    );
}

#[test]
fn intersection_types_reject_non_class_members_at_declaration_time() {
    for (declared, diagnostic) in [
        ("array", "array"),
        ("bool", "bool"),
        ("callable", "callable"),
        ("false", "false"),
        ("float", "float"),
        ("int", "int"),
        ("iterable", "Traversable|array"),
        ("mixed", "mixed"),
        ("never", "never"),
        ("null", "null"),
        ("object", "object"),
        ("string", "string"),
        ("true", "true"),
        ("void", "void"),
    ] {
        let source = format!("<?php\nfunction invalid(): {declared}&Iterator {{}}");
        let error = run_php_expect_error_with_source_context(
            &source,
            "intersection-type.php",
            ".",
        );
        let expected = format!(
            "Type {diagnostic} cannot be part of an intersection type in intersection-type.php on line 2"
        );
        assert!(
            error.to_string().contains(&expected),
            "unexpected error for {declared}: {error}"
        );
    }

    for source in [
        "<?php\nfunction invalid(int&Iterator $value) {}",
        "<?php\nclass InvalidProperty { public int&Iterator $value; }",
        "<?php\n$invalid = function (): int&Iterator {};",
    ] {
        let error = run_php_expect_error_with_source_context(
            source,
            "intersection-type.php",
            ".",
        );
        assert!(
            error.to_string().contains(
                "Type int cannot be part of an intersection type in intersection-type.php on line 2"
            ),
            "unexpected contextual error: {error}"
        );
    }

    let error = run_php_expect_error_with_source_context(
        "<?php\ninterface Contract {} class InvalidStatic { function invalid(): static&Contract {} }",
        "intersection-type.php",
        ".",
    );
    assert!(error.to_string().contains(
        "Type static cannot be part of an intersection type in intersection-type.php on line 2"
    ));
}

#[test]
fn redundant_declared_types_use_php_normalization_and_diagnostics() {
    for (source, expected) in [
        (
            "<?php\nfunction invalid(): int|INT {}",
            "Duplicate type int is redundant",
        ),
        (
            "<?php\nfunction invalid(): Example|EXAMPLE {}",
            "Duplicate type EXAMPLE is redundant",
        ),
        (
            "<?php\nclass Example { function invalid(): self|SELF {} }",
            "Duplicate type Example is redundant",
        ),
        (
            "<?php\nclass Base {} class Example extends Base { function invalid(): parent|PARENT {} }",
            "Duplicate type Base is redundant",
        ),
        (
            "<?php\nuse Original as Alias; function invalid(): Original&Alias {}",
            "Duplicate type Original is redundant",
        ),
        (
            "<?php\nfunction invalid(): bool|false {}",
            "Duplicate type false is redundant",
        ),
        (
            "<?php\nfunction invalid(): false|true {}",
            "Type contains both true and false, bool must be used instead",
        ),
        (
            "<?php\nfunction invalid(): iterable|Traversable {}",
            "Duplicate type Traversable is redundant",
        ),
        (
            "<?php\nfunction invalid(): iterable|iterable|null {}",
            "Duplicate type array is redundant",
        ),
        (
            "<?php\nfunction invalid(): ?null {}",
            "null cannot be marked as nullable",
        ),
        (
            "<?php\nfunction invalid(): object|Example {}",
            "Type Example|object contains both object and a class type, which is redundant",
        ),
        (
            "<?php\nfunction invalid(): object|iterable|Example|null {}",
            "Type Traversable|Example|object|array|null contains both object and a class type, which is redundant",
        ),
        (
            "<?php\ninterface A {} interface B {} function invalid(): (A&B)|A {}",
            "Type A&B is redundant as it is more restrictive than type A",
        ),
        (
            "<?php\ninterface X {} use Original as Alias; function invalid(): (X&Original)|(X&Alias) {}",
            "Type X&Original is redundant with type X&Original",
        ),
    ] {
        let error = run_php_expect_error_with_source_context(
            source,
            "redundant-type.php",
            ".",
        );
        assert!(
            error.to_string().contains(&format!(
                "{expected} in redundant-type.php on line 2"
            )),
            "unexpected error for {source}: {error}"
        );
    }

    assert_eq!(
        run_php(
            "<?php interface A {} interface B {} interface C {} class Both implements A, B {} function valid((A&B)|C $value): (A&B)|C { return $value; } echo valid(new Both()) instanceof B ? 'ok' : 'bad';"
        ),
        "ok"
    );
}

// ── Type hints with defaults ──

#[test]
fn test_type_hint_with_default() {
    assert_eq!(
        run_php(
            r#"<?php
function greet(string $name = "world") { echo "Hello $name"; }
greet();
"#
        ),
        "Hello world"
    );
}

#[test]
fn test_nullable_with_default_null() {
    assert_eq!(
        run_php(
            r#"<?php
function show(?int $x = null) { echo $x ?? "none"; }
show();
echo " ";
show(5);
"#
        ),
        "none 5"
    );
}

// ── Method type hints ──

#[test]
fn test_method_type_hint() {
    assert_eq!(
        run_php(
            r#"<?php
class Math {
    public function add(int $a, int $b) { echo $a + $b; }
}
$m = new Math();
$m->add(3, 4);
"#
        ),
        "7"
    );
}

#[test]
fn test_method_type_hint_fail() {
    assert_eq!(
        run_php(
            r#"<?php
class Math {
    public function add(int $a, int $b) { echo $a + $b; }
}
$m = new Math();
try {
    $m->add("x", 4);
} catch (TypeError $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

// ── Multiple type-checked params ──

#[test]
fn test_multiple_typed_params() {
    assert_eq!(
        run_php(
            r#"<?php
function info(string $name, int $age, bool $active) {
    echo "$name $age " . ($active ? "yes" : "no");
}
info("Alice", 30, true);
"#
        ),
        "Alice 30 yes"
    );
}

#[test]
fn test_second_param_fails() {
    assert_eq!(
        run_php(
            r#"<?php
function info(string $name, int $age) { echo "$name $age"; }
try {
    info("Alice", "thirty");
} catch (TypeError $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

// ── Closure type hints ──

#[test]
fn test_closure_type_hint() {
    assert_eq!(
        run_php(
            r#"<?php
$add = function(int $a, int $b) { return $a + $b; };
echo $add(3, 4);
"#
        ),
        "7"
    );
}

#[test]
fn test_closure_type_hint_fail() {
    assert_eq!(
        run_php(
            r#"<?php
$add = function(int $a, int $b) { return $a + $b; };
try {
    $add("x", 4);
} catch (TypeError $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

// ── Throwable type hint ──

#[test]
fn test_throwable_type_hint() {
    assert_eq!(
        run_php(
            r#"<?php
function handle(Throwable $e) { echo $e->getMessage(); }
handle(new Exception("test"));
"#
        ),
        "test"
    );
}

#[test]
fn standalone_keyword_literal_types_parse_and_enforce_exact_values() {
    assert_eq!(
        run_php(
            r#"<?php
function literals(true $yes, false $no, null $nothing): null {
    echo $yes ? 'T' : 'x';
    echo $no ? 'x' : 'F';
    return $nothing;
}
class Flags { public true $yes; public false $no; }
var_dump(literals(true, false, null));
try { literals(1, false, null); } catch (TypeError $error) { echo $error->getMessage(); }
$flags = new Flags();
try { $flags->yes = false; } catch (TypeError $error) { echo "\n", $error->getMessage(); }
try { $flags->no = true; } catch (TypeError $error) { echo "\n", $error->getMessage(); }
"#
        ),
        "TFNULL\nliterals(): Argument #1 ($yes) must be of type true, int given, called in <main> on line 9\nCannot assign false to property Flags::$yes of type true\nCannot assign true to property Flags::$no of type false"
    );
}

#[test]
fn null_led_dnf_property_type_accepts_null_and_its_intersection_arm() {
    assert_eq!(
        run_php(
            r#"<?php
interface Left {}
interface Right {}
class Both implements Left, Right {}
class Box { public null|(Left&Right) $value; }
function acceptDnf(null|(Left&Right) $value): void {}
$box = new Box();
$box->value = null;
var_dump($box->value);
$box->value = new Both();
echo get_class($box->value);
try { acceptDnf(new stdClass()); } catch (TypeError $error) { echo '|', $error->getMessage(); }
try { $box->value = new stdClass(); } catch (TypeError $error) { echo '|', $error->getMessage(); }
"#
        ),
        "NULL\nBoth|acceptDnf(): Argument #1 ($value) must be of type (Left&Right)|null, stdClass given, called in <main> on line 12|Cannot assign stdClass to property Box::$value of type (Left&Right)|null"
    );
}

#[test]
fn implicit_nullable_defaults_warn_and_normalize_callable_contracts() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function legacy(false $value = null) { var_dump($value); }
class LegacyBox { public function accept(Countable&Iterator $value = null) { var_dump($value); } }
$closure = function (callable $value = null) { var_dump($value); };
legacy(null);
(new LegacyBox())->accept(null);
$closure(null);
"#,
            "implicit-nullable.php",
            ".",
        ),
        "\nDeprecated: legacy(): Implicitly marking parameter $value as nullable is deprecated, the explicit nullable type must be used instead in implicit-nullable.php on line 2\n\nDeprecated: LegacyBox::accept(): Implicitly marking parameter $value as nullable is deprecated, the explicit nullable type must be used instead in implicit-nullable.php on line 3\n\nDeprecated: {closure:implicit-nullable.php:4}(): Implicitly marking parameter $value as nullable is deprecated, the explicit nullable type must be used instead in implicit-nullable.php on line 4\nNULL\nNULL\nNULL\n"
    );
}

#[test]
fn optional_parameters_before_the_last_required_parameter_are_required() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function contract($first = 1, int $legacy = null, $third = 3, $required) {}
try { contract(required: 4); } catch (Error $error) { echo $error->getMessage(); }
"#,
            "parameter-contract.php",
            ".",
        ),
        "\nDeprecated: contract(): Optional parameter $first declared before required parameter $required is implicitly treated as a required parameter in parameter-contract.php on line 2\n\nDeprecated: contract(): Implicitly marking parameter $legacy as nullable is deprecated, the explicit nullable type must be used instead in parameter-contract.php on line 2\n\nDeprecated: contract(): Optional parameter $third declared before required parameter $required is implicitly treated as a required parameter in parameter-contract.php on line 2\ncontract(): Argument #1 ($first) not passed"
    );
}

#[test]
fn typed_parameter_defaults_are_validated_before_execution() {
    let tokens = Lexer::new(
        "<?php\nfunction legacy(iterable $items = null) {}\nfunction invalid(STRING $value = 1) {}\n",
    )
    .tokenize()
    .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let failure = match Compiler::new()
        .with_source_context("default-contract.php", ".")
        .compile(&statements)
    {
        Ok(_) => panic!("invalid typed default unexpectedly compiled"),
        Err(failure) => failure,
    };
    assert_eq!(failure.deprecations.len(), 1);
    assert_eq!(
        failure.deprecations[0].message,
        "legacy(): Implicitly marking parameter $items as nullable is deprecated, the explicit nullable type must be used instead"
    );
    assert_eq!(failure.deprecations[0].line, 2);
    assert_eq!(
        failure.message,
        "Cannot use int as default value for parameter $value of type string in default-contract.php on line 3"
    );

    assert_eq!(
        run_php("<?php function widened(float $value = 1) { var_dump($value); } widened();"),
        "float(1)\n"
    );
}
