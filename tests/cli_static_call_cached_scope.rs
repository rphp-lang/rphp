use std::process::Command;

#[test]
fn cached_concrete_scope_preserves_alias_forwarding_trait_magic_and_receiver_identity() {
    let expected = concat!(
        "ScopeRoot/ScopeLeft:1\nScopeRoot/ScopeLeft:2\nScopeRoot/ScopeRight:3\n",
        "ScopeRoot/ScopeLeft:4\nScopeRoot/ScopeLeft:5\nScopeRoot/ScopeRight:6\n",
        "ScopeRoot/ScopeLeft:7\nScopeRoot/ScopeLeft:8\nScopeRoot/ScopeRight:9\n",
        "FirstScope/FirstScope:10\nSecondScope/SecondScope:11\nFirstScope/FirstScope:12\n",
        "MagicScope/missing\nMagicScope/missing\n",
        "ScopeReceiver\nScopeReceiverChild\nScopeReceiver\n",
        "scalar:5\nscalar:6\nscalar:7\nscalar:8\nscalar:2\n",
        "scalar:fallback:TypeError\nscalar:fallback:TypeError\nscalar:9\n",
        "ordered:2:1\nordered:3:2\nordered:4:3\n",
    );
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/caching_iterator/static_call_scope.php"
            ))
            .env_remove("RPHP_DISABLE_JIT");
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(output.stdout, expected.as_bytes());
    }
}
