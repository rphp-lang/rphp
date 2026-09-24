mod common;

use common::{run_php, run_php_expect_error_with_source_context, run_php_with_source_context};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TemporaryDirectory(std::path::PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rphp-front-end-declaration-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn php_path(&self, name: &str) -> String {
        self.0
            .join(name)
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn goto_labels_survive_constant_branches_and_dead_sequential_code() {
    assert_eq!(
        run_php(
            r#"<?php
do {
    if (true) {
        echo "before\n";
        goto inside_else;
    } else {
inside_else:
        echo "after\n";
    }
} while (false);

goto after_block;
{
inside_block:
    goto finished;
    return;
}
after_block:
goto inside_block;
finished:
echo "done\n";
"#,
        ),
        "before\nafter\ndone\n"
    );
}

#[test]
fn goto_compile_errors_keep_the_faulting_source_location() {
    let duplicate = run_php_expect_error_with_source_context(
        "<?php\nlabel:\nlabel:\n",
        "/virtual/goto.php",
        "/virtual",
    );
    assert_eq!(
        duplicate.to_string(),
        "Label 'label' already defined in /virtual/goto.php on line 3"
    );

    let missing = run_php_expect_error_with_source_context(
        "<?php\ngoto absent;\n",
        "/virtual/goto.php",
        "/virtual",
    );
    assert_eq!(
        missing.to_string(),
        "'goto' to undefined label 'absent' in /virtual/goto.php on line 2"
    );
}

#[test]
fn asymmetric_set_visibility_requires_lexical_adjacency() {
    assert_eq!(
        run_php("<?php class Box { public private(set) mixed $value; } echo 'valid';",),
        "valid"
    );

    let error = run_php_expect_error_with_source_context(
        "<?php\nclass Box {\n    private (set) mixed $value;\n}\n",
        "/virtual/asymmetric.php",
        "/virtual",
    );
    assert_eq!(
        error.to_string(),
        "syntax error, unexpected token \")\", expecting token \"&\" on line 3"
    );
}

#[test]
fn eval_document_strings_use_the_first_payload_line_as_trace_origin() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function trace_origin() { echo debug_backtrace()[0]['file'], '|', debug_backtrace()[0]['line']; }
eval(<<<'PHP'
trace_origin();
PHP);
"#,
            "/virtual/eval.php",
            "/virtual",
        ),
        "/virtual/eval.php(4) : eval()'d code|1"
    );
}

#[test]
fn occupied_eager_class_names_defer_until_their_declaration_marker() {
    let directory = TemporaryDirectory::new();
    let guarded = directory.php_path("guarded.php");
    std::fs::write(
        directory.0.join("guarded.php"),
        "<?php\nif (class_exists(B::class)) { echo 'guarded'; return; }\nclass B extends MissingParent {}\n",
    )
    .unwrap();

    assert_eq!(
        run_php_with_source_context(
            &format!(
                "<?php\nclass A {{}}\nclass B extends A {{}}\nspl_autoload_register(function($name) {{ echo 'autoload:', $name; }});\ninclude '{guarded}';\n"
            ),
            "/virtual/main.php",
            "/virtual",
        ),
        "guarded"
    );

    let reached = directory.php_path("reached.php");
    let autoload_marker = directory.php_path("autoload-marker");
    std::fs::write(
        directory.0.join("reached.php"),
        "<?php\nclass B extends MissingParent {}\n",
    )
    .unwrap();
    let error = run_php_expect_error_with_source_context(
        &format!(
            "<?php\nclass A {{}}\nclass B extends A {{}}\nspl_autoload_register(function($name) {{ file_put_contents('{autoload_marker}', $name); }});\ninclude '{reached}';\n"
        ),
        "/virtual/main.php",
        "/virtual",
    );
    assert!(
        error
            .to_string()
            .starts_with("Cannot redeclare class B (previously declared in /virtual/main.php:3)"),
        "unexpected error: {error}"
    );
    assert!(!directory.0.join("autoload-marker").exists());
}

#[test]
fn clone_is_a_callable_with_operator_semantics_and_lexical_visibility() {
    assert_eq!(
        run_php(
            r#"<?php
$source = (object) ['value' => 1];
$direct = \clone($source);
$string = array_map('clone', [$source, $source]);
$firstClass = array_map(\clone(...), [$source]);
echo ($source !== $direct && $direct->value === 1) ? 'direct|' : 'bad|';
echo count($string), ':', count($firstClass), '|';

class CloneScope {
    public int $copies = 0;
    private function __clone() { $this->copies++; }
    public function copy(): self { return array_map(\clone(...), [$this])[0]; }
}
$owner = new CloneScope;
$copy = $owner->copy();
echo ($owner !== $copy && $copy->copies === 1) ? 'private|' : 'bad|';
try { \clone($owner); } catch (Error $error) { echo $error->getMessage(), '|'; }

$reflection = new ReflectionFunction('clone');
echo $reflection->getNumberOfRequiredParameters(), '/', $reflection->getNumberOfParameters(),
    ':', $reflection->getParameters()[1]->getName(), ':', $reflection->getReturnType();
"#,
        ),
        concat!(
            "direct|2:1|private|",
            "Call to private method CloneScope::__clone() from global scope|",
            "1/2:withProperties:object",
        )
    );
}
