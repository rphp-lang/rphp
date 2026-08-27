use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_DIR_ID: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = TEMP_DIR_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("rphp-foreach-list-key-{}-{id}", std::process::id()));
        std::fs::create_dir(&path).expect("temporary directory should be created");
        Self(path)
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, source).expect("temporary PHP source should be written");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

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
    collect(child.wait_with_output().expect("rphp should finish"))
}

fn run_file(path: &Path) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .arg(path)
        .output()
        .expect("rphp subprocess should finish");
    collect(output)
}

fn collect(output: std::process::Output) -> (i32, String, String) {
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

#[test]
fn list_key_forms_and_source_boundaries_are_located_compile_fatals() {
    for (source, line) in [
        (
            "<?php\nfunction source_probe() { echo 'ITERABLE'; return [[1]]; }\nforeach (\n    source_probe() as\n    list($key) => $value\n) { echo 'BODY'; }\n",
            4,
        ),
        (
            "<?php\nfunction source_probe() { echo 'ITERABLE'; return [[1]]; }\nforeach (\n    source_probe() as\n    [$key] => $value\n) { echo 'BODY'; }\n",
            4,
        ),
        (
            "<?php\nforeach (\n    [[1]] as\n    list(list($key)) => list(list(), $value)\n) { echo 'BODY'; }\n",
            3,
        ),
        (
            "<?php\nnamespace Boundary;\nforeach (\n    [[1]] as\n    [$key] => $value\n) { echo 'BODY'; }\n",
            4,
        ),
        (
            "<?php\nforeach ([[1]] as list() => $value) { echo 'BODY'; }\n",
            2,
        ),
        (
            "<?php\nforeach ([[1]] as [] => $value) { echo 'BODY'; }\n",
            2,
        ),
        (
            "<?php\nforeach ([[1]] as [list($key)] => $value) { echo 'BODY'; }\n",
            2,
        ),
        (
            "<?php\nforeach (\n    1 as\n    [$key] => $value\n) { echo 'BODY'; }\n",
            3,
        ),
        (
            "<?php\nforeach (\n    \"source\" as\n    list($key) => $value\n) { echo 'BODY'; }\n",
            3,
        ),
        (
            "<?php\nforeach (\n    null as\n    [$key] => $value\n) { echo 'BODY'; }\n",
            3,
        ),
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(
            stderr,
            format!(
                "Fatal error: Cannot use list as key element in Standard input code on line {line}\n"
            )
        );
    }
}

#[test]
fn ordinary_keys_value_destructuring_and_reference_iteration_remain_valid() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
$rows = [[10, [1, 2]], [20, [3, 4]]];
foreach ($rows as $key => list($head, list($left, $right))) {
    echo "L:$key:$head:$left:$right;";
}
foreach ($rows as $key => [$head, [$left, $right]]) {
    echo "S:$key:$head:$left:$right;";
}
$values = [1, 2];
foreach ($values as $key => &$value) { $value += $key + 1; }
unset($value);
foreach ($values as &$value) { $value *= 2; }
unset($value);
echo implode(',', $values);
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "L:0:10:1:2;L:1:20:3:4;S:0:10:1:2;S:1:20:3:4;4,8");
    assert_eq!(stderr, "");
}

#[test]
fn syntax_and_declaration_diagnostics_keep_source_order_priority() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nreadonly readonly class Broken {}\nforeach ([[1]] as [$key] => $value) {}\n",
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Fatal error: Multiple readonly modifiers are not allowed in Standard input code on line 2\n"
    );

    let (status, stdout, stderr) = run_stdin(
        "<?php\nforeach ([[1]] as [$key] => $value) {}\nreadonly readonly class Broken {}\n",
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Fatal error: Cannot use list as key element in Standard input code on line 2\n"
    );

    let (status, stdout, stderr) =
        run_stdin("<?php\nforeach ([[1]] as [$key] => $value) {}\nif (\n");
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert!(stderr.starts_with("Parse error:"), "{stderr:?}");
    assert!(
        !stderr.contains("Cannot use list as key element"),
        "{stderr:?}"
    );
}

#[test]
fn included_and_evaluated_units_report_their_own_compile_fatal_origin() {
    let dir = TempDir::new();
    let included = dir.write(
        "included-boundary.php",
        "<?php\nfunction included_probe() { echo 'ITERABLE'; return [[1]]; }\nforeach (\n    included_probe() as\n    list($key) => $value\n) { echo 'BODY'; }\n",
    );
    let main = dir.write(
        "include-main.php",
        &format!(
            "<?php\necho \"INCLUDE-BEFORE\\n\";\ninclude '{}';\necho \"INCLUDE-AFTER\\n\";\n",
            included.display()
        ),
    );
    let included = std::fs::canonicalize(included).expect("included path should canonicalize");

    let (status, stdout, stderr) = run_file(&main);
    assert_eq!(status, 255);
    assert_eq!(stdout, "INCLUDE-BEFORE\n");
    assert_eq!(
        stderr,
        format!(
            "\nFatal error: Cannot use list as key element in {} on line 4\n",
            included.display()
        )
    );

    let evaluated = dir.write(
        "eval-main.php",
        r#"<?php
echo "EVAL-BEFORE\n";
eval("function evaluated_probe() { echo 'ITERABLE'; return [[1]]; }\nforeach (\n evaluated_probe() as\n [\$key] => \$value\n) { echo 'BODY'; }\n");
echo "EVAL-AFTER\n";
"#,
    );
    let evaluated = std::fs::canonicalize(evaluated).expect("eval path should canonicalize");

    let (status, stdout, stderr) = run_file(&evaluated);
    assert_eq!(status, 255);
    assert_eq!(stdout, "EVAL-BEFORE\n");
    assert_eq!(
        stderr,
        format!(
            "\nFatal error: Cannot use list as key element in {}(3) : eval()'d code on line 3\n",
            evaluated.display()
        )
    );
}
