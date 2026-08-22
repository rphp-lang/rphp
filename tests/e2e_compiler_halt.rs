use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

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
    let output = child.wait_with_output().expect("rphp should finish");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

#[test]
fn direct_absolute_dynamic_and_function_offsets_share_the_exact_source_byte() {
    let source = r#"<?php
function capturedOffset() { return __COMPILER_HALT_OFFSET__; }
const SNAPSHOT = __COMPILER_HALT_OFFSET__;
echo __COMPILER_HALT_OFFSET__, ':', \__COMPILER_HALT_OFFSET__, ':', SNAPSHOT, ':', capturedOffset(), ':', constant('__COMPILER_HALT_OFFSET__'), "\n";
__HaLt_CoMpIlEr /* trivia */ (// newline
) ; payload that is deliberately not PHP {{{
"#;
    let offset = source.find("; payload").unwrap() + 1;
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        format!("{offset}:{offset}:{offset}:{offset}:{offset}\n")
    );
    assert_eq!(stderr, "");
}

#[test]
fn eval_uses_the_eval_string_offset_and_returns_to_its_caller() {
    let evaluated = r#"echo __COMPILER_HALT_OFFSET__, ':', constant('__COMPILER_HALT_OFFSET__'), "\n"; __halt_compiler(); ignored {{{"#;
    let offset = evaluated.find("; ignored").unwrap() + 1;
    let source = format!("<?php eval({evaluated:?}); echo \"after\\n\";");
    let (status, stdout, stderr) = run_stdin(&source);

    assert_eq!(status, 0);
    assert_eq!(stdout, format!("{offset}:{offset}\nafter\n"));
    assert_eq!(stderr, "");
}

#[test]
fn dynamic_offset_uses_the_first_eval_unit_with_the_same_zend_source_name() {
    let body = r#"return function () { return constant('__COMPILER_HALT_OFFSET__'); }; __halt_compiler(); opaque"#;
    let first_eval = format!("/*a*/{body}");
    let second_eval = format!("/*longer prefix*/{body}");
    let first_offset = first_eval.find("; opaque").unwrap() + 1;
    let source = format!(
        "<?php function makeUnit($source) {{ return eval($source); }} $first = makeUnit({first_eval:?}); echo $first(), ':'; $second = makeUnit({second_eval:?}); echo $second(), ':', $first();",
    );
    let (status, stdout, stderr) = run_stdin(&source);

    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        format!("{first_offset}:{first_offset}:{first_offset}")
    );
    assert_eq!(stderr, "");
}

#[test]
fn nested_halt_compiler_is_a_compile_fatal_even_in_dead_code() {
    let source = "<?php if (false) { __halt_compiler(); ignored {{{";
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("Fatal error: __HALT_COMPILER() can only be used from the outermost scope")
    );
}

#[test]
fn repeated_include_keeps_each_source_units_offset_private() {
    let id = TEMP_ID.fetch_add(1, Ordering::SeqCst);
    let directory =
        std::env::temp_dir().join(format!("rphp_halt_compiler_{}_{}", std::process::id(), id));
    std::fs::create_dir_all(&directory).unwrap();
    let included = directory.join("payload.php");
    let included_source = "<?php echo __COMPILER_HALT_OFFSET__, ':', constant('__COMPILER_HALT_OFFSET__'), \"\\n\"; __halt_compiler(); opaque bytes";
    std::fs::write(&included, included_source).unwrap();
    let offset = included_source.find("; opaque").unwrap() + 1;
    let driver = format!(
        "<?php include {0:?}; include {0:?};",
        included.to_string_lossy()
    );
    let outcome = run_stdin(&driver);
    let _ = std::fs::remove_dir_all(&directory);

    assert_eq!(outcome.0, 0);
    assert_eq!(outcome.1, format!("{offset}:{offset}\n{offset}:{offset}\n"));
    assert_eq!(outcome.2, "");
}

#[test]
fn reserved_global_name_stays_undefined_while_namespaced_name_is_ordinary() {
    let source = r#"<?php
namespace {
    set_error_handler(function($level, $message) { echo "$level:$message\n"; });
    var_dump(define('__COMPILER_HALT_OFFSET__', 9));
}
namespace LocalScope {
    const __COMPILER_HALT_OFFSET__ = 7;
    echo __COMPILER_HALT_OFFSET__, ':', constant('LocalScope\\__COMPILER_HALT_OFFSET__');
}
"#;
    let (status, stdout, stderr) = run_stdin(source);

    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        concat!(
            "2:Constant __COMPILER_HALT_OFFSET__ already defined, this will be an error in PHP 9\n",
            "bool(false)\n",
            "7:7",
        )
    );
    assert_eq!(stderr, "");
}
