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
            "/tests/fixtures/iterator_delegation/contract.php"
        ))
        .env("RPHP_DELEGATION_CASE", name)
        .output()
        .expect("delegated iterator specimen");
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
                include_bytes!(concat!("fixtures/iterator_delegation/", $case, ".expected")),
            );
        }
    };
}
specimen!(cache, "cache");
specimen!(native_cache, "native-cache");
specimen!(limits, "limits");
specimen!(seek_protocol, "seek-protocol");
specimen!(uninitialized, "uninitialized");
specimen!(reinitialize, "reinitialize");
specimen!(aggregates, "aggregates");
specimen!(exceptions, "exceptions");
specimen!(metadata, "metadata");
specimen!(release, "release");
specimen!(references_cycles, "references-cycles");
specimen!(overrides, "overrides");
specimen!(forwarding, "forwarding");
specimen!(interface_projection, "interface-projection");
specimen!(construction_reentry, "construction-reentry");
specimen!(uninitialized_forwarding, "uninitialized-forwarding");
specimen!(abstract_metadata, "abstract-metadata");
