use std::io::Write;
use std::process::{Command, Stdio};

fn run_stdin(source: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rphp subprocess should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(source.as_bytes())
        .expect("source should be written");
    let output = child.wait_with_output().expect("rphp should finish");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

#[test]
fn removed_unset_cast_is_a_source_unit_compile_fatal() {
    for source in [
        "<?php\nvar_dump((unset) $value);\n",
        "<?php\nvar_dump((UnSeT) $value);\n",
        "<?php\nif (false) { var_dump((unset) $value); }\n",
        "<?php\nclass C { public $value = (unset) C::class; }\n",
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(
            stderr,
            "Fatal error: The (unset) cast is no longer supported in Standard input code on line 2\n"
        );
    }
}

#[test]
fn unset_statement_and_similar_identifier_grouping_remain_valid() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\n$unset = 3; $unsetValue = 4; echo ($unsetValue), '|'; unset($unset); var_dump(isset($unset));\n",
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "4|bool(false)\n");
    assert_eq!(stderr, "");
}
