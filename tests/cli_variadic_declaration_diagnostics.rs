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
fn invalid_variadic_declarations_are_located_compile_fatals() {
    for (source, expected) in [
        (
            "<?php\nfunction invalid(...$values = 1) {}\n",
            "Fatal error: Variadic parameter cannot have a default value in Standard input code on line 2\n",
        ),
        (
            "<?php\nfunction invalid(...$values, $last) {}\n",
            "Fatal error: Only the last parameter can be variadic in Standard input code on line 2\n",
        ),
        (
            "<?php\nclass Example {\n    public function invalid(...$values = []) {}\n}\n",
            "Fatal error: Variadic parameter cannot have a default value in Standard input code on line 3\n",
        ),
        (
            "<?php\n$invalid = function ($first, ...$values, $last) {};\n",
            "Fatal error: Only the last parameter can be variadic in Standard input code on line 2\n",
        ),
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(stderr, expected);
    }
}

#[test]
fn valid_variadic_functions_methods_closures_and_references_remain_executable() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
function collect(string $prefix = 'p', int ...$values): string {
    return $prefix . array_sum($values);
}
function replace(string &...$values): void {
    $values[0] = 'changed';
}
class Example {
    public function countValues(string ...$values): int {
        return count($values);
    }
}
$sum = function (int ...$values): int { return array_sum($values); };
$value = 'before';
replace($value);
echo collect(), '|', collect('x', 1, 2), '|';
echo (new Example())->countValues('a', 'b'), '|', $sum(3, 4), '|', $value;
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "p0|x3|2|7|changed");
    assert_eq!(stderr, "");
}
