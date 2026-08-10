/// End-to-end tests for include/require/include_once/require_once statements.
mod common;
use common::run_php;

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
fn test_include_shares_variables() {
    // Included file should be able to see variables set before the include
    // and set variables that are visible after the include.
    let (_dir, path) = write_temp_php("share_vars.php", "<?php echo $x; $y = 'world';");
    let source = format!("<?php $x = 'hello'; include '{}'; echo $y;", path);
    let output = run_php(&source);
    assert_eq!(output, "helloworld");
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
class IncludedCaller {
    public function call() {
        $includedCallable = "included_id";
        return ($includedCallable)::<string>("s");
    }
}
class IncludedBox<T> { public T $value; }
echo included_call();
echo (new IncludedCaller())->call();
$reflection = new ReflectionFunction("included_id");
$parameters = $reflection->getGenericParameters();
echo $reflection->isGeneric() ? ":yes:" : ":no:";
echo $parameters[0]["name"] . ":" . $parameters[0]["bound"];
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
    assert_eq!(run_php(&source), "1ss:yes:T:string:2");
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
