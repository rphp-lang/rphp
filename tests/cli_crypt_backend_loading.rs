// Mechanism test, not a portable PHP-compatibility assertion. The process
// starts fresh so other in-process crypt tests cannot initialize its backend.
#[cfg(target_os = "linux")]
#[test]
fn unused_bcrypt_backend_stays_unmapped_until_requested() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-r",
            r#"
function backendLoaded() {
    return strpos(file_get_contents('/proc/self/maps'), 'libcrypt.so') !== false;
}
var_dump(backendLoaded());
var_dump(strlen(crypt('synthetic', 'kL')) === 13);
var_dump(backendLoaded());
var_dump(strlen(crypt('synthetic', '$2y$04$abcdefghijklmnopqrstuu')) === 60);
var_dump(backendLoaded());
var_dump(constant('CRYPT_BLOWFISH') === 1);
"#,
        ])
        .output()
        .expect("fresh RPHP process");
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        b"bool(false)\nbool(true)\nbool(false)\nbool(true)\nbool(true)\nbool(true)\n"
    );
}
