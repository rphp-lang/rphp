use std::io::Write;
use std::process::{Command, Stdio};

fn run_stdin(source: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-n",
            "-d",
            "display_errors=stderr",
            "-d",
            "log_errors=0",
            "-d",
            "html_errors=0",
            "-d",
            "fatal_error_backtraces=0",
        ])
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

fn assert_diagnostic(source: &str, expected: &str) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255, "{stderr:?}");
    assert!(stdout.is_empty(), "{stdout:?}");
    assert_eq!(stderr, expected);
}

#[test]
fn attribute_calls_fail_at_the_annotated_declaration_before_execution() {
    let cases = [
        (
            "<?php\necho \"never\\n\";\n#[Probe(runNow())]\nclass InvalidCall {}\n",
            4,
        ),
        (
            "<?php\n#[Probe([1, worker()])]\nfunction invalidNestedCall(): void {}\n",
            3,
        ),
        (
            "<?php\nif (false) {\n    #[Probe(Service::load())]\n    function hiddenInvalidCall(): void {}\n}\n",
            4,
        ),
        (
            "<?php\n#[Probe((new Worker())->load())]\ninterface InvalidMethodCall {}\n",
            3,
        ),
    ];

    for (source, line) in cases {
        assert_diagnostic(
            source,
            &format!(
                "Fatal error: Constant expression contains invalid operations in Standard input code on line {line}\n"
            ),
        );
    }
}

#[test]
fn dynamic_class_constant_owners_have_their_distinct_compile_error() {
    let cases = [
        "<?php\n#[Probe(source()->name::VALUE)]\nclass InvalidPropertyOwner {}\n",
        "<?php\n#[Probe((factory())::VALUE)]\ntrait InvalidCallOwner {}\n",
    ];

    for source in cases {
        assert_diagnostic(
            source,
            "Fatal error: Dynamic class names are not allowed in compile-time class constant references in Standard input code on line 3\n",
        );
    }
}

#[test]
fn variable_attribute_names_use_the_canonical_parse_error() {
    assert_diagnostic(
        "<?php\n$name = 'Probe';\n#[$name]\nclass InvalidVariableAttribute {}\n",
        "Parse error: syntax error, unexpected variable \"$name\" in Standard input code on line 3\n",
    );
}

#[test]
fn attributed_multiple_constants_win_over_argument_errors_but_not_syntax_errors() {
    assert_diagnostic(
        "<?php\n#[Probe]\nconst FIRST = 1,\n    SECOND = 2;\n",
        "Fatal error: Cannot apply attributes to multiple constants at once in Standard input code on line 3\n",
    );
    assert_diagnostic(
        "<?php\n#[Probe(worker())]\nconst FIRST = 1, SECOND = 2;\n",
        "Fatal error: Cannot apply attributes to multiple constants at once in Standard input code on line 3\n",
    );
    assert_diagnostic(
        "<?php\n#[Probe]\nconst FIRST = 1, SECOND = 2;\nisset(, $value);\n",
        "Parse error: syntax error, unexpected token \",\" in Standard input code on line 4\n",
    );
}

#[test]
fn valid_attribute_constant_expressions_and_plain_constant_lists_are_preserved() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\n#[Attribute]\nclass ValidProbe {\n    public function __construct(public mixed $value = null) {}\n}\nclass SourceValues { public const VALUE = 41; }\n#[ValidProbe(SourceValues::VALUE)]\nconst SINGLE_VALUE = 1;\nconst PLAIN_FIRST = 2, PLAIN_SECOND = 3;\n$attribute = (new ReflectionConstant('SINGLE_VALUE'))->getAttributes()[0];\n$instance = $attribute->newInstance();\necho $attribute->getTarget(), ':', $instance->value, ':', SINGLE_VALUE, ':', PLAIN_FIRST, ':', PLAIN_SECOND, \"\\n\";\n",
    );
    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(stdout, "64:41:1:2:3\n");
    assert!(stderr.is_empty(), "{stderr:?}");
}

#[test]
fn namespaced_imported_class_constants_remain_valid_attribute_arguments() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nnamespace AttributeFront;\nuse AttributeFront\\SourceValues as ImportedValues;\n#[\\Attribute]\nclass ValidProbe { public function __construct(public mixed $value = null) {} }\nclass SourceValues { public const VALUE = 41; }\n#[ValidProbe(ImportedValues::VALUE)]\nconst SINGLE_VALUE = 1;\n$attribute = (new \\ReflectionConstant(__NAMESPACE__ . '\\\\SINGLE_VALUE'))->getAttributes()[0];\n$instance = $attribute->newInstance();\necho $attribute->getName(), ':', $instance->value, ':', SINGLE_VALUE, \"\\n\";\n",
    );
    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(stdout, "AttributeFront\\ValidProbe:41:1\n");
    assert!(stderr.is_empty(), "{stderr:?}");
}
