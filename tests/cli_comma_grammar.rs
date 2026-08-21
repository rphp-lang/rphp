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

fn assert_compile_fatal(source: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!(
            "Fatal error: Cannot use empty array elements in arrays in Standard input code on line {line}\n"
        )
    );
}

#[test]
fn empty_array_elements_are_compile_errors_at_php_source_lines() {
    assert_compile_fatal("<?php\n$value = [\n    , 1\n];\n", 3);
    assert_compile_fatal("<?php\n$value = [\n    1,\n    ,\n    2\n];\n", 3);
    assert_compile_fatal("<?php\n$value = array(1, 2, ,);\n", 2);
}

#[test]
fn match_condition_lists_accept_a_comma_before_the_arrow() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nforeach ([false, 0, true, 1, 2] as $value) {\n    echo match ($value) {\n        false, 0, => 'false',\n        true, 1, => 'true',\n        default, => 'other',\n    }, \"\\n\";\n}\n",
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "false\nfalse\ntrue\ntrue\nother\n");
    assert_eq!(stderr, "");
}

#[test]
fn array_trailing_commas_and_destructuring_holes_remain_valid() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\n$array = [1, 2,];\n[, $short] = $array;\nlist(, $long) = $array;\necho $short, ':', $long;\n",
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "2:2");
    assert_eq!(stderr, "");
}
