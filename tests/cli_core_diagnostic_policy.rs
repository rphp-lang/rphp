//! Original PHP diagnostic-policy CLI contracts.

use std::process::Command;

fn assert_cli(settings: &[&str], source: &str, status: i32, stdout: &str, stderr: &str) {
    assert_cli_bytes(
        settings,
        source,
        status,
        stdout.as_bytes(),
        stderr.as_bytes(),
    );
}

fn assert_cli_bytes(settings: &[&str], source: &str, status: i32, stdout: &[u8], stderr: &[u8]) {
    assert_cli_bytes_with_trace(settings, source, status, stdout, stderr, Some("0"));
}

fn assert_cli_bytes_with_trace(
    settings: &[&str],
    source: &str,
    status: i32,
    stdout: &[u8],
    stderr: &[u8],
    trace: Option<&str>,
) {
    let mut binaries = vec![env!("CARGO_BIN_EXE_rphp").to_string()];
    if let Ok(reference) = std::env::var("RPHP_REFERENCE_PHP") {
        binaries.push(reference);
    }
    for binary in binaries {
        let mut command = Command::new(&binary);
        command.args([
            "-n",
            "-d",
            "display_errors=1",
            "-d",
            "log_errors=0",
            "-d",
            "error_reporting=E_ALL",
            "-d",
            "html_errors=0",
            "-d",
            "zend.exception_ignore_args=0",
        ]);
        if let Some(trace) = trace {
            command.args(["-d", &format!("fatal_error_backtraces={trace}")]);
        }
        for setting in settings {
            command.args(["-d", setting]);
        }
        let output = command
            .args(["-r", source])
            .output()
            .expect("run original diagnostic specimen");
        assert_eq!(output.status.code(), Some(status), "{binary}: {output:?}");
        assert_eq!(output.stdout, stdout, "{binary}");
        assert_eq!(output.stderr, stderr, "{binary}");
    }
}

#[test]
fn implicit_php85_fatal_backtraces_are_enabled() {
    assert_cli_bytes_with_trace(
        &[],
        "eval('function emptyResult():void{return 3;}');",
        255,
        b"\nFatal error: A void function must not return a value in Command line code(1) : eval()'d code on line 1\nStack trace:\n#0 {main}\n",
        b"",
        None,
    );
}

#[test]
fn warning_default() {
    assert_cli(
        &[],
        "trigger_error('notice-body',E_USER_WARNING);echo 'after';",
        0,
        "\nWarning: notice-body in Command line code on line 1\nafter",
        "",
    );
}

#[test]
fn warning_display_off() {
    assert_cli(
        &["display_errors=0"],
        "trigger_error('hidden-body',E_USER_WARNING);echo 'after';",
        0,
        "after",
        "",
    );
}

#[test]
fn warning_stderr() {
    assert_cli(
        &["display_errors=stderr"],
        "trigger_error('channel-body',E_USER_NOTICE);echo 'after';",
        0,
        "after",
        "Notice: channel-body in Command line code on line 1\n",
    );
}

#[test]
fn warning_logged() {
    assert_cli(
        &["display_errors=0", "log_errors=1"],
        "trigger_error('logged-body',E_USER_WARNING);echo 'after';",
        0,
        "after",
        "PHP Warning:  logged-body in Command line code on line 1\n",
    );
}

#[test]
fn warning_runtime_off() {
    assert_cli(
        &[],
        "ini_set('display_errors','0');trigger_error('private-body',E_USER_WARNING);var_dump(error_get_last()['message']);",
        0,
        "string(12) \"private-body\"\n",
        "",
    );
}

#[test]
fn fatal_runtime_off() {
    assert_cli(
        &[],
        "ini_set('display_errors','0');echo 'before';throw new Exception('private-body');",
        255,
        "before",
        "",
    );
}

#[test]
fn repeat_same_source() {
    assert_cli(
        &["ignore_repeated_errors=1"],
        "for($i=0;$i<3;$i++){trigger_error('repeat-body',E_USER_WARNING);}echo 'after';",
        0,
        "\nWarning: repeat-body in Command line code on line 1\nafter",
        "",
    );
}

#[test]
fn repeat_different_source() {
    assert_cli(
        &["ignore_repeated_errors=1"],
        "trigger_error('repeat-body',E_USER_WARNING);\ntrigger_error('repeat-body',E_USER_WARNING);",
        0,
        "\nWarning: repeat-body in Command line code on line 1\n\nWarning: repeat-body in Command line code on line 2\n",
        "",
    );
}

#[test]
fn repeat_ignore_source() {
    assert_cli(
        &["ignore_repeated_errors=1", "ignore_repeated_source=1"],
        "trigger_error('repeat-body',E_USER_WARNING);\ntrigger_error('repeat-body',E_USER_WARNING);",
        0,
        "\nWarning: repeat-body in Command line code on line 1\n",
        "",
    );
}

#[test]
fn repeat_handler_false() {
    assert_cli(
        &["ignore_repeated_errors=1"],
        "set_error_handler(function($n,$s){echo 'handled;';return false;});for($i=0;$i<3;$i++){trigger_error('repeat-body',E_USER_WARNING);}",
        0,
        "handled;\nWarning: repeat-body in Command line code on line 1\nhandled;handled;",
        "",
    );
}

#[test]
fn repeat_handler_true() {
    assert_cli(
        &["ignore_repeated_errors=1"],
        "set_error_handler(function($n,$s){echo 'handled;';return true;});for($i=0;$i<3;$i++){trigger_error('repeat-body',E_USER_WARNING);}var_dump(error_get_last());",
        0,
        "handled;handled;handled;NULL\n",
        "",
    );
}

#[test]
fn repeat_clear_last() {
    assert_cli(
        &["ignore_repeated_errors=1"],
        "for($i=0;$i<2;$i++){trigger_error('repeat-body',E_USER_WARNING);error_clear_last();}",
        0,
        "\nWarning: repeat-body in Command line code on line 1\n\nWarning: repeat-body in Command line code on line 1\n",
        "",
    );
}

#[test]
fn repeat_masked_first() {
    assert_cli(
        &["ignore_repeated_errors=1"],
        "for($i=0;$i<2;$i++){error_reporting($i?E_ALL:0);trigger_error('repeat-body',E_USER_WARNING);}var_dump(error_get_last()['message']);",
        0,
        "string(11) \"repeat-body\"\n",
        "",
    );
}

#[test]
fn repeat_level_changes() {
    assert_cli(
        &["ignore_repeated_errors=1"],
        "foreach([E_USER_NOTICE,E_USER_WARNING,E_USER_NOTICE] as $level){trigger_error('same-body',$level);}",
        0,
        "\nNotice: same-body in Command line code on line 1\n",
        "",
    );
}

#[test]
fn repeat_runtime_toggle() {
    assert_cli(
        &[],
        "for($i=0;$i<3;$i++){ini_set('ignore_repeated_errors',$i>0);trigger_error('repeat-body',E_USER_WARNING);}",
        0,
        "\nWarning: repeat-body in Command line code on line 1\n",
        "",
    );
}

#[test]
fn fatal_trace() {
    assert_cli(
        &["fatal_error_backtraces=1"],
        "function buildFrame($context){eval('class RepeatedShape {} class RepeatedShape {}');}buildFrame('detail');",
        255,
        "\nFatal error: Cannot redeclare class RepeatedShape (previously declared in Command line code(1) : eval()'d code:1) in Command line code(1) : eval()'d code on line 1\nStack trace:\n#0 Command line code(1): eval()\n#1 Command line code(1): buildFrame('detail')\n#2 {main}\n",
        "",
    );
}

#[test]
fn fatal_trace_sensitive() {
    assert_cli(
        &["fatal_error_backtraces=1"],
        "function buildFrame(#[SensitiveParameter] $context){eval('class RepeatedShape {} class RepeatedShape {}');}buildFrame('secret-detail');",
        255,
        "\nFatal error: Cannot redeclare class RepeatedShape (previously declared in Command line code(1) : eval()'d code:1) in Command line code(1) : eval()'d code on line 1\nStack trace:\n#0 Command line code(1): eval()\n#1 Command line code(1): buildFrame(Object(SensitiveParameterValue))\n#2 {main}\n",
        "",
    );
}

#[test]
fn fatal_trace_no_args() {
    assert_cli(
        &["fatal_error_backtraces=1", "zend.exception_ignore_args=1"],
        "function buildFrame($context){eval('class RepeatedShape {} class RepeatedShape {}');}buildFrame('detail');",
        255,
        "\nFatal error: Cannot redeclare class RepeatedShape (previously declared in Command line code(1) : eval()'d code:1) in Command line code(1) : eval()'d code on line 1\nStack trace:\n#0 Command line code(1): eval()\n#1 Command line code(1): buildFrame()\n#2 {main}\n",
        "",
    );
}

#[test]
fn fatal_trace_last_error() {
    assert_cli(
        &["fatal_error_backtraces=1"],
        "register_shutdown_function(function(){var_dump(error_get_last());echo 'shutdown';});function buildFrame($context){eval('class RepeatedShape {} class RepeatedShape {}');}buildFrame('detail');",
        255,
        "\nFatal error: Cannot redeclare class RepeatedShape (previously declared in Command line code(1) : eval()'d code:1) in Command line code(1) : eval()'d code on line 1\nStack trace:\n#0 Command line code(1): eval()\n#1 Command line code(1): buildFrame('detail')\n#2 {main}\narray(5) {\n  [\"type\"]=>\n  int(64)\n  [\"message\"]=>\n  string(100) \"Cannot redeclare class RepeatedShape (previously declared in Command line code(1) : eval()'d code:1)\"\n  [\"file\"]=>\n  string(36) \"Command line code(1) : eval()'d code\"\n  [\"line\"]=>\n  int(1)\n  [\"trace\"]=>\n  array(2) {\n    [0]=>\n    array(3) {\n      [\"file\"]=>\n      string(17) \"Command line code\"\n      [\"line\"]=>\n      int(1)\n      [\"function\"]=>\n      string(4) \"eval\"\n    }\n    [1]=>\n    array(4) {\n      [\"file\"]=>\n      string(17) \"Command line code\"\n      [\"line\"]=>\n      int(1)\n      [\"function\"]=>\n      string(10) \"buildFrame\"\n      [\"args\"]=>\n      array(1) {\n        [0]=>\n        string(6) \"detail\"\n      }\n    }\n  }\n}\nshutdown",
        "",
    );
}

#[test]
fn html_warning() {
    assert_cli(
        &["html_errors=1"],
        "trigger_error('<tag>&value',E_USER_WARNING);echo 'after';",
        0,
        "<br />\n<b>Warning</b>:  <tag>&value in <b>Command line code</b> on line <b>1</b><br />\nafter",
        "",
    );
}

#[test]
fn html_runtime() {
    assert_cli(
        &[],
        "ini_set('html_errors',true);trigger_error('<tag>&value',E_USER_NOTICE);",
        0,
        "<br />\n<b>Notice</b>:  <tag>&value in <b>Command line code</b> on line <b>1</b><br />\n",
        "",
    );
}

#[test]
fn html_exception() {
    assert_cli(
        &["html_errors=1"],
        "throw new Exception('<tag>&value');",
        255,
        "<br />\n<b>Fatal error</b>:  Uncaught Exception: &lt;tag&gt;&amp;value in Command line code:1\nStack trace:\n#0 {main}\n  thrown in <b>Command line code</b> on line <b>1</b><br />\n",
        "",
    );
}

#[test]
fn html_invalid_bytes() {
    assert_cli_bytes(
        &["html_errors=1"],
        "trigger_error(\"x\\xff<y>\",E_USER_WARNING);",
        0,
        b"<br />\n<b>Warning</b>:  x\xff<y> in <b>Command line code</b> on line <b>1</b><br />\n",
        b"",
    );
}

#[test]
fn html_docref() {
    assert_cli(
        &["html_errors=1", "docref_root=/manual/"],
        "fopen('absent-diagnostic-oracle-file','r');",
        0,
        "<br />\n<b>Warning</b>:  fopen(absent-diagnostic-oracle-file) [<a href='/manual/function.fopen'>function.fopen</a>]: Failed to open stream: No such file or directory in <b>Command line code</b> on line <b>1</b><br />\n",
        "",
    );
}

#[test]
fn html_nested_buffer() {
    assert_cli(
        &["html_errors=1"],
        "ob_start();trigger_error('<tag>&value',E_USER_WARNING);$s=ob_get_clean();echo '['.$s.']';",
        0,
        "[<br />\n<b>Warning</b>:  <tag>&value in <b>Command line code</b> on line <b>1</b><br />\n]",
        "",
    );
}

#[test]
fn diagnostic_settings() {
    assert_cli(
        &[],
        "foreach(['ignore_repeated_errors','ignore_repeated_source','fatal_error_backtraces','html_errors','docref_root','docref_ext','error_log'] as $name){echo $name,':';var_dump(ini_get($name),ini_set($name,'1'),ini_get($name));}",
        0,
        "ignore_repeated_errors:string(1) \"0\"\nstring(1) \"0\"\nstring(1) \"1\"\nignore_repeated_source:string(1) \"0\"\nstring(1) \"0\"\nstring(1) \"1\"\nfatal_error_backtraces:string(1) \"0\"\nstring(1) \"0\"\nstring(1) \"1\"\nhtml_errors:string(1) \"0\"\nstring(1) \"0\"\nstring(1) \"1\"\ndocref_root:string(0) \"\"\nstring(0) \"\"\nstring(1) \"1\"\ndocref_ext:string(0) \"\"\nstring(0) \"\"\nstring(1) \"1\"\nerror_log:string(0) \"\"\nstring(0) \"\"\nstring(1) \"1\"\n",
        "",
    );
}

#[test]
fn binary_warning_stderr() {
    assert_cli_bytes(
        &["display_errors=stderr"],
        "trigger_error(\"raw\\xff<tag>\",E_USER_WARNING);",
        0,
        b"",
        b"Warning: raw\xff<tag> in Command line code on line 1\n",
    );
}

#[test]
fn binary_warning_handler_and_last() {
    assert_cli_bytes(&["html_errors=1"], "set_error_handler(function($n,$s){echo bin2hex($s),\";\";return false;});trigger_error(\"raw\\xff<tag>\",E_USER_WARNING);echo bin2hex(error_get_last()[\"message\"]);", 0, b"726177ff3c7461673e;<br />\n<b>Warning</b>:  raw\xff<tag> in <b>Command line code</b> on line <b>1</b><br />\n726177ff3c7461673e", b"");
}

#[test]
fn binary_native_warning() {
    assert_cli_bytes(&["html_errors=1"], "fopen(\"absent-policy-\\xffdata\",\"r\");", 0, b"<br />\n<b>Warning</b>:  fopen(absent-policy-\xef\xbf\xbddata): Failed to open stream: No such file or directory in <b>Command line code</b> on line <b>1</b><br />\n", b"");
}

#[test]
fn binary_context_native_warning() {
    assert_cli_bytes(&["html_errors=1"], "fopen(\"absent-policy-\\xffdata\",\"r\",false);", 0, b"<br />\n<b>Warning</b>:  fopen(absent-policy-\xef\xbf\xbddata): Failed to open stream: No such file or directory in <b>Command line code</b> on line <b>1</b><br />\n", b"");
}

#[test]
fn binary_native_legacy_charset() {
    assert_cli_bytes(&["html_errors=1", "default_charset=Windows-1251"], "fopen(\"absent-policy-\\xffdata\",\"r\");", 0, b"<br />\n<b>Warning</b>:  fopen(absent-policy-\xffdata): Failed to open stream: No such file or directory in <b>Command line code</b> on line <b>1</b><br />\n", b"");
}

#[test]
fn binary_throwable_utf8() {
    assert_cli_bytes(&["html_errors=1"], "throw new Exception(\"raw\\xff<tag>\");", 255, b"<br />\n<b>Fatal error</b>:  Uncaught Exception: raw\xef\xbf\xbd&lt;tag&gt; in Command line code:1\nStack trace:\n#0 {main}\n  thrown in <b>Command line code</b> on line <b>1</b><br />\n", b"");
}

#[test]
fn binary_throwable_legacy_charset() {
    assert_cli_bytes(&["html_errors=1", "default_charset=Windows-1251"], "throw new Exception(\"raw\\xff<tag>\");", 255, b"<br />\n<b>Fatal error</b>:  Uncaught Exception: raw\xff&lt;tag&gt; in Command line code:1\nStack trace:\n#0 {main}\n  thrown in <b>Command line code</b> on line <b>1</b><br />\n", b"");
}

#[test]
fn binary_throwable_plain() {
    assert_cli_bytes(&[], "throw new Exception(\"raw\\xff<tag>\");", 255, b"\nFatal error: Uncaught Exception: raw\xff<tag> in Command line code:1\nStack trace:\n#0 {main}\n  thrown in Command line code on line 1\n", b"");
}

#[test]
fn binary_throwable_string() {
    assert_cli_bytes(
        &[],
        "$e=new Exception(\"raw\\xff<tag>\");echo $e;",
        0,
        b"Exception: raw\xff<tag> in Command line code:1\nStack trace:\n#0 {main}",
        b"",
    );
}

#[test]
fn binary_throwable_chain() {
    assert_cli_bytes(&["html_errors=1"], "$first=new Exception(\"raw\\xff<tag>\");throw new Exception(\"next\",0,$first);", 255, b"<br />\n<b>Fatal error</b>:  Uncaught Exception: raw\xef\xbf\xbd&lt;tag&gt; in Command line code:1\nStack trace:\n#0 {main}\n\nNext Exception: next in Command line code:1\nStack trace:\n#0 {main}\n  thrown in <b>Command line code</b> on line <b>1</b><br />\n", b"");
}

#[test]
fn diagnostic_byte_identity() {
    assert_cli_bytes(&["ignore_repeated_errors=1"], "foreach([\"\\xff\",\"ÿ\",\"\\xc3\\xbf\"] as $text){trigger_error($text,E_USER_WARNING);}", 0, b"\nWarning: \xff in Command line code on line 1\n\nWarning: \xc3\xbf in Command line code on line 1\n", b"");
}

#[test]
fn repeated_vm_warning() {
    assert_cli_bytes(
        &["ignore_repeated_errors=1"],
        "for($i=0;$i<3;$i++){echo $missing;}",
        0,
        b"\nWarning: Undefined variable $missing in Command line code on line 1\n",
        b"",
    );
}

#[test]
fn handled_vm_warning() {
    assert_cli_bytes(&["ignore_repeated_errors=1"], "set_error_handler(function($n,$s){echo \"handled;\";return false;});for($i=0;$i<3;$i++){echo $missing;}", 0, b"handled;\nWarning: Undefined variable $missing in Command line code on line 1\nhandled;handled;", b"");
}

#[test]
fn runtime_compile_warning() {
    assert_cli_bytes(&["html_errors=1"], "eval('function annotated(int $value=null){}');", 0, b"<br />\n<b>Deprecated</b>:  annotated(): Implicitly marking parameter $value as nullable is deprecated, the explicit nullable type must be used instead in <b>Command line code(1) : eval()'d code</b> on line <b>1</b><br />\n", b"");
}

#[test]
fn startup_compile_warning() {
    assert_cli_bytes(
        &["display_errors=0"],
        "function annotated(int $value=null){} echo \"done;\";",
        0,
        b"done;",
        b"",
    );
}

#[test]
fn fatal_compile_return() {
    assert_cli_bytes(&["fatal_error_backtraces=1"], "function compileFrame($context){eval(\"function emptyResult():void{return 3;}\");}compileFrame(\"detail\");", 255, b"\nFatal error: A void function must not return a value in Command line code(1) : eval()'d code on line 1\nStack trace:\n#0 Command line code(1): compileFrame('detail')\n#1 {main}\n", b"");
}

#[test]
fn html_fatal_compile() {
    assert_cli_bytes(&["fatal_error_backtraces=1", "html_errors=1"], "eval(\"class RepeatedShape{} class RepeatedShape{}\");", 255, b"<br />\n<b>Fatal error</b>:  Cannot redeclare class RepeatedShape (previously declared in Command line code(1) : eval()'d code:1) in <b>Command line code(1) : eval()'d code</b> on line <b>1</b><br />\nStack trace:\n#0 Command line code(1): eval()\n#1 {main}\n", b"");
}

#[test]
fn trace_off_last_error() {
    assert_cli_bytes(&["fatal_error_backtraces=0"], "register_shutdown_function(function(){var_dump(error_get_last());});eval(\"class RepeatedShape{} class RepeatedShape{}\");", 255, b"\nFatal error: Cannot redeclare class RepeatedShape (previously declared in Command line code(1) : eval()'d code:1) in Command line code(1) : eval()'d code on line 1\narray(4) {\n  [\"type\"]=>\n  int(64)\n  [\"message\"]=>\n  string(100) \"Cannot redeclare class RepeatedShape (previously declared in Command line code(1) : eval()'d code:1)\"\n  [\"file\"]=>\n  string(36) \"Command line code(1) : eval()'d code\"\n  [\"line\"]=>\n  int(1)\n}\n", b"");
}

#[test]
fn trace_argument_owner() {
    assert_cli_bytes(&["fatal_error_backtraces=1"], "class TraceOwner{function __destruct(){echo \"released;\";}}register_shutdown_function(function(){echo \"shutdown;\";error_clear_last();echo \"cleared;\";});function compileFrame($owner){eval(\"class RepeatedShape{} class RepeatedShape{}\");}compileFrame(new TraceOwner);", 255, b"\nFatal error: Cannot redeclare class RepeatedShape (previously declared in Command line code(1) : eval()'d code:1) in Command line code(1) : eval()'d code on line 1\nStack trace:\n#0 Command line code(1): eval()\n#1 Command line code(1): compileFrame(Object(TraceOwner))\n#2 {main}\nshutdown;cleared;", b"");
}

#[test]
fn shutdown_diagnostic_origin() {
    assert_cli_bytes(&["html_errors=1"], "register_shutdown_function(function(){trigger_error(\"later\",E_USER_NOTICE);});throw new Exception(\"first\");", 255, b"<br />\n<b>Fatal error</b>:  Uncaught Exception: first in Command line code:1\nStack trace:\n#0 {main}\n  thrown in <b>Command line code</b> on line <b>1</b><br />\n<br />\n<b>Notice</b>:  later in <b>Command line code</b> on line <b>1</b><br />\n", b"");
}

#[test]
fn file_log_and_display() {
    assert_cli_bytes(&[], "$path=tempnam(sys_get_temp_dir(),\"rphp-diag-\");ini_set(\"log_errors\",1);ini_set(\"error_log\",$path);trigger_error(\"logged-detail\",E_USER_WARNING);$log=file_get_contents($path);echo substr($log,strpos($log,\"]\")+2);unlink($path);", 0, b"\nWarning: logged-detail in Command line code on line 1\nPHP Warning:  logged-detail in Command line code on line 1\n", b"");
}

#[test]
fn file_log_duplicates() {
    assert_cli_bytes(
        &["ignore_repeated_errors=1", "display_errors=0"],
        "$path=tempnam(sys_get_temp_dir(),\"rphp-diag-\");ini_set(\"log_errors\",1);ini_set(\"error_log\",$path);for($i=0;$i<3;$i++){trigger_error(\"logged-detail\",E_USER_WARNING);}$log=file_get_contents($path);echo substr($log,strpos($log,\"]\")+2);unlink($path);",
        0,
        b"PHP Warning:  logged-detail in Command line code on line 1\n",
        b"",
    );
}

#[test]
fn unavailable_log_fallback() {
    assert_cli_bytes(
        &[
            "log_errors=1",
            "display_errors=0",
            "error_log=/nonexistent-diagnostic-oracle-dir/log",
        ],
        "trigger_error(\"logged-detail\",E_USER_WARNING);",
        0,
        b"",
        b"PHP Warning:  logged-detail in Command line code on line 1\n",
    );
}

#[test]
fn masked_fatal() {
    assert_cli_bytes(
        &["error_reporting=0"],
        "throw new Exception(\"hidden\");",
        255,
        b"",
        b"",
    );
}
