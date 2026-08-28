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

fn assert_conflict(source: &str, message: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255, "{stderr:?}");
    assert!(stdout.is_empty(), "{stdout:?}");
    assert_eq!(
        stderr,
        format!("Fatal error: {message} in Standard input code on line {line}\n")
    );
}

#[test]
fn unit_backed_namespaced_and_nested_traits_report_canonical_conflicts() {
    let cases = [
        (
            "<?php\ntrait UnitProvider { public const Ready = 1; }\nenum UnitSignal { use UnitProvider; case Ready; }\n",
            "Cannot use trait UnitProvider, because UnitProvider::Ready conflicts with enum case UnitSignal::Ready",
            3,
        ),
        (
            "<?php\ntrait BackedProvider { private const Live = 9; }\nenum BackedSignal: int { use BackedProvider; case Live = 1; }\n",
            "Cannot use trait BackedProvider, because BackedProvider::Live conflicts with enum case BackedSignal::Live",
            3,
        ),
        (
            "<?php\nnamespace Oracle;\ntrait Provider { public const Up = 1; }\nenum Direction { use Provider; case Up; }\n",
            "Cannot use trait Oracle\\Provider, because Oracle\\Provider::Up conflicts with enum case Oracle\\Direction::Up",
            4,
        ),
        (
            "<?php\ntrait InnerProvider { public const Busy = 1; }\ntrait OuterProvider { use InnerProvider; }\nenum NestedSignal { use OuterProvider; case Busy; }\n",
            "Cannot use trait OuterProvider, because OuterProvider::Busy conflicts with enum case NestedSignal::Busy",
            4,
        ),
    ];

    for (source, message, line) in cases {
        assert_conflict(source, message, line);
    }
}

#[test]
fn conflict_selection_follows_use_and_trait_constant_order() {
    assert_conflict(
        "<?php\ntrait FirstProvider { public const First = 1; }\ntrait SecondProvider { public const Second = 2; }\nenum Signal { use SecondProvider, FirstProvider; case First; case Second; }\n",
        "Cannot use trait SecondProvider, because SecondProvider::Second conflicts with enum case Signal::Second",
        4,
    );
    assert_conflict(
        "<?php\ntrait OrderedProvider { public const Later = 2; public const Earlier = 1; }\nenum Signal { use OrderedProvider; case Earlier; case Later; }\n",
        "Cannot use trait OrderedProvider, because OrderedProvider::Later conflicts with enum case Signal::Later",
        3,
    );
}

#[test]
fn enum_shape_direct_symbols_and_trait_method_errors_keep_priority() {
    let exact = [
        (
            "<?php\ntrait Provider { public const Missing = 1; }\nenum Signal: int { use Provider; case Missing; }\n",
            "Fatal error: Case Missing of backed enum Signal must have a value in Standard input code on line 3\n",
        ),
        (
            "<?php\nenum Signal {\n    case Busy;\n    case Busy;\n}\n",
            "Fatal error: Cannot redefine class constant Signal::Busy in Standard input code on line 4\n",
        ),
        (
            "<?php\nenum Signal {\n    public const Busy = 1;\n    case Busy;\n}\n",
            "Fatal error: Cannot redefine class constant Signal::Busy in Standard input code on line 4\n",
        ),
        (
            "<?php\ntrait Provider { public const Busy = 1; }\nenum Signal { use Provider; case Busy; public function invalid(): void { break; } }\n",
            "Fatal error: 'break' not in the 'loop' or 'switch' context in Standard input code on line 3\n",
        ),
    ];
    for (source, expected) in exact {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{stderr:?}");
        assert!(stdout.is_empty(), "{stdout:?}");
        assert_eq!(stderr, expected);
    }

    for source in [
        "<?php\ntrait Left { public function work() {} }\ntrait Right { public const Busy = 1; public function work() {} }\nenum Signal { use Left, Right; case Busy; }\n",
        "<?php\ntrait Provider { public const Busy = 1; public function work() {} }\nenum Signal { use Provider { missing as renamed; } case Busy; }\n",
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{stderr:?}");
        assert!(stdout.is_empty(), "{stdout:?}");
        assert!(!stderr.contains("conflicts with enum case"), "{stderr:?}");
    }

    let (status, stdout, stderr) = run_stdin(
        "<?php\ntrait Provider { public const Busy = 1; }\nenum Signal { use Provider; case Busy; }\nisset(, $value);\n",
    );
    assert_eq!(status, 255);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected token \",\" in Standard input code on line 4\n"
    );
}

#[test]
fn constant_conflict_precedes_trait_property_magic_and_magic_alias_errors() {
    for source in [
        "<?php\ntrait Provider { public $state; public const Busy = 1; }\nenum Signal { use Provider; case Busy; }\n",
        "<?php\ntrait Provider { public const Busy = 1; public function __construct() {} }\nenum Signal { use Provider; case Busy; }\n",
        "<?php\ntrait Provider { public const Busy = 1; public function work() {} }\nenum Signal { use Provider { work as __construct; } case Busy; }\n",
    ] {
        assert_conflict(
            source,
            "Cannot use trait Provider, because Provider::Busy conflicts with enum case Signal::Busy",
            3,
        );
    }
}

#[test]
fn valid_case_sensitive_and_unreached_compositions_remain_executable() {
    let source = r#"<?php
trait Tools {
    public const ready = 7;
    public const Prefix = 'ok';
    public function label(): string { return self::Prefix . ':' . $this->name; }
}
enum LiveSignal { use Tools; case Ready; }
if (false) { enum ColdSignal { use Tools; case Prefix; } }
function skipSignal(): void {
    return;
    enum ReturnedSignal { use Tools; case Prefix; }
}
echo LiveSignal::ready, '|', LiveSignal::Ready->label();
"#;
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(stdout, "7|ok:Ready");
    assert!(stderr.is_empty(), "{stderr:?}");
}

#[test]
fn reached_conflict_is_uncatchable_and_leaves_no_partial_enum() {
    let source = r#"<?php
trait Harmless { public function label(): string { return 'label'; } }
trait Conflict { public const Busy = 1; }
register_shutdown_function(function () {
    echo class_exists('Signal', false) ? "visible\n" : "hidden\n";
});
echo "before\n";
try { enum Signal { use Harmless, Conflict; case Busy; } }
catch (Throwable $error) { echo "caught\n"; }
echo "after\n";
"#;
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nhidden\n");
    assert_eq!(
        stderr,
        "Fatal error: Cannot use trait Conflict, because Conflict::Busy conflicts with enum case Signal::Busy in Standard input code on line 8\n"
    );
}

struct IncludeFixture {
    directory: std::path::PathBuf,
    include: std::path::PathBuf,
}

impl IncludeFixture {
    fn new() -> Self {
        let identity = FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let directory = std::env::temp_dir().join(format!(
            "rphp_enum_trait_case_conflict_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let include = directory.join("conflict.php");
        std::fs::write(
            &include,
            "<?php\ntrait IncludedProvider { public const Busy = 1; }\nenum IncludedSignal { use IncludedProvider; case Busy; }\n",
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
fn include_and_eval_conflicts_preserve_source_output_and_shutdown_state() {
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
            "Fatal error: Cannot use trait IncludedProvider, because IncludedProvider::Busy conflicts with enum case IncludedSignal::Busy in {} on line 3\n",
            include.to_string_lossy()
        )
    );

    let source = "<?php\ntrait EvaluatedProvider { public const Busy = 1; }\nregister_shutdown_function(function () { echo \"shutdown\\n\"; });\necho \"before\\n\";\ntry { eval('enum EvaluatedSignal { use EvaluatedProvider; case Busy; }'); } catch (Throwable $error) { echo \"caught\\n\"; }\necho \"after\\n\";\n";
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        "Fatal error: Cannot use trait EvaluatedProvider, because EvaluatedProvider::Busy conflicts with enum case EvaluatedSignal::Busy in Standard input code(5) : eval()'d code on line 1\n"
    );
}
