use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/object_storage/expected.json")).unwrap();
    for disable_jit in [false, true] {
        let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/object_storage/contract.php"
            ))
            .env("RPHP_OBJECT_STORAGE_CASE", name)
            .env("RPHP_DISABLE_JIT", if disable_jit { "1" } else { "0" })
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
        assert_eq!(
            output.stdout,
            expected[name].as_str().unwrap().as_bytes(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
macro_rules! specimen {
    ($name:ident, $case:literal) => {
        #[test]
        fn $name() {
            contract($case);
        }
    };
}
specimen!(empty, "empty");
specimen!(identity, "identity");
specimen!(closure_keys, "closure-keys");
specimen!(comparison, "comparison");
specimen!(comparison_reentry, "comparison-reentry");
specimen!(metadata, "metadata");
specimen!(cursor, "cursor");
specimen!(cursor_removal, "cursor-removal");
specimen!(hash_collisions, "hash-collisions");
specimen!(unset_overrides, "unset-overrides");
specimen!(hash_failure, "hash-failure");
specimen!(invalid_arguments, "invalid-arguments");
specimen!(bulk, "bulk");
specimen!(bulk_reentry, "bulk-reentry");
specimen!(live_iteration, "live-iteration");
specimen!(references_clone, "references-clone");
specimen!(debug_projection, "debug-projection");
specimen!(modern_restore, "modern-restore");
specimen!(partial_restore, "partial-restore");
specimen!(members, "members");
specimen!(retirement, "retirement");
specimen!(retirement_reentry, "retirement-reentry");
specimen!(cycles, "cycles");
specimen!(deprecated_aliases, "deprecated-aliases");
specimen!(scalar_property_boundary, "scalar-property-boundary");
specimen!(method_lookup_boundary, "method-lookup-boundary");
specimen!(wire_integer_boundary, "wire-integer-boundary");
specimen!(numeric_assignment_boundary, "numeric-assignment-boundary");
specimen!(owned_assignment_boundary, "owned-assignment-boundary");
specimen!(
    scalar_call_operands_boundary,
    "scalar-call-operands-boundary"
);
specimen!(
    constructor_publication_boundary,
    "constructor-publication-boundary"
);
specimen!(
    primitive_assignment_boundary,
    "primitive-assignment-boundary"
);
