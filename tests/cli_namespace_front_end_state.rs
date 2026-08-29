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

fn fatal(message: &str, line: usize) -> String {
    format!("Fatal error: {message} in Standard input code on line {line}\n")
}

fn parse_error(message: &str, line: usize) -> String {
    format!("Parse error: {message} in Standard input code on line {line}\n")
}

#[test]
fn reserved_namespace_segments_and_qualified_alias_calls_execute() {
    let source = r#"<?php
namespace iter\fn { function test() { echo __FUNCTION__, "\n"; } }
namespace fn { function test() { echo __FUNCTION__, "\n"; } }
namespace self { function test() { echo __FUNCTION__, "\n"; } }
namespace {
    use iter\fn;
    use function fn\test as test2;
    use function self\test as test3;
    fn\test();
    test2();
    test3();
    $arrow = fn($value) => $value + 1;
    echo "arrow:", $arrow(4), "\n";
}
"#;

    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 0);
    assert_eq!(stdout, "iter\\fn\\test\nfn\\test\nself\\test\narrow:5\n");
    assert_eq!(stderr, "");
}

#[test]
fn namespace_unit_invariants_preempt_all_source_side_effects() {
    let cases = [
        (
            "<?php\necho 'before';\nnamespace Late;\necho 'after';",
            fatal(
                "Namespace declaration statement has to be the very first statement or after any declare call in the script",
                3,
            ),
        ),
        (
            "<?php\nnamespace Outer {\nnamespace Inner {}\n}",
            fatal("Namespace declarations cannot be nested", 3),
        ),
        (
            "<?php\nnamespace First {}\nnamespace Second;\necho 'after';",
            fatal(
                "Cannot mix bracketed namespace declarations with unbracketed namespace declarations",
                3,
            ),
        ),
        (
            "<?php\nnamespace First {}\necho 'outside';\nnamespace Second {}",
            fatal("No code may exist outside of namespace {}", 3),
        ),
    ];

    for (source, expected) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "source={source:?}");
        assert_eq!(stdout, "", "source={source:?}");
        assert_eq!(stderr, expected, "source={source:?}");
    }
}

#[test]
fn namespace_name_and_group_use_errors_are_contextual_and_located() {
    let cases = [
        (
            "<?php\nnamespace NAMEspace;",
            fatal("Cannot use 'NAMEspace' as namespace name", 2),
        ),
        (
            "<?php\nnamespace NAMEspace\\child;",
            parse_error(
                "syntax error, unexpected namespace-relative name \"NAMEspace\\child\", expecting \"{\"",
                2,
            ),
        ),
        (
            "<?php\nFoo \\ Bar;",
            parse_error("syntax error, unexpected token \"\\\"", 2),
        ),
        (
            "<?php\nuse const Foo\\Bar\\{ A, const B };",
            parse_error(
                "syntax error, unexpected token \"const\", expecting \"}\"",
                2,
            ),
        ),
        (
            "<?php\nuse Foo\\Bar\\{\\Baz};",
            parse_error(
                "syntax error, unexpected fully qualified name \"\\Baz\", expecting identifier or namespaced name or \"function\" or \"const\"",
                2,
            ),
        ),
        (
            "<?php\ninterface Contract {}\nclass Broken implements\\Contract {}",
            parse_error(
                "syntax error, unexpected namespaced name \"implements\\Contract\", expecting \"{\"",
                3,
            ),
        ),
        (
            "<?php\nnamespace Constants;\nconst NULL = side_effect();",
            fatal("Cannot redeclare constant 'NULL'", 3),
        ),
    ];

    for (source, expected) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "source={source:?}");
        assert_eq!(stdout, "", "source={source:?}");
        assert_eq!(stderr, expected, "source={source:?}");
    }
}

#[test]
fn declares_and_repeated_same_style_namespaces_remain_valid() {
    for (source, expected) in [
        (
            "<?php\ndeclare(ticks=1);\nnamespace First;\nfunction value() { return 41; }\nnamespace Second;\necho \\First\\value() + 1;",
            "42",
        ),
        (
            "<?php\ndeclare(ticks=1);\nnamespace First { function value() { return 41; } }\nnamespace Second { echo \\First\\value() + 1; }",
            "42",
        ),
        (
            "<?php\nnamespace Imports {\nuse Vendor\\One \\{ Alpha };\nuse Vendor\\Two\\    { Beta };\necho '42';\n}",
            "42",
        ),
        (
            "<?php\nnamespace Halted { echo '42'; }\n__HALT_COMPILER();\nnamespace Ignored { echo 'wrong'; }",
            "42",
        ),
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 0, "source={source:?}, stderr={stderr:?}");
        assert_eq!(stdout, expected, "source={source:?}");
        assert_eq!(stderr, "", "source={source:?}");
    }
}
