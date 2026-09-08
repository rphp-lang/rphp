#![cfg(all(target_os = "linux", target_pointer_width = "64"))]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

struct SpecDirectory(PathBuf);

impl Drop for SpecDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove newly owned specimen directory");
    }
}

fn check(name: &str, expected: &str) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "rphp-directory-object-spec-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).expect("adopt only a newly created directory");
    let directory = SpecDirectory(directory);
    let result = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0"])
        .arg(format!(
            "{}/tests/fixtures/directory_object_lifecycle/{name}.php",
            env!("CARGO_MANIFEST_DIR")
        ))
        .env("RPHP_DIRECTORY_SPEC_DIR", &directory.0)
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
                    "fixtures/directory_object_lifecycle/",
                    stringify!($name),
                    ".out"
                )),
            );
        }
    };
}

specimen!(basic);
specimen!(aliases);
specimen!(capabilities);
specimen!(uninitialized);
specimen!(metadata);
specimen!(errors);
specimen!(order_core);
#[cfg(feature = "stream-registry")]
specimen!(order);
specimen!(strict);
specimen!(raw);
specimen!(readonly_user);
specimen!(serial_alias);
specimen!(shared_temps);
specimen!(moved_values);
#[cfg(feature = "stream-registry")]
specimen!(wrapper);
