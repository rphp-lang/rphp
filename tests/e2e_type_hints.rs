/// Tests for parameter type hints
mod common;
use common::{
    run_php, run_php_expect_error, run_php_expect_error_with_source_context,
    run_php_with_source_context,
};
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
#[cfg(feature = "quick-loops")]
use rphp::vm::function::ScalarLongOpKind;
use rphp::vm::function::{
    CallStrategy, ComposedScalarLongOp, ComposedTypedLongOp, ReturnStrategy, ScalarLongCallGuard,
    ScalarStringSource,
};
use rphp::vm::instruction::{
    CALL_FLAG_EXACT_SCALAR_ARGS, CALL_FLAG_OBJECT_ARRAY_CONSUMERS, KnownScalarType,
    NEW_FLAG_VIRTUAL_DECLARED_READS, NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE,
};
use rphp::vm::opcode::OpCode;
#[cfg(feature = "quick-loops")]
use rphp::vm::planner::BlockPlan;
#[cfg(feature = "quick-loops")]
use rphp::vm::quick::{QuickLongOp, QuickTypedMethodCall};

fn compile_types(source: &str) -> rphp::compiler::compile::CompileResult {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    Compiler::new().compile(&statements).unwrap()
}

include!("e2e_type_hints/parameter_hints.rs");
include!("e2e_type_hints/return_hints.rs");
include!("e2e_type_hints/runtime_guards.rs");
include!("e2e_type_hints/scalar_propagation.rs");
include!("e2e_type_hints/string_plans.rs");
include!("e2e_type_hints/object_plans.rs");
include!("e2e_type_hints/static_property_hints.rs");
include!("e2e_type_hints/instance_property_hints.rs");

#[test]
fn typed_by_reference_parameter_checks_the_referenced_value() {
    assert_eq!(
        run_php(
            "<?php function appendValue(array &$values): void { $values[] = 42; } $values = []; appendValue($values); echo $values[0];"
        ),
        "42"
    );
}

#[test]
fn grouped_properties_share_type_modifiers_and_keep_individual_defaults() {
    assert_eq!(
        run_php(
            r#"<?php
trait Coordinates { public int $x = 1, $y, $z = 3; }
class Point { use Coordinates; private static ?string $a = null, $b = "b"; }
$anonymous = new class { public int $x = 4, $y = 5; };
$point = new Point;
echo $point->x, ":", $point->z, ":", $anonymous->x, ":", $anonymous->y;
"#
        ),
        "1:3:4:5"
    );
}

#[test]
fn typed_property_literal_defaults_use_php_declaration_diagnostics() {
    let file = "/virtual/typed-property-default.php";
    let cases = [
        (
            "<?php\nclass Defaults {\n    public static bool $flag = 'yes';\n}",
            "Cannot use string as default value for property Defaults::$flag of type bool",
            3,
        ),
        (
            "<?php\nclass Target {}\nclass Defaults {\n    public Target $target = null;\n}",
            "Default value for property of type Target may not be null. Use the nullable type ?Target to allow null default value",
            4,
        ),
        (
            "<?php\nclass Defaults {\n    public bool|string $value = null;\n}",
            "Default value for property of type string|bool may not be null. Use the nullable type string|bool|null to allow null default value",
            3,
        ),
        (
            "<?php\ninterface Left {}\ninterface Right {}\nclass Defaults {\n    public Left&Right $value = null;\n}",
            "Cannot use null as default value for property Defaults::$value of type Left&Right",
            5,
        ),
        (
            "<?php\ntrait Defaults {\n    public int $value = 'bad';\n}",
            "Cannot use string as default value for property Defaults::$value of type int",
            3,
        ),
        (
            "<?php\n$value = new class {\n    public int $number = 'bad';\n};",
            "Cannot use string as default value for property class@anonymous::$number of type int",
            3,
        ),
    ];

    for (source, message, line) in cases {
        let error = run_php_expect_error_with_source_context(source, file, "/virtual");
        assert_eq!(
            error.to_string(),
            format!("{message} in {file} on line {line}"),
            "{source}"
        );
    }

    assert_eq!(
        run_php(
            r#"<?php
trait GoodDefaults {
    public float $wide = 7;
    public ?int $empty = null;
}
class DefaultHolder {
    use GoodDefaults;
    public int|float $either = 9;
}
$holder = new DefaultHolder;
var_dump($holder->wide, $holder->empty, $holder->either);
"#
        ),
        "float(7)\nNULL\nint(9)\n"
    );
}

#[test]
fn grouped_property_cannot_repeat_a_type_after_the_comma() {
    let error = run_php_expect_error("<?php class Bad { public $a, int $b; }");
    assert_eq!(
        error.to_string(),
        "syntax error, unexpected identifier \"int\", expecting variable on line 1"
    );
}
