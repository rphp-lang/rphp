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

fn assert_builtin_compile_error(label: &str, source: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255, "{label}: unexpected status; stderr={stderr:?}");
    assert_eq!(stdout, "", "{label}: source unit executed before fatal");
    assert_eq!(
        stderr,
        format!(
            "Fatal error: Cannot use result of built-in function in write context in Standard input code on line {line}\n"
        ),
        "{label}"
    );
}

#[test]
fn zend_special_builtin_results_are_compile_time_write_errors() {
    let unary_calls = [
        ("strlen", "strlen('x')"),
        ("is_null", "is_null(null)"),
        ("is_bool", "is_bool(true)"),
        ("is_int", "is_int(1)"),
        ("is_integer", "is_integer(1)"),
        ("is_long", "is_long(1)"),
        ("is_float", "is_float(1.0)"),
        ("is_double", "is_double(1.0)"),
        ("is_string", "is_string('x')"),
        ("is_array", "is_array([])"),
        ("is_object", "is_object(new stdClass)"),
        ("is_resource", "is_resource(null)"),
        ("is_scalar", "is_scalar(1)"),
        ("intval", "intval(1)"),
        ("boolval", "boolval(true)"),
        ("floatval", "floatval(1)"),
        ("doubleval", "doubleval(1)"),
        ("strval", "strval(1)"),
        ("count", "count([])"),
        ("sizeof", "sizeof([])"),
        ("gettype", "gettype(1)"),
    ];
    for (label, call) in unary_calls {
        assert_builtin_compile_error(
            label,
            &format!("<?php\necho 'must not run';\n{call}[0] = 1;\n"),
            3,
        );
    }

    for (label, call) in [
        ("get_class zero arguments", "get_class()"),
        ("get_class one argument", "get_class(new stdClass)"),
        ("get_called_class", "get_called_class()"),
        ("array_key_exists", "array_key_exists('key', [])"),
        ("defined literal concat", "defined('PHP_' . 'VERSION')"),
        ("in_array literal strict", "in_array(1, [1, '1'], true)"),
    ] {
        assert_builtin_compile_error(
            label,
            &format!("<?php\necho 'must not run';\n{call}[0] = 1;\n"),
            3,
        );
    }

    assert_builtin_compile_error(
        "func_num_args in function context",
        "<?php\nfunction probe() {\n    func_num_args()[0] = 1;\n}\nprobe();\n",
        3,
    );
    assert_builtin_compile_error(
        "func_get_args in function context",
        "<?php\nfunction probe() {\n    func_get_args()[0] = 1;\n}\nprobe();\n",
        3,
    );
    assert_builtin_compile_error(
        "array_slice specialized func_get_args form",
        "<?php\nfunction probe() {\n    array_slice(func_get_args(), 1)[0] = 1;\n}\nprobe([], []);\n",
        3,
    );
}

#[test]
fn special_builtin_diagnostic_covers_write_forms() {
    for (label, expression) in [
        ("direct reference binding", "$reference =& strlen('x');"),
        ("append", "sizeof([])[] = 1;"),
        ("coalesce assignment", "count([])[0] ??= 1;"),
        ("compound assignment", "intval(1)[0] += 1;"),
        ("increment", "is_int(1)[0]++;"),
        ("unset", "unset(gettype(1)[0]);"),
        (
            "reference binding",
            "$reference =& array_key_exists('key', [])[0];",
        ),
        (
            "by-reference foreach",
            "foreach (strval(1)[0] as &$value) {}",
        ),
    ] {
        assert_builtin_compile_error(label, &format!("<?php\n{expression}\n"), 2);
    }
}

#[test]
fn only_unambiguous_php_special_call_shapes_are_rejected() {
    assert_builtin_compile_error(
        "fully qualified built-in",
        "<?php\n\\strlen('x')[0] = 1;\n",
        2,
    );
    assert_builtin_compile_error(
        "imported built-in alias",
        "<?php\nnamespace N;\nuse function strlen as length;\nlength('x')[0] = 1;\n",
        4,
    );

    let (status, stdout, stderr) = run_stdin(
        "<?php\nnamespace Local;\nfunction strlen($value) { return []; }\nfunction count($value) { return []; }\nstrlen('x')[0] = 1;\ncount([])[] = 2;\n\\array_slice([1], 0)[0] = 3;\n\\array_values([])[] = 4;\necho 'ordinary calls remain writable', \"\\n\";\n",
    );
    assert_eq!(status, 0, "stderr={stderr:?}");
    assert_eq!(stdout, "ordinary calls remain writable\n");
    assert_eq!(stderr, "");

    for (label, source) in [
        ("named argument", "<?php\nstrlen(string: 'x')[0] = 1;\n"),
        ("unpacked argument", "<?php\nstrlen(...['x'])[0] = 1;\n"),
        (
            "count second argument",
            "<?php\ncount([], COUNT_RECURSIVE)[0] = 1;\n",
        ),
        (
            "dynamic in_array haystack",
            "<?php\n$values = [1];\nin_array(1, $values, true)[0] = 1;\n",
        ),
        (
            "scoped defined name",
            "<?php\ndefined('Example::VALUE')[0] = 1;\n",
        ),
        (
            "top-level func_get_args",
            "<?php\nfunc_get_args()[0] = 1;\n",
        ),
    ] {
        let (_, _, stderr) = run_stdin(source);
        assert!(
            !stderr.contains("Cannot use result of built-in function in write context"),
            "{label}: incorrectly classified as a PHP compiler-special call: {stderr:?}"
        );
    }
}
