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
            "/tests/fixtures/array_object_serialization/contract.php"
        ))
        .env("RPHP_ARRAY_SERIALIZATION_CASE", name)
        .output()
        .expect("native serialization specimen");
    assert_eq!(result.status.code(), Some(0), "{name}: {:?}", result);
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
                    "fixtures/array_object_serialization/",
                    $case,
                    ".expected"
                )),
            );
        }
    };
}

specimen!(modern_state, "modern-state");
specimen!(modern_validation, "modern-validation");
specimen!(legacy_state, "legacy-state");
specimen!(legacy_validation, "legacy-validation");
specimen!(reference_identity, "reference-identity");
specimen!(binary_state, "binary-state");
specimen!(sorting_guard, "sorting-guard");
specimen!(callback_state, "callback-state");
specimen!(legacy_reference_table, "legacy-reference-table");
specimen!(legacy_callback_order, "legacy-callback-order");
specimen!(self_state, "self-state");
specimen!(wire_identity, "wire-identity");
specimen!(validation_mutation, "validation-mutation");
specimen!(legacy_diagnostic_origin, "legacy-diagnostic-origin");
specimen!(member_isolation, "member-isolation");
specimen!(member_release, "member-release");
specimen!(stream_byte_provenance, "stream-byte-provenance");
specimen!(readonly_members, "readonly-members");
