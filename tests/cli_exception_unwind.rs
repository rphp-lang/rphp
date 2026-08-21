use std::process::Command;

#[test]
fn destructor_exit_during_exception_unwind_bypasses_catch_and_preserves_exit_status() {
    let source = r#"
class ExitUnwindDestructor {
    public function __destruct() { echo 'destruct|'; exit(7); }
}
function failDuringUnwind(): void {
    try {
        $local = new ExitUnwindDestructor();
        throw new Exception('body');
    } finally {
        echo 'finally|';
    }
}
try {
    failDuringUnwind();
} catch (Throwable $error) {
    echo 'caught';
}
"#;
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-r", source])
        .output()
        .expect("RPHP CLI must execute the exception-unwind specimen");

    assert_eq!(output.status.code(), Some(7));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "finally|destruct|"
    );
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
}
