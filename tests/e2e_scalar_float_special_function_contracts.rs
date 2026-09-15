mod common;

#[test]
fn scalar_float_operations_preserve_domain_boundaries_and_exact_bits() {
    assert_eq!(
        common::run_php(include_str!("fixtures/scalar_float/value-contracts.php")),
        include_str!("fixtures/scalar_float/value-contracts.out"),
    );
}

#[test]
fn scalar_float_calls_preserve_coercion_names_metadata_and_errors() {
    assert_eq!(
        common::run_php(include_str!("fixtures/scalar_float/call-contracts.php")),
        include_str!("fixtures/scalar_float/call-contracts.out"),
    );
}

#[test]
fn scalar_float_failures_preserve_references_order_and_reentrant_state() {
    assert_eq!(
        common::run_php(include_str!("fixtures/scalar_float/state-contracts.php")),
        include_str!("fixtures/scalar_float/state-contracts.out"),
    );
}
