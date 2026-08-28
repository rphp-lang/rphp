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
            "rphp_unterminated_comment_location_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let include = directory.join("included.php");
        std::fs::write(&include, b"<?php\n/* included\nbody\n").unwrap();
        let driver = directory.join("driver.php");
        let source = format!(
            r#"<?php
try {{
    include {include:?};
}} catch (ParseError $error) {{
    echo 'include:', $error->getMessage(), ':', basename($error->getFile()), ':', $error->getLine(), "\n";
}}

try {{
    eval("echo 'never';\n/* evaluated\nbody");
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
fn direct_block_and_doc_comment_errors_retain_the_start_line() {
    let cases: &[(&str, &[u8], usize)] = &[
        ("ordinary-lf", b"<?php\n/* Foo\nBar", 2),
        ("doc-lf", b"<?php\n/** documentation\n * body", 2),
        ("after-code", b"<?php echo 'never'; /* inline", 1),
        ("trailing-newlines", b"<?php\n\n/* trailing\nbody\n\n", 3),
        ("crlf", b"<?php\r\n/* crlf\r\nbody", 2),
        ("cr", b"<?php\r/* cr\rbody", 2),
        (
            "reopened-segment",
            b"<?php echo 'first'; ?>\ninline\n<?php echo 'second';\n/* reopened\nbody",
            4,
        ),
        (
            "halt-compiler-trivia",
            b"<?php\n__halt_compiler /* unfinished\nbody",
            2,
        ),
    ];

    for (label, source, line) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "{label}: stderr={stderr:?}");
        assert!(stdout.is_empty(), "{label}: stdout={stdout:?}");
        assert_eq!(
            stderr,
            format!(
                "Parse error: Unterminated comment starting line {line} in Standard input code on line {line}\n"
            ),
            "{label}"
        );
    }
}

#[test]
fn valid_comments_strings_and_reopened_segments_remain_unchanged() {
    let source = br#"<?php
class Documented {
    /** retained documentation */
    public const VALUE = 42;
}
/* ordinary block */
// /* line comment
# /** hash comment
$single = '/* single';
$double = "/** double";
$nowdoc = <<<'TEXT'
/* nowdoc
TEXT;
$heredoc = <<<TEXT
/** heredoc
TEXT;
echo (new ReflectionClassConstant(Documented::class, 'VALUE'))->getDocComment(), '|';
echo $single, '|', $double, '|', $nowdoc, '|', $heredoc;
?>|inline|<?php echo '|reopened';
"#;

    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 0, "stderr={stderr:?}");
    assert_eq!(
        stdout,
        b"/** retained documentation */|/* single|/** double|/* nowdoc|/** heredoc|inline||reopened"
    );
    assert!(stderr.is_empty());
}

#[test]
fn include_and_eval_errors_preserve_their_own_source_metadata() {
    let fixture = SourceFixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .arg(&fixture.driver)
        .output()
        .unwrap();
    let driver = std::fs::canonicalize(&fixture.driver).unwrap();
    let expected = format!(
        concat!(
            "include:Unterminated comment starting line 2:included.php:2\n",
            "eval:Unterminated comment starting line 2:{}(9) : eval()'d code:2\n",
        ),
        driver.to_string_lossy(),
    );

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    assert!(output.stderr.is_empty());
    assert!(fixture.include.exists());
}
