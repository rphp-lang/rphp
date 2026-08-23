mod common;
use common::{run_php, run_php_with_source_context};

#[test]
fn internal_variadics_reject_unknown_named_arguments() {
    assert_eq!(
        run_php(
            r#"<?php
try { array_merge([1], extra: [2]); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
try { array_diff_key([1], [2], extra: [3]); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
try { call_user_func_array('sprintf', ['format' => '%s', 'extra' => 'x']); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "ArgumentCountError:array_merge() does not accept unknown named parameters\n",
            "ArgumentCountError:array_diff_key() does not accept unknown named parameters\n",
            "ArgumentCountError:sprintf() does not accept unknown named parameters\n",
        ),
    );
}

#[test]
fn array_keys_requires_an_explicit_filter_before_strict() {
    assert_eq!(
        run_php(
            r#"<?php
try { array_keys([], strict: true); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
echo json_encode(array_keys(['x' => 41, 'y' => 42], filter_value: 42, strict: true));
"#,
        ),
        concat!(
            "ArgumentCountError:array_keys(): Argument #2 ($filter_value) must be passed explicitly, because the default value is not known\n",
            "[\"y\"]",
        ),
    );
}

#[test]
fn call_user_func_array_preentry_error_retains_named_variadic_trace() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
try {
    call_user_func_array('array_multisort', ['' => 1]);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
    echo $error->getTraceAsString();
}
"#,
            "/fixture/named-internal-options.php",
            "/fixture",
        ),
        concat!(
            "ArgumentCountError:array_multisort() expects at least 1 argument, 0 given\n",
            "#0 /fixture/named-internal-options.php(3): array_multisort(: 1)\n",
            "#1 {main}",
        ),
    );
}
