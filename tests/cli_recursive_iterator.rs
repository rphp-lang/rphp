use std::process::Command;

fn contract(name: &str, expected: &[u8]) {
    let result = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/recursive_iterator/contract.php"
        ))
        .env("RPHP_RECURSIVE_CASE", name)
        .output()
        .expect("recursive iterator contract");
    assert_eq!(result.status.code(), Some(0), "{name}: {result:?}");
    assert!(result.stderr.is_empty(), "{name}: {:?}", result.stderr);
    assert_eq!(
        result.stdout,
        expected,
        "{name}: {}",
        String::from_utf8_lossy(&result.stdout)
    );
}

macro_rules! specimen {
    ($test:ident, $case:literal) => {
        #[test]
        fn $test() {
            contract(
                $case,
                include_bytes!(concat!("fixtures/recursive_iterator/", $case, ".expected")),
            );
        }
    };
}
specimen!(modes, "modes");
specimen!(depth, "depth");
specimen!(child_state, "child-state");
specimen!(child_object, "child-object");
specimen!(reference, "reference");
specimen!(subiterator, "subiterator");
specimen!(uninitialized, "uninitialized");
specimen!(constructor, "constructor");
specimen!(hooks, "hooks");
specimen!(hook_errors, "hook-errors");
specimen!(child_exception, "child-exception");
specimen!(overrides, "override");
specimen!(reflection, "reflection");
specimen!(reentry, "reentry");
specimen!(advance_error, "advance-error");
specimen!(replace_during_unwind, "replace-during-unwind");
specimen!(retirement, "retirement");
specimen!(invalid_child_retirement, "invalid-child-retirement");
specimen!(argument_reference_proof, "argument-reference-proof");
specimen!(scalar_evaluation_proof, "scalar-evaluation-proof");
specimen!(property_reference_proof, "property-reference-proof");
