use std::process::Command;

fn run(arguments: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(arguments)
        .output()
        .expect("rphp subprocess should start");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

#[test]
fn startup_exception_string_limit_controls_stored_and_live_trace_rendering() {
    let source = r#"
function captureLimit($value) {
    debug_print_backtrace();
    return new Exception();
}
$error = captureLimit('abcdefgh');
echo $error->getTraceAsString(), "\n", ini_get('zend.exception_string_param_max_len');
"#;
    let (status, stdout, stderr) =
        run(&["-d", "zend.exception_string_param_max_len=3", "-r", source]);
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        concat!(
            "#0 Command line code(6): captureLimit('abc...')\n",
            "#0 Command line code(6): captureLimit('abc...')\n",
            "#1 {main}\n",
            "3"
        )
    );
    assert_eq!(stderr, "");
}

#[test]
fn startup_exception_ignore_args_uses_type_only_match_diagnostics_and_empty_traces() {
    let source = r#"
function captureIgnored($value) { return new Exception(); }
$error = captureIgnored('hidden');
echo $error->getTraceAsString(), "\n";
try { match(7) {}; } catch (UnhandledMatchError $match) { echo $match->getMessage(); }
"#;
    let (status, stdout, stderr) = run(&["-dzend.exception_ignore_args=1", "-r", source]);
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        concat!(
            "#0 Command line code(3): captureIgnored()\n",
            "#1 {main}\n",
            "Unhandled match case of type int"
        )
    );
    assert_eq!(stderr, "");
}

#[test]
fn zero_exception_string_limit_reports_match_strings_by_type() {
    let (status, stdout, stderr) = run(&[
        "-dzend.exception_string_param_max_len=0",
        "-r",
        r#"try { match('hidden') {}; } catch (UnhandledMatchError $error) { echo $error->getMessage(); }"#,
    ]);
    assert_eq!(status, 0);
    assert_eq!(stdout, "Unhandled match case of type string");
    assert_eq!(stderr, "");
}

#[test]
fn startup_exception_ini_values_normalize_php_boolean_and_length_boundaries() {
    for (definition, source, expected) in [
        (
            "zend.exception_ignore_args=garbage",
            r#"try { match(7) {}; } catch (Throwable $error) { echo ini_get('zend.exception_ignore_args'), '|', $error->getMessage(); }"#,
            "garbage|Unhandled match case 7",
        ),
        (
            "zend.exception_ignore_args=-1",
            r#"try { match(7) {}; } catch (Throwable $error) { echo ini_get('zend.exception_ignore_args'), '|', $error->getMessage(); }"#,
            "-1|Unhandled match case of type int",
        ),
        (
            "zend.exception_string_param_max_len=-1",
            r#"try { match('abcdefghijklmnop') {}; } catch (Throwable $error) { echo ini_get('zend.exception_string_param_max_len'), '|', $error->getMessage(); }"#,
            "15|Unhandled match case 'abcdefghijklmno...'",
        ),
        (
            "zend.exception_string_param_max_len=garbage",
            r#"try { match('hidden') {}; } catch (Throwable $error) { echo ini_get('zend.exception_string_param_max_len'), '|', $error->getMessage(); }"#,
            "garbage|Unhandled match case of type string",
        ),
    ] {
        let (status, stdout, stderr) = run(&["-d", definition, "-r", source]);
        assert_eq!(status, 0, "{definition}");
        assert_eq!(stdout, expected, "{definition}");
        assert_eq!(stderr, "", "{definition}");
    }
}
