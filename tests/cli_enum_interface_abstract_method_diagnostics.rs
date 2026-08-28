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
fn unit_backed_namespaced_plural_and_inherited_requirements_are_canonical() {
    let cases = [
        (
            "<?php\ninterface Workable { public function work(): void; }\nenum Signal implements Workable {}\n",
            "Enum Signal must implement 1 abstract method (Workable::work)",
            3,
        ),
        (
            "<?php\nnamespace Oracle;\ninterface Workable { public function work(): void; }\nenum Signal: int implements Workable { case Ready = 1; }\n",
            "Enum Oracle\\Signal must implement 1 abstract method (Oracle\\Workable::work)",
            4,
        ),
        (
            "<?php\ninterface Alpha { public function alpha(): void; }\ninterface Zulu { public function zulu(): void; }\nenum Signal implements Zulu, Alpha {}\n",
            "Enum Signal must implement 2 abstract methods (Zulu::zulu, Alpha::alpha)",
            4,
        ),
        (
            "<?php\ninterface ParentContract { public function inherited(): void; }\ninterface ChildContract extends ParentContract { public function direct(): void; }\nenum Signal implements ChildContract {}\n",
            "Enum Signal must implement 2 abstract methods (ChildContract::direct, ParentContract::inherited)",
            4,
        ),
    ];

    for (source, message, line) in cases {
        assert_fatal(source, message, line);
    }
}

#[test]
fn duplicate_requirements_keep_first_owner_and_collapse_diamonds() {
    assert_fatal(
        "<?php\ninterface RootContract { public function shared(): void; }\ninterface LeftContract extends RootContract {}\ninterface RightContract extends RootContract {}\nenum Signal implements LeftContract, RightContract {}\n",
        "Enum Signal must implement 1 abstract method (RootContract::shared)",
        5,
    );
    assert_fatal(
        "<?php\ninterface LeftContract { public function shared(): void; }\ninterface RightContract { public function shared(): void; }\nenum Signal implements RightContract, LeftContract {}\n",
        "Enum Signal must implement 1 abstract method (RightContract::shared)",
        4,
    );
    assert_fatal(
        "<?php\ninterface ParentContract { public function work(): void; }\ninterface ChildContract extends ParentContract { public function work(): void; }\nenum Signal implements ChildContract {}\n",
        "Enum Signal must implement 1 abstract method (ChildContract::work)",
        4,
    );
}

#[test]
fn direct_trait_and_case_insensitive_implementations_remain_valid_and_reached() {
    let source = r#"<?php
interface DirectWork { public function work(): string; }
interface TraitWork { public function label(): string; }
trait LabelImplementation { public function label(): string { return $this->name; } }
echo enum_exists('DirectSignal', false) ? 'visible' : 'hidden', '|';
enum DirectSignal implements DirectWork {
    case Ready;
    public function wOrK(): string { return $this->name; }
}
enum TraitSignal implements TraitWork { use LabelImplementation; case Busy; }
echo DirectSignal::Ready->work(), '|', TraitSignal::Busy->label();
"#;
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(stdout, "hidden|Ready|Busy");
    assert!(stderr.is_empty(), "{stderr:?}");
}

#[test]
fn abstract_trait_requirements_use_enum_owner_and_alias_order() {
    assert_fatal(
        "<?php\ntrait WorkRequirement { abstract public function work(): void; }\nenum Signal { use WorkRequirement; case Ready; }\n",
        "Enum Signal must implement 1 abstract method (Signal::work)",
        3,
    );
    assert_fatal(
        "<?php\ntrait InnerRequirement { abstract public function work(): void; }\ntrait OuterRequirement { use InnerRequirement; }\nenum Signal { use OuterRequirement; case Ready; }\n",
        "Enum Signal must implement 1 abstract method (Signal::work)",
        4,
    );
    assert_fatal(
        "<?php\ntrait WorkRequirement { abstract public function work(): void; }\nenum Signal { use WorkRequirement { work as renamed; } case Ready; }\n",
        "Enum Signal must implement 2 abstract methods (Signal::renamed, Signal::work)",
        3,
    );
}

#[test]
fn signature_shape_composition_and_later_syntax_keep_priority() {
    let exact = [
        (
            "<?php\ninterface MissingContract { function pending(): void; }\ninterface TypedContract { function typed(): int; }\nenum Signal implements MissingContract, TypedContract { case Ready; function typed(): string { return ''; } }\n",
            "Fatal error: Declaration of Signal::typed(): string must be compatible with TypedContract::typed(): int in Standard input code on line 4\n",
        ),
        (
            "<?php\ninterface Workable { function pending(): void; }\ntrait LeftWork { function work(): void {} }\ntrait RightWork { function work(): void {} }\nenum Signal implements Workable { use LeftWork, RightWork; case Ready; }\n",
            "Fatal error: Trait method RightWork::work has not been applied as Signal::work, because of collision with LeftWork::work in Standard input code on line 5\n",
        ),
        (
            "<?php\ninterface Workable { function pending(): void; }\ntrait WorkImplementation { function work(): void {} }\nenum Signal implements Workable { use WorkImplementation { missing as renamed; } case Ready; }\n",
            "Fatal error: An alias (renamed) was defined for method missing(), but this method does not exist in Standard input code on line 4\n",
        ),
        (
            "<?php\ninterface Workable { function pending(): void; }\ntrait SignalConstants { const Ready = 1; }\nenum Signal implements Workable { use SignalConstants; case Ready; }\n",
            "Fatal error: Cannot use trait SignalConstants, because SignalConstants::Ready conflicts with enum case Signal::Ready in Standard input code on line 4\n",
        ),
        (
            "<?php\ninterface Workable { function pending(): void; }\nenum Signal: int implements Workable { case Ready; }\n",
            "Fatal error: Case Ready of backed enum Signal must have a value in Standard input code on line 3\n",
        ),
        (
            "<?php\ninterface Workable { function pending(): void; }\nenum Signal implements Workable { abstract function local(): void; case Ready; }\n",
            "Fatal error: Enum method Signal::local() must not be abstract in Standard input code on line 3\n",
        ),
        (
            "<?php\ninterface Workable { function pending(): void; }\nenum Signal implements Workable {}\nisset(, $value);\n",
            "Parse error: syntax error, unexpected token \",\" in Standard input code on line 4\n",
        ),
    ];

    for (source, expected) in exact {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{stderr:?}");
        assert!(stdout.is_empty(), "{stdout:?}");
        assert_eq!(stderr, expected);
    }

    for source in [
        "<?php\ninterface Workable { function pending(): void; }\ntrait Stateful { public $state; }\nenum Signal implements Workable { use Stateful; case Ready; }\n",
        "<?php\ninterface Workable { function pending(): void; }\ntrait Lifecycle { function __construct() {} }\nenum Signal implements Workable { use Lifecycle; case Ready; }\n",
    ] {
        assert_fatal(
            source,
            "Enum Signal must implement 1 abstract method (Workable::pending)",
            4,
        );
    }
}

#[test]
fn missing_dependency_precedes_abstract_contract_and_rolls_back_catchably() {
    let source = r#"<?php
interface Workable { public function pending(): void; }
echo "before\n";
try { enum Signal implements Workable, MissingContract {} }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
echo class_exists('Signal', false) ? "visible\n" : "hidden\n";
echo "after\n";
"#;
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(
        stdout,
        "before\nError:Interface \"MissingContract\" not found\nhidden\nafter\n"
    );
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
            "rphp_enum_interface_abstract_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let include = directory.join("missing.php");
        std::fs::write(
            &include,
            "<?php\nnamespace Included;\ninterface Workable { public function pending(): void; }\nenum Signal implements Workable {}\n",
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
fn reachability_include_eval_output_shutdown_and_visibility_are_preserved() {
    let source = r#"<?php
interface Workable { public function pending(): void; }
if (false) { enum ColdSignal implements Workable {} }
function skipSignal(): void { return; enum ReturnedSignal implements Workable {} }
skipSignal();
echo class_exists('ColdSignal', false) ? 'bad' : 'cold-hidden', '|';
echo class_exists('ReturnedSignal', false) ? 'bad' : 'returned-hidden';
"#;
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(stdout, "cold-hidden|returned-hidden");
    assert!(stderr.is_empty(), "{stderr:?}");

    let source = r#"<?php
interface Workable { public function pending(): void; }
register_shutdown_function(function () { echo class_exists('Signal', false) ? "visible\n" : "hidden\n"; });
echo "before\n";
try { enum Signal implements Workable {} }
catch (Throwable $error) { echo "caught\n"; }
echo "after\n";
"#;
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nhidden\n");
    assert_eq!(
        stderr,
        "Fatal error: Enum Signal must implement 1 abstract method (Workable::pending) in Standard input code on line 5\n"
    );

    let fixture = IncludeFixture::new();
    let source = format!(
        "<?php\nregister_shutdown_function(function () {{ echo \"shutdown\\n\"; }});\necho \"before\\n\";\ntry {{ include {:?}; }} catch (Throwable $error) {{ echo \"caught\\n\"; }}\necho \"after\\n\";\n",
        fixture.include.to_string_lossy()
    );
    let (status, stdout, stderr) = run_stdin(&source);
    let include = std::fs::canonicalize(&fixture.include).unwrap();
    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        format!(
            "Fatal error: Enum Included\\Signal must implement 1 abstract method (Included\\Workable::pending) in {} on line 4\n",
            include.to_string_lossy()
        )
    );

    let source = "<?php\ninterface Workable { public function pending(): void; }\nregister_shutdown_function(function () { echo \"shutdown\\n\"; });\necho \"before\\n\";\ntry { eval('enum EvaluatedSignal implements Workable {}'); } catch (Throwable $error) { echo \"caught\\n\"; }\necho \"after\\n\";\n";
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        "Fatal error: Enum EvaluatedSignal must implement 1 abstract method (Workable::pending) in Standard input code(5) : eval()'d code on line 1\n"
    );
}
