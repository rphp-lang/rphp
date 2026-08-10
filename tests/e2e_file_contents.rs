mod common;

use common::run_php;

struct TemporaryPath(std::path::PathBuf);

impl TemporaryPath {
    fn unique(label: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "rphp-{label}-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Self(path)
    }

    fn php_literal(&self) -> String {
        self.0
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    }
}

impl Drop for TemporaryPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn file_get_contents_reads_complete_ascii_files() {
    let path = TemporaryPath::unique("file-contents-complete");
    let payload: Vec<u8> = (0..20_000).map(|index| b'a' + (index % 26) as u8).collect();
    std::fs::write(&path.0, &payload).unwrap();
    let source = format!(
        "<?php
        $contents = file_get_contents('{}');
        echo strlen($contents); echo ':';
        echo substr($contents, 8188, 8); echo ':';
        echo substr($contents, -8);
        ",
        path.php_literal()
    );
    assert_eq!(run_php(&source), "20000:yzabcdef:yzabcdef");
}

#[test]
#[cfg(feature = "file-contents")]
fn extended_file_contents_matches_php_offsets_lengths_and_file_urls() {
    let path = TemporaryPath::unique("file-contents-offsets");
    std::fs::write(&path.0, b"abcdef").unwrap();
    let source = format!(
        "<?php
        echo file_get_contents('{}');
        echo ':'; echo file_get_contents('{}', false, null, 2);
        echo ':'; echo file_get_contents('{}', true, null, -2);
        echo ':['; echo file_get_contents('{}', false, null, 7); echo ']';
        echo ':['; echo file_get_contents('{}', false, null, 3, 0); echo ']';
        echo ':'; echo file_get_contents('{}', false, null, 1, 2);
        echo ':'; echo file_get_contents(filename: 'file://{}', offset: 2, length: 3);
        echo ':';
        if (file_get_contents('{}', false, null, -7) === false) {{ echo 'before-start'; }}
        echo ':'; echo file_get_contents('{}', false, null, '2', '2');
        ",
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal()
    );
    assert_eq!(
        run_php(&source),
        "abcdef:cdef:ef:[]:[]:bc:cde:before-start:cd"
    );
}

#[test]
#[cfg(feature = "file-contents")]
fn extended_file_contents_argument_errors_match_php_classes_and_messages() {
    let path = TemporaryPath::unique("file-contents-errors");
    std::fs::write(&path.0, b"abcdef").unwrap();
    let source = format!(
        "<?php
        try {{ file_get_contents([]); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file_get_contents(''); }}
        catch (ValueError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file_get_contents('', []); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file_get_contents('{}', []); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file_get_contents('{}', false, false); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        $stream = fopen('php://memory', 'w+');
        try {{ file_get_contents('{}', false, $stream); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file_get_contents('{}', false, null, []); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file_get_contents('{}', false, null, 0, []); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file_get_contents('{}', false, null, 0, -1); }}
        catch (ValueError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        ",
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal()
    );
    assert_eq!(
        run_php(&source),
        concat!(
            "TypeError:file_get_contents(): Argument #1 ($filename) must be of type string, array given",
            "|ValueError:Path must not be empty",
            "|TypeError:file_get_contents(): Argument #2 ($use_include_path) must be of type bool, array given",
            "|TypeError:file_get_contents(): Argument #2 ($use_include_path) must be of type bool, array given",
            "|TypeError:file_get_contents(): Argument #3 ($context) must be of type resource or null, false given",
            "|TypeError:file_get_contents(): supplied resource is not a valid Stream-Context resource",
            "|TypeError:file_get_contents(): Argument #4 ($offset) must be of type int, array given",
            "|TypeError:file_get_contents(): Argument #5 ($length) must be of type ?int, array given",
            "|ValueError:file_get_contents(): Argument #5 ($length) must be greater than or equal to 0"
        )
    );
}
