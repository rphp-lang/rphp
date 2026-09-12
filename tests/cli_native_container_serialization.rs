use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/container_serialization/expected.json"
    ))
    .unwrap();
    for disable_jit in [false, true] {
        let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/container_serialization/contract.php"
            ))
            .env("RPHP_CONTAINER_SERIALIZATION_CASE", name)
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

specimen!(projection, "projection");
specimen!(partial_validation, "partial-validation");
specimen!(flags, "flags");
specimen!(append_restore, "append-restore");
specimen!(members, "members");
specimen!(references, "references");
specimen!(identity_cycles, "identity-cycles");
specimen!(locks, "locks");
specimen!(metadata, "metadata");
specimen!(deque_projection, "deque-projection");
specimen!(deque_validation, "deque-validation");
specimen!(deque_members, "deque-members");
specimen!(deque_references, "deque-references");
specimen!(legacy_roundtrip, "legacy-roundtrip");
specimen!(legacy_validation, "legacy-validation");
specimen!(legacy_graph, "legacy-graph");
specimen!(legacy_callbacks, "legacy-callbacks");
specimen!(legacy_retirement, "legacy-retirement");
specimen!(clone_references, "clone-references");
specimen!(member_retirement, "member-retirement");
specimen!(member_errors, "member-errors");
specimen!(override_hooks, "override-hooks");
specimen!(collected_cycles, "collected-cycles");
