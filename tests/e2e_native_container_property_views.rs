mod common;

#[test]
fn native_hook_metadata_and_general_unserialize_array_keys_match_php() {
    assert_eq!(
        common::run_php(include_str!("fixtures/native_property_views/hook-keys.php")),
        include_str!("fixtures/native_property_views/hook-keys.out"),
    );
}

#[test]
fn fixed_slots_have_distinct_array_and_serialized_property_views() {
    assert_eq!(
        common::run_php(include_str!(
            "fixtures/native_property_views/fixed-views.php"
        )),
        include_str!("fixtures/native_property_views/fixed-views.out"),
    );
}

#[test]
fn fixed_slot_restore_preserves_reference_and_error_publication_boundaries() {
    assert_eq!(
        common::run_php(include_str!(
            "fixtures/native_property_views/fixed-restore.php"
        )),
        include_str!("fixtures/native_property_views/fixed-restore.out"),
    );
}

#[test]
fn native_debug_properties_hide_self_storage_and_preserve_overrides() {
    assert_eq!(
        common::run_php(include_str!(
            "fixtures/native_property_views/native-debug.php"
        )),
        include_str!("fixtures/native_property_views/native-debug.out"),
    );
}

#[test]
fn print_projection_dispatches_debug_hooks_with_recursion() {
    assert_eq!(
        common::run_php(include_str!(
            "fixtures/native_property_views/debug-print.php"
        )),
        include_str!("fixtures/native_property_views/debug-print.out"),
    );
}

#[test]
fn ordinary_print_reads_live_declared_slots_and_snapshots_dynamic_members() {
    assert_eq!(
        common::run_php(include_str!(
            "fixtures/native_property_views/ordinary-print.php"
        )),
        include_str!("fixtures/native_property_views/ordinary-print.out"),
    );
}

#[test]
fn invalid_print_debug_projection_is_an_engine_fatal() {
    for body in ["return 7;", "throw new Exception('print stopped');"] {
        let source = format!(
            "<?php class InvalidPrint {{ function __debugInfo() {{ {body} }} }} try {{ print_r(new InvalidPrint, true); }} catch (Throwable $e) {{ echo 'not catchable'; }}"
        );
        assert!(matches!(
            common::run_php_expect_error(&source),
            rphp::vm::execute::VmError::Fatal(message)
                if message.starts_with("__debuginfo() must return an array in ")
        ));
    }
}

#[test]
fn debug_deprecation_exception_obeys_each_output_boundary() {
    for mode in ["print", "dump"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_rphp"))
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/native_property_views/debug-error.php"
            ))
            .env("RPHP_DEBUG_ERROR_MODE", mode)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(if mode == "print" { 255 } else { 0 })
        );
        // Explicit display_errors=1 publishes both the warning and the engine
        // fatal on stdout, matching PHP 8.5's projection/output boundary.
        let diagnostic = String::from_utf8(output.stderr).unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.starts_with("first\ndiagnostic\n"), "{text}");
        assert!(!text.contains("second\n"));
        if mode == "print" {
            assert!(diagnostic.is_empty(), "{diagnostic}");
            assert!(text.contains("\nFatal error: __debuginfo() must return an array"));
            assert!(text.contains("Warning: Uncaught Exception: debug stopped"));
            assert!(text.find("Warning: Uncaught").unwrap() < text.find("Fatal error:").unwrap());
            assert!(!text.contains("caught:"));
            assert!(!text.contains("end\n"));
        } else {
            assert!(diagnostic.is_empty());
            assert!(text.contains("object(EmptyDebugResult)#"));
            assert!(
                text.ends_with(" (0) {\n}\ncaught:debug stopped\nend\n"),
                "{text}"
            );
        }
    }
}
