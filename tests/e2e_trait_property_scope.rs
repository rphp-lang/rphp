mod common;

#[test]
fn receiver_scope_and_target_class_are_independent() {
    assert_eq!(
        common::run_php(include_str!("fixtures/trait_property_scope/receiver.php")),
        include_str!("fixtures/trait_property_scope/receiver.out")
    );
}

#[test]
fn hidden_scope_static_read_unset_and_readonly_remain_canonical() {
    assert_eq!(
        common::run_php(include_str!("fixtures/trait_property_scope/fallback.php")),
        include_str!("fixtures/trait_property_scope/fallback.out")
    );
}
