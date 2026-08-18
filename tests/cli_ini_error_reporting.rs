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
fn startup_error_reporting_accepts_numeric_and_ini_expression_values() {
    for (definition, expected) in [
        ("error_reporting=0", "0|string(1) \"0\"\n"),
        ("error_reporting=-1", "-1|string(2) \"-1\"\n"),
        ("error_reporting=E_ALL", "30719|string(5) \"30719\"\n"),
        (
            "error_reporting=E_ALL & ~E_NOTICE",
            "30711|string(5) \"30711\"\n",
        ),
        ("error_reporting=off", "0|string(0) \"\"\n"),
    ] {
        let (status, stdout, stderr) = run(&[
            "-d",
            definition,
            "-r",
            r#"echo error_reporting(), "|"; var_dump(ini_get("error_reporting"));"#,
        ]);
        assert_eq!(status, 0, "{definition}");
        assert_eq!(stdout, expected, "{definition}");
        assert_eq!(stderr, "", "{definition}");
    }
}

#[test]
fn last_startup_error_reporting_definition_wins() {
    let (status, stdout, stderr) = run(&[
        "-derror_reporting=0",
        "-d",
        "error_reporting=8191",
        "-r",
        r#"echo error_reporting(), "|", ini_get("error_reporting");"#,
    ]);
    assert_eq!(status, 0);
    assert_eq!(stdout, "8191|8191");
    assert_eq!(stderr, "");
}

#[test]
fn startup_error_reporting_masks_runtime_diagnostics() {
    let (status, stdout, stderr) =
        run(&["-derror_reporting=0", "-r", r#"echo $missing; echo "ok";"#]);
    assert_eq!(status, 0);
    assert_eq!(stdout, "ok");
    assert_eq!(stderr, "");
}
