/// Tests for named arguments (PHP 8 style)
mod common;
use common::{run_php, run_php_expect_error_with_source_context};

#[test]
fn positional_after_named_is_compile_time_across_call_contexts() {
    for (source, line) in [
        ("<?php\nif (false) {\n    dispatch(first: 1, 2);\n}", 3),
        ("<?php\nsink(array_slice(array: $source, 1, 2));", 2),
        (
            "<?php\n#[Attribute]\nclass Marker {}\n#[Marker(first: 1, 2)]\nclass Subject {}",
            4,
        ),
        (
            "<?php\nfunction holder() {\n    static $value = new stdClass(first: 1, 2);\n}",
            3,
        ),
    ] {
        let error = run_php_expect_error_with_source_context(
            source,
            "/fixture/positional-after-named.php",
            "/fixture",
        );
        assert_eq!(
            error.to_string(),
            format!(
                "Cannot use positional argument after named argument in /fixture/positional-after-named.php on line {line}"
            ),
            "{source}",
        );
    }
}

include!("e2e_named_args/basic_calls.rs");

include!("e2e_named_args/references_and_internal_functions.rs");

include!("e2e_named_args/duplicates_keywords_and_variadics.rs");

include!("e2e_named_args/variadic_errors_and_recovery.rs");

include!("e2e_named_args/reused_and_nested_frames.rs");
