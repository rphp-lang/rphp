#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

struct SpecDirectory(PathBuf);

impl Drop for SpecDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned specimen directory");
    }
}

fn check(name: &str, expected: &str) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "rphp-link-spec-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    // Only adopt a freshly created directory; never remove an existing path.
    std::fs::create_dir(&directory).expect("create isolated specimen directory");
    let directory = SpecDirectory(directory);
    let result = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0"])
        .arg(format!(
            "{}/tests/fixtures/link_lifecycle/{name}.php",
            env!("CARGO_MANIFEST_DIR")
        ))
        .env("RPHP_LINK_SPEC_DIR", &directory.0)
        .output()
        .expect("CLI starts");
    assert_eq!(result.status.code(), Some(0), "{name}: {:?}", result.stderr);
    assert_eq!(result.stderr, b"", "{name}");
    assert_eq!(result.stdout, expected.as_bytes(), "{name}");
}

macro_rules! specimen {
    ($name:ident) => {
        #[test]
        fn $name() {
            check(
                stringify!($name),
                include_str!(concat!(
                    "fixtures/link_lifecycle/",
                    stringify!($name),
                    ".out"
                )),
            );
        }
    };
}

specimen!(lifecycle);
specimen!(relative);
specimen!(destination);
specimen!(raw);
specimen!(errors);
specimen!(weak);
specimen!(strict);
specimen!(order);
specimen!(metadata);
specimen!(cache);
#[cfg(feature = "stream-registry")]
specimen!(wrappers);
