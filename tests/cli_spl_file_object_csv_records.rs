use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/file_object_csv/expected.json")).unwrap();
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/file_object_csv/contract.php"
            ))
            .env("RPHP_FILE_CSV_CASE", name)
            .env_remove("RPHP_DISABLE_JIT");
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
specimen!(metadata, "metadata");
specimen!(control_state, "control-state");
specimen!(control_errors, "control-errors");
specimen!(read_records, "read-records");
specimen!(multiline, "multiline");
specimen!(empty_eof, "empty-eof");
specimen!(iterator_flags, "iterator-flags");
specimen!(cache_transitions, "cache-transitions");
specimen!(seek_length, "seek-length");
specimen!(binary_controls, "binary-controls");
specimen!(default_diagnostics, "default-diagnostics");
specimen!(diagnostic_exception, "diagnostic-exception");
specimen!(argument_order, "argument-order");
specimen!(strict_arguments, "strict-arguments");
specimen!(write_records, "write-records");
specimen!(write_controls, "write-controls");
specimen!(write_errors, "write-errors");
specimen!(write_field_conversion, "write-field-conversion");
specimen!(deprecation_reentry, "deprecation-reentry");
specimen!(string_projection, "string-projection");
specimen!(mixed_error_priority, "mixed-error-priority");
specimen!(write_only_read, "write-only-read");
specimen!(memory_write_permissions, "memory-write-permissions");
specimen!(empty_record_write, "empty-record-write");
specimen!(write_reentry, "write-reentry");
specimen!(cache_cow, "cache-cow");
#[cfg(feature = "stream-registry")]
specimen!(wrapper_byte_records, "wrapper-byte-records");
#[cfg(feature = "stream-registry")]
specimen!(wrapper_write_exception, "wrapper-write-exception");
#[cfg(feature = "stream-registry")]
specimen!(wrapper_interrupted_write, "wrapper-interrupted-write");
#[cfg(feature = "stream-registry")]
specimen!(wrapper_record_chunks, "wrapper-record-chunks");
#[cfg(feature = "stream-registry")]
specimen!(wrapper_partial_write, "wrapper-partial-write");
#[cfg(feature = "stream-registry")]
specimen!(wrapper_read, "wrapper-read");
#[cfg(feature = "stream-registry")]
specimen!(wrapper_write, "wrapper-write");
#[cfg(feature = "stream-registry")]
specimen!(wrapper_failure, "wrapper-failure");
