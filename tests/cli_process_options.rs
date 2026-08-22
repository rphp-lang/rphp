use std::process::Command;

fn run(arguments: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(arguments)
        .output()
        .expect("rphp subprocess should start");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

#[test]
fn php_ini_and_extended_info_flags_preserve_file_execution() {
    assert_eq!(
        run(&["-n", "-e", "-r", "echo 'short';"]),
        (0, "short".to_string(), String::new())
    );
    assert_eq!(
        run(&["--no-php-ini", "-r", "echo 'long';"]),
        (0, "long".to_string(), String::new())
    );
}

#[test]
fn interactive_mode_truthfully_reports_the_missing_readline_extension() {
    assert_eq!(
        run(&["-n", "-d", "memory_limit=4M", "-a", "ignored.php",]),
        (
            0,
            "Interactive shell (-a) requires the readline extension.\n".to_string(),
            String::new(),
        )
    );
}
