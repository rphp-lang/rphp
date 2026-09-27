use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("rphp-phpstan-regression-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("temporary directory should be created");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn phpstan_signal_probe_keeps_constants_when_pcntl_signal_is_disabled() {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-d",
            "disable_functions=proc_open,pcntl_signal",
            "-r",
            r#"echo SIGINT, ':', SIGTERM, ':', SIGUSR1, ':', SIGUSR2, ':',
                (int) function_exists('proc_open'), ':', (int) function_exists('pcntl_signal');"#,
        ])
        .output()
        .expect("rphp signal probe should run");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(output.stdout, b"2:15:10:12:0:0");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn closed_stdout_ends_the_request_silently_and_runs_shutdown_callbacks() {
    let directory = TempDir::new();
    let after = directory.0.join("after");
    let shutdown = directory.0.join("shutdown");
    let source = format!(
        r#"register_shutdown_function(fn() => file_put_contents('{}', 'shutdown'));
            echo 'ready', PHP_EOL;
            flush();
            usleep(100000);
            echo str_repeat('x', 1000000);
            file_put_contents('{}', 'after');"#,
        shutdown.display(),
        after.display(),
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-r", &source])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rphp broken-pipe probe should start");

    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut stdout = BufReader::new(stdout);
    let mut first_line = String::new();
    stdout
        .read_line(&mut first_line)
        .expect("first output line should be readable");
    assert_eq!(first_line, "ready\n");
    drop(stdout);

    let mut stderr = child.stderr.take().expect("stderr should be piped");
    let status = child.wait().expect("rphp should finish");
    let mut diagnostic = Vec::new();
    stderr
        .read_to_end(&mut diagnostic)
        .expect("stderr should be readable");

    assert_eq!(status.code(), Some(255));
    assert!(diagnostic.is_empty(), "{diagnostic:?}");
    assert!(!after.exists(), "execution must stop at the failed write");
    assert_eq!(std::fs::read(shutdown).unwrap(), b"shutdown");
}
