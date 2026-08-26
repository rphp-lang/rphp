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
fn removed_curly_offsets_use_contextual_php_parse_diagnostics() {
    for (source, expected) in [
        (
            "<?php\n$value = 'text';\nvar_dump($value\n{0});\n",
            "Parse error: syntax error, unexpected token \"{\", expecting \")\" in Standard input code on line 4\n",
        ),
        (
            "<?php\nconst VALUE = 'text'\n{0};\n",
            "Parse error: syntax error, unexpected token \"{\", expecting \",\" or \";\" in Standard input code on line 3\n",
        ),
        (
            "<?php\n\"{$value\n{'key'}}\";\n",
            "Parse error: syntax error, unexpected token \"{\", expecting \"->\" or \"?->\" or \"[\" in Standard input code on line 3\n",
        ),
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(stderr, expected);
    }
}

#[test]
fn square_offsets_blocks_and_supported_interpolation_stay_executable() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
$value = 'text';
echo $value[0], '|';
const VALUE = 'text'[1];
echo VALUE, '|';
if (true) { echo 'block|'; }
$property = 'name';
$object = new class { public string $name = 'ok'; };
echo "{$object->{$property}}|{$value[2]}";
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "t|e|block|ok|x");
    assert_eq!(stderr, "");
}
