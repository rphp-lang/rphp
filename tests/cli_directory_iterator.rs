#![cfg(unix)]

use std::os::unix::ffi::OsStrExt;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rphp-directory-iterator-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        for name in ["tree", "empty", "raw"] {
            std::fs::create_dir(root.join(name)).unwrap();
        }
        std::fs::write(root.join("tree/alpha.txt"), b"alpha").unwrap();
        std::fs::write(root.join("tree/beta.bin"), b"beta").unwrap();
        std::fs::write(
            root.join("raw")
                .join(std::ffi::OsStr::from_bytes(b"name-\xff.\x80")),
            b"rawbytes",
        )
        .unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn contract(name: &str) {
    let fixture = Fixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/directory_iterator/contract.php"
        ))
        .env("RPHP_DIRECTORY_CASE", name)
        .current_dir(&fixture.0)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
    assert!(output.stderr.is_empty(), "{name}: {:?}", output.stderr);
    assert_eq!(output.stdout, format!("{name}:ok\n").as_bytes());
}
macro_rules! specimen {
    ($name:ident, $case:literal) => {
        #[test]
        fn $name() {
            contract($case);
        }
    };
}
specimen!(cursor, "cursor");
specimen!(seek, "seek");
specimen!(flags, "flags");
specimen!(cloning, "clone");
specimen!(factories, "factory");
specimen!(invalid, "invalid");
specimen!(uninitialized, "uninitialized");
specimen!(hooks, "hooks");
specimen!(exhausted, "exhausted");
specimen!(parent_path, "parent-path");
specimen!(raw_bytes, "raw");
specimen!(renamed_open_directory, "rename");
specimen!(removed_open_directory, "removed");
specimen!(reflection, "reflection");
specimen!(uri_and_root_paths, "uri-root");
specimen!(metadata, "metadata");
