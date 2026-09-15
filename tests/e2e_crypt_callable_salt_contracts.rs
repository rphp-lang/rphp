mod common;

// Original, synthetic PHP 8.5 oracle cases. No password here is a credential.
#[test]
fn legacy_hash_families_preserve_bytes_nuls_and_failure_sentinels() {
    assert_eq!(
        common::run_php(include_str!("fixtures/crypt/hash-contracts.php")),
        include_str!("fixtures/crypt/hash-contracts.out"),
    );
}

#[test]
fn modular_salts_and_rounds_preserve_the_php_parser_contract() {
    assert_eq!(
        common::run_php(include_str!("fixtures/crypt/salt-contracts.php")),
        include_str!("fixtures/crypt/salt-contracts.out"),
    );
}

#[test]
fn crypt_calls_preserve_types_names_evaluation_order_and_sensitive_traces() {
    assert_eq!(
        common::run_php(include_str!("fixtures/crypt/call-contracts.php")),
        include_str!("fixtures/crypt/call-contracts.out"),
    );
}

#[test]
fn crypt_nested_calls_failures_and_long_inputs_preserve_state() {
    assert_eq!(
        common::run_php(include_str!("fixtures/crypt/lifecycle-contracts.php")),
        include_str!("fixtures/crypt/lifecycle-contracts.out"),
    );
}
