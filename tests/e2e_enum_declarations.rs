mod common;

use common::{
    run_php, run_php_expect_error, run_php_expect_error_with_source_context,
    run_php_with_source_context,
};
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;

#[test]
fn valid_unit_and_backed_enum_shapes_remain_executable() {
    assert_eq!(
        run_php(
            r#"<?php
enum UnitState { case Ready; }
enum ExitCode: int { case Success = 0; }
enum WireState: string { case Open = 'open'; }
echo UnitState::Ready->name, '|';
echo ExitCode::Success->value, '|';
echo WireState::Open->value;
"#,
        ),
        "Ready|0|open"
    );
}

#[test]
fn enum_backing_type_is_limited_to_one_int_or_string_type() {
    for (source, expected) in [
        (
            "<?php enum Invalid: bool {}",
            "Enum backing type must be int or string, bool given on line 1",
        ),
        (
            "<?php enum Invalid: DomainType {}",
            "Enum backing type must be int or string, DomainType given on line 1",
        ),
        (
            "<?php enum Invalid: int|string {}",
            "Enum backing type must be int or string, string|int given on line 1",
        ),
    ] {
        let error = run_php_expect_error(source);
        assert_eq!(error.to_string(), expected, "unexpected error for {source}");
    }
}

#[test]
fn enum_case_values_follow_the_backing_shape_at_declaration_time() {
    for (source, expected) in [
        (
            "<?php enum Missing: int { case Value; }",
            "Case Value of backed enum Missing must have a value on line 1",
        ),
        (
            "<?php enum Unexpected { case Value = 1; }",
            "Case Value of non-backed enum Unexpected must not have a value on line 1",
        ),
        (
            "<?php namespace Domain; enum Unexpected { case Value = 'x'; }",
            "Case Value of non-backed enum Domain\\Unexpected must not have a value on line 1",
        ),
    ] {
        let error = run_php_expect_error(source);
        assert_eq!(error.to_string(), expected, "unexpected error for {source}");
    }
}

#[test]
fn invalid_backed_enum_tables_are_lazy_repeatable_and_skip_cases() {
    assert_eq!(
        run_php(
            r#"<?php
function capture_enum_error($operation): void {
    try {
        $operation();
    } catch (Throwable $error) {
        echo get_class($error), ': ', $error->getMessage(), "\n";
    }
}

enum ReusedNumber: int {
    case First = 7;
    case Second = 7;
    const MARKER = 99;
    public static function announce(): void { echo "loaded\n"; }
}

echo count(ReusedNumber::cases()), "\n";
ReusedNumber::announce();
function enum_default($case = ReusedNumber::First): void { echo $case->name, "\n"; }
enum_default();
capture_enum_error(fn() => ReusedNumber::MARKER);
capture_enum_error(fn() => ReusedNumber::First);
capture_enum_error(fn() => ReusedNumber::First);
capture_enum_error(fn() => ReusedNumber::from(99));
capture_enum_error(fn() => ReusedNumber::tryFrom('wrong-kind'));

enum ReusedText: string {
    case Early = 'shared';
    case Middle = 'other';
    case Late = 'shared';
}
capture_enum_error(fn() => ReusedText::Late);
capture_enum_error(fn() => ReusedText::tryFrom('absent'));

enum WrongKind: int { case Text = 'value'; }
echo count(WrongKind::cases()), "\n";
capture_enum_error(fn() => WrongKind::Text);
capture_enum_error(fn() => WrongKind::from(99));
capture_enum_error(fn() => WrongKind::from('wrong-kind'));
"#,
        ),
        "2\nloaded\nFirst\nError: Duplicate value in enum ReusedNumber for cases First and Second\nError: Duplicate value in enum ReusedNumber for cases First and Second\nError: Duplicate value in enum ReusedNumber for cases First and Second\nError: Duplicate value in enum ReusedNumber for cases First and Second\nTypeError: ReusedNumber::tryFrom(): Argument #1 ($value) must be of type int, string given\nError: Duplicate value in enum ReusedText for cases Early and Late\nError: Duplicate value in enum ReusedText for cases Early and Late\n1\nTypeError: Enum case type string does not match enum backing type int\nTypeError: Enum case type string does not match enum backing type int\nTypeError: WrongKind::from(): Argument #1 ($value) must be of type int, string given\n"
    );
}

#[test]
fn synthesized_enum_methods_keep_internal_arity_diagnostics() {
    assert_eq!(
        run_php(
            r#"<?php
enum Signal: int { case Ready = 1; }
try {
    Signal::from();
} catch (ArgumentCountError $error) {
    echo $error->getMessage(), "\n";
}
"#,
        ),
        "Signal::from() expects exactly 1 argument, 0 given\n"
    );
}

#[test]
fn enum_property_syntax_reaches_the_php_declaration_diagnostic() {
    for source in [
        "<?php enum Invalid { public $value; }",
        "<?php enum Invalid { public string $value; }",
        "<?php enum Invalid { public static $value; }",
        "<?php enum Invalid { public string $value { get => 'x'; } }",
    ] {
        let error = run_php_expect_error_with_source_context(
            source,
            "/virtual/enum-declaration.php",
            "/virtual",
        );
        assert_eq!(
            error.to_string(),
            "Enum Invalid cannot include properties in /virtual/enum-declaration.php on line 1",
            "unexpected error for {source}"
        );
    }
}

#[test]
fn enum_only_interfaces_are_validated_on_the_concrete_implementor() {
    assert_eq!(
        run_php(
            r#"<?php
interface UnitMarker extends UnitEnum {}
interface OtherUnitMarker extends UnitEnum {}
interface BackedMarker extends BackedEnum {}
enum UnitState implements UnitMarker, OtherUnitMarker { case Ready; }
enum ExitState: int implements BackedMarker { case Ready = 0; }
echo (int) (UnitState::Ready instanceof UnitMarker), '|';
echo (int) (UnitState::Ready instanceof UnitEnum), '|';
echo (int) (ExitState::Ready instanceof BackedMarker), '|';
echo (int) (ExitState::Ready instanceof BackedEnum);
"#,
        ),
        "1|1|1|1"
    );

    for (source, expected) in [
        (
            "<?php\nclass Invalid implements UnitEnum {}",
            "Non-enum class Invalid cannot implement interface UnitEnum in /virtual/enum-interfaces.php on line 2",
        ),
        (
            "<?php\ninterface EnumBacked extends BackedEnum {}\nclass Invalid implements EnumBacked {}",
            "Non-enum class Invalid cannot implement interface UnitEnum in /virtual/enum-interfaces.php on line 3",
        ),
        (
            "<?php\ninterface EnumBacked extends BackedEnum {}\nenum Invalid implements EnumBacked { case Value; }",
            "Non-backed enum Invalid cannot implement interface BackedEnum in /virtual/enum-interfaces.php on line 3",
        ),
        (
            "<?php\nenum Invalid implements UnitEnum { case Value; }",
            "Enum Invalid cannot implement previously implemented interface UnitEnum in /virtual/enum-interfaces.php on line 2",
        ),
    ] {
        let error = run_php_expect_error_with_source_context(
            source,
            "/virtual/enum-interfaces.php",
            "/virtual",
        );
        assert_eq!(error.to_string(), expected, "unexpected error for {source}");
    }
}

#[test]
fn serializable_deprecation_names_the_concrete_class_and_honors_magic_pair() {
    let source = r#"<?php
interface LegacyContract extends Serializable {}
abstract class LegacyBase implements Serializable {}
class LegacyValue extends LegacyBase implements LegacyContract { public function serialize(): string { return ''; } public function unserialize(string $data): void {} }
class ModernValue extends LegacyBase implements LegacyContract { public function serialize(): string { return ''; } public function unserialize(string $data): void {} public function __serialize(): array { return []; } public function __unserialize(array $data): void {} }
class InheritedLegacyValue extends LegacyBase { public function serialize(): string { return ''; } public function unserialize(string $data): void {} }
class InheritedModernValue extends ModernValue {}
echo 'ok';
"#;
    assert_eq!(
        run_php_with_source_context(source, "/virtual/serializable.php", "/virtual"),
        "\nDeprecated: LegacyValue implements the Serializable interface, which is deprecated. Implement __serialize() and __unserialize() instead (or in addition, if support for old PHP versions is necessary) in /virtual/serializable.php on line 4\n\nDeprecated: InheritedLegacyValue implements the Serializable interface, which is deprecated. Implement __serialize() and __unserialize() instead (or in addition, if support for old PHP versions is necessary) in /virtual/serializable.php on line 6\nok"
    );
}

#[test]
fn serializable_deprecation_is_retained_before_enum_compile_failure() {
    let source = "<?php\ninterface LegacyContract extends Serializable {}\nenum Invalid implements LegacyContract { case Value; }";
    let statements = Parser::new(Lexer::new(source).tokenize().unwrap())
        .parse()
        .unwrap();
    let failure = match Compiler::new()
        .with_source_context("/virtual/enum-serializable.php", "/virtual")
        .compile(&statements)
    {
        Ok(_) => panic!("Serializable enum declaration unexpectedly compiled"),
        Err(failure) => failure,
    };
    assert_eq!(
        failure.message,
        "Enum Invalid cannot implement the Serializable interface in /virtual/enum-serializable.php on line 3"
    );
    assert_eq!(failure.deprecations.len(), 1);
    assert_eq!(
        failure.deprecations[0].message,
        "Invalid implements the Serializable interface, which is deprecated. Implement __serialize() and __unserialize() instead (or in addition, if support for old PHP versions is necessary)"
    );
    assert_eq!(
        failure.deprecations[0].file,
        "/virtual/enum-serializable.php"
    );
    assert_eq!(failure.deprecations[0].line, 3);
    assert!(!failure.deprecations[0].warning);
}
