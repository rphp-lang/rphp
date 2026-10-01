mod common;

use common::run_php;

fn check_compact_and_wide_frames(body: &str, expected: &str) {
    let body = body.strip_prefix("<?php").expect("a PHP fixture");
    for padding in [0, 70] {
        let padding = (0..padding)
            .map(|index| format!("$padding{index} = {index};"))
            .collect::<String>();
        let source = format!("<?php function exercise() {{{padding}{body}}} exercise();");
        assert_eq!(run_php(&source), expected);
    }
}

#[test]
fn exact_return_inspection_preserves_real_array_and_object_owners() {
    check_compact_and_wide_frames(
        include_str!("fixtures/return_type_storage/exact.php"),
        "1:1:7|array-live:7|drop|gone:1|",
    );
}

#[test]
fn failed_exact_check_holds_the_source_through_reentrant_conversion() {
    check_compact_and_wide_frames(
        include_str!("fixtures/return_type_storage/coercion.php"),
        "convert|snapshot:1|drop|converted:1|",
    );
}
