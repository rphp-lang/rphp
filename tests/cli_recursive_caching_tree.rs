use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/recursive_tree/expected.json")).unwrap();
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/recursive_tree/contract.php"
            ))
            .env("RPHP_RECURSIVE_TREE_CASE", name)
            .env_remove("RPHP_DISABLE_JIT");
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
        assert!(
            output.stdout == expected[name].as_str().unwrap().as_bytes(),
            "{name}: expected {:?}\nactual {:?}",
            expected[name].as_str().unwrap(),
            String::from_utf8_lossy(&output.stdout)
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
specimen!(cache_cursor);
specimen!(child_identity);
specimen!(flags);
specimen!(tree_modes);
specimen!(tree_prefix);
specimen!(tree_empty);
specimen!(aggregate);
specimen!(arguments);
specimen!(strict);
specimen!(uninitialized);
specimen!(repeat);
specimen!(callback_order);
specimen!(child_throw);
specimen!(invalid_child);
specimen!(conversion_throw);
specimen!(overrides);
specimen!(full_cache);
specimen!(reference_cow);
specimen!(retirement);
specimen!(cycles);
specimen!(serialization);
specimen!(child_subclass);
specimen!(state_reentry);
specimen!(cache_exception_order);
specimen!(retirement_reset);
specimen!(serialization_hooks);
specimen!(interface_ancestry);

fn static_contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/recursive_tree/static_expected.json")).unwrap();
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/recursive_tree/static_calls.php"
            ))
            .env("RPHP_STATIC_GUARD_CASE", name)
            .env_remove("RPHP_DISABLE_JIT");
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
        assert_eq!(output.stdout, expected[name].as_str().unwrap().as_bytes());
    }
}

#[test]
fn cached_static_values_and_failed_plan_are_exact() {
    static_contract("values");
}
#[test]
fn cached_static_scope_and_receiver_guards_are_exact() {
    static_contract("scope");
}
#[test]
fn cached_static_arguments_are_not_replayed() {
    static_contract("arguments");
}
#[test]
fn cached_static_magic_trait_and_dynamic_fallbacks_are_exact() {
    static_contract("fallback");
}
