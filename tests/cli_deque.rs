use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/deque/expected.json")).unwrap();
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/deque/contract.php"
            ))
            .env("RPHP_DEQUE_CASE", name);
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
        // Expand only the fixture location; diagnostic lines and every output
        // byte are still checked without publishing a machine-specific path.
        let expected_stdout = expected[name].as_str().unwrap().replace(
            "{{fixture}}",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/deque/contract.php"
            ),
        );
        assert_eq!(
            output.stdout,
            expected_stdout.as_bytes(),
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
specimen!(ends, "ends");
specimen!(
    arithmetic_projection_boundaries,
    "arithmetic-projection-boundaries"
);
specimen!(
    property_result_slot_boundaries,
    "property-result-slot-boundaries"
);
specimen!(call_type_scope_boundaries, "call-type-scope-boundaries");
specimen!(empty, "empty");
specimen!(offsets, "offsets");
specimen!(add, "add");
specimen!(modes, "modes");
specimen!(manual_cursor, "manual-cursor");
specimen!(cursor_mutations, "cursor-mutations");
specimen!(foreach, "foreach");
specimen!(foreach_mutation, "foreach-mutation");
specimen!(mode_during_iteration, "mode-during-iteration");
specimen!(clone, "clone");
specimen!(references, "references");
specimen!(compound, "compound");
specimen!(retirement, "retirement");
specimen!(reentry, "reentry");
specimen!(cycles, "cycles");
specimen!(projection, "projection");
specimen!(subclass, "subclass");
specimen!(consumer_projections, "consumer-projections");
specimen!(invisible_cursor, "invisible-cursor");
specimen!(prev_delete, "prev-delete");
specimen!(empty_coercion, "empty-coercion");
specimen!(append_contexts, "append-contexts");
specimen!(strict_indices, "strict-indices");
specimen!(delete_reentry, "delete-reentry");
specimen!(reference_append_owning_slot, "reference-append-owning-slot");
specimen!(
    reference_append_getter_contract,
    "reference-append-getter-contract"
);
specimen!(delete_mode_transition, "delete-mode-transition");
specimen!(retired_cursor_slot_reuse, "retired-cursor-slot-reuse");
specimen!(constructor_proof_boundaries, "constructor-proof-boundaries");
specimen!(scalar_call_proof_boundaries, "scalar-call-proof-boundaries");
specimen!(
    property_fetch_consumer_boundaries,
    "property-fetch-consumer-boundaries"
);
