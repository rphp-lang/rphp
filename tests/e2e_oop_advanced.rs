mod common;
use common::{run_php, run_php_expect_error};

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
            "<?php trait T { public $value; } enum E { use T; }",
            "cannot include properties",
        ),
        (
            "<?php trait T { function __construct() {} } enum E { use T; }",
            "cannot include magic method",
        ),
    ] {
        let error = run_php_expect_error(source);
        assert!(format!("{error:?}").contains(expected));
    }
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
