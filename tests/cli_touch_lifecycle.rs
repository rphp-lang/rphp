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
        "rphp-touch-spec-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).expect("adopt only a newly created directory");
    let directory = SpecDirectory(directory);
    let result = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0"])
        .arg(format!(
            "{}/tests/fixtures/touch_lifecycle/{name}.php",
            env!("CARGO_MANIFEST_DIR")
        ))
        .env("RPHP_TOUCH_SPEC_DIR", &directory.0)
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
                    "fixtures/touch_lifecycle/",
                    stringify!($name),
                    ".out"
                )),
            );
        }
    };
}

specimen!(basic);
specimen!(defaults);
specimen!(errors);
specimen!(types);
specimen!(strict);
specimen!(order);
specimen!(links);
specimen!(permissions);
specimen!(metadata);
specimen!(raw);
specimen!(cache);
specimen!(callbacks);
specimen!(file_url);
