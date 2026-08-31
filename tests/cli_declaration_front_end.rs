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
        String::from_utf8(output.stderr).expect("diagnostic should be UTF-8"),
    )
}

#[test]
fn declaration_and_expression_keywords_are_ascii_case_insensitive() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nAbStRaCt ClAsS BaseNode {}\nFiNaL ClAsS LeafNode extends BaseNode {}\nvar_dump(new LeafNode InStAnCeOf BaseNode);\n",
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "bool(true)\n");
    assert_eq!(stderr, "");
}

#[test]
fn a_self_extending_interface_fails_at_the_declaration_without_reaching_instanceof() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\ninterface RecursiveFooFar extends RecursiveFooFar {}\nclass A implements RecursiveFooFar {}\n$a = new A();\nvar_dump($a InStAnCeOf A);\necho \"ok\\n\";\n",
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "\nFatal error: Uncaught Error: Interface \"RecursiveFooFar\" not found in Standard input code:2\nStack trace:\n#0 {main}\n  thrown in Standard input code on line 2\n"
    );
}

#[test]
fn duplicate_mixed_case_class_modifiers_are_rejected_at_the_duplicate() {
    let (status, stdout, stderr) = run_stdin("<?php\nFiNaL final class DuplicateModifier {}\n");
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Fatal error: Multiple final modifiers are not allowed in Standard input code on line 2\n"
    );
}

#[test]
fn qualified_and_fully_qualified_property_types_share_the_type_parser() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nnamespace Types { class Item {} }\nnamespace { class Holder { public Types\\Item $local; public ?\\Types\\Item $absolute; } new Holder; echo 'ok'; }\n",
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "ok");
    assert_eq!(stderr, "");
}

#[test]
fn nullable_intersections_fail_before_a_function_body_is_parsed() {
    let (status, stdout, stderr) = run_stdin("<?php\nfunction invalid(): ?Countable&Iterator {}\n");
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected token \"&\", expecting \"{\" in Standard input code on line 2\n"
    );
}

#[test]
fn attributes_cannot_split_an_intersection_type() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nclass Holder {\n    public LeftType& #[Attribute] RightType $value;\n}\n",
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected token \"#[\" in Standard input code on line 3\n"
    );
}

#[test]
fn a_misplaced_type_in_a_grouped_property_uses_php_syntax_diagnostics() {
    let (status, stdout, stderr) =
        run_stdin("<?php\nclass Holder { public $first, int $second; }\n");
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected identifier \"int\", expecting variable in Standard input code on line 2\n"
    );
}

#[test]
fn unsupported_mixed_casts_report_the_following_expression_token() {
    let (status, stdout, stderr) = run_stdin("<?php\n$value = (mixed) 12;\n");
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected integer \"12\" in Standard input code on line 2\n"
    );
}

#[test]
fn a_bare_ampersand_is_a_canonical_parse_error_not_an_internal_token_dump() {
    let (status, stdout, stderr) = run_stdin("<?php\n+&\n");
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected token \"&\" in Standard input code on line 2\n"
    );
}

#[test]
fn try_without_catch_or_finally_is_a_compile_time_fatal() {
    let (status, stdout, stderr) =
        run_stdin("<?php\nfunction runTask() {\n    try { echo 'unreachable'; }\n}\nrunTask();\n");
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Fatal error: Cannot use try without catch or finally in Standard input code on line 3\n"
    );
}
