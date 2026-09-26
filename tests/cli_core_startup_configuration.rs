//! Original PHP 8.5 CLI startup-configuration regressions.
use std::process::Command;

fn assert_cli(settings: &[&str], source: &str, stdout: &str, stderr: &str) {
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
            "display_startup_errors=1",
            "-d",
            "log_errors=0",
            "-d",
            "zend.assertions=1",
        ]);
        for setting in settings {
            command.args(["-d", setting]);
        }
        let output = command
            .args(["-r", source])
            .env("RPHP_STARTUP_ORACLE", "visible")
            .env(
                "RPHP_STARTUP_SOURCE",
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/startup_configuration/include.php"
                ),
            )
            .output()
            .expect("run startup contract");
        assert_eq!(output.status.code(), Some(0), "{binary}: {output:?}");
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
fn reflection_rejects_disabled_and_unknown_functions_but_allows_replacements() {
    assert_cli(
        &["disable_functions=strlen"],
        "foreach(['strlen','missing_startup_probe'] as $name){try{new ReflectionFunction($name);}catch(ReflectionException $e){echo $e->getMessage(),'|';}}",
        "Function strlen() does not exist|Function missing_startup_probe() does not exist|",
        "",
    );
    assert_cli(
        &["disable_functions=strlen"],
        "function strlen($text){return 61;}$r=new ReflectionFunction('strlen');var_dump($r->isUserDefined());echo $r->invoke('abc'),'|';",
        "bool(true)\n61|",
        "",
    );
}

#[test]
fn included_source_inherits_disabled_builtins_and_user_replacement() {
    assert_cli(
        &["disable_functions=strlen"],
        "try{include getenv('RPHP_STARTUP_SOURCE');}catch(Error $e){echo $e->getMessage(),'|';}",
        "Call to undefined function strlen()|",
        "",
    );
    assert_cli(
        &["disable_functions=strlen"],
        "function strlen($text){return 59;}echo include getenv('RPHP_STARTUP_SOURCE'),'|';",
        "59|",
        "",
    );
}

#[test]
fn unset_request_globals_keep_global_diagnostics_in_root_and_nested_scopes() {
    for source in [
        "unset($_SERVER);var_dump($_SERVER);",
        "function inspect(){unset($_SERVER);var_dump($_SERVER);}inspect();",
        "class Sample{static function inspect(){unset($_SERVER);var_dump($_SERVER);}}Sample::inspect();",
        "unset($_ENV);var_dump($_ENV);",
    ] {
        let name = if source.contains("_ENV") {
            "_ENV"
        } else {
            "_SERVER"
        };
        assert_cli(
            &["variables_order=G"],
            source,
            &format!(
                "\nWarning: Undefined global variable ${name} in Command line code on line 1\nNULL\n"
            ),
            "",
        );
    }
}

#[test]
fn disabled_mixed_call_case_0() {
    assert_cli(
        &["disable_functions=strlen"],
        r###"try{STRLEN('abc');}catch(Error $e){echo $e->getMessage(),'|';}"###,
        "Call to undefined function STRLEN()|",
        "",
    );
}

#[test]
fn uppercase_list_is_inert_1() {
    assert_cli(
        &["disable_functions=STRLEN"],
        r###"echo STRLEN('abc'),'|';var_dump(function_exists('strlen'));"###,
        "3|bool(true)\n",
        "",
    );
}

#[test]
fn last_setting_wins_2() {
    assert_cli(
        &["disable_functions=strlen", "disable_functions=count"],
        r###"echo strlen('abc'),'|';var_dump(function_exists('strlen'),function_exists('count'));"###,
        "3|bool(true)\nbool(false)\n",
        "",
    );
}

#[test]
fn replacement_alias_target_3() {
    assert_cli(
        &["disable_functions=rtrim"],
        r###"function rtrim($x){return 37;}echo rtrim('x '),'|',chop('x '),'|';"###,
        "37|x|",
        "",
    );
}

#[test]
fn replacement_nested_4() {
    assert_cli(
        &["disable_functions=strlen"],
        r###"function strlen($text){return 37;}function ordinary(){return strlen('abc');}class Sample{static function method(){return strlen('abc');}}$closure=fn()=>strlen('abc');echo ordinary(),'|',Sample::method(),'|',$closure(),'|';"###,
        "37|37|37|",
        "",
    );
}

#[test]
fn replacement_import_5() {
    assert_cli(
        &["disable_functions=strlen"],
        r###"namespace Example {use function strlen as length;echo length('abc'),'|';}namespace {function strlen($x){return 43;}}"###,
        "43|",
        "",
    );
}

#[test]
fn replacement_namespace_6() {
    assert_cli(
        &["disable_functions=strlen"],
        r###"namespace Example;function strlen($text){return 47;}echo strlen('x'),'|';try{\strlen('x');}catch(\Error $e){echo $e->getMessage(),'|';}"###,
        "47|Call to undefined function strlen()|",
        "",
    );
}

#[test]
fn disabled_predicates_7() {
    assert_cli(
        &["disable_functions=is_string,count,defined,intdiv"],
        r###"foreach(['is_string','count','defined','intdiv'] as $f){var_dump(function_exists($f));}try{is_string('abc');}catch(Error $e){echo $e->getMessage(),'|';}try{count([]);}catch(Error $e){echo $e->getMessage(),'|';}try{defined('PHP_VERSION');}catch(Error $e){echo $e->getMessage(),'|';}try{intdiv(6,2);}catch(Error $e){echo $e->getMessage(),'|';}"###,
        "bool(false)\nbool(false)\nbool(false)\nbool(false)\nCall to undefined function is_string()|Call to undefined function count()|Call to undefined function defined()|Call to undefined function intdiv()|",
        "",
    );
}

#[test]
fn replacement_predicates_8() {
    assert_cli(
        &["disable_functions=is_string,count,defined,intdiv"],
        r###"function is_string($x){return 31;}function count($x){return 37;}function defined($x){return 41;}function intdiv($a,$b){return 43;}echo is_string('x'),'|',count([]),'|',defined('PHP_VERSION'),'|',intdiv(6,2),'|';"###,
        "31|37|41|43|",
        "",
    );
}

#[test]
fn disabled_callback_intrinsics_9() {
    assert_cli(
        &["disable_functions=call_user_func,call_user_func_array"],
        r###"try{call_user_func(fn()=>17);}catch(Error $e){echo $e->getMessage(),'|';}try{call_user_func_array(fn()=>19,[]);}catch(Error $e){echo $e->getMessage(),'|';}"###,
        "Call to undefined function call_user_func()|Call to undefined function call_user_func_array()|",
        "",
    );
}

#[test]
fn replacement_ref_signature_10() {
    assert_cli(
        &["disable_functions=sort"],
        r###"function sort($arg){echo $arg,'|';}sort(23);"###,
        "23|",
        "",
    );
}

#[test]
fn replacement_reference_11() {
    assert_cli(
        &["disable_functions=strlen"],
        r###"function &strlen(&$x){return $x;}$x=29;$y=&strlen($x);$y=31;echo $x,'|';"###,
        "31|",
        "",
    );
}

#[test]
fn eval_replacement_ref_12() {
    assert_cli(
        &["disable_functions=sort"],
        r###"eval('function sort($arg){echo $arg,"|";}');sort(23);"###,
        "23|",
        "",
    );
}

#[test]
fn eval_disabled_13() {
    assert_cli(
        &["disable_functions=strlen"],
        r###"eval('try{strlen("abc");}catch(Error $e){echo $e->getMessage(),"|";}');"###,
        "Call to undefined function strlen()|",
        "",
    );
}

#[test]
fn argument_order_14() {
    assert_cli(
        &["disable_functions=strlen"],
        r###"function side(){echo 'side|';return 'abc';}try{strlen(side());}catch(Error $e){echo $e->getMessage(),'|';}"###,
        "Call to undefined function strlen()|",
        "",
    );
}

#[test]
fn negative_method_control_15() {
    assert_cli(
        &["disable_functions=Sample::strlen,strrev"],
        r###"class Sample{static function strlen($x){return 53;}}echo Sample::strlen('x'),'|',strlen('abc'),'|';"###,
        "53|3|",
        "",
    );
}

#[test]
fn first_class_disabled_16() {
    assert_cli(
        &["disable_functions=strlen"],
        r###"try{$f=strlen(...);}catch(Error $e){echo $e->getMessage(),'|';}"###,
        "Call to undefined function strlen()|",
        "",
    );
}

#[test]
fn readonly_startup_17() {
    assert_cli(
        &["disable_functions=strlen", "variables_order=G"],
        r###"var_dump(ini_set('disable_functions',''),ini_set('variables_order','EGPCS'),ini_get('disable_functions'),ini_get('variables_order'));"###,
        "bool(false)\nbool(false)\nstring(6) \"strlen\"\nstring(1) \"G\"\n",
        "",
    );
}

#[test]
fn globals_unset_18() {
    assert_cli(
        &["variables_order=EGPCS"],
        r###"echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|';unset($_ENV);echo (int)isset($_ENV),'|';unset($_SERVER);echo (int)isset($_SERVER),'|';"###,
        "1|0|0|",
        "",
    );
}

#[test]
fn globals_nested_19() {
    assert_cli(
        &["variables_order=G"],
        r###"function inspect(){echo count($_ENV),'|',count($_SERVER),'|';}inspect();class Sample{function __destruct(){echo count($_SERVER),'|';}}$o=new Sample;"###,
        "0|0|0|",
        "",
    );
}

#[test]
fn list__strlen__20() {
    assert_cli(
        &["disable_functions=STRLEN"],
        r###"var_dump(ini_get('disable_functions'),function_exists('strlen'),function_exists('count'));"###,
        "string(6) \"STRLEN\"\nbool(true)\nbool(true)\n",
        "",
    );
}

#[test]
fn list__strlen__21() {
    assert_cli(
        &["disable_functions=strlen"],
        r###"var_dump(ini_get('disable_functions'),function_exists('strlen'),function_exists('count'));"###,
        "string(6) \"strlen\"\nbool(false)\nbool(true)\n",
        "",
    );
}

#[test]
fn list___strlen___count___22() {
    assert_cli(
        &["disable_functions= strlen , COUNT "],
        r###"var_dump(ini_get('disable_functions'),function_exists('strlen'),function_exists('count'));"###,
        "string(16) \" strlen , COUNT \"\nbool(false)\nbool(true)\n",
        "",
    );
}

#[test]
fn list__strlen_count__23() {
    assert_cli(
        &["disable_functions=strlen count"],
        r###"var_dump(ini_get('disable_functions'),function_exists('strlen'),function_exists('count'));"###,
        "string(12) \"strlen count\"\nbool(false)\nbool(false)\n",
        "",
    );
}

#[test]
fn list__strlen_tcount__24() {
    assert_cli(
        &["disable_functions=strlen\tcount"],
        r###"var_dump(ini_get('disable_functions'),function_exists('strlen'),function_exists('count'));"###,
        "string(12) \"strlen\tcount\"\nbool(true)\nbool(true)\n",
        "",
    );
}

#[test]
fn list____strlen__25() {
    assert_cli(
        &["disable_functions=\\strlen"],
        r###"var_dump(ini_get('disable_functions'),function_exists('strlen'),function_exists('count'));"###,
        "string(7) \"\\strlen\"\nbool(true)\nbool(true)\n",
        "",
    );
}

#[test]
fn list__strlen__count__26() {
    assert_cli(
        &["disable_functions=strlen,,count"],
        r###"var_dump(ini_get('disable_functions'),function_exists('strlen'),function_exists('count'));"###,
        "string(13) \"strlen,,count\"\nbool(false)\nbool(false)\n",
        "",
    );
}

#[test]
fn list__exit_die_exit__27() {
    assert_cli(
        &["disable_functions=exit,die,exit"],
        r###"var_dump(ini_get('disable_functions'),function_exists('strlen'),function_exists('count'));"###,
        "\nWarning: Cannot disable function exit,die,exit() in Unknown on line 0\n\nWarning: Cannot disable function die,exit() in Unknown on line 0\n\nWarning: Cannot disable function exit() in Unknown on line 0\nstring(13) \"exit,die,exit\"\nbool(true)\nbool(true)\n",
        "",
    );
}

#[test]
fn list__exit_die__28() {
    assert_cli(
        &["disable_functions=EXIT,Die"],
        r###"var_dump(ini_get('disable_functions'),function_exists('strlen'),function_exists('count'));"###,
        "string(8) \"EXIT,Die\"\nbool(true)\nbool(true)\n",
        "",
    );
}

#[test]
fn assert__1_29() {
    assert_cli(
        &["disable_functions=assert", "zend.assertions=-1"],
        r###"try{assert(echo_arg());}catch(Throwable $e){echo get_class($e),':',$e->getMessage(),'|';}function echo_arg(){echo 'arg|';return false;}"###,
        "Error:Call to undefined function assert()|",
        "",
    );
}

#[test]
fn assert_0_30() {
    assert_cli(
        &["disable_functions=assert", "zend.assertions=0"],
        r###"try{assert(echo_arg());}catch(Throwable $e){echo get_class($e),':',$e->getMessage(),'|';}function echo_arg(){echo 'arg|';return false;}"###,
        "Error:Call to undefined function assert()|",
        "",
    );
}

#[test]
fn assert_1_31() {
    assert_cli(
        &["disable_functions=assert", "zend.assertions=1"],
        r###"try{assert(echo_arg());}catch(Throwable $e){echo get_class($e),':',$e->getMessage(),'|';}function echo_arg(){echo 'arg|';return false;}"###,
        "Error:Call to undefined function assert()|",
        "",
    );
}

#[test]
fn alias_target_32() {
    assert_cli(
        &["disable_functions=rtrim"],
        r###"var_dump(function_exists('rtrim'),function_exists('chop'));echo chop('x '),'|';$a=get_defined_functions();echo (int)in_array('chop',$a['internal']),'|';$r=new ReflectionFunction('chop');echo $r->getName(),'|';"###,
        "bool(false)\nbool(true)\nx|1|chop|",
        "",
    );
}

#[test]
fn alias_replace_33() {
    assert_cli(
        &["disable_functions=chop"],
        r###"function chop($x){return 27;}echo chop('x '),'|',rtrim('x '),'|';$a=get_defined_functions();echo (int)in_array('chop',$a['internal']),'|',(int)in_array('chop',$a['user']),'|';"###,
        "27|x|0|1|",
        "",
    );
}

#[test]
fn case_name_34() {
    assert_cli(
        &["DISABLE_FUNCTIONS=strlen", "VARIABLES_ORDER=G"],
        r###"var_dump(ini_get('disable_functions'),ini_get('variables_order'),function_exists('strlen'));echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|';"###,
        "string(0) \"\"\nstring(5) \"EGPCS\"\nbool(true)\n1|",
        "",
    );
}

#[test]
fn order__g__35() {
    assert_cli(
        &["variables_order=G"],
        r###"echo ini_get('variables_order'),'|';echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['PHP_SELF']),'|',count($_SERVER)?'some':'empty';"###,
        "G|0|0|0|empty",
        "",
    );
}

#[test]
fn order__e__36() {
    assert_cli(
        &["variables_order=e"],
        r###"echo ini_get('variables_order'),'|';echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['PHP_SELF']),'|',count($_SERVER)?'some':'empty';"###,
        "e|1|0|0|empty",
        "",
    );
}

#[test]
fn order__s__37() {
    assert_cli(
        &["variables_order=s"],
        r###"echo ini_get('variables_order'),'|';echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['PHP_SELF']),'|',count($_SERVER)?'some':'empty';"###,
        "s|0|1|1|some",
        "",
    );
}

#[test]
fn order__gp__38() {
    assert_cli(
        &["variables_order=GP"],
        r###"echo ini_get('variables_order'),'|';echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['PHP_SELF']),'|',count($_SERVER)?'some':'empty';"###,
        "GP|0|0|0|empty",
        "",
    );
}

#[test]
fn order_____39() {
    assert_cli(
        &["variables_order= "],
        r###"echo ini_get('variables_order'),'|';echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['PHP_SELF']),'|',count($_SERVER)?'some':'empty';"###,
        " |0|0|0|empty",
        "",
    );
}

#[test]
fn order__off__40() {
    assert_cli(
        &["variables_order=off"],
        r###"echo ini_get('variables_order'),'|';echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['PHP_SELF']),'|',count($_SERVER)?'some':'empty';"###,
        "EGPCS|1|1|1|some",
        "",
    );
}

#[test]
fn order__false__41() {
    assert_cli(
        &["variables_order=false"],
        r###"echo ini_get('variables_order'),'|';echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['PHP_SELF']),'|',count($_SERVER)?'some':'empty';"###,
        "EGPCS|1|1|1|some",
        "",
    );
}

#[test]
fn order__none__42() {
    assert_cli(
        &["variables_order=none"],
        r###"echo ini_get('variables_order'),'|';echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['PHP_SELF']),'|',count($_SERVER)?'some':'empty';"###,
        "EGPCS|1|1|1|some",
        "",
    );
}

#[test]
fn order__0__43() {
    assert_cli(
        &["variables_order=0"],
        r###"echo ini_get('variables_order'),'|';echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['PHP_SELF']),'|',count($_SERVER)?'some':'empty';"###,
        "0|0|0|0|empty",
        "",
    );
}

#[test]
fn order__1__44() {
    assert_cli(
        &["variables_order=1"],
        r###"echo ini_get('variables_order'),'|';echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['PHP_SELF']),'|',count($_SERVER)?'some':'empty';"###,
        "1|0|0|0|empty",
        "",
    );
}

#[test]
fn order__pse__45() {
    assert_cli(
        &["variables_order=pSe"],
        r###"echo ini_get('variables_order'),'|';echo (int)isset($_ENV['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['RPHP_STARTUP_ORACLE']),'|',(int)isset($_SERVER['PHP_SELF']),'|',count($_SERVER)?'some':'empty';"###,
        "pSe|1|1|1|some",
        "",
    );
}

#[test]
fn warning_display_errors_0_46() {
    assert_cli(
        &["disable_functions=exit", "display_errors=0"],
        r###"echo 'body|';"###,
        "body|",
        "PHP Warning:  Cannot disable function exit() in Unknown on line 0\n",
    );
}

#[test]
fn warning_display_startup_errors_0_47() {
    assert_cli(
        &["disable_functions=exit", "display_startup_errors=0"],
        r###"echo 'body|';"###,
        "body|",
        "PHP Warning:  Cannot disable function exit() in Unknown on line 0\n",
    );
}

#[test]
fn warning_display_errors_stderr_48() {
    assert_cli(
        &["disable_functions=exit", "display_errors=stderr"],
        r###"echo 'body|';"###,
        "body|",
        "Warning: Cannot disable function exit() in Unknown on line 0\n",
    );
}

#[test]
fn warning_log_errors_1_49() {
    assert_cli(
        &["disable_functions=exit", "log_errors=1"],
        r###"echo 'body|';"###,
        "\nWarning: Cannot disable function exit() in Unknown on line 0\nbody|",
        "PHP Warning:  Cannot disable function exit() in Unknown on line 0\n",
    );
}

#[test]
fn warning_error_reporting_0_50() {
    assert_cli(
        &["disable_functions=exit", "error_reporting=0"],
        r###"echo 'body|';"###,
        "body|",
        "",
    );
}
