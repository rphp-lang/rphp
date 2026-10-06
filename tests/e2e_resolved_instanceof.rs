mod common;
use common::run_php;

#[test]
fn declared_relations_preserve_receiver_changes_aliases_references_and_lazy_scope() {
    assert_eq!(
        run_php(include_str!("fixtures/resolved_instanceof/contracts.php")),
        include_str!("fixtures/resolved_instanceof/contracts.out"),
    );
}
