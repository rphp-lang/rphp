use std::process::Command;

/// `tests/fixtures/phar/hello.phar` is an executable phar whose stub maps the
/// archive, includes members through `phar://` and reports `Phar::running()`
/// from inside a member; running it must print what PHP prints.
#[test]
fn executable_phar_runs_as_the_main_script() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/phar/hello.phar"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .arg(fixture)
        .arg("Bob")
        .output()
        .expect("rphp should start");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "hello Bob|lib|run-ok|plain-ok\n"
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[test]
fn map_phar_outside_a_stub_reports_the_missing_halt_compiler() {
    let script = std::env::temp_dir().join(format!("rphp-map-phar-{}.php", std::process::id()));
    std::fs::write(
        &script,
        "<?php try { Phar::mapPhar('x'); } catch (PharException $e) { echo $e->getMessage(); }",
    )
    .expect("temp script should be written");
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .arg(&script)
        .output()
        .expect("rphp should start");
    let _ = std::fs::remove_file(&script);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "__HALT_COMPILER(); must be declared in a phar"
    );
}

#[test]
fn phar_running_is_empty_outside_an_archive() {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-r",
            "var_dump(Phar::running(), Phar::running(false), extension_loaded('phar'));",
        ])
        .output()
        .expect("rphp should start");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "string(0) \"\"\nstring(0) \"\"\nbool(true)\n"
    );
}
