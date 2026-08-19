/// End-to-end tests for include/require/include_once/require_once statements.
mod common;
use common::{run_php, run_php_expect_error_with_source_context, run_php_with_source_context};

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// RAII wrapper for a temporary directory — removed on drop.
struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("rphp_test_{}_{}", pid, id));
        std::fs::create_dir_all(&path).expect("failed to create temp dir");
        Self { path }
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Helper: create a temp PHP file with given content and return its absolute path.
fn write_temp_php(name: &str, content: &str) -> (TempDir, String) {
    let dir = TempDir::new();
    let file_path = dir.path().join(name);
    let mut f = std::fs::File::create(&file_path).expect("failed to create temp file");
    f.write_all(content.as_bytes())
        .expect("failed to write temp file");
    let abs = file_path.to_string_lossy().to_string();
    (dir, abs)
}

#[test]
fn test_basic_include() {
    let (_dir, path) = write_temp_php("included.php", "<?php echo 'from included';");
    let source = format!("<?php include '{}';", path);
    let output = run_php(&source);
    assert_eq!(output, "from included");
}

#[test]
fn test_basic_require() {
    let (_dir, path) = write_temp_php("required.php", "<?php echo 'from required';");
    let source = format!("<?php require '{}';", path);
    let output = run_php(&source);
    assert_eq!(output, "from required");
}

#[test]
fn included_interface_graph_enforces_enum_only_and_serializable_contracts() {
    let dir = TempDir::new();
    let contracts = dir.path().join("enum-contracts.php");
    std::fs::write(
        &contracts,
        "<?php interface ExternalUnit extends UnitEnum {} interface ExternalBacked extends BackedEnum {} interface ExternalLegacy extends Serializable {}",
    )
    .unwrap();

    let invalid = dir.path().join("invalid-enum-implementor.php");
    std::fs::write(&invalid, "<?php class Invalid implements ExternalUnit {}").unwrap();
    let invalid_source = format!(
        "<?php require '{}'; require '{}';",
        contracts.to_string_lossy(),
        invalid.to_string_lossy()
    );
    let error = run_php_expect_error_with_source_context(
        &invalid_source,
        "/virtual/include-driver.php",
        "/virtual",
    );
    assert_eq!(
        error.to_string(),
        format!(
            "Non-enum class Invalid cannot implement interface UnitEnum in {} on line 1",
            invalid.to_string_lossy()
        )
    );

    let aliased = dir.path().join("aliased-enum-implementor.php");
    std::fs::write(
        &aliased,
        "<?php class AliasedInvalid implements ExternalBackedAlias {}",
    )
    .unwrap();
    let aliased_source = format!(
        "<?php require '{}'; class_alias(ExternalBacked::class, 'ExternalBackedAlias'); require '{}';",
        contracts.to_string_lossy(),
        aliased.to_string_lossy(),
    );
    let error = run_php_expect_error_with_source_context(
        &aliased_source,
        "/virtual/include-driver.php",
        "/virtual",
    );
    assert_eq!(
        error.to_string(),
        format!(
            "Non-enum class AliasedInvalid cannot implement interface UnitEnum in {} on line 1",
            aliased.to_string_lossy()
        )
    );

    let legacy = dir.path().join("legacy-implementor.php");
    std::fs::write(
        &legacy,
        "<?php class LegacyValue implements ExternalLegacy { public function serialize(): string { return ''; } public function unserialize(string $data): void {} }",
    )
    .unwrap();
    let legacy_source = format!(
        "<?php require '{}'; require '{}'; echo 'ok';",
        contracts.to_string_lossy(),
        legacy.to_string_lossy()
    );
    assert_eq!(
        run_php_with_source_context(&legacy_source, "/virtual/include-driver.php", "/virtual"),
        format!(
            "\nDeprecated: LegacyValue implements the Serializable interface, which is deprecated. Implement __serialize() and __unserialize() instead (or in addition, if support for old PHP versions is necessary) in {} on line 1\nok",
            legacy.to_string_lossy()
        )
    );
}

#[test]
fn eval_returns_values_and_shares_the_callers_local_scope() {
    assert_eq!(
        run_php(
            r#"<?php
$outer = 'before';
function run_eval() {
    $local = 4;
    $result = eval('$local += 3; $created = "inside"; return $local * 2;');
    echo $result, '|', $local, '|', $created;
}
run_eval();
echo '|', $outer;
"#,
        ),
        "14|7|inside|before"
    );
}

#[test]
fn eval_backtrace_links_synthetic_source_frames() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function show_eval_trace($label) {
    echo $label, "\n";
    foreach (debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS) as $frame) {
        echo $frame['file'], ':', $frame['line'], ':', $frame['function'], "\n";
    }
}
eval("show_eval_trace('inline');");
eval("eval(\"show_eval_trace('nested');\");");
function capture_eval_trace() { return new Exception(); }
$exception = eval('return capture_eval_trace();');
echo "stored\n";
foreach ($exception->getTrace() as $frame) {
    echo $frame['file'], ':', $frame['line'], ':', $frame['function'], "\n";
}
"#,
            "/app/eval-trace.php",
            "/app",
        ),
        concat!(
            "inline\n",
            "/app/eval-trace.php(8) : eval()'d code:1:show_eval_trace\n",
            "/app/eval-trace.php:8:eval\n",
            "nested\n",
            "/app/eval-trace.php(9) : eval()'d code(1) : eval()'d code:1:show_eval_trace\n",
            "/app/eval-trace.php(9) : eval()'d code:1:eval\n",
            "/app/eval-trace.php:9:eval\n",
            "stored\n",
            "/app/eval-trace.php(11) : eval()'d code:1:capture_eval_trace\n",
            "/app/eval-trace.php:11:eval\n",
        )
    );
}

#[test]
fn eval_scope_preserves_reference_assignment_unset_and_rebinding() {
    assert_eq!(
        run_php(
            r#"<?php
function probe_eval_scope() {
    $array = [1, 2];
    $arrayAlias =& $array;
    foreach ($array as $value) {
        eval('array_pop($array);');
    }
    echo count($array), ':', count($arrayAlias), '|';

    $left = 1;
    $alias =& $left;
    eval('$left = 7; $alias = 8;');
    echo $left, ':', $alias, '|';

    eval('unset($left);');
    echo isset($left) ? 'set' : 'unset', ':', $alias, '|';

    $target = 2;
    eval('$left =& $target; $left = 9;');
    echo $left, ':', $target, ':', $alias;
}
probe_eval_scope();
"#,
        ),
        "0:0|8:8|unset:8|9:9:8"
    );
}

#[test]
fn include_scope_preserves_reference_assignment_unset_and_rebinding() {
    let (_dir, path) = write_temp_php(
        "reference_scope.php",
        "<?php $left = 7; $right = 8; unset($drop); $rebound =& $target; $rebound = 9;",
    );
    let source = format!(
        "<?php function probe_include_scope($path) {{ $left = 1; $right =& $left; $drop =& $left; $target = 2; include $path; echo $left, ':', $right, ':', isset($drop) ? 'set' : 'unset', ':', $target, ':', $rebound; }} probe_include_scope('{}');",
        path
    );

    assert_eq!(run_php(&source), "8:8:unset:9:9");
}

#[test]
fn eval_inherits_this_and_lexical_class_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class EvalScopeParent {
    private const LABEL = 'parent';
    private $value = 2;
    public function run(): void {
        eval('$this->value += 3; echo self::LABEL, "|", static::class, "|", $this->value;');
    }
}
class EvalScopeChild extends EvalScopeParent {}
(new EvalScopeChild)->run();
"#,
        ),
        "parent|EvalScopeChild|5"
    );
}

#[test]
fn eval_registers_declarations_and_throws_parse_errors() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(eval('$value = 1;'));
eval('function eval_function() { return 6; } class EvalClass { public $value = 7; }');
echo eval_function(), '|', (new EvalClass)->value, '|';
try {
    eval('this is not valid PHP');
} catch (ParseError $error) {
    echo get_class($error);
}
"#,
        ),
        "NULL\n6|7|ParseError"
    );
}

#[test]
fn error_suppressed_eval_masks_diagnostics_and_restores_after_catchable_parse_errors() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(30719);
@eval('echo error_reporting(); trigger_error("hidden", E_USER_WARNING);');
echo '|', error_reporting(), '|';
try {
    @eval('not php ;');
} catch (ParseError $error) {
    echo get_class($error), '|', error_reporting();
}
"#,
        ),
        "4437|30719|ParseError|30719"
    );
}

#[test]
fn eval_compile_errors_are_fatal_and_bypass_throwable_handlers() {
    let error = run_php_expect_error_with_source_context(
        r#"<?php
try {
    eval('class self {}');
} catch (Throwable $error) {
    echo 'caught';
}
echo 'after';
"#,
        "/app/eval-compile-fatal.php",
        "/app",
    );

    assert_eq!(
        error.to_string(),
        "Cannot use \"self\" as a class name as it is reserved in /app/eval-compile-fatal.php(3) : eval()'d code on line 1"
    );
}

#[test]
fn eval_compiles_generated_wide_addition_without_host_stack_overflow() {
    let expression = std::iter::repeat_n("1", 2_001)
        .collect::<Vec<_>>()
        .join(" + ");
    let source = format!("<?php eval('echo {expression};');");

    assert_eq!(run_php(&source), "2001");
}

#[test]
fn include_expressions_return_explicit_and_implicit_values() {
    let (_explicit_dir, explicit) = write_temp_php("explicit.php", "<?php return 'loaded';");
    let (_implicit_dir, implicit) = write_temp_php("implicit.php", "<?php $value = 1;");
    let source = format!(
        "<?php $explicit = require '{}'; $implicit = include '{}'; var_dump($explicit, $implicit);",
        explicit, implicit
    );

    assert_eq!(run_php(&source), "string(6) \"loaded\"\nint(1)\n");
}

#[test]
fn include_once_expression_returns_true_after_the_first_execution() {
    let (_dir, path) = write_temp_php("once_value.php", "<?php return 42;");
    let source = format!(
        "<?php var_dump(include_once '{}'); var_dump(include_once '{}');",
        path, path
    );

    assert_eq!(run_php(&source), "int(42)\nbool(true)\n");
}

#[test]
fn included_file_inventory_is_unique_ordered_and_shared_by_both_aliases() {
    let dir = TempDir::new();
    let inner = dir.path().join("inner.php");
    std::fs::write(&inner, "<?php echo 'i';").unwrap();
    let outer = dir.path().join("outer.php");
    std::fs::write(
        &outer,
        format!("<?php echo 'o'; include '{}';", inner.to_string_lossy()),
    )
    .unwrap();
    let source = format!(
        r#"<?php
include '{outer}';
include_once '{inner}';
$included = get_included_files();
$required = get_required_files();
echo '|', count($included), '|', basename($included[0]), ',', basename($included[1]), '|', $included === $required ? 'same' : 'different';
"#,
        outer = outer.to_string_lossy(),
        inner = inner.to_string_lossy(),
    );

    assert_eq!(run_php(&source), "oi|2|outer.php,inner.php|same");
}

#[test]
fn missing_include_expression_warns_and_returns_false() {
    let output = run_php(
        "<?php $result = include '/nonexistent/rphp/include-expression.php'; var_dump($result);",
    );
    assert!(output.contains("Warning: include("), "{output}");
    assert!(output.ends_with("bool(false)\n"), "{output}");
}

#[test]
fn nested_include_expression_forwards_the_inner_return_value() {
    let dir = TempDir::new();
    let inner = dir.path().join("inner-return.php");
    std::fs::write(&inner, "<?php return 'nested';").unwrap();
    let outer = dir.path().join("outer-return.php");
    std::fs::write(
        &outer,
        format!("<?php return require '{}';", inner.to_string_lossy()),
    )
    .unwrap();
    let source = format!(
        "<?php $result = require '{}'; var_dump($result);",
        outer.to_string_lossy()
    );

    assert_eq!(run_php(&source), "string(6) \"nested\"\n");
}

#[test]
fn test_include_shares_variables() {
    // Included file should be able to see variables set before the include
    // and set variables that are visible after the include.
    let (_dir, path) = write_temp_php("share_vars.php", "<?php echo $x; $y = 'world';");
    let source = format!("<?php $x = 'hello'; include '{}'; echo $y;", path);
    let output = run_php(&source);
    assert_eq!(output, "helloworld");
}

#[test]
fn included_code_can_remove_a_literal_local_before_a_later_read() {
    let (dir, fragment) = write_temp_php("drop-scope-value.php", "<?php unset($payload);");
    let driver = dir.path().join("scope-driver.php");
    let source = format!(
        r#"<?php
function inspect_fragment($fragment) {{
    $payload = "route-ready\n";
    echo $payload;
    include $fragment;
    echo $payload;
}}
inspect_fragment('{fragment}');"#
    );

    assert_eq!(
        run_php_with_source_context(
            &source,
            driver.to_string_lossy().as_ref(),
            dir.path().to_string_lossy().as_ref(),
        ),
        format!(
            "route-ready\n\nWarning: Undefined variable $payload in {} on line 6\n",
            driver.to_string_lossy()
        )
    );
}

#[test]
fn included_code_can_replace_a_proven_string_with_an_integer() {
    let (dir, fragment) = write_temp_php("replace-scope-value.php", "<?php $payload = 17;");
    let driver = dir.path().join("replacement-driver.php");
    let source = format!(
        r#"<?php
function render_fragment($fragment) {{
    $payload = "stale";
    include $fragment;
    echo $payload;
}}
render_fragment('{fragment}');"#
    );

    assert_eq!(
        run_php_with_source_context(
            &source,
            driver.to_string_lossy().as_ref(),
            dir.path().to_string_lossy().as_ref(),
        ),
        "17"
    );
}

#[test]
fn echo_of_a_removed_local_reports_the_original_source_location() {
    let dir = TempDir::new();
    let driver = dir.path().join("removed-local.php");
    let source = "<?php\n$ticket = 'active';\nunset($ticket);\necho $ticket;";

    assert_eq!(
        run_php_with_source_context(
            source,
            driver.to_string_lossy().as_ref(),
            dir.path().to_string_lossy().as_ref(),
        ),
        format!(
            "\nWarning: Undefined variable $ticket in {} on line 4\n",
            driver.to_string_lossy()
        )
    );
}

#[test]
fn include_inside_function_shares_locals_without_leaking_into_global_scope() {
    let (_dir, path) = write_temp_php(
        "local_scope.php",
        "<?php $seen = $shared; $shared = 'changed'; $created = 'included';",
    );
    let source = format!(
        r#"<?php
$shared = "global";
function run_local_include($path) {{
    $shared = "local";
    include $path;
    echo $seen, "|", $shared, "|", $created, "|";
}}
run_local_include('{}');
echo $shared, "|";
var_dump(isset($created));
"#,
        path
    );

    assert_eq!(
        run_php(&source),
        "local|changed|included|global|bool(false)\n"
    );
}

#[test]
fn global_declaration_inside_include_uses_and_updates_the_real_global() {
    let (_dir, path) = write_temp_php(
        "global_scope.php",
        "<?php global $shared; echo $shared, '|'; $shared = 'changed-global';",
    );
    let source = format!(
        r#"<?php
$shared = "global";
function run_global_include($path) {{
    $shared = "local";
    include $path;
    echo $shared, "|";
}}
run_global_include('{}');
echo $shared;
"#,
        path
    );

    assert_eq!(run_php(&source), "global|changed-global|changed-global");
}

#[test]
fn include_inside_method_inherits_this() {
    let (_dir, path) = write_temp_php(
        "method_scope.php",
        "<?php $this->value = $this->value . ':included'; return $this->value;",
    );
    let source = format!(
        r#"<?php
class IncludeOwner {{
    public $value = "owner";
    public function run($path) {{
        $result = include $path;
        return $result . "|" . $this->value;
    }}
}}
echo (new IncludeOwner())->run('{}');
"#,
        path
    );

    assert_eq!(run_php(&source), "owner:included|owner:included");
}

#[test]
fn include_inside_method_inherits_private_class_scope() {
    let (_dir, path) = write_temp_php(
        "private_scope.php",
        "<?php return $this->secret . '|' . self::label();",
    );
    let source = format!(
        r#"<?php
class PrivateIncludeOwner {{
    private $secret = "private";
    private static function label() {{ return "scope"; }}
    public function run($path) {{ return include $path; }}
}}
echo (new PrivateIncludeOwner())->run('{}');
"#,
        path
    );

    assert_eq!(run_php(&source), "private|scope");
}

#[test]
fn exception_from_included_file_is_catchable_and_keeps_prior_scope_writes() {
    let (_dir, path) = write_temp_php(
        "throw.php",
        "<?php $value = 'mutated'; throw new Exception('included');",
    );
    let source = format!(
        r#"<?php
function run_throwing_include($path) {{
    $value = "before";
    try {{
        include $path;
    }} catch (Exception $error) {{
        echo $error->getMessage(), "|";
    }}
    echo $value;
}}
run_throwing_include('{}');
"#,
        path
    );

    assert_eq!(run_php(&source), "included|mutated");
}

#[test]
fn parse_error_from_included_file_is_catchable() {
    let (_dir, path) = write_temp_php("parse_error.php", "<?php function broken( {");
    let source = format!(
        r#"<?php
try {{
    include '{}';
}} catch (CompileError $error) {{
    echo "caught";
}}
"#,
        path
    );

    assert_eq!(run_php(&source), "caught");
}

#[test]
fn compile_error_from_included_file_is_fatal_and_not_catchable() {
    let (_dir, path) = write_temp_php("compile_error.php", "<?php class self {}");
    let canonical = std::fs::canonicalize(&path)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let source = format!(
        r#"<?php
try {{
    include '{}';
}} catch (Throwable $error) {{
    echo 'caught';
}}
echo 'after';
"#,
        path
    );
    let error =
        run_php_expect_error_with_source_context(&source, "/app/include-compile-fatal.php", "/app");

    assert_eq!(
        error.to_string(),
        format!("Cannot use \"self\" as a class name as it is reserved in {canonical} on line 1")
    );
}

#[test]
fn numeric_separator_parse_error_from_include_preserves_php_origin_metadata() {
    let (_dir, path) = write_temp_php("numeric-separator.php", "<?php\n100_;");
    let canonical = std::fs::canonicalize(&path)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let source = format!(
        r#"<?php
try {{
    include '{}';
}} catch (CompileError $error) {{
    echo get_class($error), '|', $error->getMessage(), '|', $error->getFile(), '|', $error->getLine();
}}
"#,
        path
    );

    assert_eq!(
        run_php(&source),
        format!("ParseError|syntax error, unexpected identifier \"_\"|{canonical}|2")
    );
}

#[test]
fn heredoc_indentation_parse_error_from_include_preserves_php_origin_metadata() {
    let (_dir, path) = write_temp_php(
        "heredoc-indentation.php",
        "<?php\necho <<<DOC\n  first\nsecond\n  DOC;",
    );
    let canonical = std::fs::canonicalize(&path)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let source = format!(
        r#"<?php
try {{
    include '{}';
}} catch (CompileError $error) {{
    echo get_class($error), '|', $error->getMessage(), '|', $error->getFile(), '|', $error->getLine();
}}
"#,
        path
    );

    assert_eq!(
        run_php(&source),
        format!(
            "ParseError|Invalid body indentation level (expecting an indentation level of at least 2)|{canonical}|4"
        )
    );
}

#[test]
fn excessive_syntax_nesting_in_an_included_file_is_catchable() {
    let nested = format!(
        "<?php\nfunction shelter() {{ return {}0{}; }}",
        "[".repeat(300),
        "]".repeat(300),
    );
    let (_dir, path) = write_temp_php("nested.php", &nested);
    let canonical = std::fs::canonicalize(&path)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let source = format!(
        r#"<?php
try {{
    include '{}';
}} catch (CompileError $error) {{
    echo $error->getMessage();
}}
"#,
        path
    );

    assert_eq!(
        run_php(&source),
        format!("Parse error: memory exhausted in {canonical} on line 2")
    );
}

#[test]
fn missing_require_throws_a_catchable_error() {
    let output = run_php(
        r#"<?php
try {
    require "/nonexistent/rphp/required.php";
} catch (Error $error) {
    echo "caught";
}
"#,
    );

    assert!(output.contains("Warning: require("), "{output}");
    assert!(output.ends_with("caught"), "{output}");
}

#[test]
fn test_include_function_declaration() {
    let (_dir, path) = write_temp_php(
        "func.php",
        "<?php function greet($name) { return 'Hello ' . $name; }",
    );
    let source = format!("<?php include '{}'; echo greet('World');", path);
    let output = run_php(&source);
    assert_eq!(output, "Hello World");
}

#[test]
fn test_included_static_return_contract_uses_runtime_called_class() {
    let (_dir, path) = write_temp_php(
        "late_static.php",
        r#"<?php
class IncludedStaticBase {
    public static function factory(): static { return new IncludedStaticChild(); }
    public static function wrong(): static { return new IncludedStaticBase(); }
}
class IncludedStaticChild extends IncludedStaticBase {}
"#,
    );
    let source = format!(
        r#"<?php
include '{}';
echo IncludedStaticChild::factory() instanceof IncludedStaticChild ? "yes:" : "no:";
try {{ IncludedStaticChild::wrong(); }} catch (TypeError $error) {{ echo "caught"; }}
"#,
        path
    );
    assert_eq!(run_php(&source), "yes:caught");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn test_include_merges_and_relocates_generic_metadata() {
    let (_dir, path) = write_temp_php(
        "generic.php",
        r#"<?php
function included_id<T : string>(T $value): T { return $value; }
function included_call() {
    $includedCallable = "included_id";
    return ($includedCallable)::<string>("s");
}
function included_closure() {
    return function<C : object = stdClass>() {};
}
class IncludedCaller {
    public function call() {
        $includedCallable = "included_id";
        return ($includedCallable)::<string>("s");
    }
}
class IncludedBox<T> { public T $value; }
class IncludedParent<T : int> {}
class IncludedChild<U : int> extends IncludedParent<U> {}
echo included_call();
echo (new IncludedCaller())->call();
$reflection = new ReflectionFunction("included_id");
$parameters = $reflection->getGenericParameters();
echo $reflection->isGeneric() ? ":yes:" : ":no:";
echo $parameters[0]->getName() . ":" . $parameters[0]->getBound()->getName();
echo ":" . (new ReflectionFunction(included_closure()))->getGenericParameters()[0]->getDefault()->getName();
echo (new IncludedChild::<int>()) instanceof IncludedParent ? ":linked" : ":missing";
"#,
    );
    let source = format!(
        r#"<?php
function main_id<T : int>(T $value): T {{ return $value; }}
$mainCallable = "main_id";
echo ($mainCallable)::<int>(1);
include '{}';
$box = new IncludedBox::<int>();
$box->value = 2;
echo ":" . $box->value;
"#,
        path
    );
    assert_eq!(run_php(&source), "1ss:yes:T:string:stdClass:linked:2");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn test_include_revalidates_cross_unit_inheritance_variance() {
    let (_dir, path) = write_temp_php(
        "generic_variance.php",
        "<?php interface Bad<+T> extends Consumer<T> {}",
    );
    let source = format!("<?php interface Consumer<-T> {{}} include '{}';", path);
    let error = common::run_php_expect_error(&source);
    assert!(
        format!("{error:?}").contains("in contravariant position"),
        "{error:?}"
    );
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn test_include_validates_cross_unit_parametric_lsp() {
    let (_dir, path) = write_temp_php(
        "generic_lsp.php",
        "<?php class Bad implements Source<int> { public function get(): string { return 'bad'; } }",
    );
    let source = format!(
        "<?php interface Source<T> {{ public function get(): T; }} include '{}';",
        path
    );
    let error = common::run_php_expect_error(&source);
    assert!(
        format!("{error:?}").contains("Parametric LSP violation"),
        "{error:?}"
    );
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn test_include_links_reified_instance_method_contracts() {
    let (_dir, path) = write_temp_php(
        "generic_reified_method.php",
        "<?php class IncludedParent<T> { public function id(T $value): T { return $value; } } class IncludedChild<U> extends IncludedParent<U> {}",
    );
    let source = format!(
        "<?php include '{}'; $box = new IncludedChild::<int>(); $box->id('bad');",
        path
    );
    let error = common::run_php_expect_error(&source);
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #1 passed to IncludedParent::id()"),
        "{rendered:?}"
    );
    assert!(rendered.contains("reified class type"), "{rendered:?}");
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn test_include_reflects_structured_reified_object_arguments() {
    let (_dir, path) = write_temp_php(
        "generic_reified_reflection.php",
        r#"<?php
class IncludedReflectedPair<T, U = T> {}
function included_reflected_pair() {
    return new IncludedReflectedPair::<int>();
}
"#,
    );
    let source = format!(
        "<?php include '{}'; $arguments = (new ReflectionObject(included_reflected_pair()))->getGenericArguments(); echo get_class($arguments[0]) . ':' . $arguments[0]->getName() . ':' . $arguments[1]->getName();",
        path
    );
    assert_eq!(run_php(&source), "ReflectionNamedType:int:int");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn test_include_links_inherited_generic_property_contracts() {
    let (_dir, path) = write_temp_php(
        "generic_property_child.php",
        "<?php class IncludedPropertyChild extends IncludedPropertyParent<int> {}",
    );
    let source = format!(
        "<?php class IncludedPropertyParent<T> {{ public T $value; }} include '{}'; $box = new IncludedPropertyChild(); $box->value = 'bad';",
        path
    );
    let error = common::run_php_expect_error(&source);
    let rendered = format!("{error:?}");
    assert!(rendered.contains("bound-erased property"), "{rendered:?}");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn test_include_links_inherited_generic_method_and_constructor_contracts() {
    let (_dir, path) = write_temp_php(
        "generic_method_child.php",
        "<?php class IncludedMethodChild extends IncludedMethodParent<int> {}",
    );
    for operation in [
        "$box = new IncludedMethodChild(1); $box->id('bad');",
        "new IncludedMethodChild('bad');",
    ] {
        let source = format!(
            "<?php class IncludedMethodParent<T> {{ public function __construct(T $value) {{}} public function id(T $value): T {{ return $value; }} }} include '{}'; {}",
            path, operation
        );
        let error = common::run_php_expect_error(&source);
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains("linked generic class type"),
            "{rendered:?}"
        );
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn test_include_alpha_links_generic_method_parameters_and_bounds() {
    let (_dir, path) = write_temp_php(
        "generic_method_parameter_child.php",
        "<?php class IncludedMethodChild extends IncludedMethodParent<int> {}",
    );
    let source = format!(
        "<?php class IncludedMethodParent<T> {{ public function id<U : T>(U $value): U {{ return $value; }} }} include '{}'; $box = new IncludedMethodChild(); $box->id('bad');",
        path
    );
    let error = common::run_php_expect_error(&source);
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #1 passed to IncludedMethodChild::id()"),
        "{rendered:?}"
    );
    assert!(
        rendered.contains("linked generic class type"),
        "{rendered:?}"
    );

    let (_dir, path) = write_temp_php(
        "generic_method_alpha_child.php",
        "<?php class IncludedBadMapper implements IncludedMapper { public function map<X, Y>(Y $value): X { return $value; } }",
    );
    let source = format!(
        "<?php interface IncludedMapper {{ public function map<A, B>(A $value): B; }} include '{}';",
        path
    );
    let error = common::run_php_expect_error(&source);
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Parametric LSP violation"),
        "{rendered:?}"
    );
    assert!(rendered.contains("parameter 1"), "{rendered:?}");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn test_include_merges_cross_unit_generic_diamond_contracts() {
    let (_dir, path) = write_temp_php(
        "generic_diamond_child.php",
        "<?php class IncludedDiamond { use IncludedPipeline<Renderable>, IncludedPipeline<Cacheable>; }",
    );
    let prefix = format!(
        "<?php interface Renderable {{}} interface Cacheable {{}} class Article implements Renderable, Cacheable {{}} class RenderOnly implements Renderable {{}} trait IncludedPipeline<T : object> {{ public T $value; public function process(T $value): T {{ return new Article(); }} }} include '{}';",
        path
    );
    let output = run_php(&format!(
        "{} $diamond = new IncludedDiamond(); $diamond->value = new RenderOnly(); echo ($diamond->process(new RenderOnly()) instanceof Cacheable) ? 'merged' : 'missing';",
        prefix
    ));
    assert_eq!(output, "merged");

    let error = common::run_php_expect_error(&format!(
        "{} $diamond = new IncludedDiamond(); $diamond->value = new stdClass();",
        prefix
    ));
    assert!(format!("{error:?}").contains("property"), "{error:?}");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn test_include_reflects_plural_generic_ancestor_bindings() {
    let (_dir, path) = write_temp_php(
        "generic_reflection_diamond.php",
        "<?php interface IncludedStrings extends IncludedView<string> {} interface IncludedInts extends IncludedView<int> {} class IncludedViewDiamond implements IncludedStrings, IncludedInts {}",
    );
    let source = format!(
        "<?php interface IncludedView<T> {{}} include '{}'; $reflection = new ReflectionClass('IncludedViewDiamond'); $bindings = $reflection->getGenericArgumentsForParentInterface('IncludedView'); echo count($bindings) . ':' . $bindings[0][0]->getName() . ':' . $bindings[1][0]->getName();",
        path
    );
    assert_eq!(run_php(&source), "2:string:int");
}

#[test]
fn test_require_missing_file_fatal_error() {
    let source = "<?php require '/nonexistent/path/to/file.php';";
    let err = common::run_php_expect_error(source);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Failed opening required"),
        "Expected fatal error about missing file, got: {}",
        msg
    );
}

#[test]
fn test_include_missing_file_warning() {
    // include with missing file should produce a warning but continue execution
    let source = "<?php include '/nonexistent/path/to/file.php'; echo 'still running';";
    let output = run_php(source);
    assert!(
        output.contains("Warning"),
        "Expected warning about missing file, got: {}",
        output
    );
    assert!(
        output.contains("still running"),
        "Expected execution to continue after include warning, got: {}",
        output
    );
}

#[test]
fn test_include_once_only_runs_once() {
    let (_dir, path) = write_temp_php("once.php", "<?php echo 'X';");
    let source = format!("<?php include_once '{}'; include_once '{}';", path, path);
    let output = run_php(&source);
    assert_eq!(
        output, "X",
        "include_once should only execute the file once"
    );
}

#[test]
fn test_require_once_only_runs_once() {
    let (_dir, path) = write_temp_php("ronce.php", "<?php echo 'Y';");
    let source = format!("<?php require_once '{}'; require_once '{}';", path, path);
    let output = run_php(&source);
    assert_eq!(
        output, "Y",
        "require_once should only execute the file once"
    );
}

#[test]
fn test_include_once_and_include() {
    // include_once followed by regular include should run twice
    let (_dir, path) = write_temp_php("mixed.php", "<?php echo 'Z';");
    let source = format!("<?php include_once '{}'; include '{}';", path, path);
    let output = run_php(&source);
    assert_eq!(output, "ZZ", "include_once + include should run file twice");
}

#[test]
fn test_nested_include() {
    let dir = TempDir::new();

    let inner_path = dir.path().join("inner.php");
    let mut f = std::fs::File::create(&inner_path).unwrap();
    f.write_all(b"<?php echo 'inner';").unwrap();

    let outer_path = dir.path().join("outer.php");
    let mut f = std::fs::File::create(&outer_path).unwrap();
    let outer_content = format!(
        "<?php echo 'outer'; include '{}';",
        inner_path.to_string_lossy()
    );
    f.write_all(outer_content.as_bytes()).unwrap();

    let source = format!("<?php include '{}';", outer_path.to_string_lossy());
    let output = run_php(&source);
    assert_eq!(output, "outerinner");
}

#[test]
fn test_include_inside_function() {
    // Include inside a function should see local variables
    let (_dir, path) = write_temp_php("func_scope.php", "<?php echo $x;");
    let source = format!(
        r#"<?php
function f() {{
    $x = 42;
    include '{}';
}}
f();
"#,
        path
    );
    let output = run_php(&source);
    assert_eq!(output, "42");
}

#[test]
fn test_relative_include_from_file_directory() {
    // When a.php includes "b.php", it should resolve relative to a.php's directory
    let dir = TempDir::new();
    let sub = dir.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();

    let b_path = sub.join("b.php");
    let mut f = std::fs::File::create(&b_path).unwrap();
    f.write_all(b"<?php echo 'OK';").unwrap();

    // a.php uses relative path "b.php" — should resolve relative to sub/ not CWD
    let a_path = sub.join("a.php");
    let mut f = std::fs::File::create(&a_path).unwrap();
    f.write_all(b"<?php include 'b.php';").unwrap();

    let source = format!("<?php include '{}';", a_path.to_string_lossy());
    let output = run_php(&source);
    assert_eq!(output, "OK");
}
