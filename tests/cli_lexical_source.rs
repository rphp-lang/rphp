use std::process::Command;

fn specimen(name: &str) {
    let sources: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/lexical_source/sources.json")).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/lexical_source/expected.json")).unwrap();
    let directory = std::env::temp_dir().join(format!(
        "rphp-lexical-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let file = directory.join("program.php");
    std::fs::write(&file, sources[name].as_str().unwrap()).unwrap();
    for disabled in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(&file)
            .env_remove("RPHP_DISABLE_JIT");
        if disabled {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
        assert_eq!(
            output.stdout,
            expected[name].as_str().unwrap().as_bytes(),
            "{name} JIT disabled={disabled}"
        );
    }
}

macro_rules! case {
    ($name:ident) => {
        #[test]
        fn $name() {
            specimen(stringify!($name));
        }
    };
}
case!(tag_spelling);
case!(parameter_spelling);
case!(relative_parameter_scope);
case!(control_spelling);
case!(declarations);
case!(member_spelling);
case!(qualified_spelling);
case!(variable_case);
case!(segments);
case!(leading_space);
case!(plain_text);
case!(non_tag_prefix);
case!(tag_eof);
case!(included_tag);
case!(keyword_substrings);
case!(named_reserved);
case!(named_specials);
case!(constant_case);
case!(enum_case);
case!(switch_keyword);
case!(tag_delimiters);
case!(commented_labels);
case!(method_specials);
case!(tag_comment_text);
case!(shebang_lf);
case!(shebang_crlf);
case!(shebang_plain);
case!(shebang_only);
case!(shebang_later);
case!(included_shebang);
case!(class_literal_constant);
case!(literal_array_keys);
case!(typed_keyword_constants);
case!(typed_literal_constants);
case!(shebang_cr);

#[test]
fn reserved_declaration_diagnostics_keep_the_source_name() {
    for (source, message) in [
        (
            "namespace NAMEspace;",
            "Cannot use 'NAMEspace' as namespace name",
        ),
        (
            "namespace NAMEspace\\xyz;",
            "unexpected namespace-relative name \"NAMEspace\\xyz\"",
        ),
        (
            "namespace scope; const NULL = 1;",
            "Cannot redeclare constant 'NULL'",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
            .args(["-r", source])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(255));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr).unwrap().contains(message),
            "{source}"
        );
    }
}
