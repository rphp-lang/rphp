use std::process::Command;

fn run_script(source: &str, arguments: &[&str]) -> String {
    let directory = std::env::temp_dir().join(format!(
        "rphp-cli-request-globals-{}-{}",
        std::process::id(),
        arguments.len()
    ));
    std::fs::create_dir_all(&directory).expect("temporary directory should be created");
    let script = directory.join("cli1.php");
    std::fs::write(&script, source).expect("script should be written");
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .current_dir(&directory)
        .arg("cli1.php")
        .args(arguments)
        .output()
        .expect("rphp should run the script");
    std::fs::remove_dir_all(&directory).expect("temporary directory should be removed");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout should be UTF-8")
}

fn run_code(code: &str, arguments: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-r", code])
        .args(arguments)
        .output()
        .expect("rphp should run the code");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout should be UTF-8")
}

#[test]
fn script_arguments_populate_argv_argc_and_request_superglobals() {
    assert_eq!(
        run_script(
            r##"<?php
echo json_encode([$argc, $argv, $_SERVER['argc'], $_SERVER['argv'] === $argv, isset($_SERVER['REQUEST_TIME'], $_SERVER['REQUEST_TIME_FLOAT']), $_SERVER['SCRIPT_NAME'] === $argv[0], $_SERVER['PHP_SELF'] === $argv[0], $_SERVER['SCRIPT_FILENAME'] === $argv[0], $_GET, $_POST, $_COOKIE, $_FILES, $_REQUEST, is_array($_ENV), (function () { global $argv; return count($argv); })()]), "\n";
"##,
            &["a", "b c", "--x"],
        ),
        "[4,[\"cli1.php\",\"a\",\"b c\",\"--x\"],4,true,true,true,true,true,[],[],[],[],[],true,4]\n"
    );
}

#[test]
fn inline_code_receives_trailing_arguments_after_the_standard_input_name() {
    assert_eq!(
        run_code("echo json_encode([$argc, $argv]), \"\\n\";", &["q", "r"]),
        "[3,[\"Standard input code\",\"q\",\"r\"]]\n"
    );
    assert_eq!(
        run_code("echo json_encode($argv), \"\\n\";", &["--", "s"]),
        "[\"Standard input code\",\"s\"]\n"
    );
}
