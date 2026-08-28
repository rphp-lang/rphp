use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn run_stdin(source: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-n",
            "-d",
            "display_errors=stderr",
            "-d",
            "log_errors=0",
            "-d",
            "html_errors=0",
            "-d",
            "fatal_error_backtraces=0",
            "-d",
            "zend.exception_ignore_args=1",
        ])
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

fn assert_fatal(source: &str, message: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255, "{stderr:?}");
    assert!(stdout.is_empty(), "{stdout:?}");
    assert_eq!(
        stderr,
        format!("Fatal error: {message} in Standard input code on line {line}\n")
    );
}

#[test]
fn rounding_mode_is_an_internal_unit_enum_with_stable_case_singletons() {
    let source = r#"<?php
var_dump(enum_exists('RoundingMode', false));
var_dump(class_exists('RoundingMode', false));
var_dump(interface_exists('RoundingMode', false));
var_dump(is_a(RoundingMode::class, UnitEnum::class, true));
var_dump(is_a(RoundingMode::class, BackedEnum::class, true));
foreach (RoundingMode::cases() as $case) { echo $case->name, "\n"; }
$cases = RoundingMode::cases();
var_dump($cases[0] === RoundingMode::HalfAwayFromZero);
var_dump($cases[2] === RoundingMode::HalfEven);
var_dump($cases[7] === RoundingMode::PositiveInfinity);
var_dump(RoundingMode::HalfOdd instanceof UnitEnum);
var_dump(RoundingMode::HalfOdd instanceof BackedEnum);
$serialized = serialize(RoundingMode::HalfEven);
echo $serialized, "\n";
var_dump(unserialize($serialized) === RoundingMode::HalfEven);
echo match (RoundingMode::TowardsZero) {
    RoundingMode::TowardsZero => 'match',
    default => 'bad',
}, "\n";
"#;
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(
        stdout,
        "bool(true)\nbool(true)\nbool(false)\nbool(true)\nbool(false)\n\
HalfAwayFromZero\nHalfTowardsZero\nHalfEven\nHalfOdd\nTowardsZero\nAwayFromZero\n\
NegativeInfinity\nPositiveInfinity\nbool(true)\nbool(true)\nbool(true)\nbool(true)\n\
bool(false)\nE:21:\"RoundingMode:HalfEven\";\nbool(true)\nmatch\n"
    );
    assert!(stderr.is_empty(), "{stderr:?}");
}

#[test]
fn named_namespaced_aliased_and_case_insensitive_parents_are_canonical() {
    for (source, child, line) in [
        ("<?php\nclass Demo extends RoundingMode {}\n", "Demo", 2),
        ("<?php\nclass Demo extends roundingmode {}\n", "Demo", 2),
        (
            "<?php\nnamespace Oracle;\nclass Demo extends \\RoundingMode {}\n",
            "Oracle\\Demo",
            3,
        ),
        (
            "<?php\nnamespace Oracle;\nuse RoundingMode as Mode;\nclass Demo extends Mode {}\n",
            "Oracle\\Demo",
            4,
        ),
    ] {
        assert_fatal(
            source,
            &format!("Class {child} cannot extend enum RoundingMode"),
            line,
        );
    }
}

#[test]
fn anonymous_parent_and_declaration_priority_use_source_aware_compile_fatals() {
    assert_fatal(
        "<?php\nnew class extends RoundingMode {};\n",
        "Class RoundingMode@anonymous cannot extend enum RoundingMode",
        2,
    );
    assert_fatal(
        "<?php\nreadonly class Demo extends RoundingMode {}\n",
        "Class Demo cannot extend enum RoundingMode",
        2,
    );
    assert_fatal(
        "<?php\nabstract final class Demo extends RoundingMode {}\n",
        "Cannot use the final modifier on an abstract class",
        2,
    );

    let (status, stdout, stderr) =
        run_stdin("<?php\nclass Demo extends RoundingMode {}\nisset(, $value);\n");
    assert_eq!(status, 255);
    assert!(stdout.is_empty(), "{stdout:?}");
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected token \",\" in Standard input code on line 3\n"
    );
}

#[test]
fn reached_parent_failure_preserves_output_shutdown_and_no_partial_class() {
    let source = r#"<?php
register_shutdown_function(function () { echo class_exists('Demo', false) ? "visible\n" : "hidden\n"; });
echo "before\n";
try { class Demo extends RoundingMode {} }
catch (Throwable $error) { echo "caught\n"; }
echo "after\n";
"#;
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nhidden\n");
    assert_eq!(
        stderr,
        "Fatal error: Class Demo cannot extend enum RoundingMode in Standard input code on line 4\n"
    );

    let source = r#"<?php
if (false) { class ColdDemo extends RoundingMode {} }
function skipDemo(): void { return; class ReturnedDemo extends RoundingMode {} }
skipDemo();
echo class_exists('ColdDemo', false) ? 'bad' : 'cold-hidden', '|';
echo class_exists('ReturnedDemo', false) ? 'bad' : 'returned-hidden';
"#;
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(stdout, "cold-hidden|returned-hidden");
    assert!(stderr.is_empty(), "{stderr:?}");
}

struct IncludeFixture {
    directory: std::path::PathBuf,
    include: std::path::PathBuf,
}

impl IncludeFixture {
    fn new() -> Self {
        let identity = FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let directory = std::env::temp_dir().join(format!(
            "rphp_rounding_mode_parent_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let include = directory.join("included.php");
        std::fs::write(
            &include,
            "<?php\nnamespace Included;\nclass Demo extends \\RoundingMode {}\n",
        )
        .unwrap();
        Self { directory, include }
    }
}

impl Drop for IncludeFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn include_and_eval_keep_origin_prior_output_shutdown_and_uncatchability() {
    let fixture = IncludeFixture::new();
    let source = format!(
        "<?php\nregister_shutdown_function(function () {{ echo \"shutdown\\n\"; }});\necho \"before\\n\";\ntry {{ include '{}'; }}\ncatch (Throwable $error) {{ echo \"caught\\n\"; }}\necho \"after\\n\";\n",
        fixture.include.display()
    );
    let (status, stdout, stderr) = run_stdin(&source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        format!(
            "Fatal error: Class Included\\Demo cannot extend enum RoundingMode in {} on line 3\n",
            fixture.include.display()
        )
    );

    let source = r#"<?php
register_shutdown_function(function () { echo "shutdown\n"; });
echo "before\n";
try { eval('class EvaluatedDemo extends RoundingMode {}'); }
catch (Throwable $error) { echo "caught\n"; }
echo "after\n";
"#;
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        "Fatal error: Class EvaluatedDemo cannot extend enum RoundingMode in Standard input code(4) : eval()'d code on line 1\n"
    );
}

#[test]
fn missing_parent_remains_catchable_and_other_enum_parents_share_the_guard() {
    let source = r#"<?php
echo "before\n";
try { class Demo extends MissingParent {} }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
echo class_exists('Demo', false) ? "visible\n" : "hidden\n";
"#;
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(
        stdout,
        "before\nError:Class \"MissingParent\" not found\nhidden\n"
    );
    assert!(stderr.is_empty(), "{stderr:?}");

    for (source, parent) in [
        (
            "<?php\nenum ParentMode { case Value; }\nclass Demo extends ParentMode {}\n",
            "ParentMode",
        ),
        (
            "<?php\nclass Demo extends \\Random\\IntervalBoundary {}\n",
            "Random\\IntervalBoundary",
        ),
    ] {
        let line = if parent == "ParentMode" { 3 } else { 2 };
        assert_fatal(
            source,
            &format!("Class Demo cannot extend enum {parent}"),
            line,
        );
    }
}
