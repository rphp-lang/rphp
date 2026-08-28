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

#[test]
fn reached_conflict_uses_engine_order_qualified_name_and_declaration_line() {
    let source = "<?php\nnamespace Oracle;\necho \"before\\n\";\nenum Signal: int {\n    case Ready = 1;\n    public static function TrYfRoM(int $value): ?self { return null; }\n    public static function CaSeS(): array { return []; }\n    public static function FrOm(int $value): self { return self::Ready; }\n}\n";
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 255);
    assert_eq!(stdout, "before\n");
    assert_eq!(
        stderr,
        "Fatal error: Cannot redeclare Oracle\\Signal::cases() in Standard input code on line 4\n"
    );
}

#[test]
fn declaration_shape_and_abstract_errors_precede_the_synthesized_conflict() {
    let cases = [
        (
            "backing",
            "<?php\nenum Signal: int {\n    case Ready;\n    public static function cases(): array { return []; }\n}\n",
            "Fatal error: Case Ready of backed enum Signal must have a value in Standard input code on line 3\n",
        ),
        (
            "property",
            "<?php\nenum Signal {\n    public int $value;\n    public static function cases(): array { return []; }\n}\n",
            "Fatal error: Enum Signal cannot include properties in Standard input code on line 3\n",
        ),
        (
            "abstract",
            "<?php\nenum Signal {\n    abstract public function pending();\n    public static function cases(): array { return []; }\n}\n",
            "Fatal error: Enum method Signal::pending() must not be abstract in Standard input code on line 3\n",
        ),
    ];

    for (label, source, expected) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{label}: {stderr:?}");
        assert!(stdout.is_empty(), "{label}: {stdout:?}");
        assert_eq!(stderr, expected, "{label}");
    }
}

#[test]
fn synthesized_conflict_precedes_magic_and_interface_link_checks() {
    let cases = [
        (
            "magic",
            "<?php\nenum Signal {\n    public function __construct() {}\n    public static function cases(): array { return []; }\n}\n",
        ),
        (
            "interface",
            "<?php\nenum Signal implements UnitEnum {\n    public static function cases(): array { return []; }\n}\n",
        ),
    ];

    for (label, source) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{label}: {stderr:?}");
        assert!(stdout.is_empty(), "{label}: {stdout:?}");
        assert_eq!(
            stderr,
            "Fatal error: Cannot redeclare Signal::cases() in Standard input code on line 2\n",
            "{label}"
        );
    }
}

#[test]
fn global_body_and_later_syntax_errors_still_win() {
    let cases = [
        (
            "body",
            "<?php\nenum Signal {\n    public static function cases(): array {\n        break;\n    }\n}\n",
            "Fatal error: 'break' not in the 'loop' or 'switch' context in Standard input code on line 4\n",
        ),
        (
            "syntax",
            "<?php\necho 'never';\nenum Signal {\n    public static function cases(): array { return []; }\n}\nisset(, $value);\n",
            "Parse error: syntax error, unexpected token \",\" in Standard input code on line 6\n",
        ),
    ];

    for (label, source, expected) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{label}: {stderr:?}");
        assert!(stdout.is_empty(), "{label}: {stdout:?}");
        assert_eq!(stderr, expected, "{label}");
    }
}

#[test]
fn unreachable_conflicts_and_valid_unit_or_trait_methods_remain_executable() {
    let source = r#"<?php
if (false) {
    enum Hidden { public static function cases(): array { return []; } }
}
function loadHidden(): void {
    return;
    enum Later: int { case Ready = 1; public static function from(int $value): self { return self::Ready; } }
}
enum Plain {
    case Ready;
    public static function from(int $value): self { return self::Ready; }
    public static function tryFrom(int $value): ?self { return null; }
}
trait ShadowCases { public static function cases(): array { return []; } }
enum Composed { use ShadowCases; case Ready; }
echo Plain::from(1)->name, '|';
var_dump(Plain::tryFrom(1));
echo Composed::cases()[0]->name, "\n";
"#;
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(stdout, "Ready|NULL\nReady\n");
    assert!(stderr.is_empty(), "{stderr:?}");
}

struct IncludeFixture {
    directory: std::path::PathBuf,
    include: std::path::PathBuf,
    driver: std::path::PathBuf,
}

impl IncludeFixture {
    fn new() -> Self {
        let identity = FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let directory = std::env::temp_dir().join(format!(
            "rphp_enum_synthesized_method_conflict_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let include = directory.join("included.php");
        std::fs::write(
            &include,
            "<?php\nnamespace Included;\nenum Signal {\n    public static function CaSeS(): array { return []; }\n}\n",
        )
        .unwrap();
        let driver = directory.join("driver.php");
        let source = format!(
            r#"<?php
register_shutdown_function(function () {{ echo "shutdown\n"; }});
echo "before\n";
try {{ include {include:?}; }} catch (Throwable $error) {{ echo "caught\n"; }}
echo "after\n";
"#,
            include = include.to_string_lossy(),
        );
        std::fs::write(&driver, source).unwrap();
        Self {
            directory,
            include,
            driver,
        }
    }
}

impl Drop for IncludeFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn include_conflict_is_uncatchable_and_preserves_prior_state() {
    let fixture = IncludeFixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
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
        ])
        .arg(&fixture.driver)
        .output()
        .expect("rphp should run include fixture");
    let include = std::fs::canonicalize(&fixture.include).unwrap();

    assert_eq!(output.status.code(), Some(255));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "before\nshutdown\n"
    );
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "Fatal error: Cannot redeclare Included\\Signal::cases() in {} on line 3\n",
            include.to_string_lossy()
        )
    );
}

#[test]
fn eval_conflict_is_uncatchable_and_preserves_prior_state() {
    let source = "<?php\nregister_shutdown_function(function () { echo \"shutdown\\n\"; });\necho \"before\\n\";\ntry {\n    eval(\"namespace Evaluated; enum Signal: int { case Ready = 1; public static function FrOm(int \\$value): self { return self::Ready; } }\");\n} catch (Throwable $error) {\n    echo \"caught\\n\";\n}\necho \"after\\n\";\n";
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 255);
    assert_eq!(stdout, "before\nshutdown\n");
    assert_eq!(
        stderr,
        "Fatal error: Cannot redeclare Evaluated\\Signal::from() in Standard input code(5) : eval()'d code on line 1\n"
    );
}
