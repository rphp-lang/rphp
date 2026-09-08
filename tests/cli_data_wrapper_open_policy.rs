use std::process::Command;

// Original protocol specimens; stdout, stderr and exit status are captured
// independently against PHP, not copied from an upstream PHPT expectation.
fn check(name: &str, policy: &str, expected: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-d",
            &format!("allow_url_fopen={policy}"),
            "-d",
            "log_errors=0",
        ])
        .arg(format!(
            "{}/tests/fixtures/data_wrapper/{name}.php",
            env!("CARGO_MANIFEST_DIR")
        ))
        .output()
        .expect("CLI starts");
    assert_eq!(output.status.code(), Some(0), "{name}: {:?}", output.stderr);
    assert_eq!(output.stderr, b"", "{name}");
    assert_eq!(output.stdout, expected.as_bytes(), "{name}");
}

macro_rules! specimen {
    ($name:ident, $file:literal) => {
        #[test]
        fn $name() {
            check(
                $file,
                "1",
                include_str!(concat!("fixtures/data_wrapper/", $file, ".out")),
            );
        }
    };
}
specimen!(binary_open_cursor_and_read_consumers, "open");
specimen!(
    header_metadata_order_duplicates_and_detached_view,
    "metadata"
);
specimen!(invalid_header_and_reentrant_diagnostic_snapshot, "invalid");
specimen!(all_open_modes_keep_an_immutable_readable_payload, "modes");
specimen!(embedded_null_paths_fail_before_open, "null_path");
specimen!(null_scan_boundaries_preserve_error_priority, "null_scan");
specimen!(native_write_bytes_and_warning_snapshot, "write_snapshot");
#[cfg(feature = "stream-registry")]
specimen!(
    filter_write_keeps_the_original_string_owner,
    "filtered_write_snapshot"
);
specimen!(
    typed_float_calls_do_not_skip_required_widening,
    "float_argument"
);
specimen!(
    strict_float_widening_does_not_accept_strings,
    "float_argument_strict"
);
specimen!(
    native_open_fallback_keeps_reentrant_diagnostics,
    "native_open_error"
);

#[test]
fn throwing_policy_diagnostic_suppresses_later_warning_and_keeps_state() {
    check(
        "policy_exception",
        "0",
        include_str!("fixtures/data_wrapper/policy_exception.out"),
    );
    check(
        "null_path",
        "0",
        include_str!("fixtures/data_wrapper/null_path.out"),
    );
    check(
        "null_scan",
        "0",
        include_str!("fixtures/data_wrapper/null_scan.out"),
    );
}

#[cfg(feature = "stream-registry")]
#[test]
fn url_flag_denies_user_factories_but_not_local_wrappers() {
    check(
        "user_policy",
        "0",
        include_str!("fixtures/data_wrapper/user_policy.out"),
    );
}
#[cfg(all(
    feature = "file-lines",
    feature = "file-contents",
    feature = "stream-contents"
))]
specimen!(
    physical_line_and_offset_consumers_share_the_data_backend,
    "lines"
);

#[test]
fn startup_policy_is_visible_but_not_runtime_mutable() {
    check(
        "policy",
        "1",
        include_str!("fixtures/data_wrapper/policy-1.out"),
    );
}
#[test]
fn disabled_urls_do_not_disable_local_memory_streams() {
    check(
        "policy",
        "0",
        include_str!("fixtures/data_wrapper/policy-0.out"),
    );
}
#[cfg(all(feature = "stream-registry", feature = "file-contents"))]
#[test]
fn filter_open_checks_underlying_policy_before_factory() {
    check(
        "filter_policy",
        "0",
        include_str!("fixtures/data_wrapper/filter_policy-0.out"),
    );
    check(
        "filter_policy",
        "1",
        include_str!("fixtures/data_wrapper/filter_policy-1.out"),
    );
}
