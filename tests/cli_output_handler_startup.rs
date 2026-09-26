//! Original PHP CLI output-handler startup and lifecycle contracts.

#[test]
fn caught_internal_final() {
    assert_cli(
        &["output_handler=strtoupper"],
        r###"set_exception_handler(function($e){echo 'handled';});echo 'body';"###,
        0,
        "bodyhandled",
        "",
    );
}

#[test]
fn caught_user_final() {
    assert_cli(
        &[],
        r###"set_exception_handler(function($e){echo 'handled';});ob_start(function($s){throw new Exception('stop');});echo 'body';"###,
        0,
        "bodyhandled",
        "",
    );
}

#[test]
fn caught_nested_final() {
    assert_cli(
        &[],
        r###"set_exception_handler(function($e){echo 'handler-level=',ob_get_level();});ob_start(function($s){return '['.$s.']';});echo 'outer';ob_start(function($s){throw new Exception('stop');});echo 'inner';"###,
        0,
        "[outerinnerhandler-level=2]",
        "",
    );
}

#[test]
fn catcher_observes_contents() {
    assert_cli(
        &[],
        r###"set_exception_handler(function($e){echo ob_get_level(),':',ob_get_contents();});ob_start(function($s){throw new Exception('stop');});echo 'body';"###,
        0,
        "body1:body1:",
        "",
    );
}

#[test]
fn catcher_throws() {
    assert_cli(
        &[],
        r###"set_exception_handler(function($e){echo 'handled';throw new Exception('again');});ob_start(function($s){throw new Exception('stop');});echo 'body';"###,
        255,
        "",
        "Fatal error: Uncaught Exception: again in Command line code:1\nStack trace:\n#0 [internal function]: {closure:Command line code:1}(Object(Exception))\n#1 {main}\n  thrown in Command line code on line 1\n",
    );
}

#[test]
fn catcher_exits() {
    assert_cli(
        &[],
        r###"set_exception_handler(function($e){echo 'handled';exit(7);});ob_start(function($s){throw new Exception('stop');});echo 'body';"###,
        7,
        "",
        "",
    );
}

#[test]
fn catcher_cannot_clean() {
    assert_cli(
        &[],
        r###"set_exception_handler(function($e){ob_clean();echo 'new';});ob_start(function($s){throw new Exception('stop');});echo 'body';"###,
        255,
        "",
        "Fatal error: ob_clean(): Cannot use output buffering in output buffering display handlers in Command line code on line 1\n",
    );
}

#[test]
fn original_remains_after_handled_replacement() {
    assert_cli(
        &[],
        r###"set_exception_handler(function($e){echo 'handled';});ob_start(function($s){throw new Exception('replacement');});echo 'body';throw new Exception('original');"###,
        255,
        "bodyhandled",
        "Fatal error: Uncaught Exception: original in Command line code:1\nStack trace:\n#0 {main}\n  thrown in Command line code on line 1\n",
    );
}

#[test]
fn exit_destructor() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"class FinalItem{function __destruct(){echo '<last>';}}$owner=new FinalItem;echo '<body>';exit(7);"###,
        7,
        "&lt;body&gt;&lt;last&gt;",
        "",
    );
}

#[test]
fn nonstatic_internal() {
    assert_cli(
        &["output_handler=Exception::getMessage"],
        r###"echo 'body';"###,
        0,
        "body",
        "Warning: PHP Request Startup: non-static method Exception::getMessage() cannot be called statically in Unknown on line 0\n",
    );
}

#[test]
fn missing_internal_method() {
    assert_cli(
        &["output_handler=Exception::missing"],
        r###"echo 'body';"###,
        0,
        "body",
        "Warning: PHP Request Startup: class Exception does not have a method \"missing\" in Unknown on line 0\n",
    );
}

#[test]
fn warning_record() {
    assert_cli(
        &["output_handler=missing_transform"],
        r###"var_dump(error_get_last());"###,
        0,
        "array(4) {\n  [\"type\"]=>\n  int(2)\n  [\"message\"]=>\n  string(84) \"PHP Request Startup: function \"missing_transform\" not found or invalid function name\"\n  [\"file\"]=>\n  string(7) \"Unknown\"\n  [\"line\"]=>\n  int(0)\n}\n",
        "Warning: PHP Request Startup: function \"missing_transform\" not found or invalid function name in Unknown on line 0\n",
    );
}

#[test]
fn warning_hidden() {
    assert_cli(
        &["output_handler=missing_transform", "display_errors=0"],
        r###"var_dump(error_get_last());"###,
        0,
        "array(4) {\n  [\"type\"]=>\n  int(2)\n  [\"message\"]=>\n  string(84) \"PHP Request Startup: function \"missing_transform\" not found or invalid function name\"\n  [\"file\"]=>\n  string(7) \"Unknown\"\n  [\"line\"]=>\n  int(0)\n}\n",
        "",
    );
}

#[test]
fn warning_startup_hidden() {
    assert_cli(
        &[
            "output_handler=missing_transform",
            "display_startup_errors=0",
        ],
        r###"var_dump(error_get_last());"###,
        0,
        "array(4) {\n  [\"type\"]=>\n  int(2)\n  [\"message\"]=>\n  string(84) \"PHP Request Startup: function \"missing_transform\" not found or invalid function name\"\n  [\"file\"]=>\n  string(7) \"Unknown\"\n  [\"line\"]=>\n  int(0)\n}\n",
        "",
    );
}

#[test]
fn warning_masked() {
    assert_cli(
        &["output_handler=missing_transform", "error_reporting=0"],
        r###"var_dump(error_get_last());"###,
        0,
        "array(4) {\n  [\"type\"]=>\n  int(2)\n  [\"message\"]=>\n  string(84) \"PHP Request Startup: function \"missing_transform\" not found or invalid function name\"\n  [\"file\"]=>\n  string(7) \"Unknown\"\n  [\"line\"]=>\n  int(0)\n}\n",
        "",
    );
}

#[test]
fn warning_logged() {
    assert_cli(
        &["output_handler=missing_transform", "log_errors=1"],
        r###"echo 'body';"###,
        0,
        "body",
        "PHP Warning:  PHP Request Startup: function \"missing_transform\" not found or invalid function name in Unknown on line 0\nWarning: PHP Request Startup: function \"missing_transform\" not found or invalid function name in Unknown on line 0\n",
    );
}

#[test]
fn warning_stdout() {
    assert_cli(
        &["output_handler=missing_transform", "display_errors=1"],
        r###"echo 'body';"###,
        0,
        "\nWarning: PHP Request Startup: function \"missing_transform\" not found or invalid function name in Unknown on line 0\nbody",
        "",
    );
}

#[test]
fn double_quoted() {
    assert_cli(
        &["output_handler=\"htmlspecialchars\""],
        r###"echo '<tag>';"###,
        0,
        "&lt;tag&gt;",
        "",
    );
}

#[test]
fn single_quoted() {
    assert_cli(
        &["output_handler='htmlspecialchars'"],
        r###"echo '<tag>';"###,
        0,
        "&lt;tag&gt;",
        "",
    );
}

#[test]
fn trailing_comment() {
    assert_cli(
        &["output_handler=htmlspecialchars;comment"],
        r###"echo '<tag>';"###,
        0,
        "&lt;tag&gt;",
        "",
    );
}

#[test]
fn quoted_false() {
    assert_cli(
        &["output_handler=\"false\""],
        r###"var_dump(ini_get('output_handler'));"###,
        0,
        "string(5) \"false\"\n",
        "Warning: PHP Request Startup: function \"false\" not found or invalid function name in Unknown on line 0\n",
    );
}

#[test]
fn off_trailing() {
    assert_cli(
        &["output_handler=off "],
        r###"var_dump(ini_get('output_handler'),ob_get_level());"###,
        0,
        "string(0) \"\"\nint(0)\n",
        "",
    );
}

#[test]
fn qualified() {
    assert_cli(
        &["output_handler=\\htmlspecialchars"],
        r###"echo '<tag>';"###,
        0,
        "&lt;tag&gt;",
        "",
    );
}

#[test]
fn alternate_internal() {
    assert_cli(
        &["output_handler=str_repeat"],
        r###"echo 'x';"###,
        0,
        "xxxxxxxxx",
        "",
    );
}

#[test]
fn shutdown_exit() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"register_shutdown_function(function(){echo '<last>';exit(3);});echo '<first>';exit(7);"###,
        3,
        "&lt;first&gt;&lt;last&gt;",
        "",
    );
}

#[test]
fn script_exception_nested() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"ob_start();echo '<first>';throw new Exception('stopped');"###,
        255,
        "&lt;first&gt;",
        "Fatal error: Uncaught Exception: stopped in Command line code:1\nStack trace:\n#0 {main}\n  thrown in Command line code on line 1\n",
    );
}

#[test]
fn final_user_throw() {
    assert_cli(
        &[],
        r###"ob_start(function($s){throw new Exception('stop');});echo 'payload';"###,
        255,
        "",
        "Fatal error: Uncaught Exception: stop in Command line code:1\nStack trace:\n#0 [internal function]: {closure:Command line code:1}('payload', 9)\n#1 {main}\n  thrown in Command line code on line 1\n",
    );
}

#[test]
fn final_user_throw_after_exit() {
    assert_cli(
        &[],
        r###"ob_start(function($s){throw new Exception('stop');});echo 'payload';exit(7);"###,
        255,
        "",
        "Fatal error: Uncaught Exception: stop in Command line code:1\nStack trace:\n#0 [internal function]: {closure:Command line code:1}('payload', 9)\n#1 {main}\n  thrown in Command line code on line 1\n",
    );
}

#[test]
fn final_user_arity() {
    assert_cli(
        &[],
        r###"ob_start(function($s,$p,$missing){return $s;});echo 'payload';"###,
        255,
        "",
        "Fatal error: Uncaught ArgumentCountError: Too few arguments to function {closure:Command line code:1}(), 2 passed and exactly 3 expected in Command line code:1\nStack trace:\n#0 [internal function]: {closure:Command line code:1}('payload', 9)\n#1 {main}\n  thrown in Command line code on line 1\n",
    );
}

#[test]
fn explicit_throw_caught() {
    assert_cli(
        &[],
        r###"ob_start(function($s){throw new Exception('stop');});echo 'payload';try{ob_end_flush();}catch(Throwable $e){echo 'caught';}"###,
        0,
        "payloadcaught",
        "",
    );
}

#[test]
fn exit_nested() {
    assert_cli(
        &[],
        r###"ob_start(function($s){return '['.$s.']';});ob_start(function($s){return '{'.$s.'}';});echo 'payload';exit(7);"###,
        7,
        "[{payload}]",
        "",
    );
}

#[test]
fn parent_after_handler_throw() {
    assert_cli(
        &[],
        r###"ob_start(function($s){return '['.$s.']';});echo 'outer';ob_start(function($s){throw new Exception('stop');});echo 'inner';"###,
        255,
        "",
        "Fatal error: Uncaught Exception: stop in Command line code:1\nStack trace:\n#0 [internal function]: {closure:Command line code:1}('inner', 9)\n#1 {main}\n  thrown in Command line code on line 1\n",
    );
}

#[test]
fn handler_swaps_exception() {
    assert_cli(
        &[],
        r###"ob_start(function($s){throw new Exception('replacement');});echo 'payload';throw new Exception('original');"###,
        255,
        "",
        "Fatal error: Uncaught Exception: original in Command line code:1\nStack trace:\n#0 {main}\n  thrown in Command line code on line 1\nFatal error: Uncaught Exception: replacement in Command line code:1\nStack trace:\n#0 [internal function]: {closure:Command line code:1}('payload', 9)\n#1 {main}\n  thrown in Command line code on line 1\n",
    );
}
use std::process::Command;

#[test]
fn final_exception_callback_cannot_reenter_the_failed_chunk_handler() {
    assert_cli(
        &[],
        "set_exception_handler(function($e){echo '123456789';});ob_start(function($s){throw new Exception('stop');},5);echo 'body';",
        0,
        "body123456789",
        "",
    );
}

#[test]
fn handler_runs_for_file_and_stdin_requests_before_shutdown() {
    use std::io::Write;
    use std::process::Stdio;

    let source =
        "<?php register_shutdown_function(function(){echo '<last>';});echo '<first>';exit(7);";
    let path = std::env::temp_dir().join(format!("rphp-handler-source-{}.php", std::process::id()));
    std::fs::write(&path, source).expect("create owned source specimen");
    let mut binaries = vec![env!("CARGO_BIN_EXE_rphp").to_string()];
    if let Ok(reference) = std::env::var("RPHP_REFERENCE_PHP") {
        binaries.push(reference);
    }
    for binary in binaries {
        for stdin in [false, true] {
            let mut command = Command::new(&binary);
            command.args(["-n", "-d", "output_handler=htmlspecialchars"]);
            if !stdin {
                command.arg(&path);
            }
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("run source");
            let mut input = child.stdin.take().unwrap();
            if stdin {
                input.write_all(source.as_bytes()).unwrap();
            }
            drop(input);
            let output = child.wait_with_output().unwrap();
            assert_eq!(output.status.code(), Some(7), "{binary}: {output:?}");
            assert_eq!(output.stdout, b"&lt;first&gt;&lt;last&gt;", "{binary}");
            assert!(output.stderr.is_empty(), "{binary}: {output:?}");
        }
    }
    std::fs::remove_file(path).expect("remove owned source specimen");
}

#[test]
fn startup_validation_precedes_frontend_rejection() {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-n",
            "-d",
            "display_errors=stderr",
            "-d",
            "log_errors=0",
            "-d",
            "output_handler=missing_output_transform",
            "-r",
            "not valid php !",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(255));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.starts_with("Warning: PHP Request Startup: function \"missing_output_transform\" not found or invalid function name in Unknown on line 0\nParse error:"), "{error}");
    // Parse-error spelling itself is a separately owned front-end contract.
}

fn assert_cli(settings: &[&str], source: &str, status: i32, stdout: &str, stderr: &str) {
    let mut binaries = vec![env!("CARGO_BIN_EXE_rphp").to_string()];
    if let Ok(reference) = std::env::var("RPHP_REFERENCE_PHP") {
        binaries.push(reference);
    }
    for binary in binaries {
        let mut command = Command::new(&binary);
        command.args([
            "-n",
            "-d",
            "display_errors=stderr",
            "-d",
            "fatal_error_backtraces=0",
            "-d",
            "log_errors=0",
        ]);
        for setting in settings {
            command.args(["-d", setting]);
        }
        let output = command
            .args(["-r", source])
            .output()
            .expect("run original output handler case");
        assert_eq!(output.status.code(), Some(status), "{binary}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            stdout,
            "{binary}"
        );
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            stderr,
            "{binary}"
        );
    }
}

#[test]
fn default() {
    assert_cli(
        &[],
        r###"echo ob_get_level(),'|';var_dump(ini_get('output_handler'));"###,
        0,
        "0|string(0) \"\"\n",
        "",
    );
}

#[test]
fn empty() {
    assert_cli(
        &["output_handler="],
        r###"echo ob_get_level(),'|';var_dump(ini_get('output_handler'));"###,
        0,
        "0|string(0) \"\"\n",
        "",
    );
}

#[test]
fn html() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"echo '<tag>&';"###,
        0,
        "&lt;tag&gt;&amp;",
        "",
    );
}

#[test]
fn html_buffer_state() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"echo '<tag>'; $before=ob_get_contents(); $length=ob_get_length(); ob_clean(); var_dump(ob_get_level(),$before,$length);"###,
        0,
        "int(1)\nstring(5) \"&lt;tag&gt;\"\nint(5)\n",
        "",
    );
}

#[test]
fn html_case() {
    assert_cli(
        &["output_handler=HTMLSPECIALCHARS"],
        r###"echo '<tag>&';"###,
        0,
        "&lt;tag&gt;&amp;",
        "",
    );
}

#[test]
fn html_flush() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"echo '<first>';ob_flush();echo '<second>';"###,
        0,
        "&lt;first&gt;&lt;second&gt;",
        "",
    );
}

#[test]
fn html_clean() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"echo '<first>';ob_clean();echo '<second>';"###,
        0,
        "&lt;second&gt;",
        "",
    );
}

#[test]
fn html_remove() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"echo '<first>';ob_end_clean();echo '<second>';"###,
        0,
        "<second>",
        "",
    );
}

#[test]
fn html_nested() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"ob_start();echo '<first>';ob_end_flush();echo '<second>';"###,
        0,
        "&lt;first&gt;&lt;second&gt;",
        "",
    );
}

#[test]
fn html_throw() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"echo '<first>';throw new Exception('stopped');"###,
        255,
        "&lt;first&gt;",
        "Fatal error: Uncaught Exception: stopped in Command line code:1\nStack trace:\n#0 {main}\n  thrown in Command line code on line 1\n",
    );
}

#[test]
fn undefined() {
    assert_cli(
        &["output_handler=missing_output_transform"],
        r###"echo 'body';"###,
        0,
        "body",
        "Warning: PHP Request Startup: function \"missing_output_transform\" not found or invalid function name in Unknown on line 0\n",
    );
}

#[test]
fn declared_late() {
    assert_cli(
        &["output_handler=local_transform"],
        r###"function local_transform($s,$p){return 'changed';}echo 'body';"###,
        0,
        "body",
        "Warning: PHP Request Startup: function \"local_transform\" not found or invalid function name in Unknown on line 0\n",
    );
}

#[test]
fn class_declared_late() {
    assert_cli(
        &["output_handler=LocalTransform::apply"],
        r###"class LocalTransform{static function apply($s,$p){return 'changed';}}echo 'body';"###,
        0,
        "body",
        "Warning: PHP Request Startup: class \"LocalTransform\" not found in Unknown on line 0\n",
    );
}

#[test]
fn space() {
    assert_cli(
        &["output_handler= "],
        r###"echo 'body';"###,
        0,
        "body",
        "Warning: PHP Request Startup: function \" \" not found or invalid function name in Unknown on line 0\n",
    );
}

#[test]
fn off() {
    assert_cli(
        &["output_handler=off"],
        r###"echo ob_get_level(),'|';var_dump(ini_get('output_handler'));"###,
        0,
        "0|string(0) \"\"\n",
        "",
    );
}

#[test]
fn last_empty() {
    assert_cli(
        &["output_handler=htmlspecialchars", "output_handler="],
        r###"echo ob_get_level(),'|<tag>';"###,
        0,
        "0|<tag>",
        "",
    );
}

#[test]
fn last_html() {
    assert_cli(
        &["output_handler=", "output_handler=htmlspecialchars"],
        r###"echo ob_get_level(),'|<tag>';"###,
        0,
        "1|&lt;tag&gt;",
        "",
    );
}

#[test]
fn runtime_set() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"var_dump(ini_set('output_handler',''),ini_get('output_handler'));echo '<tag>';"###,
        0,
        "bool(false)\nstring(16) \"htmlspecialchars\"\n&lt;tag&gt;",
        "",
    );
}

#[test]
fn runtime_set_default() {
    assert_cli(
        &[],
        r###"var_dump(ini_set('output_handler','htmlspecialchars'),ini_get('output_handler'));echo '<tag>';"###,
        0,
        "bool(false)\nstring(0) \"\"\n<tag>",
        "",
    );
}

#[test]
fn uppercase_setting() {
    assert_cli(
        &["OUTPUT_HANDLER=htmlspecialchars"],
        r###"echo ob_get_level(),'|';var_dump(ini_get('output_handler'));echo '<tag>';"###,
        0,
        "0|string(0) \"\"\n<tag>",
        "",
    );
}

#[test]
fn zero() {
    assert_cli(
        &["output_handler=0"],
        r###"echo ob_get_level(),'|';var_dump(ini_get('output_handler'));"###,
        0,
        "0|string(1) \"0\"\n",
        "Warning: PHP Request Startup: function \"0\" not found or invalid function name in Unknown on line 0\n",
    );
}

#[test]
fn boolean_true() {
    assert_cli(
        &["output_handler=true"],
        r###"echo ob_get_level(),'|';var_dump(ini_get('output_handler'));"###,
        0,
        "0|string(1) \"1\"\n",
        "Warning: PHP Request Startup: function \"1\" not found or invalid function name in Unknown on line 0\n",
    );
}

#[test]
fn prefixed_space() {
    assert_cli(
        &["output_handler= htmlspecialchars"],
        r###"echo ob_get_level(),'|';var_dump(ini_get('output_handler'));"###,
        0,
        "0|string(17) \" htmlspecialchars\"\n",
        "Warning: PHP Request Startup: function \" htmlspecialchars\" not found or invalid function name in Unknown on line 0\n",
    );
}

#[test]
fn trailing_space() {
    assert_cli(
        &["output_handler=htmlspecialchars "],
        r###"echo ob_get_level(),'|';var_dump(ini_get('output_handler'));"###,
        0,
        "1|string(16) \"htmlspecialchars\"\n",
        "",
    );
}

#[test]
fn disabled_handler() {
    assert_cli(
        &[
            "disable_functions=htmlspecialchars",
            "output_handler=htmlspecialchars",
        ],
        r###"echo ob_get_level(),'|<tag>';"###,
        0,
        "0|<tag>",
        "Warning: PHP Request Startup: function \"htmlspecialchars\" not found or invalid function name in Unknown on line 0\n",
    );
}

#[test]
fn native_arity() {
    assert_cli(
        &["output_handler=strtoupper"],
        r###"echo 'mixed';"###,
        255,
        "",
        "Fatal error: Uncaught ArgumentCountError: strtoupper() expects exactly 1 argument, 2 given in [no active file]:0\nStack trace:\n#0 [internal function]: strtoupper('mixed', 9)\n#1 {main}\n  thrown in [no active file] on line 0\n",
    );
}

#[test]
fn no_output() {
    assert_cli(&["output_handler=htmlspecialchars"], r###""###, 0, "", "");
}

#[test]
fn explicit_exit() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"echo '<tag>';exit(7);"###,
        7,
        "&lt;tag&gt;",
        "",
    );
}

#[test]
fn shutdown_callback() {
    assert_cli(
        &["output_handler=htmlspecialchars"],
        r###"register_shutdown_function(function(){echo '<last>';});echo '<first>';"###,
        0,
        "&lt;first&gt;&lt;last&gt;",
        "",
    );
}
