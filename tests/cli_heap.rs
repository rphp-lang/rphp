use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/heap/expected.json")).unwrap();
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/heap/contract.php"
            ))
            .env("RPHP_HEAP_CASE", name);
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

#[test]
fn inherited_registration_preserves_canonical_names_owners_and_hooks() {
    for disabled in [false, true] {
        let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/heap/inherited_registration.php"
            ))
            .env("RPHP_DISABLE_JIT", if disabled { "1" } else { "0" })
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(
            output.stdout,
            include_bytes!("fixtures/heap/inherited_registration.out")
        );
    }
}
specimen!(empty, "empty");
specimen!(order, "order");
specimen!(ties, "ties");
specimen!(flags, "flags");
specimen!(compare, "compare");
specimen!(corrupt_insert, "corrupt-insert");
specimen!(corrupt_extract, "corrupt-extract");
specimen!(write_lock, "write-lock");
specimen!(read_lock, "read-lock");
specimen!(iteration, "iteration");
specimen!(nested_iteration, "nested-iteration");
specimen!(rewind, "rewind");
specimen!(references_clone, "references-clone");
specimen!(object_identity, "object-identity");
specimen!(retirement, "retirement");
specimen!(reentrant_retirement, "reentrant-retirement");
specimen!(subclass_iterator, "subclass-iterator");
specimen!(metadata, "metadata");
specimen!(named_arity, "named-arity");
specimen!(debug_info, "debug-info");
specimen!(cycles, "cycles");
specimen!(clone_corrupt, "clone-corrupt");
specimen!(conversion_lock, "conversion-lock");
specimen!(conversion_failure, "conversion-failure");
specimen!(compare_visibility, "compare-visibility");
specimen!(nested_retirement, "nested-retirement");
specimen!(retirement_resurrection, "retirement-resurrection");
specimen!(mutation_views, "mutation-views");
