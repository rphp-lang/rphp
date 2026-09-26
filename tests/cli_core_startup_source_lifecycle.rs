//! Original PHP 8.5 prepend/main/append request-lifecycle contracts.
use std::process::Command;

struct Fixture(std::path::PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn check(mode: &str, pre: Option<&str>, main: &str, post: Option<&str>, exit: i32, stdout: &str) {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let fixture = Fixture(
        std::env::temp_dir().join(format!("rphp-startup-units-{}-{nonce}", std::process::id())),
    );
    std::fs::create_dir(&fixture.0).unwrap();
    for (name, source) in [
        ("pre.php", pre),
        ("main.php", Some(main)),
        ("post.php", post),
    ] {
        if let Some(source) = source {
            std::fs::write(fixture.0.join(name), source).unwrap();
        }
    }
    let include_path = format!("include_path={}", fixture.0.display());
    let expected_binary =
        std::env::var_os("RPHP_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_rphp").into());
    let mut binaries = vec![expected_binary];
    if let Some(reference) = std::env::var_os("RPHP_REFERENCE_PHP") {
        binaries.push(reference);
    }
    for binary in binaries {
        let mut command = Command::new(&binary);
        command.current_dir(&fixture.0).args([
            "-n",
            "-d",
            "display_errors=1",
            "-d",
            "log_errors=0",
            "-d",
            "fatal_error_backtraces=0",
            "-d",
            &include_path,
            "-d",
            if mode == "append-only" {
                "auto_prepend_file="
            } else if mode == "none" {
                "auto_prepend_file=none"
            } else {
                "auto_prepend_file=pre.php"
            },
            "-d",
            if mode == "prepend-only" {
                "auto_append_file="
            } else if mode == "none" {
                "auto_append_file=none"
            } else {
                "auto_append_file=post.php"
            },
        ]);
        if mode == "inline" {
            command.args(["-r", main]);
        } else if mode != "stdin" {
            command.arg(fixture.0.join("main.php"));
        }
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().unwrap();
        if mode == "stdin" {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(main.as_bytes())
                .unwrap();
        }
        drop(child.stdin.take());
        let output = child.wait_with_output().unwrap();
        let normalize = |bytes: &[u8]| {
            String::from_utf8(bytes.to_vec())
                .unwrap()
                .replace(fixture.0.to_str().unwrap(), "<DIR>")
        };
        assert_eq!(
            output.status.code(),
            Some(exit),
            "{binary:?}: status={} stderr={}",
            output.status,
            normalize(&output.stderr)
        );
        assert_eq!(normalize(&output.stdout), stdout, "{binary:?}");
        assert_eq!(normalize(&output.stderr), "", "{binary:?}");
        // One oracle deliberately rewrites the primary file before compilation.
        std::fs::write(fixture.0.join("main.php"), main).unwrap();
    }
}

macro_rules! contract {
    ($name:ident, $pre:expr, $main:expr, $post:expr, $exit:expr, $out:expr) => {
        #[test]
        fn $name() {
            check("file", Some($pre), $main, Some($post), $exit, $out);
        }
    };
}

contract!(
    compile_each_unit_only_at_its_execution_phase,
    "<?php echo 'pre:';var_dump(function_exists('body_fn'));$data='P';",
    "<?php function body_fn(){} echo \"body:$data;\";$data.='M';",
    "<?php echo \"post:$data;\";",
    0,
    "pre:bool(false)\nbody:P;post:PM;"
);

contract!(
    included_file_identity_order,
    "<?php foreach(get_included_files() as $f)echo basename($f),';';echo '|';",
    "<?php foreach(get_included_files() as $f)echo basename($f),';';echo '|';",
    "<?php foreach(get_included_files() as $f)echo basename($f),';';",
    0,
    "main.php;pre.php;|main.php;pre.php;|main.php;pre.php;post.php;"
);

contract!(
    return_from_prepend_continues_request,
    "<?php echo 'pre;';return 7;echo 'BAD';",
    "<?php echo 'body;';",
    "<?php echo 'post;';",
    0,
    "pre;body;post;"
);
contract!(
    return_from_main_continues_append,
    "<?php echo 'pre;';",
    "<?php echo 'body;';return 9;echo 'BAD';",
    "<?php echo 'post;';",
    0,
    "pre;body;post;"
);
contract!(
    exit_from_prepend_runs_only_shutdown,
    "<?php register_shutdown_function(function(){echo 'end;';});echo 'pre;';exit(3);",
    "<?php echo 'BAD';",
    "<?php echo 'BAD';",
    3,
    "pre;end;"
);
contract!(
    exit_from_main_skips_append,
    "<?php register_shutdown_function(function(){echo 'end;';});echo 'pre;';",
    "<?php echo 'body;';exit(4);",
    "<?php echo 'BAD';",
    4,
    "pre;body;end;"
);
contract!(
    exit_from_append_keeps_its_status,
    "<?php register_shutdown_function(function(){echo 'end;';});",
    "<?php echo 'body;';",
    "<?php echo 'post;';exit(5);",
    5,
    "body;post;end;"
);
contract!(
    handled_main_exception_continues_append,
    "<?php set_exception_handler(function($e){echo 'caught:'.$e->getMessage().';';});",
    "<?php throw new Exception('body');",
    "<?php echo 'post;';",
    0,
    "caught:body;post;"
);
contract!(
    handled_prepend_exception_continues_main,
    "<?php set_exception_handler(function($e){echo 'caught:'.$e->getMessage().';';});throw new Exception('pre');",
    "<?php echo 'body;';",
    "<?php echo 'post;';",
    0,
    "caught:pre;body;post;"
);
contract!(
    references_survive_all_units_and_shutdown,
    "<?php $a=2;$b=&$a;register_shutdown_function(function(){global $a,$b;echo \"end:$a/$b;\";});",
    "<?php $b=9;echo \"body:$a;\";",
    "<?php $a=11;echo \"post:$b;\";",
    0,
    "body:9;post:11;end:11/11;"
);
contract!(
    require_once_recognizes_prepend,
    "<?php echo 'pre;';",
    "<?php require_once __DIR__.'/pre.php';echo 'body;';",
    "<?php echo 'post;';",
    0,
    "pre;body;post;"
);
contract!(
    objects_retire_after_append_and_shutdown,
    "<?php class Item{function __destruct(){echo 'destroy;';}}$item=new Item;register_shutdown_function(function(){echo 'end;';});",
    "<?php echo 'body;';",
    "<?php echo 'post;';",
    0,
    "body;post;end;destroy;"
);
contract!(
    one_final_output_handler_phase,
    "<?php ob_start(function($s){return '['.$s.']';});register_shutdown_function(function(){echo 'end;';});echo 'pre;';",
    "<?php echo 'body;';",
    "<?php echo 'post;';",
    0,
    "[pre;body;post;end;]"
);
contract!(
    startup_ini_is_published_but_not_runtime_writable,
    "<?php echo basename(ini_get('auto_prepend_file')),';';var_dump(ini_set('auto_append_file',''));",
    "<?php echo 'body;';",
    "<?php echo 'post;';",
    0,
    "pre.php;bool(false)\nbody;post;"
);
contract!(
    main_parse_failure_keeps_prepend_shutdown,
    "<?php echo 'pre;';register_shutdown_function(function(){echo 'end;';});",
    "<?php if (true) {",
    "<?php echo 'BAD';",
    255,
    "pre;\nParse error: Unclosed '{' in <DIR>/main.php on line 1\nend;"
);
contract!(
    primary_parse_error_is_not_a_catchable_include_error,
    "<?php set_exception_handler(function($e){echo 'caught:'.get_class($e).';';});register_shutdown_function(function(){echo 'end;';});",
    "<?php if (true) {",
    "<?php echo 'BAD';",
    255,
    "\nParse error: Unclosed '{' in <DIR>/main.php on line 1\nend;"
);
contract!(
    main_is_read_after_prepend,
    "<?php file_put_contents(__DIR__.'/main.php',\"<?php echo 'new;';\");",
    "<?php echo 'old;';",
    "<?php echo 'post;';",
    0,
    "new;post;"
);
contract!(
    replacing_main_path_does_not_replace_the_open_program,
    "<?php rename(__DIR__.'/main.php',__DIR__.'/old.php');file_put_contents(__DIR__.'/main.php',\"<?php echo 'replacement;';\");",
    "<?php echo 'original;';",
    "<?php echo 'post;';",
    0,
    "original;post;"
);
contract!(
    unlinked_main_file_remains_executable,
    "<?php unlink(__DIR__.'/main.php');",
    "<?php echo 'original;',basename(__FILE__),';';",
    "<?php echo 'post;';",
    0,
    "original;main.php;post;"
);
contract!(
    unhandled_exception_keeps_fatal_then_shutdown,
    "<?php register_shutdown_function(function(){echo 'end;';});",
    "<?php throw new Exception('stop');",
    "<?php echo 'BAD';",
    255,
    "\nFatal error: Uncaught Exception: stop in <DIR>/main.php:1\nStack trace:\n#0 {main}\n  thrown in <DIR>/main.php on line 1\nend;"
);

#[test]
fn inline_cli_ignores_startup_units() {
    check(
        "inline",
        Some("<?php echo 'BAD';"),
        "echo 'inline;';",
        Some("<?php echo 'BAD';"),
        0,
        "inline;",
    );
}

contract!(
    dynamic_globals_share_the_request_symbol_table,
    "<?php $key='shared';$$key=4;",
    "<?php echo \"$shared;\";${'shared'}=9;",
    "<?php echo \"$shared;\";",
    0,
    "4;9;"
);
contract!(
    array_snapshots_keep_copy_on_write_across_units,
    "<?php $a=[1];$b=$a;",
    "<?php $a[0]=7;echo $b[0],';';",
    "<?php echo $a[0],';',$b[0],';';",
    0,
    "1;7;1;"
);
contract!(
    prepend_symbols_are_available_during_main_compilation,
    "<?php define('STARTUP_VALUE',17);function source_value(){return 23;}",
    "<?php const MAIN_VALUE=STARTUP_VALUE;echo MAIN_VALUE,';',source_value(),';';",
    "<?php echo MAIN_VALUE,';';",
    0,
    "17;23;17;"
);
contract!(
    unset_does_not_resurrect_previous_unit_globals,
    "<?php $x=1;$y=2;",
    "<?php unset($x);$y=3;",
    "<?php var_dump(isset($x));echo \"$y;\";",
    0,
    "bool(false)\n3;"
);
contract!(
    shutdown_callbacks_from_all_units_keep_registration_order,
    "<?php register_shutdown_function(function(){echo 'pre-end;';});",
    "<?php register_shutdown_function(function(){echo 'main-end;';});",
    "<?php register_shutdown_function(function(){echo 'post-end;';});echo 'body-end;';",
    0,
    "body-end;pre-end;main-end;post-end;"
);
contract!(
    throwing_exception_handler_is_not_dispatched_twice,
    "<?php set_exception_handler(function($e){echo 'handler;';throw new Exception('replacement');});",
    "<?php throw new Exception('first');",
    "<?php echo 'BAD';",
    255,
    "handler;\nFatal error: Uncaught Exception: replacement in <DIR>/pre.php:1\nStack trace:\n#0 [internal function]: {closure:<DIR>/pre.php:1}(Object(Exception))\n#1 {main}\n  thrown in <DIR>/pre.php on line 1\n"
);

#[test]
fn missing_append_warning_uses_the_user_handler() {
    check(
        "file",
        Some(
            "<?php set_error_handler(function($n,$m){echo 'warning-handler;';return true;});register_shutdown_function(function(){echo 'end;';});",
        ),
        "<?php echo 'body;';",
        None,
        255,
        "body;warning-handler;\nFatal error: Failed opening required 'post.php' (include_path='<DIR>') in Unknown on line 0\nend;",
    );
}

#[test]
fn missing_append_preserves_a_throwing_warning_handler() {
    check(
        "file",
        Some(
            "<?php set_error_handler(function(){throw new Exception('open');});register_shutdown_function(function(){echo 'end;';});",
        ),
        "<?php echo 'body;';",
        None,
        255,
        "body;\nFatal error: Uncaught Exception: open in <DIR>/pre.php:1\nStack trace:\n#0 [internal function]: {closure:<DIR>/pre.php:1}(2, 'Unknown: Failed...', 'Unknown', 0)\n#1 {main}\n  thrown in <DIR>/pre.php on line 1\nend;",
    );
}

#[test]
fn handled_open_exception_cannot_make_required_append_optional() {
    check(
        "file",
        Some(
            "<?php set_error_handler(function(){throw new Exception('open');});set_exception_handler(function($e){echo 'caught:'.$e->getMessage().';';});register_shutdown_function(function(){echo 'end;';});",
        ),
        "<?php echo 'body;';",
        None,
        255,
        "body;caught:open;\nFatal error: Failed opening required 'post.php' (include_path='<DIR>') in Unknown on line 0\nend;",
    );
}
#[test]
fn stdin_cli_uses_startup_units() {
    check(
        "stdin",
        Some("<?php $x='P';echo 'pre;';"),
        "<?php echo \"stdin:$x;\";",
        Some("<?php echo 'post;';"),
        0,
        "pre;stdin:P;post;",
    );
}
#[test]
fn none_disables_startup_units() {
    check(
        "none",
        Some("<?php echo 'BAD';"),
        "<?php echo 'body;';",
        Some("<?php echo 'BAD';"),
        0,
        "body;",
    );
}
#[test]
fn missing_prepend_prevents_main_and_append() {
    check(
        "file",
        None,
        "<?php echo 'BAD';",
        Some("<?php echo 'BAD';"),
        255,
        "\nWarning: Unknown: Failed to open stream: No such file or directory in Unknown on line 0\n\nFatal error: Failed opening required 'pre.php' (include_path='<DIR>') in Unknown on line 0\n",
    );
}
#[test]
fn missing_append_still_runs_shutdown() {
    check(
        "file",
        Some("<?php echo 'pre;';register_shutdown_function(function(){echo 'end;';});"),
        "<?php echo 'body;';",
        None,
        255,
        "pre;body;\nWarning: Unknown: Failed to open stream: No such file or directory in Unknown on line 0\n\nFatal error: Failed opening required 'post.php' (include_path='<DIR>') in Unknown on line 0\nend;",
    );
}

#[test]
fn prepend_without_append_still_uses_one_request() {
    check(
        "prepend-only",
        Some("<?php $shared=3;echo 'pre;';"),
        "<?php echo $shared,';';",
        Some("<?php echo 'BAD';"),
        0,
        "pre;3;",
    );
}

#[test]
fn append_without_prepend_sees_main_scope() {
    check(
        "append-only",
        Some("<?php echo 'BAD';"),
        "<?php $shared=5;echo 'main;';",
        Some("<?php echo $shared,';';"),
        0,
        "main;5;",
    );
}

#[test]
fn missing_required_file_publishes_terminal_error_state_to_shutdown() {
    check(
        "file",
        Some(
            "<?php register_shutdown_function(function(){$e=error_get_last();echo 'last:',$e['type'],':',$e['file'],':',$e['line'],';';});",
        ),
        "<?php echo 'main;';",
        None,
        255,
        "main;\nWarning: Unknown: Failed to open stream: No such file or directory in Unknown on line 0\n\nFatal error: Failed opening required 'post.php' (include_path='<DIR>') in Unknown on line 0\nlast:1:Unknown:0;",
    );
}

contract!(
    primary_parse_failure_publishes_error_state_before_shutdown,
    "<?php register_shutdown_function(function(){$e=error_get_last();echo 'last:',$e['type'],':',basename($e['file']),':',$e['line'],';';});",
    "<?php if (true) {",
    "<?php echo 'BAD';",
    255,
    "\nParse error: Unclosed '{' in <DIR>/main.php on line 1\nlast:4:main.php:1;"
);
