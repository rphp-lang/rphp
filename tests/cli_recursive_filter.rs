use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/recursive_filter/expected.json")).unwrap();
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/recursive_filter/contract.php"
            ))
            .env("RPHP_RECURSIVE_FILTER_CASE", name)
            .env_remove("RPHP_DISABLE_JIT");
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected[name].as_str().unwrap(),
            "{name} JIT disabled={disable_jit}"
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
specimen!(metadata);
specimen!(callback_forms);
specimen!(lazy_selection);
specimen!(accept_direct);
specimen!(magic_callback);
specimen!(callback_identity);
specimen!(recursive_filter);
specimen!(recursive_children);
specimen!(subclass_factory);
specimen!(parent_filter);
specimen!(layered_filters);
specimen!(invalid_arguments);
specimen!(repeated_constructor);
specimen!(uninitialized);
specimen!(exception_state);
specimen!(by_reference);
specimen!(live_mutation);
specimen!(reentrant);
specimen!(retirement);
specimen!(callback_cycle);
specimen!(clone_and_serialize);
specimen!(private_scope);
specimen!(relative_scope);
specimen!(callback_reference_snapshot);
specimen!(child_constructor_throw);
specimen!(nested_callback_snapshot);
specimen!(unpack_arity);
specimen!(nullable_children);
specimen!(typed_callbacks);
specimen!(reference_warning_throw);
specimen!(relative_instance_scope);
specimen!(private_child_scope);
specimen!(coercion_retirement);
specimen!(argument_retirement);
specimen!(result_retirement);
specimen!(coercion_retirement_throw);
specimen!(failed_child_retirement);
specimen!(constructor_cache_contract);
specimen!(child_argument_retirement);
specimen!(callback_argument_errors);
specimen!(native_recursive_cursor_contract);
