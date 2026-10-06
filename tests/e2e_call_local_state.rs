mod common;

#[test]
fn nested_invokable_and_magic_arguments_keep_receiver_and_error_order() {
    assert_eq!(
        common::run_php_with_source_context(
            include_str!("fixtures/call_side_state/nested.php"),
            "/fixture/call-side-state/nested.php",
            "/fixture/call-side-state",
        ),
        include_str!("fixtures/call_side_state/nested.out")
    );
}

#[test]
fn wide_late_static_owner_keeps_scope_around_ordinary_calls() {
    assert_eq!(
        common::run_php_with_source_context(
            include_str!("fixtures/call_side_state/wide_scopes.php"),
            "/fixture/call-side-state/wide_scopes.php",
            "/fixture/call-side-state",
        ),
        include_str!("fixtures/call_side_state/wide_scopes.out")
    );
}

#[test]
fn suspended_argument_keeps_its_pending_invocation_owner() {
    assert_eq!(
        common::run_php(include_str!("fixtures/call_side_state/fiber.php")),
        include_str!("fixtures/call_side_state/fiber.out")
    );
}
