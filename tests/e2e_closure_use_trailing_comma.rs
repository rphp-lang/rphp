mod common;

use common::{run_php, run_php_expect_error, run_php_with_source_context};
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

static INCLUDE_COUNTER: AtomicU64 = AtomicU64::new(0);

struct IncludeFixture {
    directory: std::path::PathBuf,
    file: std::path::PathBuf,
}

impl IncludeFixture {
    fn new(source: &str) -> Self {
        let identity = INCLUDE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let directory = std::env::temp_dir().join(format!(
            "rphp_closure_use_trailing_comma_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("source.php");
        let mut output = std::fs::File::create(&file).unwrap();
        output.write_all(source.as_bytes()).unwrap();
        Self { directory, file }
    }
}

impl Drop for IncludeFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn trailing_capture_preserves_snapshot_reference_order_and_declaration_context() {
    assert_eq!(
        run_php(
            r#"<?php
#[Attribute(Attribute::TARGET_FUNCTION)] class Marker {}
$left = 'L';
$right = 'R';
$events = [];
$closure = #[Marker] static function (int $delta) use (
    $left,
    &$right,
    &$events,
): string {
    $events[] = $left;
    $right .= $delta;
    return "$left:$right";
};
$left = 'changed';
echo $closure(1), "\n";
var_export([$left, $right, $events]);
"#,
        ),
        concat!(
            "L:R1\n",
            "array (\n",
            "  0 => 'changed',\n",
            "  1 => 'R1',\n",
            "  2 => \n",
            "  array (\n",
            "    0 => 'L',\n",
            "  ),\n",
            ")",
        )
    );
}

#[test]
fn direct_include_and_eval_share_the_capture_list_grammar() {
    let fixture = IncludeFixture::new(
        "<?php\n$included = 'include';\nreturn static function () use ($included,) { return $included; };\n",
    );
    let source = format!(
        r#"<?php
$direct = 'direct';
$directClosure = function () use ($direct,) {{ return $direct; }};
$includeClosure = include {include:?};
$evalClosure = eval(<<<'PHP'
$evaluated = 'eval';
return function () use ($evaluated,) {{ return $evaluated; }};
PHP);
echo $directClosure(), ':', $includeClosure(), ':', $evalClosure();
"#,
        include = fixture.file.to_string_lossy(),
    );

    assert_eq!(
        run_php_with_source_context(&source, "/virtual/closure-use-context.php", "/virtual"),
        "direct:include:eval"
    );
}

#[test]
fn malformed_capture_lists_keep_php_parse_diagnostics_and_lines() {
    for (source, expected) in [
        (
            "<?php\n$closure = function () use () {};",
            r#"syntax error, unexpected token ")", expecting variable or "&" or token "&""#,
        ),
        (
            "<?php\n$value = 1;\n$closure = function () use (,$value) {};",
            r#"syntax error, unexpected token ",", expecting variable or "&" or token "&""#,
        ),
        (
            "<?php\n$value = 1;\n$closure = function () use ($value,,) {};",
            r#"syntax error, unexpected token ",", expecting ")""#,
        ),
        (
            "<?php\n$value = 1;\n$closure = function () use ($value,,,$value) {};",
            r#"syntax error, unexpected token ",", expecting ")""#,
        ),
        (
            "<?php\n$closure = function () use (&) {};",
            r#"syntax error, unexpected token ")", expecting variable"#,
        ),
        (
            "<?php\n$closure = function () use (&42,) {};",
            r#"syntax error, unexpected integer "42", expecting variable"#,
        ),
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let error = Parser::new(tokens)
            .with_source_name("/virtual/closure-use-invalid.php")
            .parse()
            .unwrap_err();
        let line = source.lines().count();
        assert_eq!(
            error,
            format!("{expected} in /virtual/closure-use-invalid.php on line {line}")
        );
    }
}

#[test]
fn capture_semantic_errors_preempt_closure_creation_at_first_declaration_lines() {
    for (source, expected) in [
        (
            "<?php\n$value = 1;\n$closure = function () use (\n    $value,\n    &$value,\n) {};",
            "Cannot use variable $value twice on line 4",
        ),
        (
            "<?php\n$value = 1;\n$closure = function (\n    $value,\n) use (\n    $value,\n) {};",
            "Cannot use lexical variable $value as a parameter name on line 4",
        ),
        (
            "<?php\n$closure = function () use (\n    $GLOBALS,\n) {};",
            "Cannot use auto-global as lexical variable on line 3",
        ),
    ] {
        assert_eq!(run_php_expect_error(source).to_string(), expected);
    }
}

#[test]
fn arrow_call_parameter_array_and_destructuring_commas_remain_unchanged() {
    assert_eq!(
        run_php(
            r#"<?php
function join_values($first, $second = 'B',) { return $first . $second; }
$arrow = fn ($value,) => $value + 1;
$array = ['A', 'B',];
[$first, $second,] = $array;
echo join_values($first,), ':', $arrow(2,), ':', $second;
"#,
        ),
        "AB:3:B"
    );
}
