use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/array_object_projection/expected.json"
    ))
    .unwrap();
    for disabled in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/array_object_projection/contract.php"
            ))
            .env("RPHP_ARRAY_OBJECT_PROJECTION_CASE", name)
            .env_remove("RPHP_DISABLE_JIT");
        if disabled {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected[name].as_str().unwrap(),
            "{name} JIT disabled={disabled}"
        );
    }
}
macro_rules! specimen {
    ($name:ident) => {
        #[test]
        fn $name() {
            contract(stringify!($name));
        }
    };
}
specimen!(enum_admission);
specimen!(overloaded_admission);
specimen!(legacy_admission);
specimen!(diagnostic_abort);
specimen!(numeric_property_keys);
specimen!(declared_value_sort);
specimen!(dynamic_key_sort);
specimen!(nested_and_self_sort);
specimen!(sort_references_cow);
specimen!(sort_comparison_throw);
specimen!(sort_reentry);
specimen!(lazy_proxy_current);
specimen!(lazy_proxy_retry);
specimen!(lazy_projection);
specimen!(raw_slot_constraints);
specimen!(array_backing_control);
specimen!(legacy_rejection_members);
specimen!(sorted_object_surfaces);
specimen!(sorted_table_retirement);
specimen!(lazy_foreach_throw);
specimen!(native_payload_trace);
specimen!(sorted_output_views);
specimen!(sorted_cycle_gc);
specimen!(lazy_sort_argument_order);
specimen!(sorted_hook_iteration);
