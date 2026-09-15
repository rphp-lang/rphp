mod common;

#[test]
fn autoload_exceptions_retain_the_live_internal_or_user_origin() {
    assert_eq!(
        common::run_php_with_source_context(
            include_str!("fixtures/autoload_public/trace-contracts.php"),
            "autoload-trace.php",
            ".",
        ),
        include_str!("fixtures/autoload_public/trace-contracts.out"),
    );
}

#[test]
fn autoload_diagnostics_preserve_php_order_and_commit_boundaries() {
    assert_eq!(
        common::run_php(include_str!(
            "fixtures/autoload_public/diagnostic-contracts.php"
        )),
        include_str!("fixtures/autoload_public/diagnostic-contracts.out"),
    );
}

#[test]
fn autoload_registration_uses_resolved_identity_and_shared_closure_cells() {
    assert_eq!(
        common::run_php(include_str!(
            "fixtures/autoload_public/identity-contracts.php"
        )),
        include_str!("fixtures/autoload_public/identity-contracts.out"),
    );
}

#[test]
fn autoload_walk_observes_live_mutation_and_unwinds_reentrant_guards() {
    assert_eq!(
        common::run_php(include_str!("fixtures/autoload_public/live-contracts.php")),
        include_str!("fixtures/autoload_public/live-contracts.out"),
    );
}
