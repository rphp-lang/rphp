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

fn assert_run(source: &str, status: i32, stdout: &str, stderr: &str) {
    let actual = run_stdin(source);
    assert_eq!(actual, (status, stdout.to_string(), stderr.to_string()));
}

fn assert_parse_error(source: &str, expected: &str, line: usize) {
    assert_run(
        source,
        255,
        "",
        &format!("Parse error: {expected} in Standard input code on line {line}\n"),
    );
}

#[test]
fn exit_keyword_is_reserved_for_global_declarations_imports_and_labels() {
    for (source, expected, line) in [
        (
            "<?php\nconst DIE = 1;\n",
            "syntax error, unexpected token \"exit\", expecting identifier",
            2,
        ),
        (
            "<?php\nnamespace Example;\nconst exit = 1;\n",
            "syntax error, unexpected token \"exit\", expecting identifier",
            3,
        ),
        (
            "<?php\nfunction die() {}\n",
            "syntax error, unexpected token \"exit\", expecting \"(\"",
            2,
        ),
        (
            "<?php\nnamespace Example;\nfunction EXIT() {}\n",
            "syntax error, unexpected token \"exit\", expecting \"(\"",
            3,
        ),
        (
            "<?php\necho 'unreachable';\nexit:\n",
            "syntax error, unexpected token \":\"",
            3,
        ),
        (
            "<?php\ngoto Die;\necho 'unreachable';\n",
            "syntax error, unexpected token \"exit\", expecting identifier",
            2,
        ),
        (
            "<?php\nclass exit {}\n",
            "syntax error, unexpected token \"exit\", expecting identifier",
            2,
        ),
        (
            "<?php\nuse function exit as stop;\n",
            "syntax error, unexpected token \"exit\", expecting identifier or fully qualified name or namespaced name",
            2,
        ),
    ] {
        assert_parse_error(source, expected, line);
    }
}

#[test]
fn relaxed_member_and_named_argument_contexts_preserve_exit_and_die_spellings() {
    assert_run(
        r#"<?php
class Keywords {
    const exit = 10;
    const die = 5;
    function exit() { return 20; }
    function die() { return 15; }
}
enum KeywordCases { case exit; case die; }
function named($exit, $die) { return "$exit,$die"; }
$object = new Keywords;
echo Keywords::die, ',', Keywords::exit, ',', $object->die(), ',', $object->exit(), '|';
echo KeywordCases::exit->name, ',', KeywordCases::die->name, '|';
echo named(die: 2, exit: 1);
"#,
        0,
        "5,10,15,20|exit,die|1,2",
        "",
    );
}

#[test]
fn bare_named_and_first_class_forms_share_the_canonical_exit_function() {
    assert_run(
        r#"<?php
$callback = DIE(...);
var_dump($callback);
echo 'before>';
EXIT;
echo 'unreachable';
"#,
        0,
        concat!(
            "object(Closure)#1 (2) {\n",
            "  [\"function\"]=>\n",
            "  string(4) \"exit\"\n",
            "  [\"parameter\"]=>\n",
            "  array(1) {\n",
            "    [\"$status\"]=>\n",
            "    string(10) \"<optional>\"\n",
            "  }\n",
            "}\n",
            "before>",
        ),
        "",
    );
    assert_run("<?php\nexit(status: 7);\n", 7, "", "");
    assert_run(
        "<?php\nfunction argument() { echo 'argument>'; return 3; }\nexit(status: argument());\n",
        3,
        "argument>",
        "",
    );
}

#[test]
fn weak_exit_argument_conversion_matches_php_boundaries() {
    for (expression, status, stdout) in [
        ("false", 0, ""),
        ("true", 1, ""),
        ("10.0", 10, ""),
        ("'12'", 0, "12"),
        ("1.0E+20", 0, "1.0E+20"),
        ("-9223372036854775808.0", 0, ""),
    ] {
        assert_run(&format!("<?php\nexit({expression});\n"), status, stdout, "");
    }

    assert_run(
        "<?php\nexit(null);\n",
        0,
        concat!(
            "\nDeprecated: exit(): Passing null to parameter #1 ($status) of type ",
            "string|int is deprecated in Standard input code on line 2\n",
        ),
        "",
    );
    assert_run(
        "<?php\nexit(15.5);\n",
        15,
        concat!(
            "\nDeprecated: Implicit conversion from float 15.5 to int loses precision ",
            "in Standard input code on line 2\n",
        ),
        "",
    );
    assert_run(
        "<?php\nexit(NAN);\n",
        0,
        concat!(
            "\nWarning: unexpected NAN value was coerced to string in Standard input code ",
            "on line 2\n",
            "NAN",
        ),
        "",
    );
}

#[test]
fn invalid_strict_and_structural_values_throw_before_exit() {
    assert_run(
        r#"<?php
foreach ([[], new stdClass] as $value) {
    try { exit($value); } catch (Throwable $error) {
        echo $error::class, ': ', $error->getMessage(), "\n";
    }
}
"#,
        0,
        concat!(
            "TypeError: exit(): Argument #1 ($status) must be of type string|int, array given\n",
            "TypeError: exit(): Argument #1 ($status) must be of type string|int, stdClass given\n",
        ),
        "",
    );
    assert_run(
        r#"<?php
declare(strict_types=1);
try { exit(true); } catch (Throwable $error) {
    echo $error::class, ': ', $error->getMessage(), "\n";
}
"#,
        0,
        "TypeError: exit(): Argument #1 ($status) must be of type string|int, true given\n",
        "",
    );

    assert_run(
        r#"<?php
class StringStatus {
    function __toString() { echo 'cast>'; return 'done'; }
}
exit(new StringStatus);
"#,
        0,
        "cast>done",
        "",
    );
    assert_run(
        r#"<?php
class ThrowingStatus {
    function __toString() { echo 'cast>'; throw new Exception('stop'); }
}
try { exit(new ThrowingStatus); } catch (Throwable $error) {
    echo 'caught:', $error->getMessage();
}
"#,
        0,
        "cast>caught:stop",
        "",
    );
    assert_run(
        r#"<?php
set_error_handler(function ($level, $message) {
    echo "handled:$message>";
    throw new Exception('stop');
});
try { exit(15.5); } catch (Throwable $error) {
    echo 'caught:', $error->getMessage();
}
"#,
        0,
        concat!(
            "handled:Implicit conversion from float 15.5 to int loses precision>",
            "caught:stop",
        ),
        "",
    );
}
