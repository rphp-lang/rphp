use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/file_object/expected.json")).unwrap();
    for disable_jit in [false, true] {
        let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/file_object/contract.php"
            ))
            .env("RPHP_FILE_OBJECT_CASE", name)
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
specimen!(metadata, "metadata");
specimen!(empty_and_termination, "empty-and-termination");
specimen!(cached_current, "cached-current");
specimen!(advance_before_fetch, "advance-before-fetch");
specimen!(flags, "flags");
specimen!(flag_transitions, "flag-transitions");
specimen!(seek, "seek");
specimen!(seek_direct_read, "seek-direct-read");
specimen!(line_length, "line-length");
specimen!(get_current_line, "get-current-line");
specimen!(binary_lines, "binary-lines");
specimen!(arguments_preserve_state, "arguments-preserve-state");
specimen!(constructor_errors, "constructor-errors");
specimen!(reconstruction, "reconstruction");
specimen!(string_projection, "string-projection");
specimen!(direct_eof_error, "direct-eof-error");
specimen!(inherited_metadata, "inherited-metadata");
specimen!(override_line, "override-line");
specimen!(override_error, "override-error");
specimen!(override_invalid_return, "override-invalid-return");
specimen!(lifetime_cycle, "lifetime-cycle");
#[cfg(feature = "stream-registry")]
specimen!(wrapper_order, "wrapper-order");
#[cfg(feature = "stream-registry")]
specimen!(wrapper_stat_missing, "wrapper-stat-missing");
#[cfg(feature = "stream-registry")]
specimen!(wrapper_cycle, "wrapper-cycle");
