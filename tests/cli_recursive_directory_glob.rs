#![cfg(unix)]
use std::{
    fs,
    os::unix::{ffi::OsStrExt, fs::symlink},
    process::Command,
};

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rphp-recursive-directory-glob-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn contract(name: &str) {
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/recursive_directory_glob/expected.json"
    ))
    .unwrap();
    for disable_jit in [false, true] {
        let root = Fixture::new();
        for directory in ["grove/stem/branch", "grove/empty", "links", "raw"] {
            fs::create_dir_all(root.path().join(directory)).unwrap();
        }
        for (file, data) in [
            ("grove/leaf.txt", "leaf"),
            ("grove/stem/bud.txt", "bud"),
            ("grove/stem/branch/seed.txt", "seed"),
            ("grove/.hidden", "hidden"),
            ("first.note", "first"),
            ("second.note", "second"),
        ] {
            fs::write(root.path().join(file), data).unwrap();
        }
        for (target, link) in [
            ("../grove/stem", "to-dir"),
            ("../grove/leaf.txt", "to-file"),
            ("absent", "dangling"),
        ] {
            symlink(target, root.path().join("links").join(link)).unwrap();
        }
        let raw = root
            .path()
            .join(std::ffi::OsStr::from_bytes(b"raw/dir-\xff"));
        fs::create_dir(&raw).unwrap();
        fs::write(
            raw.join(std::ffi::OsStr::from_bytes(b"leaf-\x80.dat")),
            "raw",
        )
        .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/recursive_directory_glob/contract.php"
            ))
            .env("RPHP_RECURSIVE_DIRECTORY_GLOB_CASE", name)
            .env_remove("RPHP_DISABLE_JIT")
            .current_dir(root.path());
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected[name].as_str().unwrap(),
            "{name} JIT disabled={disable_jit}"
        );
    }
}
macro_rules! specimen {
    ($name:ident) => {
        #[test]
        fn $name() {
            contract(stringify!($name));
        }
    };
}
specimen!(metadata);
specimen!(recursive_flags);
specimen!(recursive_dots);
specimen!(recursive_subpaths);
specimen!(recursive_traversal);
specimen!(child_identity);
specimen!(child_subclass);
specimen!(child_constructor_throw);
specimen!(symlinks);
specimen!(recursive_child_errors);
specimen!(recursive_clone);
specimen!(arguments);
specimen!(strict);
specimen!(uninitialized);
specimen!(repeat);
specimen!(glob_patterns);
specimen!(glob_flags);
specimen!(glob_snapshot);
specimen!(glob_dot_flags);
specimen!(glob_first_class);
specimen!(glob_clone);
specimen!(glob_empty);
specimen!(glob_empty_metadata);
specimen!(glob_method_lookup);
specimen!(raw_bytes);
specimen!(reference_cow);
specimen!(retirement);
