mod common;

#[test]
fn contextual_returns_keep_scopes_alias_publication_and_references() {
    assert_eq!(
        common::run_php_with_source_context(
            include_str!("fixtures/type_scope_context.php"),
            "/fixture/type_scope_context.php",
            "/fixture",
        ),
        include_str!("fixtures/type_scope_context.out")
    );
}
