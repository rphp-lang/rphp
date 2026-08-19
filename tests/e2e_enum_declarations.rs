mod common;

use common::{run_php, run_php_expect_error, run_php_expect_error_with_source_context};

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
