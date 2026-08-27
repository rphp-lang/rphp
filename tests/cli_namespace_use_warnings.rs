use std::io::Write;
use std::process::{Command, Stdio};

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

fn warning(name: &str, line: usize) -> String {
    format!(
        "\nWarning: The use statement with non-compound name '{name}' has no effect in Standard input code on line {line}\n"
    )
}

#[test]
fn global_unaliased_class_function_and_const_imports_warn_at_the_first_name_line() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nuse\n    Plain,\n    \\Leading;\nuse function helper;\nuse const VALUE;\necho 'ok';\n",
    );

    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        format!(
            "{}{}{}{}ok",
            warning("Plain", 3),
            warning("Leading", 3),
            warning("helper", 5),
            warning("VALUE", 6),
        )
    );
    assert_eq!(stderr, "");
}

#[test]
fn explicit_aliases_compound_names_and_named_namespace_imports_do_not_warn() {
    for source in [
        "<?php\nuse Plain as Plain;\nuse function helper as helper;\nuse const VALUE as VALUE;\nuse Vendor\\Thing;\necho 'global';\n",
        "<?php\nnamespace Named {\n    use Local;\n    use function helper;\n    use const VALUE;\n    echo 'named';\n}\n",
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 0);
        assert!(matches!(stdout.as_str(), "global" | "named"));
        assert_eq!(stderr, "");
    }
}

#[test]
fn eval_compile_warning_enters_the_active_error_handler_and_execution_continues() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nfunction handle_use_warning($level, $message, $file, $line) {\n    echo $level, ':', $message, ':', $line, \"\\n\";\n    return true;\n}\nset_error_handler('handle_use_warning');\neval('namespace { use RuntimeName; echo \"eval\"; }');\necho '|done';\n",
    );

    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "2:The use statement with non-compound name 'RuntimeName' has no effect:1\neval|done"
    );
    assert_eq!(stderr, "");
}

#[test]
fn exception_from_eval_compile_warning_handler_aborts_eval_and_is_catchable() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nset_error_handler(function ($level, $message) {\n    echo $level, ':', $message, \"\\n\";\n    throw new Exception('stop');\n});\ntry {\n    eval('namespace { use RuntimeName; echo \"eval\"; }');\n} catch (Throwable $error) {\n    echo 'caught:', $error->getMessage();\n}\n",
    );

    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "2:The use statement with non-compound name 'RuntimeName' has no effect\ncaught:stop"
    );
    assert_eq!(stderr, "");
}
