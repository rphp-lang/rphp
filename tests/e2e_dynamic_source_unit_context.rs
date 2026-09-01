mod common;

use common::{run_php, run_php_with_source_context};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "rphp_dynamic_source_unit_{}_{}",
            std::process::id(),
            id
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn write(&self, name: &str, contents: &str) -> std::path::PathBuf {
        let path = self.path.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn included_source_accepts_initial_inline_html_and_long_plain_text() {
    let dir = TempDir::new();
    let plain = "passed, ".repeat(1024);
    let plain_path = dir.write("plain.inc", &plain);
    let mixed_path = dir.write(
        "mixed.inc",
        "leading\n<?php echo \"php\\n\"; ?>\ntrailing\n",
    );
    let source = format!(
        "<?php require '{}'; require '{}';",
        plain_path.to_string_lossy(),
        mixed_path.to_string_lossy()
    );
    assert_eq!(
        run_php_with_source_context(&source, "/virtual/include-driver.php", "/virtual"),
        format!("{plain}leading\nphp\ntrailing\n")
    );
}

#[test]
fn relative_includes_follow_the_owning_source_unit_through_functions_and_eval() {
    let dir = TempDir::new();
    dir.write("function-target.inc", "<?php echo \"function-target\\n\";");
    let function = dir.write(
        "function.inc",
        "<?php function dynamic_source_relative_function() { require 'function-target.inc'; }",
    );
    dir.write("eval-target.inc", "<?php echo \"eval-target\\n\";");
    dir.write("nested-target.inc", "<?php echo \"nested-target\\n\";");
    let eval = dir.write(
        "eval.inc",
        "<?php eval(\"require 'eval-target.inc'; eval(\\\"require 'nested-target.inc';\\\");\");",
    );
    let driver = dir.path.join("driver.php");
    let source = format!(
        "<?php require '{}'; dynamic_source_relative_function(); require '{}';",
        function.to_string_lossy(),
        eval.to_string_lossy()
    );
    assert_eq!(
        run_php_with_source_context(
            &source,
            &driver.to_string_lossy(),
            &dir.path.to_string_lossy(),
        ),
        "function-target\neval-target\nnested-target\n"
    );
}

#[test]
fn source_unit_statics_are_fresh_and_write_back_without_cross_unit_aliasing() {
    assert_eq!(
        run_php(
            r#"<?php
function left_scope() { eval('static $n = 0; echo "left=", ++$n, "\\n";'); }
function right_scope() { eval('static $n = 0; echo "right=", ++$n, "\\n";'); }
function nested_scope() { eval('eval(\'static $n = 0; echo "nested=", ++$n, "\\\\n";\');'); }
class CounterScope { public function method() { eval('static $n = 0; echo "method=", ++$n, "\\n";'); } }
$closure = function () { eval('static $n = 0; echo "closure=", ++$n, "\\n";'); };
for ($i = 0; $i < 2; $i++) {
    left_scope(); right_scope(); nested_scope(); (new CounterScope())->method(); $closure();
    eval('static $n = 0; echo "root=", ++$n, "\\n";');
}
"#,
        ),
        concat!(
            "left=1\nright=1\nnested=1\nmethod=1\nclosure=1\nroot=1\n",
            "left=1\nright=1\nnested=1\nmethod=1\nclosure=1\nroot=1\n",
        )
    );
}

#[test]
fn included_and_eval_static_bindings_replace_only_the_current_caller_cv() {
    let dir = TempDir::new();
    let included = dir.write(
        "static.inc",
        "<?php static $value = 0; echo 'include-static=', ++$value, \"\\n\";",
    );
    let source = format!(
        r#"<?php
function include_static_probe() {{ include '{}'; }}
function eval_over_native_static() {{
    static $value = 40;
    eval('static $value = 0; echo "eval-native=", ++$value, "\\n";');
    echo 'caller-native=', $value, "\n";
}}
include_static_probe(); include_static_probe();
eval_over_native_static(); eval_over_native_static();
"#,
        included.to_string_lossy()
    );
    assert_eq!(
        run_php(&source),
        concat!(
            "include-static=1\ninclude-static=1\n",
            "eval-native=1\ncaller-native=1\n",
            "eval-native=1\ncaller-native=1\n",
        )
    );
}

#[test]
fn strict_included_call_reports_declaration_and_include_trace_origin() {
    let dir = TempDir::new();
    let strict = dir.write(
        "strict.inc",
        concat!(
            "<?php\n",
            "declare(strict_types=1);\n",
            "\n",
            "function takes_dynamic_source_int(int $value): void {}\n",
            "takes_dynamic_source_int(1.0);\n",
        ),
    );
    let source = format!(
        r#"<?php
try {{
    require '{}';
}} catch (Throwable $error) {{
    echo get_class($error), ':', $error->getMessage(), "\n";
    echo basename($error->getFile()), ':', $error->getLine(), "\n";
    foreach ($error->getTrace() as $frame) {{
        echo ($frame['function'] ?? '{{main}}'), '@', basename($frame['file'] ?? ''), ':', ($frame['line'] ?? 0), "\n";
    }}
}}
"#,
        strict.to_string_lossy()
    );
    assert_eq!(
        run_php_with_source_context(&source, "/virtual/strict-driver.php", "/virtual"),
        format!(
            concat!(
                "TypeError:takes_dynamic_source_int(): Argument #1 ($value) must be of type int, float given, called in {} on line 5\n",
                "strict.inc:4\n",
                "takes_dynamic_source_int@strict.inc:5\n",
                "require@strict-driver.php:3\n",
            ),
            strict.to_string_lossy()
        )
    );
}

#[test]
fn eval_during_unwind_preserves_the_outer_exception_and_continues_the_scope() {
    let source = r#"<?php
class DestructorProbe {
    public function __destruct() {
        eval('try { throw new Error("caught-inner"); } catch (Error $error) {}');
        echo "destructor-after-eval\n";
    }
}
$probe = new DestructorProbe();
try {
    try { throw new Error("outer"); }
    finally { unset($probe); }
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
"#;
    assert_eq!(
        run_php_with_source_context(source, "/virtual/pending.php", "/virtual"),
        concat!("destructor-after-eval\n", "Error:outer\n",)
    );
}

#[test]
fn escaping_eval_exception_chains_the_exception_displaced_by_unwind() {
    assert_eq!(
        run_php(
            r#"<?php
class ReplacementProbe {
    public function __destruct() { eval('throw new RuntimeException("inner-replacement");'); }
}
$probe = new ReplacementProbe();
try {
    try { throw new Error("outer-replaced"); }
    finally { unset($probe); }
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
    $previous = $error->getPrevious();
    echo $previous ? get_class($previous) . ':' . $previous->getMessage() : 'no-previous', "\n";
}
"#,
        ),
        concat!(
            "RuntimeException:inner-replacement\n",
            "Error:outer-replaced\n",
        )
    );
}

#[test]
fn eval_compile_errors_are_catchable_and_do_not_poison_later_evals() {
    let source = r#"<?php
function try_dynamic_compile(string $code): void {
    try { eval($code); }
    catch (CompileError $error) {
        echo get_class($error), ':', $error->getMessage(), '|', basename($error->getFile()), ':', $error->getLine(), "\n";
    }
}
try_dynamic_compile('if (false) {class C { final final function foo($value) {}}}');
try_dynamic_compile('if (false) {class C { private protected $value; }}');
try_dynamic_compile('if (true) { __HALT_COMPILER(); }');
try_dynamic_compile('declare(encoding=[]);');
eval('echo "after-compile-errors\\n";');
"#;
    assert_eq!(
        run_php_with_source_context(source, "/virtual/eval-compile.php", "/virtual"),
        concat!(
            "CompileError:Multiple final modifiers are not allowed|eval-compile.php(3) : eval()'d code:1\n",
            "CompileError:Multiple access type modifiers are not allowed|eval-compile.php(3) : eval()'d code:1\n",
            "CompileError:__HALT_COMPILER() can only be used from the outermost scope|eval-compile.php(3) : eval()'d code:1\n",
            "CompileError:Encoding must be a literal|eval-compile.php(3) : eval()'d code:1\n",
            "after-compile-errors\n",
        )
    );
}

#[test]
fn include_warnings_report_the_include_keyword_line() {
    let source = r#"<?php
class IncludeProbe {
    public static function run(): void {
        include 'missing-dynamic-source-unit-file.php';
    }
}
IncludeProbe::run();
"#;
    assert_eq!(
        run_php_with_source_context(source, "/virtual/include-warning.php", "/virtual"),
        concat!(
            "\nWarning: include(missing-dynamic-source-unit-file.php): Failed to open stream: No such file or directory in /virtual/include-warning.php on line 4\n",
            "\nWarning: include(): Failed opening 'missing-dynamic-source-unit-file.php' for inclusion (include_path='.') in /virtual/include-warning.php on line 4\n",
        )
    );
}

#[test]
fn eval_rejects_ascii_control_bytes_with_php_source_metadata() {
    let source = r#"<?php
try { eval("\$a\x7Fb = 3;"); }
catch (ParseError $error) {
    echo $error->getMessage(), '|', basename($error->getFile()), ':', $error->getLine();
}
"#;
    assert_eq!(
        run_php_with_source_context(source, "/virtual/eval-control.php", "/virtual"),
        "syntax error, unexpected character 0x7F|eval-control.php(2) : eval()'d code:1"
    );
}

#[test]
fn eval_eof_parse_errors_use_the_synthetic_source_envelope() {
    let source = r#"<?php
foreach (["y&#", "y&#  ", "y&//", "-"] as $input) {
    try { eval($input); }
    catch (Throwable $error) {
        echo base64_encode($input), '|', $error->getMessage(), '|', basename($error->getFile()), ':', $error->getLine(), "\n";
    }
}
"#;
    assert_eq!(
        run_php_with_source_context(source, "/virtual/eval-parse.php", "/virtual"),
        concat!(
            "eSYj|syntax error, unexpected end of file|eval-parse.php(3) : eval()'d code:1\n",
            "eSYjICA=|syntax error, unexpected end of file|eval-parse.php(3) : eval()'d code:1\n",
            "eSYvLw==|syntax error, unexpected end of file|eval-parse.php(3) : eval()'d code:1\n",
            "LQ==|syntax error, unexpected end of file|eval-parse.php(3) : eval()'d code:1\n",
        )
    );
}
