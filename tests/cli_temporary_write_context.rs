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

fn assert_compile_error(label: &str, source: &str, message: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255, "{label}: unexpected status; stderr={stderr:?}");
    assert_eq!(stdout, "", "{label}: source unit executed before fatal");
    assert_eq!(
        stderr,
        format!("Fatal error: {message} in Standard input code on line {line}\n"),
        "{label}"
    );
}

#[test]
fn temporary_array_roots_fail_for_every_mutating_write_form() {
    for (label, expression) in [
        ("indexed assignment", "[0, 1][0] = 2;"),
        ("nested append", "[[0]][0][] = 2;"),
        ("compound assignment", "[0][0] += 2;"),
        ("coalescing assignment", "[null][0] ??= 2;"),
        ("prefix increment", "++[0][0];"),
        ("postfix increment", "[0][0]++;"),
        ("postfix decrement", "[0][0]--;"),
        ("unset", "unset([0][0]);"),
        ("direct reference", "$reference =& [0][0];"),
        ("element reference", "$target[0] =& [0][0];"),
    ] {
        assert_compile_error(
            label,
            &format!("<?php\necho 'must not run';\n{expression}\n"),
            "Cannot use temporary expression in write context",
            3,
        );
    }
}

#[test]
fn all_non_call_temporary_root_families_share_the_php_diagnostic() {
    for (label, expression) in [
        ("string literal", "'ab'[0] = 'z';"),
        ("binary expression", "(1 + 2)[0] = 3;"),
        ("ternary expression", "(true ? [0] : [1])[0] = 2;"),
        ("coalescing expression", "($missing ?? [0])[0] = 2;"),
        ("match expression", "(match (1) { 1 => [0] })[0] = 2;"),
        ("assignment expression", "($array = [0])[0] = 2;"),
        ("new expression", "(new ArrayObject())[0] = 2;"),
        ("pipe expression", "([] |> (fn($value) => $value))[0] = 2;"),
    ] {
        assert_compile_error(
            label,
            &format!("<?php\necho 'must not run';\n{expression}\n"),
            "Cannot use temporary expression in write context",
            3,
        );
    }

    assert_compile_error(
        "global array constant",
        "<?php\nconst VALUES = [0];\necho 'must not run';\nVALUES[0] = 2;\n",
        "Cannot use temporary expression in write context",
        4,
    );
    assert_compile_error(
        "class array constant",
        "<?php\nclass Constants { const VALUES = [0]; }\necho 'must not run';\nConstants::VALUES[0] = 2;\n",
        "Cannot use temporary expression in write context",
        4,
    );
}

#[test]
fn clone_roots_use_the_php_builtin_result_diagnostic() {
    for (label, expression) in [
        ("indexed assignment", "(clone $object)[0] = 2;"),
        ("append", "(clone $object)[] = 2;"),
        ("compound assignment", "(clone $object)[0] += 2;"),
        ("coalescing assignment", "(clone $object)[0] ??= 2;"),
        ("increment", "(clone $object)[0]++;"),
        ("unset", "unset((clone $object)[0]);"),
        ("reference", "$reference =& (clone $object)[0];"),
        (
            "by-reference foreach",
            "foreach ((clone $object)[0] as &$value) {}",
        ),
    ] {
        assert_compile_error(
            label,
            &format!("<?php\n$object = new stdClass;\n{expression}\n"),
            "Cannot use result of built-in function in write context",
            3,
        );
    }
}

#[test]
fn ordinary_call_results_and_non_write_uses_remain_valid() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nfunction values() { return [1]; }\n$dynamic = fn() => [2];\nvalues()[0] = 3;\narray_values([1])[0] = 4;\n$dynamic()[0] = 5;\nvar_dump([6][0], isset([7][0]));\nfunction by_value($value) { return $value; }\nvar_dump(by_value([8][0]));\nforeach ([9] as &$value) { echo $value, \"\\n\"; }\n",
    );
    assert_eq!(status, 0, "stderr={stderr:?}");
    assert_eq!(stdout, "int(6)\nbool(true)\nint(8)\n9\n");
    assert_eq!(stderr, "");
}
