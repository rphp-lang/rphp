mod common;

use common::run_php;

#[test]
#[cfg(target_os = "linux")]
fn pcntl_signal_and_mask_constants_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'SIGQUIT', 'SIGKILL', 'SIGUSR1', 'SIGUSR2', 'SIGTERM',
    'SIG_BLOCK', 'SIG_UNBLOCK', 'SIG_SETMASK',
] as $name) {
    echo $name, '=', constant($name), '|';
}
"#,
        ),
        "SIGQUIT=3|SIGKILL=9|SIGUSR1=10|SIGUSR2=12|SIGTERM=15|SIG_BLOCK=0|SIG_UNBLOCK=1|SIG_SETMASK=2|",
    );
}
