//! Original PHP 8.5 request INI/include-path state contracts.
use std::process::Command;

#[cfg(feature = "include-path")]
#[test]
fn include_resolution_observes_every_writer_and_startup_restore() {
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("rphp-ini-path-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let fixture = Fixture(root);
    let first = fixture.0.join("first");
    let second = fixture.0.join("second");
    for (directory, text) in [(&first, "first"), (&second, "second")] {
        std::fs::create_dir(directory).unwrap();
        std::fs::write(
            directory.join("probe.php"),
            format!("<?php echo '{text};';"),
        )
        .unwrap();
    }
    let php_path = |path: &std::path::Path| {
        path.to_str()
            .unwrap()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    };
    let source = format!(
        "include 'probe.php';ini_set('include_path','{}');include 'probe.php';set_include_path('{}');include 'probe.php';ini_set('include_path','{}');ini_restore('include_path');include 'probe.php';",
        php_path(&second),
        php_path(&first),
        php_path(&second),
    );
    let setting = format!("include_path={}", first.display());
    check_settings(
        &source,
        0,
        b"first;second;first;first;",
        b"",
        &["-d", &setting],
    );
}
#[test]
fn case_precision() {
    check(
        "var_dump(ini_get('Precision'),ini_set('Precision',3));ini_restore('Precision');var_dump(ini_get('precision'));",
        0,
        b"bool(false)\nbool(false)\nstring(2) \"14\"\n",
        b"",
    );
}

#[test]
fn alias_type() {
    check(
        "try{ini_alter('include_path',[]);}catch(Throwable $e){echo $e->getMessage();}",
        0,
        b"ini_alter(): Argument #2 ($value) must be of type string|int|float|bool|null",
        b"",
    );
}

#[test]
fn no_stringable_value() {
    check("class Text{function __toString(){echo 'converted';return 'changed';}}foreach(['ini_set','ini_alter'] as $f){try{$f('include_path',new Text);}catch(Throwable $e){echo $e->getMessage(),\"\\n\";}}var_dump(ini_get('include_path'));", 0, b"ini_set(): Argument #2 ($value) must be of type string|int|float|bool|null\nini_alter(): Argument #2 ($value) must be of type string|int|float|bool|null\nstring(4) \"seed\"\n", b"");
}

#[cfg(feature = "include-path")]
#[test]
fn strict_option() {
    check("declare(strict_types=1);foreach(['ini_get','ini_restore','set_include_path'] as $f){try{$f(17);}catch(Throwable $e){echo $e->getMessage(),\"\\n\";}}", 0, b"ini_get(): Argument #1 ($option) must be of type string, int given\nini_restore(): Argument #1 ($option) must be of type string, int given\nset_include_path(): Argument #1 ($include_path) must be of type string, int given\n", b"");
}

#[cfg(feature = "include-path")]
#[test]
fn typed_metadata() {
    check("foreach(['ini_get','ini_set','ini_alter','ini_restore','get_include_path','set_include_path']as $n){$f=new ReflectionFunction($n);echo $n,':';foreach($f->getParameters() as $p)echo $p->getName(),'=',(string)$p->getType(),';';echo '=>',(string)$f->getReturnType(),\"\\n\";}", 0, b"ini_get:option=string;=>string|false\nini_set:option=string;value=string|int|float|bool|null;=>string|false\nini_alter:option=string;value=string|int|float|bool|null;=>string|false\nini_restore:option=string;=>void\nget_include_path:=>string|false\nset_include_path:include_path=string;=>string|false\n", b"");
}

#[cfg(feature = "include-path")]
#[test]
fn nul_path_rejection() {
    check("try{set_include_path(\"a\\0b\");}catch(Throwable $e){echo $e->getMessage(),\"\\n\";}var_dump(ini_get('include_path'));", 0, b"set_include_path(): Argument #1 ($include_path) must not contain any null bytes\nstring(4) \"seed\"\n", b"");
}

#[test]
fn error_mask() {
    check("foreach(['E_ALL','2junk','0x10','-1','1+2','on','1e3','9999999999999999999999','-9999999999999999999999'] as $v){var_dump(ini_set('error_reporting',$v),ini_get('error_reporting'),error_reporting());}error_reporting(7);var_dump(ini_get('error_reporting'));ini_restore('error_reporting');var_dump(error_reporting(),ini_get('error_reporting'));", 0, b"string(5) \"30719\"\nstring(5) \"E_ALL\"\nint(0)\nstring(5) \"E_ALL\"\nstring(5) \"2junk\"\nint(2)\nstring(5) \"2junk\"\nstring(4) \"0x10\"\nint(0)\nstring(4) \"0x10\"\nstring(2) \"-1\"\nint(-1)\nstring(2) \"-1\"\nstring(3) \"1+2\"\nint(1)\nstring(3) \"1+2\"\nstring(2) \"on\"\nint(0)\nstring(2) \"on\"\nstring(3) \"1e3\"\nint(1)\nstring(3) \"1e3\"\nstring(22) \"9999999999999999999999\"\nint(-1)\nstring(22) \"9999999999999999999999\"\nstring(23) \"-9999999999999999999999\"\nint(0)\nstring(1) \"7\"\nint(30719)\nstring(5) \"30719\"\n", b"");
}

#[cfg(feature = "include-path")]
#[test]
fn restore_repeat() {
    check(
        "ini_set('include_path','one');ini_restore('include_path');ini_set('include_path','two');ini_restore('include_path');ini_restore('include_path');var_dump(get_include_path());",
        0,
        b"string(4) \"seed\"\n",
        b"",
    );
}

#[test]
fn restore_rejected() {
    check(
        "var_dump(ini_set('precision','-2'),ini_get('precision'));ini_set('precision',3);ini_restore('precision');var_dump(ini_get('precision'));",
        0,
        b"bool(false)\nstring(2) \"14\"\nstring(2) \"14\"\n",
        b"",
    );
}

#[test]
fn reference_value() {
    check(
        "$value='first';$alias=&$value;ini_set('include_path',$alias);$value='second';var_dump(ini_get('include_path'),$alias);ini_restore('include_path');var_dump($value);",
        0,
        b"string(5) \"first\"\nstring(6) \"second\"\nstring(6) \"second\"\n",
        b"",
    );
}

#[test]
fn unconfigured_restore() {
    check_settings(
        "ini_set('precision',3);ini_set('serialize_precision',2);gc_disable();foreach(['precision','serialize_precision','zend.enable_gc']as $n){ini_restore($n);var_dump(ini_get($n));}",
        0,
        b"string(2) \"14\"\nstring(2) \"-1\"\nstring(1) \"1\"\n",
        b"",
        &[],
    );
}

#[test]
fn custom_restore() {
    check_settings(
        "ini_set('precision',3);gc_enable();ini_restore('precision');ini_restore('zend.enable_gc');var_dump(ini_get('precision'),gc_enabled(),ini_get('zend.enable_gc'));",
        0,
        b"string(1) \"7\"\nbool(false)\nstring(0) \"\"\n",
        b"",
        &["-d", "precision=7", "-d", "zend.enable_gc=off"],
    );
}

#[test]
fn startup_last_empty() {
    check_settings(
        "$initial=ini_get('include_path');var_dump($initial==='seed');ini_set('include_path','runtime');ini_restore('include_path');var_dump(ini_get('include_path')===$initial);",
        0,
        b"bool(false)\nbool(true)\n",
        b"",
        &["-d", "include_path=seed", "-d", "include_path="],
    );
}

fn check(source: &str, status: i32, stdout: &[u8], stderr: &[u8]) {
    check_settings(
        source,
        status,
        stdout,
        stderr,
        &[
            "-d",
            "error_reporting=E_ALL",
            "-d",
            "include_path=seed",
            "-d",
            "precision=14",
        ],
    );
}

fn check_settings(source: &str, status: i32, stdout: &[u8], stderr: &[u8], settings: &[&str]) {
    let mut binaries = vec![env!("CARGO_BIN_EXE_rphp").to_string()];
    if let Ok(reference) = std::env::var("RPHP_REFERENCE_PHP") {
        binaries.push(reference);
    }
    for binary in binaries {
        let output = Command::new(&binary)
            .args(["-n", "-d", "display_errors=1", "-d", "log_errors=0"])
            .args(settings)
            .args(["-r", source])
            .output()
            .expect("execute original INI specimen");
        assert_eq!(output.status.code(), Some(status), "{binary}: {output:?}");
        assert_eq!(output.stdout, stdout, "{binary}");
        assert_eq!(output.stderr, stderr, "{binary}");
    }
}

#[cfg(feature = "include-path")]
#[test]
fn shared_writers() {
    check("var_dump(ini_get('include_path'),get_include_path());var_dump(ini_set('include_path','first'),get_include_path());var_dump(set_include_path('second'),ini_get('include_path'));ini_restore('include_path');var_dump(ini_get('include_path'),get_include_path());", 0, b"string(4) \"seed\"\nstring(4) \"seed\"\nstring(4) \"seed\"\nstring(5) \"first\"\nstring(5) \"first\"\nstring(6) \"second\"\nstring(4) \"seed\"\nstring(4) \"seed\"\n", b"");
}

#[test]
fn suppression_keeps_explicit_runtime_ini_mask_without_startup_override() {
    check_settings(
        "error_reporting(7);var_dump(ini_get('error_reporting'),@ini_get('error_reporting'),ini_get('error_reporting'));ini_restore('error_reporting');var_dump(ini_get('error_reporting')===@ini_get('error_reporting'));",
        0,
        b"string(1) \"7\"\nstring(1) \"7\"\nstring(1) \"7\"\nbool(true)\n",
        b"",
        &[],
    );
}

#[test]
fn suppression_keeps_unconfigured_ini_mask_without_materializing_a_setter() {
    check_settings(
        "$before=ini_get('error_reporting');var_dump($before===@ini_get('error_reporting'),$before===ini_get('error_reporting'));",
        0,
        b"bool(true)\nbool(true)\n",
        b"",
        &[],
    );
}

#[cfg(feature = "include-path")]
#[test]
fn empty_writers() {
    check(
        "var_dump(ini_set('include_path',''),get_include_path());var_dump(set_include_path(''),ini_get('include_path'));ini_restore('include_path');var_dump(get_include_path());",
        0,
        b"bool(false)\nstring(4) \"seed\"\nbool(false)\nstring(4) \"seed\"\nstring(4) \"seed\"\n",
        b"",
    );
}

#[test]
fn nul_ini() {
    check(
        "var_dump(ini_set('include_path',\"a\\0b\"),ini_get('include_path'));",
        0,
        b"string(4) \"seed\"\nstring(3) \"a\x00b\"\n",
        b"",
    );
}

#[test]
fn case_sensitive() {
    check("foreach(['include_path','INCLUDE_PATH','Include_Path']as $key){var_dump(ini_get($key),ini_set($key,'changed'),ini_restore($key),ini_get($key));}", 0, b"string(4) \"seed\"\nstring(4) \"seed\"\nNULL\nstring(4) \"seed\"\nbool(false)\nbool(false)\nNULL\nbool(false)\nbool(false)\nbool(false)\nNULL\nbool(false)\n", b"");
}

#[test]
fn alias() {
    check(
        "var_dump(ini_alter('include_path','first'),ini_get('include_path'));ini_restore('include_path');var_dump(ini_get('include_path'));",
        0,
        b"string(4) \"seed\"\nstring(5) \"first\"\nstring(4) \"seed\"\n",
        b"",
    );
}

#[test]
fn restore_unknown() {
    check(
        "var_dump(ini_restore('missing_option'));",
        0,
        b"NULL\n",
        b"",
    );
}

#[test]
fn restore_diagnostics() {
    check(
        "ini_set('display_errors','0');ini_restore('display_errors');var_dump(ini_get('display_errors'));ini_set('ignore_repeated_errors','1');ini_restore('ignore_repeated_errors');var_dump(ini_get('ignore_repeated_errors'));",
        0,
        b"string(1) \"1\"\nstring(1) \"0\"\n",
        b"",
    );
}

#[test]
fn restore_error_level() {
    check(
        "ini_set('error_reporting','1');ini_restore('error_reporting');var_dump(error_reporting(),ini_get('error_reporting'));",
        0,
        b"int(30719)\nstring(5) \"30719\"\n",
        b"",
    );
}

#[test]
fn restore_precision() {
    check(
        "ini_set('precision','3');ini_restore('precision');var_dump(ini_get('precision'));",
        0,
        b"string(2) \"14\"\n",
        b"",
    );
}

#[test]
fn restore_gc() {
    check(
        "gc_disable();ini_restore('zend.enable_gc');var_dump(gc_enabled(),ini_get('zend.enable_gc'));",
        0,
        b"bool(true)\nstring(1) \"1\"\n",
        b"",
    );
}

#[cfg(feature = "include-path")]
#[test]
fn set_argument_types() {
    check("foreach([null,false,true,17,1.5,[],new stdClass] as $v){try{var_dump(ini_set('include_path',$v),get_include_path());}catch(Throwable $e){echo get_class($e),':',$e->getMessage(),\"\n\";}}", 0, b"bool(false)\nstring(4) \"seed\"\nbool(false)\nstring(4) \"seed\"\nstring(4) \"seed\"\nstring(1) \"1\"\nstring(1) \"1\"\nstring(2) \"17\"\nstring(2) \"17\"\nstring(3) \"1.5\"\nTypeError:ini_set(): Argument #2 ($value) must be of type string|int|float|bool|null\nTypeError:ini_set(): Argument #2 ($value) must be of type string|int|float|bool|null\n", b"");
}

#[cfg(feature = "include-path")]
#[test]
fn set_path_types() {
    check("foreach([null,false,true,17,1.5,[],new stdClass] as $v){try{var_dump(set_include_path($v),ini_get('include_path'));}catch(Throwable $e){echo get_class($e),':',$e->getMessage(),\"\n\";}}", 0, b"\nDeprecated: set_include_path(): Passing null to parameter #1 ($include_path) of type string is deprecated in Command line code on line 1\nbool(false)\nstring(4) \"seed\"\nbool(false)\nstring(4) \"seed\"\nstring(4) \"seed\"\nstring(1) \"1\"\nstring(1) \"1\"\nstring(2) \"17\"\nstring(2) \"17\"\nstring(3) \"1.5\"\nTypeError:set_include_path(): Argument #1 ($include_path) must be of type string, array given\nTypeError:set_include_path(): Argument #1 ($include_path) must be of type string, stdClass given\n", b"");
}

#[test]
fn restore_types() {
    check("foreach([null,false,true,17,[],new stdClass] as $v){try{var_dump(ini_restore($v));}catch(Throwable $e){echo get_class($e),':',$e->getMessage(),\"\n\";}}", 0, b"\nDeprecated: ini_restore(): Passing null to parameter #1 ($option) of type string is deprecated in Command line code on line 1\nNULL\nNULL\nNULL\nNULL\nTypeError:ini_restore(): Argument #1 ($option) must be of type string, array given\nTypeError:ini_restore(): Argument #1 ($option) must be of type string, stdClass given\n", b"");
}

#[cfg(feature = "include-path")]
#[test]
fn metadata() {
    check("foreach(['ini_get','ini_set','ini_alter','ini_restore','get_include_path','set_include_path'] as $n){$f=new ReflectionFunction($n);echo $f->getName(),':',$f->getNumberOfParameters(),':',$f->getNumberOfRequiredParameters(),\";\";}", 0, b"ini_get:1:1;ini_set:2:2;ini_alter:2:2;ini_restore:1:1;get_include_path:0:0;set_include_path:1:1;", b"");
}

#[test]
fn strict_value() {
    check("declare(strict_types=1);foreach([null,false,true,17,1.5,[],new stdClass] as $v){try{var_dump(ini_set('include_path',$v));}catch(Throwable $e){echo get_class($e),':',$e->getMessage(),\"\n\";}}", 0, b"bool(false)\nbool(false)\nstring(4) \"seed\"\nstring(1) \"1\"\nstring(2) \"17\"\nTypeError:ini_set(): Argument #2 ($value) must be of type string|int|float|bool|null\nTypeError:ini_set(): Argument #2 ($value) must be of type string|int|float|bool|null\n", b"");
}
