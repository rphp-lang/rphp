#[test]
fn constant_names_survive_autoload_and_reference_call_fallbacks() {
    for disable_jit in [false, true] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rphp"));
        command.arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/static_call_name_ownership.php"
        ));
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(
            output.stdout,
            b"3,4,5,6,6\nNameOwnerChild:NameOwnerChild:NameOwnerChild\nNameOwnerChild\nload:DeferredNameOwner\n7:UnrelatedName:replaced\n8\nError:8\nError:8\nError:8\nError:8\n9\n"
        );
    }
}
