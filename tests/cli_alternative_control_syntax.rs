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
fn alternative_if_and_loop_bodies_reuse_ordinary_control_flow() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nif (false):\n    echo 'no';\nelseif (true):\n    echo 'if';\nelse:\n    echo 'else';\nendif;\n$i = 0;\nwhile ($i < 2): echo $i++; endwhile;\nfor ($j = 2; $j < 4; $j++): echo $j; endfor;\nforeach ([4, 5] as $value): echo $value; endforeach;\n",
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "if012345");
    assert_eq!(stderr, "");
}

#[test]
fn alternative_switch_accepts_both_label_separators() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nswitch (2):\ncase 1:\n    echo 'one';\n    break;\ndefault\n    ;\n    echo 'default';\nendswitch;\n",
    );
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "\nDeprecated: Case statements followed by a semicolon (;) are deprecated, use a colon (:) instead in Standard input code on line 7\ndefault"
    );
    assert_eq!(stderr, "");
}

#[test]
fn braced_controls_match_and_reserved_named_arguments_remain_valid() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nfunction label($endif) { return $endif; }\nif (true) {\n    switch (1) { case 1: echo label(endif: 'ok'); break; }\n}\n",
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "ok");
    assert_eq!(stderr, "");
}
