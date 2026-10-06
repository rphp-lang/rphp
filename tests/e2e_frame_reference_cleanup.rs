mod common;

use common::run_php;

#[test]
fn returned_reference_cells_survive_compact_and_wide_frame_cleanup() {
    for padding in [0, 75] {
        let mut source = String::from(
            "<?php class ReferenceReturnOwner { public int $marker = 7; function __destruct() { echo 'drop|'; } } function &returnReferenceCell() { $owner = new ReferenceReturnOwner; $alias =& $owner;\n",
        );
        for index in 0..padding {
            source.push_str(&format!("$padding{index} = {index};\n"));
        }
        source.push_str("return $alias; } $held =& returnReferenceCell(); echo 'live:', $held->marker, '|'; $copy = $held; unset($held); echo 'copy|'; unset($copy); echo 'after';");
        assert_eq!(
            run_php(&source),
            "live:7|copy|drop|after",
            "{padding} padding CVs"
        );
    }
}
