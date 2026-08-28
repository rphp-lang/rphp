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

fn assert_traversable_fatal(source: &str, enum_name: &str) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255, "{stderr:?}");
    assert!(stdout.is_empty(), "{stdout:?}");
    assert_eq!(
        stderr,
        format!(
            "Fatal error: Enum {enum_name} must implement interface Traversable as part of either Iterator or IteratorAggregate in Unknown on line 0\n"
        )
    );
}

#[test]
fn direct_aliased_and_inherited_traversable_relations_use_enum_diagnostic() {
    for (source, enum_name) in [
        ("<?php\nenum Signal implements Traversable {}\n", "Signal"),
        (
            "<?php\nenum Signal: int implements Traversable { case Ready = 1; }\n",
            "Signal",
        ),
        (
            "<?php\nnamespace Oracle;\nenum Signal implements \\Traversable {}\n",
            "Oracle\\Signal",
        ),
        (
            "<?php\nnamespace Oracle;\nuse Traversable as Walkable;\nenum Signal implements Walkable {}\n",
            "Oracle\\Signal",
        ),
        (
            "<?php\ninterface Walkable extends Traversable {}\nenum Signal implements Walkable {}\n",
            "Signal",
        ),
        (
            "<?php\ninterface LeftWalk extends Traversable {}\ninterface RightWalk extends Traversable {}\nenum Signal implements LeftWalk, RightWalk {}\n",
            "Signal",
        ),
    ] {
        assert_traversable_fatal(source, enum_name);
    }
}

#[test]
fn iterator_and_iterator_aggregate_relations_remain_valid() {
    let sources = [
        r#"<?php
enum Signal implements Iterator {
    case Ready;
    public function current(): mixed { return $this; }
    public function key(): mixed { return 0; }
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool { return false; }
}
echo "ok";
"#,
        r#"<?php
enum Signal implements Traversable, IteratorAggregate {
    case Ready;
    public function getIterator(): Traversable { yield $this; }
}
echo "ok";
"#,
        r#"<?php
interface Walkable extends IteratorAggregate {}
trait Iteration { public function getIterator(): Traversable { yield $this; } }
enum Signal implements Walkable { use Iteration; case Ready; }
echo "ok";
"#,
        r#"<?php
namespace Local;
interface Traversable { public function local(): void; }
enum Signal implements Traversable { public function local(): void {} }
echo "ok";
"#,
    ];

    for source in sources {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 0, "{stderr:?}");
        assert_eq!(stdout, "ok");
        assert!(stderr.is_empty(), "{stderr:?}");
    }
}

#[test]
fn traversable_precedes_ordinary_missing_methods_but_not_dependency_errors() {
    for source in [
        "<?php\ninterface Workable { public function work(): void; }\nenum Signal implements Workable, Traversable {}\n",
        "<?php\ninterface Workable { public function work(): void; }\nenum Signal implements Traversable, Workable {}\n",
    ] {
        assert_traversable_fatal(source, "Signal");
    }

    for interfaces in [
        "MissingContract, Traversable",
        "Traversable, MissingContract",
    ] {
        let source = format!(
            "<?php\necho \"before\\n\";\ntry {{ enum Signal implements {interfaces} {{}} }}\ncatch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), \"\\n\"; }}\necho class_exists('Signal', false) ? \"visible\\n\" : \"hidden\\n\";\n"
        );
        let (status, stdout, stderr) = run_stdin(&source);
        assert_eq!(status, 0, "{stderr:?}");
        assert_eq!(
            stdout,
            "before\nError:Interface \"MissingContract\" not found\nhidden\n"
        );
        assert!(stderr.is_empty(), "{stderr:?}");
    }
}

#[test]
fn composition_shape_and_later_syntax_keep_priority() {
    let exact = [
        (
            "<?php\ntrait LeftWork { public function work(): void {} }\ntrait RightWork { public function work(): void {} }\nenum Signal implements Traversable { use LeftWork, RightWork; }\n",
            "Fatal error: Trait method RightWork::work has not been applied as Signal::work, because of collision with LeftWork::work in Standard input code on line 4\n",
        ),
        (
            "<?php\ntrait Work { public function work(): void {} }\nenum Signal implements Traversable { use Work { missing as renamed; } }\n",
            "Fatal error: An alias (renamed) was defined for method missing(), but this method does not exist in Standard input code on line 3\n",
        ),
        (
            "<?php\nenum Signal: int implements Traversable { case Ready; }\n",
            "Fatal error: Case Ready of backed enum Signal must have a value in Standard input code on line 2\n",
        ),
        (
            "<?php\nenum Signal implements Traversable { abstract public function local(): void; }\n",
            "Fatal error: Enum method Signal::local() must not be abstract in Standard input code on line 2\n",
        ),
        (
            "<?php\nenum Signal implements Traversable {}\nisset(, $value);\n",
            "Parse error: syntax error, unexpected token \",\" in Standard input code on line 3\n",
        ),
    ];

    for (source, expected) in exact {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{stderr:?}");
        assert!(stdout.is_empty(), "{stdout:?}");
        assert_eq!(stderr, expected);
    }
}

#[test]
fn reached_and_unreachable_declarations_preserve_output_shutdown_and_visibility() {
    let source = r#"<?php
register_shutdown_function(function () { echo class_exists('Signal', false) ? "visible\n" : "hidden\n"; });
echo "before\n";
try { enum Signal implements Traversable {} }
catch (Throwable $error) { echo "caught\n"; }
echo "after\n";
"#;
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nhidden\n");
    assert_eq!(
        stderr,
        "Fatal error: Enum Signal must implement interface Traversable as part of either Iterator or IteratorAggregate in Unknown on line 0\n"
    );

    let source = r#"<?php
if (false) { enum ColdSignal implements Traversable {} }
function skipSignal(): void { return; enum ReturnedSignal implements Traversable {} }
skipSignal();
echo class_exists('ColdSignal', false) ? 'bad' : 'cold-hidden', '|';
echo class_exists('ReturnedSignal', false) ? 'bad' : 'returned-hidden';
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
            "rphp_enum_traversable_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let include = directory.join("traversable.php");
        std::fs::write(
            &include,
            "<?php\nnamespace Included;\nenum Signal implements \\Traversable {}\n",
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
fn include_and_eval_keep_unknown_location_and_are_uncatchable() {
    let fixture = IncludeFixture::new();
    let source = format!(
        "<?php\nregister_shutdown_function(function () {{ echo \"shutdown\\n\"; }});\necho \"before\\n\";\ntry {{ include {:?}; }} catch (Throwable $error) {{ echo \"caught\\n\"; }}\necho \"after\\n\";\n",
        fixture.include.to_string_lossy()
    );
    let (status, stdout, stderr) = run_stdin(&source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        "Fatal error: Enum Included\\Signal must implement interface Traversable as part of either Iterator or IteratorAggregate in Unknown on line 0\n"
    );

    let source = r#"<?php
register_shutdown_function(function () { echo "shutdown\n"; });
echo "before\n";
try { eval('enum EvaluatedSignal implements Traversable {}'); }
catch (Throwable $error) { echo "caught\n"; }
echo "after\n";
"#;
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        "Fatal error: Enum EvaluatedSignal must implement interface Traversable as part of either Iterator or IteratorAggregate in Unknown on line 0\n"
    );
}
