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
fn memory_stream_round_trip_preserves_position_and_eof() {
    assert_eq!(
        run_php(
            "<?php
            $stream = fopen('php://memory', 'w+');
            echo gettype($stream); echo ':';
            echo get_resource_type($stream); echo ':';
            echo fwrite($stream, 'abcdef', 4); echo ':';
            echo ftell($stream); echo ':';
            echo rewind($stream); echo ':';
            echo fread($stream, 3); echo ':';
            echo ftell($stream); echo ':';
            if (feof($stream)) { echo 'early'; } else { echo 'open'; }
            echo ':'; echo fread($stream, 3); echo ':';
            if (feof($stream)) { echo 'eof'; } else { echo 'not-yet'; }
            echo ':'; echo fread($stream, 1); echo ':';
            if (feof($stream)) { echo 'eof'; } else { echo 'open'; }
            "
        ),
        "resource:stream:4:4:1:abc:3:open:d:not-yet::eof"
    );
}

#[test]
fn temporary_stream_spills_and_retains_seekable_contents() {
    assert_eq!(
        run_php(
            "<?php
            $stream = fopen('php://temp/maxmemory:4', 'w+');
            echo fwrite($stream, 'abcdef'); echo ':';
            echo fseek($stream, -3, SEEK_END); echo ':';
            echo fread($stream, 3); echo ':';
            echo ftell($stream); echo ':';
            echo fclose($stream);
            "
        ),
        "6:0:def:6:1"
    );
}

#[test]
fn line_reads_preserve_newlines_limits_cursor_and_eof() {
    assert_eq!(
        run_php(
            "<?php
            $stream = fopen('php://memory', 'w+');
            fwrite($stream, \"a\\nbc\\nlast\"); rewind($stream);
            echo '['; echo fgets($stream); echo ']';
            echo '['; echo fgets($stream, 4); echo ']';
            echo '['; echo fgets($stream, 3); echo ']';
            echo '['; echo fgets($stream); echo ']';
            echo ':'; if (feof($stream)) { echo 'eof'; }
            echo ':'; if (fgets($stream) === false) { echo 'false'; }
            rewind($stream);
            echo ':'; if (fgets($stream, 1) === false) { echo 'zero'; }
            echo ':'; echo ftell($stream);

            $temp = fopen('php://temp/maxmemory:4', 'w+');
            fwrite($temp, \"x\\nyz\"); rewind($temp);
            echo ':'; echo strlen(fgets($temp));
            echo ':'; echo strlen(fgets($temp));
            echo ':'; if (feof($temp)) { echo 'eof'; }
            "
        ),
        "[a\n][bc\n][la][st]:eof:false:zero:0:2:2:eof"
    );
}

#[test]
fn memory_and_temp_metadata_report_backend_specific_snapshots() {
    assert_eq!(
        run_php(
            "<?php
            $memory = fopen('php://memory', 'w+');
            $meta = stream_get_meta_data($memory);
            echo count($meta); echo ':'; echo $meta['wrapper_type'];
            echo ':'; echo $meta['stream_type']; echo ':'; echo $meta['mode'];
            echo ':'; echo $meta['unread_bytes']; echo ':'; echo $meta['seekable'];
            echo ':'; echo $meta['uri'];
            echo ':'; if ($meta['timed_out']) { echo 'timeout'; } else { echo 'ready'; }
            echo ':'; if ($meta['blocked']) { echo 'blocked'; }
            echo ':'; if ($meta['eof']) { echo 'eof'; } else { echo 'open'; }
            fgets($memory);
            $after = stream_get_meta_data($memory);
            echo ':'; if ($after['eof']) { echo 'eof'; }

            $temp = fopen('php://temp/maxmemory:4', 'a+');
            $meta = stream_get_meta_data($temp);
            echo ':'; echo count($meta); echo ':'; echo $meta['wrapper_type'];
            echo ':'; echo $meta['stream_type']; echo ':'; echo $meta['mode'];
            echo ':'; echo $meta['unread_bytes']; echo ':'; echo $meta['seekable'];
            echo ':'; echo $meta['uri'];
            fclose($temp);
            echo ':'; if (stream_get_meta_data($temp) === false) { echo 'closed'; }
            "
        ),
        "9:PHP:MEMORY:w+b:0:1:php://memory:ready:blocked:open:eof:6:PHP:TEMP:a+b:0:1:php://temp/maxmemory:4:closed"
    );
}

#[test]
fn closing_one_alias_invalidates_every_alias_but_preserves_id() {
    assert_eq!(
        run_php(
            "<?php
            $stream = fopen('php://memory', 'w+');
            $alias = $stream;
            $id = get_resource_id($stream);
            if ($stream === $alias) { echo 'same'; }
            echo ':';
            if (intval($stream) === $id) { echo 'numeric'; }
            echo ':'; echo fclose($stream); echo ':';
            if (is_resource($alias)) { echo 'open'; } else { echo 'closed'; }
            echo ':'; echo gettype($alias); echo ':';
            echo get_resource_type($alias); echo ':';
            if (get_resource_id($alias) === $id) { echo 'same-id'; }
            echo ':';
            if (fclose($alias)) { echo 'twice'; } else { echo 'once'; }
            echo ':';
            if (fread($alias, 1) === false) { echo 'unusable'; }
            "
        ),
        "same:numeric:1:closed:resource (closed):Unknown:same-id:once:unusable"
    );
}

#[test]
fn seek_constants_and_append_mode_follow_stream_policy() {
    assert_eq!(
        run_php(
            "<?php
            $stream = fopen('php://memory', 'a+');
            fwrite($stream, 'ab');
            fseek($stream, 0, SEEK_SET);
            fwrite($stream, 'c');
            fseek($stream, -2, SEEK_END);
            echo fread($stream, 2); echo ':';
            echo fseek($stream, -1, SEEK_SET); echo ':';
            echo fflush($stream); echo ':';
            echo fclose($stream);
            "
        ),
        "bc:-1:1:1"
    );
}

#[test]
fn unsupported_wrapper_and_invalid_mode_fail_without_a_resource() {
    assert_eq!(
        run_php(
            "<?php
            $first = fopen('http://example.invalid', 'r');
            $second = fopen('php://memory', 'r++');
            if ($first === false) { echo 'wrapper'; }
            echo ':';
            if ($second === false) { echo 'mode'; }
            echo ':';
            if (is_resource($first)) { echo 'resource'; } else { echo 'scalar'; }
            "
        ),
        "wrapper:mode:scalar"
    );
}

#[test]
fn resource_survives_and_cleans_up_through_large_frame_fallback() {
    assert_eq!(
        run_php(
            "<?php
            function large_resource_frame() {
                $v00=0; $v01=1; $v02=2; $v03=3; $v04=4; $v05=5;
                $v06=6; $v07=7; $v08=8; $v09=9; $v10=10; $v11=11;
                $v12=12; $v13=13; $v14=14; $v15=15; $v16=16; $v17=17;
                $v18=18; $v19=19; $v20=20; $v21=21; $v22=22; $v23=23;
                $v24=24; $v25=25; $v26=26; $v27=27; $v28=28; $v29=29;
                $v30=30; $v31=31; $v32=32; $v33=33; $v34=34; $v35=35;
                $v36=36; $v37=37; $v38=38; $v39=39; $v40=40; $v41=41;
                $v42=42; $v43=43; $v44=44; $v45=45; $v46=46; $v47=47;
                $v48=48; $v49=49; $v50=50; $v51=51; $v52=52; $v53=53;
                $v54=54; $v55=55; $v56=56; $v57=57; $v58=58; $v59=59;
                $v60=60; $v61=61; $v62=62; $v63=63; $v64=64; $v65=65;
                $stream = fopen('php://memory', 'w+');
                fwrite($stream, 'ok');
                rewind($stream);
                return fread($stream, 2);
            }
            echo large_resource_frame(); echo ':'; echo large_resource_frame();
            "
        ),
        "ok:ok"
    );
}

#[test]
fn file_stream_reads_seeks_writes_and_flushes_real_files() {
    let path = TemporaryPath::unique("stream");
    std::fs::write(&path.0, b"abcdef").unwrap();
    let source = format!(
        "<?php
        $stream = fopen('{}', 'r+');
        echo fread($stream, 2); echo ':';
        echo fseek($stream, -2, SEEK_END); echo ':';
        echo fwrite($stream, 'XY'); echo ':';
        echo fflush($stream); echo ':';
        rewind($stream); echo fread($stream, 6); echo ':';
        echo fclose($stream);
        ",
        path.php_literal()
    );
    assert_eq!(run_php(&source), "ab:0:2:1:abcdXY:1");
    assert_eq!(std::fs::read(&path.0).unwrap(), b"abcdXY");
}

#[test]
fn file_metadata_retains_original_uri_and_mode() {
    let path = TemporaryPath::unique("stream-metadata");
    std::fs::write(&path.0, b"line\n").unwrap();
    let source = format!(
        "<?php
        $stream = fopen('file://{}', 'r+');
        $meta = stream_get_meta_data($stream);
        echo count($meta); echo ':'; echo $meta['wrapper_type'];
        echo ':'; echo $meta['stream_type']; echo ':'; echo $meta['mode'];
        echo ':'; echo $meta['unread_bytes']; echo ':'; echo $meta['seekable'];
        echo ':'; if ($meta['uri'] === 'file://{}') {{ echo 'uri'; }}
        echo ':'; if ($meta['timed_out']) {{ echo 'timeout'; }} else {{ echo 'ready'; }}
        echo ':'; if ($meta['blocked']) {{ echo 'blocked'; }}
        echo ':'; if ($meta['eof']) {{ echo 'eof'; }} else {{ echo 'open'; }}
        ",
        path.php_literal(),
        path.php_literal()
    );
    assert_eq!(
        run_php(&source),
        "9:plainfile:STDIO:r+:0:1:uri:ready:blocked:open"
    );
}

#[test]
fn file_modes_cover_truncate_append_exclusive_and_non_truncating_create() {
    let path = TemporaryPath::unique("stream-modes");
    let source = format!(
        "<?php
        $write = fopen('{}', 'w');
        fwrite($write, 'one'); fclose($write);
        $append = fopen('file://{}', 'a+');
        fseek($append, 0, SEEK_SET);
        fwrite($append, 'two'); rewind($append);
        echo fread($append, 6); fclose($append); echo ':';
        $exclusive = fopen('{}', 'x');
        if ($exclusive === false) {{ echo 'exclusive'; }}
        echo ':';
        $create = fopen('{}', 'c+');
        fwrite($create, 'X'); rewind($create);
        echo fread($create, 6); fclose($create);
        ",
        path.php_literal(),
        path.php_literal(),
        path.php_literal(),
        path.php_literal()
    );
    assert_eq!(run_php(&source), "onetwo:exclusive:Xnetwo");
    assert_eq!(std::fs::read(&path.0).unwrap(), b"Xnetwo");
}
