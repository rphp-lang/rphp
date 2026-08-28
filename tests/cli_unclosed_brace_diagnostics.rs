use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn run_stdin(source: &[u8]) -> (i32, Vec<u8>, String) {
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
        .write_all(source)
        .expect("source should be written");
    let output = child.wait_with_output().expect("rphp should finish");
    (
        output.status.code().expect("rphp should exit normally"),
        output.stdout,
        String::from_utf8(output.stderr).expect("diagnostic should be UTF-8"),
    )
}

struct SourceFixture {
    directory: std::path::PathBuf,
    include: std::path::PathBuf,
    driver: std::path::PathBuf,
}

impl SourceFixture {
    fn new() -> Self {
        let identity = FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let directory = std::env::temp_dir().join(format!(
            "rphp_unclosed_brace_diagnostics_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let include = directory.join("included.php");
        std::fs::write(
            &include,
            b"<?php\n/** retained documentation */\nfunction included() {\n",
        )
        .unwrap();
        let driver = directory.join("driver.php");
        let source = format!(
            r#"<?php
try {{
    include {include:?};
}} catch (ParseError $error) {{
    echo 'include:', $error->getMessage(), ':', basename($error->getFile()), ':', $error->getLine(), "\n";
}}

try {{
    eval("function evaluated() {{\n");
}} catch (ParseError $error) {{
    echo 'eval:', $error->getMessage(), ':', $error->getFile(), ':', $error->getLine(), "\n";
}}
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

impl Drop for SourceFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn direct_eof_reports_the_opener_and_terminal_lines() {
    let cases: &[(&str, &[u8], &str)] = &[
        (
            "same-line",
            b"<?php {",
            "Parse error: Unclosed '{' in Standard input code on line 1\n",
        ),
        (
            "crlf-terminal",
            b"<?php\r\nif (true) {\r\n",
            "Parse error: Unclosed '{' on line 2 in Standard input code on line 3\n",
        ),
        (
            "cr-opener-and-terminal",
            b"<?php\rif (true) {\r",
            "Parse error: Unclosed '{' on line 2 in Standard input code on line 3\n",
        ),
        (
            "nested-innermost",
            b"<?php\nif (true) {\nfunction nested() {",
            "Parse error: Unclosed '{' in Standard input code on line 3\n",
        ),
        (
            "doc-comment",
            b"<?php\n/** retained documentation */\nfunction documented() {",
            "Parse error: Unclosed '{' in Standard input code on line 3\n",
        ),
    ];

    for (label, source, expected) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{label}: stderr={stderr:?}");
        assert!(stdout.is_empty(), "{label}: stdout={stdout:?}");
        assert_eq!(stderr, *expected, "{label}");
    }
}

#[test]
fn an_earlier_parse_error_keeps_priority_over_the_eof_brace() {
    let (status, stdout, stderr) = run_stdin(b"<?php\nisset(, $value);\n{");

    assert_eq!(status, 255);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected token \",\" in Standard input code on line 2\n"
    );
}

#[test]
fn parenthesis_and_bracket_eof_diagnostics_remain_out_of_scope() {
    for source in [b"<?php\n{\n(".as_slice(), b"<?php\n{\n[".as_slice()] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "Parse error: Expected expression, got Eof\n");
        assert!(!stderr.contains("Unclosed '{'"));
    }
}

#[test]
fn include_and_eval_errors_preserve_their_source_metadata() {
    let fixture = SourceFixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .arg(&fixture.driver)
        .output()
        .unwrap();
    let driver = std::fs::canonicalize(&fixture.driver).unwrap();
    let expected = format!(
        concat!(
            "include:Unclosed '{{' on line 3:included.php:4\n",
            "eval:Unclosed '{{' on line 1:{}(9) : eval()'d code:2\n",
        ),
        driver.to_string_lossy(),
    );

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    assert!(output.stderr.is_empty());
    assert!(fixture.include.exists());
}

#[test]
fn braced_namespace_halt_uses_the_directive_as_its_terminal_line() {
    let source = b"<?php\nnamespace Oracle {\n    __halt_compiler();\nopaque payload";
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 255);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        "Parse error: Unclosed '{' on line 2 in Standard input code on line 3\n"
    );
}

#[test]
fn valid_blocks_segments_metadata_and_halt_payload_remain_unchanged() {
    let valid_blocks = br#"<?php
/** class documentation */
class Box {
    /** retained constant documentation */
    public const VALUE = 42;
}
function answer() {
    $closure = function () { return Box::VALUE; };
    return $closure();
}
echo answer(), '|', (new ReflectionClassConstant(Box::class, 'VALUE'))->getDocComment();
"#;
    let (status, stdout, stderr) = run_stdin(valid_blocks);
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        b"42|/** retained constant documentation */".as_slice()
    );
    assert!(stderr.is_empty());

    let namespace = b"<?php\nnamespace Scoped { function value() { return 7; } echo value(); }";
    let (status, stdout, stderr) = run_stdin(namespace);
    assert_eq!((status, stdout, stderr), (0, b"7".to_vec(), String::new()));

    let segments = b"<?php { echo 'a'; } ?>b<?php { echo 'c'; }";
    let (status, stdout, stderr) = run_stdin(segments);
    assert_eq!(
        (status, stdout, stderr),
        (0, b"abc".to_vec(), String::new())
    );

    let halt = b"<?php echo __COMPILER_HALT_OFFSET__; __halt_compiler(); payload {";
    let expected_offset = halt
        .windows(b"; payload".len())
        .position(|window| window == b"; payload")
        .unwrap()
        + 1;
    let (status, stdout, stderr) = run_stdin(halt);
    assert_eq!(
        (status, stdout, stderr),
        (0, expected_offset.to_string().into_bytes(), String::new())
    );
}
