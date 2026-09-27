mod common;

use common::run_php;

#[test]
fn proc_status_and_terminate_match_the_php_85_contract() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['proc_get_status', 'proc_terminate'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':',
        $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), ':',
        $function->getReturnType(), '|';
}
$pipes = [];
$process = proc_open(['/bin/sh', '-c', 'sleep 5'], [], $pipes);
$status = proc_get_status($process);
echo $status['command'], ':', (int) $status['running'], ':', $status['exitcode'], ':';
echo (int) proc_terminate($process), ':';
usleep(20_000);
$status = proc_get_status($process);
echo (int) !$status['running'], ':', (int) $status['signaled'], ':', $status['termsig'];
proc_close($process);
"#,
        ),
        concat!(
            "proc_get_status:standard:1/1:array|proc_terminate:standard:1/2:bool|",
            "/bin/sh:1:-1:1:1:1:15",
        ),
    );
}
