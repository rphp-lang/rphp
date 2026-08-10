#![cfg(feature = "include-path")]

mod common;

use std::sync::atomic::{AtomicU64, Ordering};

use common::run_php;

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TemporaryDirectory(std::path::PathBuf);

impl TemporaryDirectory {
    fn unique() -> Self {
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rphp-include-path-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn php_literal(path: &std::path::Path) -> String {
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn include_path_drives_all_existing_file_surfaces_in_order() {
    let root = TemporaryDirectory::unique();
    let first = root.0.join("first");
    let second = root.0.join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    std::fs::write(first.join("shared.txt"), b"FIRST").unwrap();
    std::fs::write(second.join("shared.txt"), b"SECOND").unwrap();
    std::fs::write(first.join("lines.txt"), b"one\ntwo\n").unwrap();
    std::fs::write(second.join("write.txt"), b"old").unwrap();
    std::fs::write(first.join("included.php"), b"<?php echo 'INCLUDED';").unwrap();

    let include_path = format!(
        "{}{}{}",
        TemporaryDirectory::php_literal(&first),
        if cfg!(windows) { ';' } else { ':' },
        TemporaryDirectory::php_literal(&second)
    );
    let written = TemporaryDirectory::php_literal(&second.join("write.txt"));
    let expected_uri =
        TemporaryDirectory::php_literal(&std::fs::canonicalize(first.join("shared.txt")).unwrap());
    let source = format!(
        "<?php
        echo get_include_path() === '.'; echo ':';
        echo set_include_path('{include_path}') === '.'; echo ':';
        echo get_include_path() === '{include_path}'; echo '|';
        echo file_get_contents('shared.txt', true); echo ':';
        $stream = fopen('shared.txt', 'r', true);
        echo fread($stream, 5); echo ':';
        $metadata = stream_get_meta_data($stream);
        echo $metadata['uri']; echo ':';
        echo implode('/', file('lines.txt', FILE_USE_INCLUDE_PATH | FILE_IGNORE_NEW_LINES)); echo ':';
        echo file_put_contents('write.txt', 'NEW', FILE_USE_INCLUDE_PATH); echo ':';
        echo file_get_contents('{written}'); echo '|';
        include_once 'included.php'; include_once 'included.php';
        "
    );

    assert_eq!(
        run_php(&source),
        format!("1:1:1|FIRST:FIRST:{expected_uri}:one/two:3:NEW|INCLUDED")
    );
}

#[test]
fn include_path_setter_matches_weak_values_and_errors() {
    assert_eq!(
        run_php(
            "<?php
            echo get_include_path(); echo '|';
            echo set_include_path('alpha') === '.'; echo ':'; echo get_include_path(); echo '|';
            echo set_include_path('') === false; echo ':'; echo get_include_path(); echo '|';
            echo set_include_path(null) === false; echo ':'; echo get_include_path(); echo '|';
            echo set_include_path(12); echo ':'; echo get_include_path(); echo '|';
            echo defined(chr(0) . 'rphp-include-path'); echo '|';
            try { set_include_path([]); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { set_include_path('a' . chr(0) . 'b'); }
            catch (ValueError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            "
        ),
        concat!(
            ".|1:alpha|1:alpha|1:alpha|alpha:12||",
            "TypeError:set_include_path(): Argument #1 ($include_path) must be of type string, array given|",
            "ValueError:set_include_path(): Argument #1 ($include_path) must not contain any null bytes"
        )
    );
}
