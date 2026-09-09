use std::process::Command;

fn contract(name: &str, expected: &[u8]) {
    let result = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-d",
            "display_errors=1",
            "-d",
            "log_errors=0",
            "-d",
            "error_reporting=32767",
        ])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/array_iterator_cursor/contract.php"
        ))
        .env("RPHP_ITERATOR_CASE", name)
        .output()
        .expect("native iterator cursor specimen");
    assert_eq!(
        result.status.code(),
        Some(0),
        "{name}: {}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(result.stderr.is_empty(), "{name}: {:?}", result.stderr);
    assert_eq!(
        result.stdout,
        expected,
        "{name}: {}",
        String::from_utf8_lossy(&result.stdout)
    );
}
macro_rules! specimen {
    ($name:ident, $case:literal) => {
        #[test]
        fn $name() {
            contract(
                $case,
                include_bytes!(concat!(
                    "fixtures/array_iterator_cursor/",
                    $case,
                    ".expected"
                )),
            );
        }
    };
}
specimen!(position, "position");
specimen!(seek, "seek");
specimen!(mutation, "mutation");
specimen!(shared, "shared");
specimen!(clone, "clone");
specimen!(nested, "nested");
specimen!(references, "references");
specimen!(live_foreach, "live-foreach");
specimen!(object, "object");
specimen!(object_unset, "object-unset");
specimen!(byte_keys, "byte-keys");
specimen!(overrides, "overrides");
specimen!(drivers, "drivers");
specimen!(exceptions, "exceptions");
specimen!(driver_contract, "driver-contract");
specimen!(metadata, "metadata");
specimen!(callback_binding, "callback-binding");
specimen!(projection_references, "projection-references");
specimen!(release_boundary, "release-boundary");
specimen!(apply_release, "apply-release");
specimen!(method_projections, "method-projections");
specimen!(foreach_release_projections, "foreach-release-projections");
specimen!(foreach_keyless_projections, "foreach-keyless-projections");
specimen!(
    foreach_created_reference_sources,
    "foreach-created-reference-sources"
);
