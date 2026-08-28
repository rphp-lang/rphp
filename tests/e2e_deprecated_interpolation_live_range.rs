mod common;

use common::{run_php_expect_error_with_source_context, run_php_with_source_context};
use std::io::Write;
use std::process::Command;
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
            "rphp_deprecated_interpolation_{}_{}",
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
fn nested_quoted_expressions_preserve_evaluation_order_and_dynamic_lookup() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
$prefix = 'slot';
$slotX = 'VALUE';
$suffix = function (): string { echo 'suffix|'; return 'X'; };
echo "left|${"{$prefix}{$suffix()}"}|right";
$values = ['key' => 'MODERN'];
echo "|{$values["key"]}";
"#,
            "/virtual/nested-deprecated-interpolation.php",
            "/virtual",
        ),
        concat!(
            "\nDeprecated: Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead in /virtual/nested-deprecated-interpolation.php on line 5\n",
            "suffix|left|VALUE|right|MODERN",
        )
    );
}

#[test]
fn throwing_array_conversion_preempts_later_dynamic_name_evaluation() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
set_error_handler(function (int $number, string $message): never {
    echo "handler:$number:$message|";
    throw new RuntimeException('handled');
});
try {
    $items = [];
    $result = "left:$items|${"slot$items"}|right";
    echo "after:$result|";
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), '|';
}
echo 'state:', count($items), ':', isset($result) ? 'set' : 'unset', '|done';
"#,
            "/virtual/deprecated-interpolation-priority.php",
            "/virtual",
        ),
        concat!(
            "\nDeprecated: Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead in /virtual/deprecated-interpolation-priority.php on line 8\n",
            "handler:2:Array to string conversion|RuntimeException:handled|state:0:unset|done",
        )
    );
}

#[test]
fn cli_error_reporting_zero_suppresses_only_the_compile_deprecation() {
    let fixture = IncludeFixture::new(
        r#"<?php
$key = 'slot';
$slot = 'VALUE';
echo "value:${"$key"}";
"#,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .arg("-d")
        .arg("error_reporting=0")
        .arg(&fixture.file)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(output.stdout, b"value:VALUE");
    assert!(output.stderr.is_empty());
}

#[test]
fn direct_include_and_eval_use_their_own_deprecation_source_context() {
    let fixture = IncludeFixture::new("<?php\nreturn \"include:${\"$key\"}\";\n");
    let source = format!(
        r#"<?php
$key = 'slot';
$slot = 'VALUE';
echo "direct:${{"$key"}}|";
echo include {include:?};
echo '|';
echo eval('return "eval:${{"$key"}}";');
"#,
        include = fixture.file.to_string_lossy(),
    );
    let expected = format!(
        concat!(
            "\nDeprecated: Using ${{expr}} (variable variables) in strings is deprecated, use {{${{expr}}}} instead in /virtual/deprecated-context.php on line 4\n",
            "direct:VALUE|",
            "\nDeprecated: Using ${{expr}} (variable variables) in strings is deprecated, use {{${{expr}}}} instead in {} on line 2\n",
            "include:VALUE|",
            "\nDeprecated: Using ${{expr}} (variable variables) in strings is deprecated, use {{${{expr}}}} instead in /virtual/deprecated-context.php(7) : eval()'d code on line 1\n",
            "eval:VALUE",
        ),
        fixture.file.to_string_lossy(),
    );

    assert_eq!(
        run_php_with_source_context(&source, "/virtual/deprecated-context.php", "/virtual"),
        expected,
    );
}

#[test]
fn empty_legacy_expression_remains_a_parse_error() {
    let error = run_php_expect_error_with_source_context(
        "<?php\necho \"before${}after\";",
        "/virtual/invalid-deprecated-interpolation.php",
        "/virtual",
    );

    assert_eq!(
        error.to_string(),
        "syntax error, unexpected token \"}\" on line 2"
    );
}
