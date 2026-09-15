mod common;

#[test]
fn radix_decoders_preserve_digits_prefixes_bytes_and_float_overflow() {
    assert_eq!(
        common::run_php(include_str!("fixtures/radix/conversion-contracts.php")),
        include_str!("fixtures/radix/conversion-contracts.out"),
    );
}

#[test]
fn radix_calls_preserve_php_types_names_reflection_and_diagnostics() {
    assert_eq!(
        common::run_php(include_str!("fixtures/radix/call-contracts.php")),
        include_str!("fixtures/radix/call-contracts.out"),
    );
}

#[test]
fn radix_diagnostic_and_conversion_failures_preserve_state_and_reentry() {
    assert_eq!(
        common::run_php(include_str!("fixtures/radix/lifecycle-contracts.php")),
        include_str!("fixtures/radix/lifecycle-contracts.out"),
    );
}
