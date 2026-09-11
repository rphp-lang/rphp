use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/fixed_array/expected.json")).unwrap();
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fixed_array/contract.php"
            ))
            .env("RPHP_FIXED_ARRAY_CASE", name);
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
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
specimen!(construction, "construction");
specimen!(resize, "resize");
specimen!(slots, "slots");
specimen!(offset_coercion, "offset-coercion");
specimen!(offset_mutation, "offset-mutation");
specimen!(from_array, "from-array");
specimen!(projection_cow, "projection-cow");
specimen!(references, "references");
specimen!(reference_assignment, "reference-assignment");
specimen!(compound, "compound");
specimen!(iteration, "iteration");
specimen!(iteration_reentry, "iteration-reentry");
specimen!(subclass, "subclass");
specimen!(uninitialized, "uninitialized");
specimen!(retirement, "retirement");
specimen!(resize_reentry, "resize-reentry");
specimen!(resize_throw, "resize-throw");
specimen!(throwing_coercion, "throwing-coercion");
specimen!(debug_projection, "debug-projection");
specimen!(overwrite_reentry, "overwrite-reentry");
specimen!(cycles, "cycles");
specimen!(evaluation_order, "evaluation-order");
specimen!(reflection, "reflection");
specimen!(print_projection, "print-projection");
specimen!(object_slot, "object-slot");
specimen!(exists_conversion, "exists-conversion");
specimen!(cursor_cycle, "cursor-cycle");
specimen!(overloaded_object_view, "overloaded-object-view");
specimen!(overflow_count, "overflow-count");
specimen!(member_projection, "member-projection");
specimen!(empty_native_override, "empty-native-override");

#[test]
fn overflowing_allocation_is_a_php_fatal_not_a_rust_panic() {
    // Safety/phase regression, not an allocation-limit or OOM-equivalence claim.
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/fixed_array/contract.php"
        ))
        .env("RPHP_FIXED_ARRAY_CASE", "overflow-allocation")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(255), "{output:?}");
    // VmError::Fatal currently uses the CLI fatal channel. This asserts the
    // checked failure boundary, not PHP's version-dependent fatal stack trace.
    let text = String::from_utf8([output.stdout, output.stderr].concat()).unwrap();
    assert!(text.starts_with("before:1\n\nFatal error: Possible integer overflow in memory allocation (2305843009213693953 * 16 + 0) in "), "{text}");
    assert!(!text.contains("unreachable") && !text.contains("panicked"));
}
