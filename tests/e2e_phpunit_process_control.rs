mod common;

use common::run_php;
#[cfg(feature = "stream-contents")]
use std::process::Command;

#[test]
fn proc_open_and_proc_close_match_php_85_reflection_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['proc_open', 'proc_close'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':';
    var_export($function->hasReturnType() ? (string) $function->getReturnType() : null);
    echo ':', $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), '|';
    foreach ($function->getParameters() as $parameter) {
        echo $parameter->getName(), ':', $parameter->isPassedByReference() ? '&' : '-', ':';
        echo $parameter->hasType() ? (string) $parameter->getType() : '-', ':';
        if ($parameter->isDefaultValueAvailable()) {
            var_export($parameter->getDefaultValue());
        } else {
            echo '-';
        }
        echo '|';
    }
}
"#,
        ),
        concat!(
            "proc_open:standard:NULL:3/6|",
            "command:-:array|string:-|descriptor_spec:-:array:-|pipes:&:-:-|",
            "cwd:-:?string:NULL|env_vars:-:?array:NULL|options:-:?array:NULL|",
            "proc_close:standard:'int':1/1|process:-:-:-|",
        ),
    );
}

#[test]
#[cfg(feature = "stream-contents")]
fn proc_open_projects_argv_cwd_environment_and_bidirectional_pipes() {
    let source = r#"<?php
$binary = '__RPHP_BINARY__';
$cwd = getcwd();
$program = <<<'PHP'
echo (getcwd() === getenv('EXPECTED_CWD') ? 'cwd' : 'bad'), ':', getenv('X'), ':', stream_get_contents(STDIN);
fwrite(STDERR, 'err');
exit(7);
PHP;
$process = proc_open(
    [$binary, '-n', '-r', $program],
    [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']],
    $pipes,
    $cwd,
    ['EXPECTED_CWD' => $cwd, 'X' => 'yes'],
);
echo get_resource_type($process), ':', implode(',', array_keys($pipes)), ':';
echo implode(',', array_map('get_resource_type', $pipes)), '|';
fwrite($pipes[0], 'input');
fclose($pipes[0]);
echo stream_get_contents($pipes[1]), '|', stream_get_contents($pipes[2]), '|';
fclose($pipes[1]);
fclose($pipes[2]);
echo proc_close($process);
"#
    .replace("__RPHP_BINARY__", env!("CARGO_BIN_EXE_rphp"));
    assert_eq!(
        run_php(&source),
        "process:0,1,2:stream,stream,stream|cwd:yes:input|err|7",
    );
}

#[test]
#[cfg(feature = "stream-contents")]
fn proc_open_supports_phpunit_shell_and_compiled_defaults_shapes() {
    let source = r#"<?php
$binary = '__RPHP_BINARY__';
$process = proc_open('printf shell', [1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
echo stream_get_contents($pipes[1]), ':', stream_get_contents($pipes[2]), ':', proc_close($process), '|';

$process = proc_open(
    [$binary, '-n', '-r', 'echo "compiled";'],
    [1 => ['pipe', 'w']],
    $pipes,
);
$stdout = stream_get_contents($pipes[1]);
echo $stdout, ':', proc_close($process), '|';

$pipes = ['unchanged'];
$process = proc_open(
    ['rphp-command-that-does-not-exist'],
    [1 => ['pipe', 'w']],
    $pipes,
    null,
    null,
    ['suppress_errors' => true],
);
var_export($process);
echo ':', implode(',', $pipes);
"#
    .replace("__RPHP_BINARY__", env!("CARGO_BIN_EXE_rphp"));
    assert_eq!(run_php(&source), "shell::0|compiled:0|false:unchanged",);
}

#[test]
fn proc_open_rejects_invalid_commands_and_proc_close_rejects_other_resources() {
    assert_eq!(
        run_php(
            r#"<?php
$spec = [1 => ['pipe', 'w']];
foreach ([[], [''], ["php\0bad"], ['php', "bad\0arg"]] as $command) {
    try {
        proc_open($command, $spec, $pipes);
    } catch (ValueError $error) {
        echo $error->getMessage(), '|';
    }
}
try {
    proc_close(STDOUT);
} catch (TypeError $error) {
    echo $error->getMessage();
}
"#,
        ),
        concat!(
            "proc_open(): Argument #1 ($command) must not be empty|",
            "First element must contain a non-empty program name|",
            "Command array element 1 contains a null byte|",
            "Command array element 2 contains a null byte|",
            "proc_close(): supplied resource is not a valid process resource",
        ),
    );
}

#[test]
fn proc_close_closes_owned_pipes_before_waiting_for_the_child() {
    assert_eq!(
        run_php(
            r#"<?php
$process = proc_open(['/bin/cat'], [0 => ['pipe', 'r'], 1 => ['pipe', 'w']], $pipes);
echo proc_close($process), ':';
var_export(is_resource($pipes[0]));
echo ':', get_resource_type($pipes[0]), ':';
var_export(is_resource($pipes[1]));
echo ':', get_resource_type($pipes[1]);
"#,
        ),
        "0:false:Unknown:false:Unknown",
    );
}

#[test]
#[cfg(feature = "stream-contents")]
fn php_binary_can_spawn_a_no_ini_child_through_proc_open() {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-n",
            "-r",
            r#"$process = proc_open([PHP_BINARY, '-n', '-r', 'echo "child";'], [1 => ['pipe', 'w']], $pipes); echo stream_get_contents($pipes[1]), ':', proc_close($process);"#,
        ])
        .output()
        .expect("run rphp process-control CLI probe");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "child:0");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
