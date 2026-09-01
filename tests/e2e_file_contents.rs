mod common;

use common::run_php;

#[test]
fn dirname_supports_multiple_levels() {
    assert_eq!(
        run_php("<?php echo dirname('/one/two/three/file.php', 3), ':', dirname('file.php');"),
        "/one:."
    );
}

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
fn filemtime_reports_unix_seconds_and_warns_for_missing_paths() {
    let existing = TemporaryPath::unique("filemtime-existing");
    let missing = TemporaryPath::unique("filemtime-missing");
    std::fs::write(&existing.0, b"timestamped").unwrap();
    let source = format!(
        "<?php
        $timestamp = filemtime('{}');
        echo is_int($timestamp) && $timestamp > 0 ? 'timestamp' : 'invalid';
        echo '|';
        var_dump(filemtime('{}'));
        ",
        existing.php_literal(),
        missing.php_literal()
    );
    assert_eq!(
        run_php(&source),
        format!(
            "timestamp|\nWarning: filemtime(): stat failed for {} in <main> on line 5\nbool(false)\n",
            missing.php_literal()
        )
    );
}

#[cfg(unix)]
#[test]
fn is_link_uses_lstat_semantics_for_existing_and_broken_links() {
    use std::os::unix::fs::symlink;

    let target = TemporaryPath::unique("link-target");
    let link = TemporaryPath::unique("link");
    std::fs::write(&target.0, b"target").unwrap();
    symlink(&target.0, &link.0).unwrap();
    let source = format!(
        "<?php echo is_link('{}') ? 'link' : 'bad'; echo ':'; echo is_link('{}') ? 'bad' : 'file';",
        link.php_literal(),
        target.php_literal()
    );
    assert_eq!(run_php(&source), "link:file");

    std::fs::remove_file(&target.0).unwrap();
    assert_eq!(
        run_php(&format!(
            "<?php echo is_link('{}') ? 'broken-link' : 'bad';",
            link.php_literal()
        )),
        "broken-link"
    );
}

#[cfg(unix)]
#[test]
fn chmod_fileperms_and_umask_support_atomic_file_writers() {
    let path = TemporaryPath::unique("permissions");
    std::fs::write(&path.0, b"permissions").unwrap();
    let source = format!(
        "<?php echo is_int(umask()) ? 'mask' : 'bad'; echo ':'; echo chmod('{}', 0o600) ? 'changed' : 'bad'; echo ':'; echo (fileperms('{}') & 0o777) === 0o600 ? '0600' : 'bad';",
        path.php_literal(),
        path.php_literal()
    );
    assert_eq!(run_php(&source), "mask:changed:0600");
}

#[test]
fn flock_constants_and_regular_file_locking_are_available() {
    let path = TemporaryPath::unique("flock");
    let source = format!(
        "<?php $stream = fopen('{}', 'c'); echo LOCK_SH, ':', LOCK_EX, ':', LOCK_UN, ':', LOCK_NB, '|'; echo flock($stream, LOCK_EX) ? 'locked' : 'bad'; echo ':'; echo flock($stream, LOCK_UN) ? 'unlocked' : 'bad'; fclose($stream);",
        path.php_literal()
    );
    assert_eq!(run_php(&source), "1:2:3:4|locked:unlocked");
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
fn file_put_contents_writes_complete_ascii_files() {
    let path = TemporaryPath::unique("file-contents-default-write");
    let source = format!(
        "<?php
        $payload = str_repeat('x', 20000);
        echo file_put_contents('{}', $payload); echo ':';
        $contents = file_get_contents('{}');
        echo strlen($contents); echo ':';
        echo substr($contents, 8188, 8);
        ",
        path.php_literal(),
        path.php_literal()
    );
    assert_eq!(run_php(&source), "20000:20000:xxxxxxxx");
}

#[test]
#[cfg(not(feature = "file-write"))]
fn default_file_put_contents_appends_under_exclusive_lock_during_shutdown() {
    let path = TemporaryPath::unique("file-contents-shutdown-append-lock");
    let source = format!(
        "<?php
        echo FILE_APPEND, ':', LOCK_EX, '|';
        register_shutdown_function(function () {{
            file_put_contents('{}', 'first', FILE_APPEND | LOCK_EX);
            file_put_contents('{}', ':second', FILE_APPEND | LOCK_EX);
            echo 'done';
        }});
        ",
        path.php_literal(),
        path.php_literal()
    );

    assert_eq!(run_php(&source), "8:2|done");
    assert_eq!(std::fs::read(&path.0).unwrap(), b"first:second");
}

#[test]
fn file_reads_complete_lines_across_stack_sized_boundaries() {
    let path = TemporaryPath::unique("file-lines-default");
    let mut payload = vec![b'x'; 9_000];
    payload.extend_from_slice(b"\n\nlast");
    std::fs::write(&path.0, payload).unwrap();
    let source = format!(
        "<?php
        $lines = file('{}');
        echo count($lines); echo ':';
        echo strlen($lines[0]); echo ':';
        if ($lines[1] === \"\n\") {{ echo 'blank'; }}
        echo ':'; echo $lines[2];
        ",
        path.php_literal()
    );
    assert_eq!(run_php(&source), "3:9001:blank:last");
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

#[test]
#[cfg(feature = "file-write")]
fn extended_file_writes_match_php_flags_arrays_urls_and_named_arguments() {
    let path = TemporaryPath::unique("file-contents-extended-write");
    let source = format!(
        "<?php
        echo FILE_USE_INCLUDE_PATH; echo ':'; echo LOCK_EX; echo ':'; echo FILE_APPEND; echo '|';
        echo file_put_contents('{}', 'abc'); echo ':'; echo file_get_contents('{}'); echo '|';
        echo file_put_contents('{}', 'de', FILE_APPEND); echo ':'; echo file_get_contents('{}'); echo '|';
        echo file_put_contents('{}', 'xy', LOCK_EX); echo ':'; echo file_get_contents('{}'); echo '|';
        echo file_put_contents('{}', 'z', FILE_APPEND | LOCK_EX); echo ':'; echo file_get_contents('{}'); echo '|';
        echo file_put_contents('{}', ['a', 2, true, null, 3.5]); echo ':'; echo file_get_contents('{}'); echo '|';
        echo file_put_contents(filename: '{}', data: 'q', flags: '8'); echo ':'; echo file_get_contents('{}'); echo '|';
        echo file_put_contents('file://{}', 'uv', 4); echo ':'; echo file_get_contents('{}'); echo '|';
        if (file_put_contents('php://memory', 'x', LOCK_EX) === false) {{ echo 'wrapper-lock'; }}
        ",
        path.php_literal(), path.php_literal(),
        path.php_literal(), path.php_literal(),
        path.php_literal(), path.php_literal(),
        path.php_literal(), path.php_literal(),
        path.php_literal(), path.php_literal(),
        path.php_literal(), path.php_literal(),
        path.php_literal(), path.php_literal()
    );
    assert_eq!(
        run_php(&source),
        "1:2:8|3:abc|2:abcde|2:xy|1:xyz|6:a213.5|1:a213.5q|2:uv|wrapper-lock"
    );
}

#[test]
#[cfg(feature = "file-write")]
fn extended_file_writes_copy_stream_data_in_fixed_size_chunks() {
    let path = TemporaryPath::unique("file-contents-stream-write");
    let source = format!(
        "<?php
        $stream = fopen('php://memory', 'w+');
        fwrite($stream, str_repeat('x', 20000));
        fseek($stream, 8188);
        echo file_put_contents('{}', $stream); echo ':';
        echo strlen(file_get_contents('{}')); echo ':';
        echo ftell($stream); echo ':';
        if (feof($stream)) {{ echo 'eof'; }}
        ",
        path.php_literal(),
        path.php_literal()
    );
    assert_eq!(run_php(&source), "11812:11812:20000:eof");
}

#[test]
#[cfg(feature = "file-write")]
fn extended_file_write_argument_errors_and_validation_order_match_php() {
    let path = TemporaryPath::unique("file-contents-write-errors");
    let source = format!(
        "<?php
        try {{ file_put_contents([], 'x'); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file_put_contents('', 'x', []); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file_put_contents('{}', 'x', 0, false); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        $context = fopen('php://memory', 'r');
        try {{ file_put_contents('{}', 'x', 0, $context); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        $closed = fopen('php://memory', 'r'); fclose($closed);
        try {{ file_put_contents('{}', $closed); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file_put_contents('', $closed); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file_put_contents('', 'x'); }}
        catch (ValueError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        file_put_contents('{}', 'keep');
        if (file_put_contents('{}', new stdClass()) === false) {{ echo 'false:'; }}
        echo '['; echo file_get_contents('{}'); echo ']';
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
            "TypeError:file_put_contents(): Argument #1 ($filename) must be of type string, array given",
            "|TypeError:file_put_contents(): Argument #3 ($flags) must be of type int, array given",
            "|TypeError:file_put_contents(): Argument #4 ($context) must be of type resource or null, false given",
            "|TypeError:file_put_contents(): supplied resource is not a valid Stream-Context resource",
            "|TypeError:file_put_contents(): supplied resource is not a valid stream resource",
            "|TypeError:file_put_contents(): supplied resource is not a valid stream resource",
            "|ValueError:Path must not be empty",
            "|false:[]"
        )
    );
}

#[test]
#[cfg(feature = "file-lines")]
fn extended_file_lines_match_php_newline_and_empty_line_flags() {
    let path = TemporaryPath::unique("file-lines-flags");
    std::fs::write(&path.0, b"a\n\nb\r\n\r\n0\n \n\t\nlast").unwrap();
    let source = format!(
        "<?php
        echo FILE_USE_INCLUDE_PATH; echo ':'; echo FILE_IGNORE_NEW_LINES; echo ':'; echo FILE_SKIP_EMPTY_LINES; echo '|';
        echo json_encode(file('{}')); echo '|';
        echo json_encode(file('{}', FILE_IGNORE_NEW_LINES)); echo '|';
        echo json_encode(file('{}', FILE_SKIP_EMPTY_LINES)); echo '|';
        echo json_encode(file(filename: '{}', flags: 6)); echo '|';
        echo json_encode(file('file://{}', '2'));
        ",
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal()
    );
    assert_eq!(
        run_php(&source),
        concat!(
            "1:2:4|[\"a\\n\",\"\\n\",\"b\\r\\n\",\"\\r\\n\",\"0\\n\",\" \\n\",\"\\t\\n\",\"last\"]",
            "|[\"a\",\"\",\"b\",\"\",\"0\",\" \",\"\\t\",\"last\"]",
            "|[\"a\\n\",\"\\n\",\"b\\r\\n\",\"\\r\\n\",\"0\\n\",\" \\n\",\"\\t\\n\",\"last\"]",
            "|[\"a\",\"b\",\"0\",\" \",\"\\t\",\"last\"]",
            "|[\"a\",\"\",\"b\",\"\",\"0\",\" \",\"\\t\",\"last\"]"
        )
    );
}

#[test]
#[cfg(feature = "file-lines")]
fn extended_file_line_errors_and_validation_order_match_php() {
    let path = TemporaryPath::unique("file-lines-errors");
    std::fs::write(&path.0, b"line\n").unwrap();
    let source = format!(
        "<?php
        try {{ file([]); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file('{}', []); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file('{}', 0, false); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file('', 8, false); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file('', 8); }}
        catch (ValueError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        $stream = fopen('php://memory', 'r');
        try {{ file('{}', 8, $stream); }}
        catch (ValueError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file('{}', 0, $stream); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ file(''); }}
        catch (ValueError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        ",
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal()
    );
    assert_eq!(
        run_php(&source),
        concat!(
            "TypeError:file(): Argument #1 ($filename) must be of type string, array given",
            "|TypeError:file(): Argument #2 ($flags) must be of type int, array given",
            "|TypeError:file(): Argument #3 ($context) must be of type resource or null, false given",
            "|TypeError:file(): Argument #3 ($context) must be of type resource or null, false given",
            "|ValueError:file(): Argument #2 ($flags) must be a valid flag value",
            "|ValueError:file(): Argument #2 ($flags) must be a valid flag value",
            "|TypeError:file(): supplied resource is not a valid Stream-Context resource",
            "|ValueError:Path must not be empty"
        )
    );
}

#[test]
#[cfg(all(
    feature = "stream-context",
    feature = "file-contents",
    feature = "file-write",
    feature = "file-lines"
))]
fn valid_stream_contexts_flow_through_bounded_file_surfaces() {
    let path = TemporaryPath::unique("stream-context-file");
    let source = format!(
        "<?php
        $context = stream_context_create();
        echo file_put_contents('{}', \"one\\ntwo\", 0, $context); echo ':';
        echo file_get_contents('{}', false, $context); echo ':';
        $lines = file('{}', FILE_IGNORE_NEW_LINES, $context);
        echo count($lines); echo ':'; echo $lines[0]; echo ':'; echo $lines[1];
        ",
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
    );
    assert_eq!(run_php(&source), "7:one\ntwo:2:one:two");
}
