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

fn assert_parse_error(source: &str, expected: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255, "stderr={stderr:?}");
    assert_eq!(stdout, "", "invalid source must not execute");
    assert_eq!(
        stderr,
        format!("Parse error: {expected} in Standard input code on line {line}\n")
    );
}

#[test]
fn dynamic_class_expressions_run_before_arguments_and_accept_postfixes_after_ctor_parens() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
class SelectedClass {
    public $value;
    public function __construct($value = 0) { echo "ctor:$value>"; $this->value = $value; }
    public function describe() { return "value=$this->value"; }
}
class Registry { public static $selected = SelectedClass::class; }
class Holder {
    public function __get($name) { echo "get:$name>"; return SelectedClass::class; }
}
function chooseClass() { echo "class>"; return SelectedClass::class; }
function chooseMember() { echo "member>"; return 'selected'; }
function buildArgument($value) { echo "arg:$value>"; return $value; }
function packedArguments() { echo "pack>"; return [4]; }
function failingClass() { echo "class-fail>"; throw new Exception('stop'); }
function skippedArgument() { echo "must-not-run>"; return 9; }

echo (new (chooseClass())(buildArgument(1)))->describe(), "\n";
echo (new Registry::${chooseMember()}(buildArgument(2)))->describe(), "\n";
$holder = new Holder;
echo (new $holder->selected(buildArgument(3)))->describe(), "\n";
echo get_class(new Registry::$selected), "\n";
$registryClass = Registry::class;
echo get_class(new $registryClass::$selected), "\n";
echo (new (chooseClass())(...packedArguments()))->describe(), "\n";
try {
    new (failingClass())(skippedArgument());
} catch (Throwable $error) {
    echo $error->getMessage(), "\n";
}
"#,
    );

    assert_eq!(status, 0, "stderr={stderr:?}");
    assert_eq!(
        stdout,
        concat!(
            "class>arg:1>ctor:1>value=1\n",
            "member>arg:2>ctor:2>value=2\n",
            "get:selected>arg:3>ctor:3>value=3\n",
            "ctor:0>SelectedClass\n",
            "ctor:0>SelectedClass\n",
            "class>pack>ctor:4>value=4\n",
            "class-fail>stop\n",
        )
    );
    assert_eq!(stderr, "");
}

#[test]
fn dynamic_class_operand_survives_argument_suspension_without_re_evaluation() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
class SuspendedClass {
    public function __construct($value) { echo "ctor:$value>"; }
}
function constructAcrossYields() {
    $object = new (yield 'class')(yield 'argument');
    return get_class($object);
}
$generator = constructAcrossYields();
echo $generator->current(), '>';
echo $generator->send(SuspendedClass::class), '>';
$generator->send(7);
echo $generator->getReturn();
"#,
    );

    assert_eq!(status, 0, "stderr={stderr:?}");
    assert_eq!(stdout, "class>argument>ctor:7>SuspendedClass");
    assert_eq!(stderr, "");
}

#[test]
fn named_new_without_constructor_parentheses_rejects_result_postfixes() {
    for (source, expected) in [
        (
            "<?php\nclass Probe { public $value; }\necho new Probe->value;\n",
            "syntax error, unexpected token \"->\", expecting \",\" or \";\"",
        ),
        (
            "<?php\nclass Probe { public function read() {} }\nnew Probe->read();\n",
            "syntax error, unexpected token \"->\"",
        ),
        (
            "<?php\nclass Probe {}\necho new Probe['key'];\n",
            "syntax error, unexpected token \"[\", expecting \",\" or \";\"",
        ),
        (
            "<?php\nclass Probe { const VALUE = 1; }\necho new Probe::VALUE;\n",
            "syntax error, unexpected identifier \"VALUE\", expecting variable or \"$\"",
        ),
        (
            "<?php\nclass Probe { public static function read() {} }\nnew Probe::read();\n",
            "syntax error, unexpected identifier \"read\", expecting variable or \"$\"",
        ),
        (
            "<?php\nclass Probe { public $value; }\nvar_dump(new Probe->value);\n",
            "syntax error, unexpected token \"->\", expecting \")\"",
        ),
        (
            "<?php\nclass Probe { public $value; }\nfunction read() { return new Probe->value; }\n",
            "syntax error, unexpected token \"->\", expecting \";\"",
        ),
    ] {
        assert_parse_error(source, expected, 3);
    }
}

#[test]
fn bare_new_results_reject_assignment_unset_and_grouped_postfixes() {
    assert_parse_error(
        "<?php\nclass Probe {}\nnew Probe() = 1;\n",
        "syntax error, unexpected token \"=\"",
        3,
    );
    assert_parse_error(
        "<?php\nclass Probe {}\nunset(new Probe());\n",
        "syntax error, unexpected token \")\", expecting \"->\" or \"?->\" or \"[\"",
        3,
    );
    assert_parse_error(
        "<?php\nclass Probe { public $value; }\necho new (Probe::class)->value;\n",
        "syntax error, unexpected token \"->\", expecting \",\" or \";\"",
        3,
    );
}
