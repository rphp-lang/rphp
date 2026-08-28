mod common;
use common::{run_php, run_php_expect_error, run_php_expect_error_with_source_context};

// -- final class --

#[test]
fn test_final_class_no_extend() {
    let err = run_php_expect_error(
        r#"<?php
final class Sealed {}
class Sub extends Sealed {}
"#,
    );
    let msg = format!("{:?}", err);
    assert!(msg.contains("final") || msg.contains("cannot"));
}

#[test]
fn test_final_method_no_override() {
    let err = run_php_expect_error(
        r#"<?php
class Base {
    final public function locked(): int { return 1; }
}
class Child extends Base {
    public function locked(): int { return 2; }
}
"#,
    );
    let msg = format!("{:?}", err);
    assert!(msg.contains("final") || msg.contains("override"));
}

#[test]
fn test_final_class_can_instantiate() {
    assert_eq!(
        run_php(
            r#"<?php
final class Config {
    public function get(): string { return "ok"; }
}
$c = new Config();
echo $c->get();
"#
        ),
        "ok"
    );
}

// -- readonly --

#[test]
fn test_readonly_property() {
    assert_eq!(
        run_php(
            r#"<?php
class User {
    public readonly string $name;
    public function __construct(string $name) {
        $this->name = $name;
    }
}
$u = new User("Alice");
echo $u->name;
"#
        ),
        "Alice"
    );
}

// -- constructor promotion --

#[test]
fn test_constructor_promotion() {
    assert_eq!(
        run_php(
            r#"<?php
class Point {
    public function __construct(
        public float $x,
        public float $y,
    ) {}
}
$p = new Point(1.5, 2.5);
echo $p->x . " " . $p->y;
"#
        ),
        "1.5 2.5"
    );
}

#[test]
fn test_constructor_promotion_readonly() {
    assert_eq!(
        run_php(
            r#"<?php
class Config {
    public function __construct(
        public readonly string $dsn,
    ) {}
}
$c = new Config("mysql://localhost");
echo $c->dsn;
"#
        ),
        "mysql://localhost"
    );
}

// -- enums --

#[test]
fn test_enum_basic() {
    assert_eq!(
        run_php(
            r#"<?php
enum Color {
    case Red;
    case Green;
    case Blue;
}
$c = Color::Red;
echo $c->name;
"#
        ),
        "Red"
    );
}

#[test]
fn test_enum_backed_string() {
    assert_eq!(
        run_php(
            r#"<?php
enum Suit: string {
    case Hearts = 'H';
    case Diamonds = 'D';
}
echo Suit::Hearts->value;
"#
        ),
        "H"
    );
}

#[test]
fn test_enum_backed_int() {
    assert_eq!(
        run_php(
            r#"<?php
enum Status: int {
    case Active = 1;
    case Inactive = 0;
}
echo Status::Active->value . " " . Status::Active->name;
"#
        ),
        "1 Active"
    );
}

#[test]
fn test_enum_comparison() {
    assert_eq!(
        run_php(
            r#"<?php
enum Color {
    case Red;
    case Green;
}
$a = Color::Red;
$b = Color::Red;
echo $a === $b ? "same" : "diff";
"#
        ),
        "same"
    );
}

// -- match exhaustiveness --

#[test]
fn test_match_unhandled_error() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    $x = match(3) {
        1 => "one",
        2 => "two",
    };
} catch (\Throwable $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

// -- readonly scope check --

#[test]
fn test_readonly_no_init_from_outside() {
    assert_eq!(
        run_php(
            r#"<?php
class C {
    public readonly int $x;
}
$c = new C();
try {
    $c->x = 1;
    echo "made";
} catch (Error $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

#[test]
fn test_readonly_modify_after_init() {
    assert_eq!(
        run_php(
            r#"<?php
class C {
    public readonly int $x;
    public function __construct(int $x) { $this->x = $x; }
}
$c = new C(42);
try {
    $c->x = 99;
    echo "made";
} catch (Error $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

// -- enum not instantiable --

#[test]
fn test_enum_not_instantiable() {
    assert_eq!(
        run_php(
            r#"<?php
enum Color { case Red; }
try {
    new Color();
    echo "made";
} catch (Error $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

#[test]
fn classes_cannot_extend_user_or_internal_enums() {
    for (source, expected) in [
        (
            "<?php enum ParentEnum { case One; } class Child extends ParentEnum {}",
            "Class Child cannot extend enum ParentEnum",
        ),
        (
            "<?php class Child extends Random\\IntervalBoundary {}",
            "Class Child cannot extend enum Random\\\\IntervalBoundary",
        ),
    ] {
        let error = run_php_expect_error(source);
        assert!(
            format!("{error:?}").contains(expected),
            "unexpected error: {error:?}"
        );
    }
}

#[test]
fn test_enum_case_name_immutable() {
    assert_eq!(
        run_php(
            r#"<?php
enum Color { case Red; }
$c = Color::Red;
try {
    $c->name = "X";
    echo "mutated";
} catch (Error $e) {
    echo "caught";
}
echo Color::Red->name;
"#
        ),
        "caughtRed"
    );
}

#[test]
fn test_enum_case_no_dynamic_property() {
    assert_eq!(
        run_php(
            r#"<?php
enum Color { case Red; }
$c = Color::Red;
try {
    $c->foo = 1;
    echo "mutated";
} catch (Error $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

#[test]
fn test_enum_backed_value_immutable() {
    assert_eq!(
        run_php(
            r#"<?php
enum Suit: string { case Hearts = "H"; }
$s = Suit::Hearts;
try {
    $s->value = "X";
    echo "mutated";
} catch (Error $e) {
    echo "caught";
}

echo Suit::Hearts->value;
"#
        ),
        "caughtH"
    );
}

#[test]
fn enum_case_property_mutations_distinguish_readonly_and_dynamic_members() {
    assert_eq!(
        run_php(
            r#"<?php
enum UnitState { case Ready; }
enum ExitCode: int { case Success = 0; }
function attempt(string $label, Closure $operation): void {
    try {
        $operation();
        echo $label, ":ok\n";
    } catch (Error $error) {
        echo $label, ':', $error->getMessage(), "\n";
    }
}
$unit = UnitState::Ready;
$code = ExitCode::Success;
attempt('write-name', function () use ($unit) { $unit->name = 'Changed'; });
attempt('write-unit-value', function () use ($unit) { $unit->value = 1; });
attempt('write-backed-value', function () use ($code) { $code->value = 2; });
attempt('write-missing', function () use ($code) { $code->other = 2; });
attempt('unset-name', function () use ($unit) { unset($unit->name); });
attempt('unset-value', function () use ($code) { unset($code->value); });
attempt('unset-missing', function () use ($code) { unset($code->missing); });
echo UnitState::Ready->name, ':', ExitCode::Success->name, ':', ExitCode::Success->value;
"#,
        ),
        concat!(
            "write-name:Cannot modify readonly property UnitState::$name\n",
            "write-unit-value:Cannot create dynamic property UnitState::$value\n",
            "write-backed-value:Cannot modify readonly property ExitCode::$value\n",
            "write-missing:Cannot create dynamic property ExitCode::$other\n",
            "unset-name:Cannot unset readonly property UnitState::$name\n",
            "unset-value:Cannot unset readonly property ExitCode::$value\n",
            "unset-missing:ok\n",
            "Ready:Success:0",
        )
    );
}

#[test]
fn enum_case_property_references_fail_before_alias_publication() {
    assert_eq!(
        run_php(
            r#"<?php
enum ExitCode: int { case Success = 0; }
readonly class ReadonlyBox {
    public function __construct(public int $value) {}
}
class MutableBox { public int $value = 1; }
function overwrite(&$slot): void { $slot = 9; }
function &enumValue(): mixed { return ExitCode::Success->value; }
function attempt(string $label, Closure $operation): void {
    try {
        $operation();
        echo $label, ":ok\n";
    } catch (Error $error) {
        echo $label, ':', $error->getMessage(), "\n";
    }
}
$code = ExitCode::Success;
attempt('fetch', function () use ($code) { $alias =& $code->value; });
attempt('pass', function () use ($code) { overwrite($code->value); });
attempt('return', function () { $alias =& enumValue(); });
attempt('missing', function () use ($code) { $alias =& $code->missing; });
$readonly = new ReadonlyBox(2);
attempt('readonly', function () use ($readonly) { $alias =& $readonly->value; });
$mutable = new MutableBox();
overwrite($mutable->value);
echo 'values:', ExitCode::Success->value, ':', $readonly->value, ':', $mutable->value;
"#,
        ),
        concat!(
            "fetch:Cannot indirectly modify readonly property ExitCode::$value\n",
            "pass:Cannot indirectly modify readonly property ExitCode::$value\n",
            "return:Cannot indirectly modify readonly property ExitCode::$value\n",
            "missing:Cannot create dynamic property ExitCode::$missing\n",
            "readonly:Cannot indirectly modify readonly property ReadonlyBox::$value\n",
            "values:0:2:9",
        )
    );
}

#[test]
fn enum_case_temporary_property_unset_fails_during_compilation() {
    let error = run_php_expect_error_with_source_context(
        "<?php\nenum ExitCode: int { case Success = 0; }\nunset(ExitCode::Success->value);",
        "/virtual/enum-property-unset.php",
        "/virtual",
    );
    assert_eq!(
        error.to_string(),
        "Cannot use temporary expression in write context in /virtual/enum-property-unset.php on line 3"
    );
}

#[test]
fn test_enum_implements_interfaces_and_inherits_constants() {
    assert_eq!(
        run_php(
            r#"<?php
interface Labelled {
    const PREFIX = "item:";
    public function label(): string;
}
enum Item: string implements Labelled {
    case One = "one";
    public function label(): string { return self::PREFIX . $this->value; }
}
echo Item::One->label(), "\n";
var_dump(Item::One instanceof Labelled);
var_dump(Item::One instanceof UnitEnum);
var_dump(Item::One instanceof BackedEnum);
"#
        ),
        "item:one\nbool(true)\nbool(true)\nbool(true)\n"
    );
}

#[test]
fn test_enum_rejects_explicit_implicit_and_serializable_interfaces() {
    for source in [
        "<?php enum Bad implements UnitEnum {}",
        "<?php enum Bad: int implements BackedEnum {}",
        "<?php enum Bad implements Serializable {}",
    ] {
        let error = run_php_expect_error(source);
        assert!(format!("{error:?}").contains("cannot implement"));
    }
}

#[test]
fn enum_composes_trait_methods_constants_and_aliases() {
    assert_eq!(
        run_php(
            r#"<?php
trait Labelled {
    private const PREFIX = "case:";
    public function label(): string { return self::PREFIX . $this->name; }
}
enum Item {
    use Labelled { label as private hiddenLabel; }
    case One;
    public function reveal(): string { return $this->hiddenLabel(); }
}
echo Item::One->reveal();
"#
        ),
        "case:One"
    );
}

#[test]
fn enum_rejects_trait_properties_and_forbidden_magic_methods() {
    for (source, expected) in [
        (
            "<?php\ntrait T { public $value; }\nenum E { use T; }",
            "Enum E cannot include properties in enum-trait.php on line 3",
        ),
        (
            "<?php\ntrait T { function __construct() {} }\nenum E { use T; }",
            "Enum E cannot include magic method __construct in enum-trait.php on line 3",
        ),
        (
            "<?php\ntrait Inner { public $value; } trait Outer { use Inner; }\nenum E { use Outer; }",
            "Enum E cannot include properties in enum-trait.php on line 3",
        ),
        (
            "<?php\ntrait T { function stringify() {} }\nenum E { use T { stringify as __toString; } }",
            "Enum E cannot include magic method __toString in enum-trait.php on line 3",
        ),
    ] {
        let error = run_php_expect_error_with_source_context(source, "enum-trait.php", ".");
        assert!(
            format!("{error:?}").contains(expected),
            "unexpected error: {error:?}"
        );
    }

    assert_eq!(
        run_php(
            "<?php trait Invokable { function __invoke() { return 7; } } enum E { use Invokable; case One; } echo (E::One)();"
        ),
        "7"
    );
}

#[test]
fn enum_cases_returns_declaration_order_and_overrides_trait_method() {
    assert_eq!(
        run_php(
            r#"<?php
trait EmptyCases {
    public static function cases(): array { return []; }
}
enum Suit {
    use EmptyCases;
    case Hearts;
    case Diamonds;
    const First = self::Hearts;
}
foreach (Suit::cases() as $case) {
    echo $case->name, ":", $case === Suit::{$case->name} ? "same" : "copy", "\n";
}
echo Suit::First === Suit::Hearts ? "alias\n" : "copy\n";
"#
        ),
        "Hearts:same\nDiamonds:same\nalias\n"
    );
}

#[test]
fn enum_rejects_an_explicit_cases_method() {
    let error = run_php_expect_error(
        "<?php enum Suit { public static function cases(): array { return []; } }",
    );
    assert!(format!("{error:?}").contains("Cannot redeclare Suit::cases()"));
}

#[test]
fn backed_enum_from_and_try_from_resolve_cases_and_unknown_values() {
    assert_eq!(
        run_php(
            r#"<?php
enum Suit: string { case Hearts = "H"; case Diamonds = "D"; }
enum Code: int { case Missing = -1; case Ready = 2; }
echo Suit::from("H")->name, "\n";
echo Code::from(-1)->name, "\n";
var_dump(Suit::tryFrom("X"));
var_dump(Code::tryFrom(3));
try { Suit::from("X"); } catch (ValueError $error) { echo $error->getMessage(), "\n"; }
try { Code::from(3); } catch (ValueError $error) { echo $error->getMessage(), "\n"; }
"#
        ),
        "Hearts\nMissing\nNULL\nNULL\n\"X\" is not a valid backing value for enum Suit\n3 is not a valid backing value for enum Code\n"
    );
}

#[test]
fn backed_enum_rejects_explicit_from_methods() {
    for method in ["from", "tryFrom"] {
        let source =
            format!("<?php enum Suit: int {{ public static function {method}(int $value) {{}} }}");
        let error = run_php_expect_error(&source);
        assert!(format!("{error:?}").contains(&format!(
            "Cannot redeclare Suit::{}()",
            method.to_ascii_lowercase()
        )));
    }
}

#[test]
fn enum_relational_comparison_is_identity_only() {
    assert_eq!(
        run_php(
            r#"<?php
enum State { case Ready; case Done; }
$ready = State::Ready;
$alias = $ready;
$done = State::Done;
foreach ([
    $ready < $alias, $ready <= $alias, $ready > $alias, $ready >= $alias,
    $ready < $done, $ready <= $done, $ready > $done, $ready >= $done,
    $ready < true, $ready <= true, true < $ready, true <= $ready,
] as $result) var_dump($result);
"#
        ),
        "bool(false)\nbool(true)\nbool(false)\nbool(true)\nbool(false)\nbool(false)\nbool(false)\nbool(false)\nbool(false)\nbool(true)\nbool(false)\nbool(true)\n"
    );
}

#[test]
fn enum_virtual_properties_are_typed_and_readonly() {
    assert_eq!(
        run_php(
            r#"<?php
enum Unit { case Ready; }
enum Code: int { case Ready = 2; }
$unitName = new ReflectionProperty(Unit::class, "name");
$codeName = new ReflectionProperty(Code::class, "name");
$codeValue = new ReflectionProperty(Code::class, "value");
echo $unitName->getType()->getName(), ":", $unitName->isReadOnly() ? "ro" : "rw", "\n";
echo $codeName->getType()->getName(), ":", $codeValue->getType()->getName(), ":",
     $codeValue->isReadOnly() ? "ro" : "rw", "\n";
try { Code::Ready->value = 3; } catch (Error $error) { echo $error->getMessage(), "\n"; }
echo Code::Ready->value;
"#
        ),
        "string:ro\nstring:int:ro\nCannot modify readonly property Code::$value\n2"
    );
}
