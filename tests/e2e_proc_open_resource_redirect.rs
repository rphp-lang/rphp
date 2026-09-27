mod common;

use common::run_php;

#[test]
fn proc_open_accepts_an_open_stream_descriptor() {
    assert_eq!(
        run_php(
            r#"<?php
$stderr = tmpfile();
$pipes = [];
$process = proc_open(
    ['/bin/sh', '-c', 'printf worker-output; printf worker-error >&2'],
    [1 => ['pipe', 'w'], 2 => $stderr],
    $pipes,
);
echo fread($pipes[1], 13), ':', proc_close($process), ':';
rewind($stderr);
echo fread($stderr, 12);
"#,
        ),
        "worker-output:0:worker-error",
    );
}
