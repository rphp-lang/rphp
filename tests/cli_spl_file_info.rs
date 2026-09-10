#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::os::unix::{
    ffi::OsStrExt,
    fs::{PermissionsExt, symlink},
};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rphp-file-info-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("folder")).unwrap();
        std::fs::write(root.join("item.bin"), b"payload").unwrap();
        std::fs::set_permissions(
            root.join("item.bin"),
            std::fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        symlink("item.bin", root.join("alias.bin")).unwrap();
        symlink("absent.bin", root.join("broken.bin")).unwrap();
        let raw = std::ffi::OsStr::from_bytes(b"raw-\xff.\x80");
        std::fs::write(root.join(raw), b"bytes").unwrap();
        symlink(raw, root.join("raw-link")).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn contract(name: &str, expected: &[u8]) {
    let fixture = Fixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/spl_file_info/contract.php"
        ))
        .env("RPHP_FILE_INFO_CASE", name)
        .current_dir(&fixture.0)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
    assert!(output.stderr.is_empty(), "{name}: {:?}", output.stderr);
    assert_eq!(
        output.stdout,
        expected,
        "{name}: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
macro_rules! specimen {
    ($name:ident, $case:literal) => {
        #[test]
        fn $name() {
            contract(
                $case,
                include_bytes!(concat!("fixtures/spl_file_info/", $case, ".expected")),
            );
        }
    };
}
specimen!(lexical, "lexical");
specimen!(arguments, "arguments");
specimen!(uninitialized, "uninitialized");
specimen!(missing, "missing");
specimen!(metadata, "metadata");
specimen!(symlinks, "symlink");
#[cfg(feature = "stream-truncate")]
specimen!(cache, "cache");
specimen!(factories, "factories");
specimen!(factory_errors, "factory-errors");
specimen!(cloning, "clone");
specimen!(native_policy, "native-policy");
specimen!(debug, "debug");
specimen!(raw_bytes, "raw-bytes");
specimen!(reflection, "reflection");
specimen!(inherited_string_contract, "inherited-string-contract");
specimen!(local_file_wrapper, "local-file-wrapper");
specimen!(strict_and_error_order, "strict-order");
specimen!(callback_state, "callback-state");
specimen!(factory_native_construction, "factory-native-construction");
specimen!(raw_path_errors, "raw-errors");
#[cfg(feature = "stream-registry")]
specimen!(wrapper_stat, "wrapper-stat");

#[test]
fn external_mutation_preserves_cached_metadata_until_explicit_clear() {
    let fixture = Fixture::new();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/spl_file_info/external-cache.php"
        ))
        .current_dir(&fixture.0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    stdout.read_line(&mut ready).unwrap();
    if ready == "7\n" {
        std::fs::OpenOptions::new()
            .write(true)
            .open(fixture.0.join("item.bin"))
            .unwrap()
            .set_len(2)
            .unwrap();
        std::fs::write(fixture.0.join("advance"), b"").unwrap();
    }
    let mut remaining = String::new();
    std::io::Read::read_to_string(&mut stdout, &mut remaining).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    assert_eq!(ready, "7\n");
    assert_eq!(remaining, "7:7:7\n2:2:2\n");
}

#[test]
fn final_internal_method_cannot_be_overridden() {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "log_errors=0", "-r", "class FinalInfoChild extends SplFileInfo { function _bad_state_ex(): void {} } echo 'unreachable';"])
        .output().unwrap();
    assert_eq!(output.status.code(), Some(255));
    let rendered = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(
        rendered.contains("Cannot override final method SplFileInfo::_bad_state_ex()"),
        "{rendered}"
    );
    assert!(!rendered.contains("unreachable"), "{rendered}");
}
