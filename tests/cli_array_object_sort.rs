use std::process::Command;

fn specimen(name: &str) {
    let result = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-d",
            "display_errors=1",
            "-d",
            "log_errors=0",
            "-d",
            "error_reporting=32767",
        ])
        .arg(format!(
            "{}/tests/fixtures/array_object_sort/{name}.php",
            env!("CARGO_MANIFEST_DIR")
        ))
        .output()
        .expect("run native array sort specimen");
    assert_eq!(
        result.status.code(),
        Some(0),
        "{name}: {:?} {:?}",
        result.stdout,
        result.stderr
    );
    assert!(result.stderr.is_empty(), "{name}: {:?}", result.stderr);
    assert_eq!(result.stdout, format!("{name}:ok\n").as_bytes());
}

#[test]
fn basic() {
    specimen("basic");
}
#[test]
fn contracts() {
    specimen("contracts");
}
#[test]
fn strict() {
    specimen("strict");
}
#[test]
fn callbacks() {
    specimen("callbacks");
}
#[test]
fn mutation() {
    specimen("mutation");
}
#[test]
fn references() {
    specimen("references");
}
#[test]
fn boundaries() {
    specimen("boundaries");
}

#[test]
fn objects() {
    specimen("objects");
}

#[test]
fn weak() {
    specimen("weak");
}

#[test]
fn local_updates() {
    specimen("local_updates");
}
