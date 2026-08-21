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

fn assert_compile_fatal(source: &str, message: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!("Fatal error: {message} in Standard input code on line {line}\n")
    );
}

fn assert_parse_error(source: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!(
            "Parse error: syntax error, unexpected token \"static\", expecting identifier in Standard input code on line {line}\n"
        )
    );
}

#[test]
fn reserved_static_declaration_names_remain_syntax_errors() {
    for declaration in ["class", "interface", "trait", "enum"] {
        assert_parse_error(&format!("<?php\n{declaration}\n    static {{}}\n"), 3);
    }
}

#[test]
fn this_cannot_be_declared_as_a_static_variable() {
    assert_compile_fatal(
        "<?php\nfunction invalid() {\n    static $this;\n}\n",
        "Cannot use $this as static variable",
        3,
    );
}

#[test]
fn reserved_static_reports_the_classlike_relationship_role() {
    for (source, message, line) in [
        (
            "<?php\nclass Child\n    extends static {}\n",
            "Cannot use \"static\" as class name, as it is reserved",
            2,
        ),
        (
            "<?php\nclass Child\n    implements static {}\n",
            "Cannot use \"static\" as interface name, as it is reserved",
            2,
        ),
        (
            "<?php\ninterface Child\n    extends static {}\n",
            "Cannot use \"static\" as interface name, as it is reserved",
            2,
        ),
        (
            "<?php\nenum Choice implements\n    static {}\n",
            "Cannot use \"static\" as interface name, as it is reserved",
            2,
        ),
        (
            "<?php\n$value = new class\n    extends static {};\n",
            "Cannot use \"static\" as class name, as it is reserved",
            2,
        ),
        (
            "<?php\n$value = new class\n    implements static {};\n",
            "Cannot use \"static\" as interface name, as it is reserved",
            2,
        ),
    ] {
        assert_compile_fatal(source, message, line);
    }
}

#[test]
fn catch_static_is_a_located_source_unit_compile_error() {
    assert_compile_fatal(
        "<?php\nfunction invalid() {\n    try {} catch (\n        static\n        $error\n    ) {}\n}\necho 'unreachable';\n",
        "Bad class name in the catch statement",
        4,
    );
}

#[test]
fn trait_static_diagnostics_cover_composition_and_precedence() {
    for (source, line) in [
        ("<?php\nclass Consumer {\n    use\n        static;\n}\n", 4),
        ("<?php\ntrait Consumer {\n    use\n        static;\n}\n", 4),
        ("<?php\nenum Consumer {\n    use\n        static;\n}\n", 4),
        (
            "<?php\n$value = new class {\n    use\n        static;\n};\n",
            4,
        ),
        (
            "<?php\ntrait Source { public function work() {} }\nclass Consumer {\n    use Source {\n        Source::work insteadof\n            static;\n    }\n}\n",
            4,
        ),
    ] {
        assert_compile_fatal(
            source,
            "Cannot use \"static\" as trait name, as it is reserved",
            line,
        );
    }
}

#[test]
fn ordinary_static_forms_and_classlike_relationships_stay_valid() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\ntrait Helper { public function helper() { return 'trait'; } }\ninterface Marker {}\nclass Base {\n    public static function make(): static { return new static(); }\n}\nclass Child extends Base implements Marker { use Helper; }\nfunction label($static) { return $static; }\n$factory = static fn() => Child::make();\n$value = $factory();\necho label(static: $value->helper());\n",
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "trait");
    assert_eq!(stderr, "");
}
