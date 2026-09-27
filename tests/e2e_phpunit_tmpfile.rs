mod common;

use std::process::Command;

use common::run_php;

#[test]
fn tmpfile_matches_php_85_reflection_and_plain_file_lifecycle() {
    assert_eq!(
        run_php(
            r#"<?php
$reflection = new ReflectionFunction('tmpfile');
echo $reflection->getExtensionName(), '|',
    $reflection->getNumberOfParameters(), '/',
    $reflection->getNumberOfRequiredParameters(), '|',
    $reflection->hasReturnType() ? (string) $reflection->getReturnType() : 'none', "\n";

$stream = tmpfile();
$metadata = stream_get_meta_data($stream);
echo get_resource_type($stream), '|',
    $metadata['wrapper_type'], '|',
    $metadata['stream_type'], '|',
    $metadata['mode'], '|',
    (int) $metadata['seekable'], '|',
    (int) file_exists($metadata['uri']), '|',
    (int) (dirname($metadata['uri']) === sys_get_temp_dir()), "\n";

echo fwrite($stream, 'abc'), '|', ftell($stream), '|', (int) rewind($stream), '|',
    fread($stream, 3), '|', fstat($stream)['size'], "\n";

$alias = $stream;
unset($stream);
gc_collect_cycles();
echo (int) file_exists($metadata['uri']), '|';
fclose($alias);
echo (int) file_exists($metadata['uri']), '|', (int) is_resource($alias), '|',
    get_resource_type($alias), "\n";
"#,
        ),
        concat!(
            "standard|0/0|none\n",
            "stream|plainfile|STDIO|r+b|1|1|1\n",
            "3|3|1|abc|3\n",
            "1|0|0|Unknown\n",
        )
    );
}

#[test]
fn tmpfile_rejects_surplus_arguments_at_the_internal_call_boundary() {
    assert_eq!(
        run_php("<?php try { tmpfile(1); } catch (Throwable $e) { echo $e->getMessage(); }"),
        "tmpfile() expects exactly 0 arguments, 1 given",
    );
}

#[test]
fn tmpfile_path_is_removed_when_the_request_ends() {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-r", "$f=tmpfile();echo stream_get_meta_data($f)['uri'];"])
        .output()
        .expect("run RPHP tmpfile request");
    assert!(output.status.success(), "{:?}", output.stderr);
    let path = String::from_utf8(output.stdout).expect("temporary path is UTF-8");
    assert!(!path.is_empty());
    assert!(
        !std::path::Path::new(&path).exists(),
        "request-owned temporary file survived shutdown: {path}"
    );
}
