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
#[cfg(feature = "stream-context")]
fn stream_context_resources_round_trip_options_params_and_independent_streams() {
    assert_eq!(
        run_php(
            "<?php
            $context = stream_context_create(
                [
                    'file' => ['probe' => 'yes'],
                    'http' => ['method' => 'POST'],
                ],
                [
                    'notification' => 'strlen',
                    'ignored' => 'strlen',
                    'options' => ['file' => ['from_params' => 'merged']],
                ]
            );
            echo gettype($context); echo ':';
            echo get_resource_type($context); echo ':';
            $options = stream_context_get_options($context);
            echo $options['file']['probe']; echo ':';
            echo $options['file']['from_params']; echo ':';
            echo $options['http']['method']; echo ':';
            $params = stream_context_get_params($context);
            echo count($params); echo ':';
            echo $params['notification']; echo ':';
            echo $params['options']['file']['from_params']; echo ':';

            $stream = fopen('php://memory', 'w+', false, $context);
            echo get_resource_type($stream); echo ':';
            fwrite($stream, 'ok'); rewind($stream); echo fread($stream, 2); echo ':';
            $streamOptions = stream_context_get_options($stream);
            echo count($streamOptions); echo ':';
            $streamParams = stream_context_get_params($stream);
            echo count($streamParams['options']); echo ':';
            stream_context_set_option($stream, 'http', 'method', 'STREAM');
            $streamOptions = stream_context_get_options($stream);
            $contextOptions = stream_context_get_options($context);
            echo $streamOptions['http']['method']; echo ':';
            echo $contextOptions['http']['method'];
            "
        ),
        "resource:stream-context:yes:merged:POST:2:strlen:merged:stream:ok:0:0:STREAM:POST"
    );
}

#[test]
#[cfg(feature = "stream-context")]
fn default_stream_context_is_stable_and_merges_request_local_options() {
    assert_eq!(
        run_php(
            "<?php
            $first = stream_context_get_default();
            $second = stream_context_get_default([
                'http' => ['method' => 'GET'],
                'file' => ['probe' => 1],
            ]);
            echo get_resource_id($first) === get_resource_id($second) ? 'same:' : 'different:';
            $options = stream_context_get_options($first);
            echo $options['http']['method']; echo ':';
            echo $options['file']['probe']; echo ':';

            $third = stream_context_set_default([
                'http' => ['timeout' => 2],
                'ftp' => ['overwrite' => true],
            ]);
            echo get_resource_id($first) === get_resource_id($third) ? 'same:' : 'different:';
            $options = stream_context_get_options($third);
            echo $options['http']['method']; echo ':';
            echo $options['http']['timeout']; echo ':';
            echo $options['ftp']['overwrite']; echo ':';

            stream_context_set_option($first, 'file', 'after', 3);
            $options = stream_context_get_options(stream_context_get_default());
            echo $options['file']['after']; echo ':';

            $explicit = stream_context_create();
            echo get_resource_id($explicit) === get_resource_id($first) ? 'same:' : 'different:';
            echo count(stream_context_get_options($explicit)); echo ':';
            $stream = fopen('php://memory', 'w+');
            echo count(stream_context_get_options($stream)); echo ':';

            $id = get_resource_id($first);
            unset($first, $second, $third);
            echo $id === get_resource_id(stream_context_get_default()) ? 'same' : 'different';
            "
        ),
        "same:GET:1:same:GET:2:1:3:different:0:0:same"
    );
}

#[test]
#[cfg(feature = "stream-context")]
fn default_stream_context_errors_match_php_and_preserve_prior_updates() {
    assert_eq!(
        run_php(
            "<?php
            try { stream_context_get_default(false); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_context_set_default(null); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try {
                stream_context_get_default([
                    'http' => ['early' => 1],
                    'broken' => 1,
                ]);
            } catch (ValueError $error) { echo get_class($error); }
            $options = stream_context_get_options(stream_context_get_default());
            echo ':'; echo $options['http']['early']; echo '|';
            try {
                stream_context_set_default([
                    'file' => [0 => 'ignored', 'set' => 2],
                    0 => [],
                ]);
            } catch (ValueError $error) { echo get_class($error); }
            $options = stream_context_get_options(stream_context_get_default());
            echo ':'; echo $options['file']['set']; echo ':';
            echo count($options['file']);
            "
        ),
        concat!(
            "TypeError:stream_context_get_default(): Argument #1 ($options) must be of type ?array, false given",
            "|TypeError:stream_context_set_default(): Argument #1 ($options) must be of type array, null given",
            "|ValueError:1|ValueError:2:1"
        )
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn stream_registry_reports_only_integrated_wrappers_transports_and_filters() {
    assert_eq!(
        run_php(
            "<?php
            $wrappers = stream_get_wrappers();
            echo implode(',', $wrappers); echo ':';
            echo count(stream_get_transports()); echo ':';
            echo count(stream_get_filters()); echo ':';
            $wrappers[0] = 'changed';
            $fresh = stream_get_wrappers();
            echo $fresh[0];
            "
        ),
        "php,file:0:0:php"
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn stream_locality_matches_php_wrappers_resources_and_string_conversion() {
    assert_eq!(
        run_php(
            "<?php
            $cases = [
                ['relative.php', true],
                ['php://memory', true],
                ['glob://*.php', true],
                ['unknown://target', true],
                ['file:///tmp/file.php', true],
                ['file://localhost/tmp/file.php', true],
                ['http://example.com', false],
                ['data://text/plain,value', false],
                ['file://remote/tmp/file.php', false],
                ['file://localhost', false],
                ['a' . chr(0) . 'b', true],
            ];
            foreach ($cases as $case) {
                echo stream_is_local($case[0]) === $case[1];
            }
            echo '|';

            $stream = fopen('php://memory', 'w+');
            echo stream_is_local($stream); echo ':';
            fclose($stream);
            try { stream_is_local($stream); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            echo stream_is_local([]); echo stream_is_local(null); echo stream_is_local(1); echo '|';

            class RemoteStreamName {
                public function __toString(): string { return 'http://example.com'; }
            }
            echo stream_is_local(new RemoteStreamName()) === false; echo '|';
            try { stream_is_local(new stdClass()); }
            catch (Error $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_is_local(function () {}); }
            catch (Error $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            "
        ),
        concat!(
            "11111111111|1:",
            "TypeError:stream_is_local(): supplied resource is not a valid stream resource|",
            "111|1|",
            "Error:Object of class stdClass could not be converted to string|",
            "Error:Object of class Closure could not be converted to string"
        )
    );
}

#[test]
#[cfg(feature = "stream-line")]
fn stream_line_preserves_endings_limits_cursor_and_eof() {
    assert_eq!(
        run_php(
            "<?php
            $stream = fopen('php://memory', 'w+');
            fwrite($stream, 'ab--cd--ef'); rewind($stream);
            echo '['; echo stream_get_line($stream, 99, '--'); echo ']';
            echo ':'; echo ftell($stream); echo ':';
            echo '['; echo stream_get_line($stream, 4, '--'); echo ']';
            echo ':'; echo ftell($stream); echo ':';
            echo '['; echo stream_get_line($stream, 99, '--'); echo ']';
            echo ':'; echo feof($stream); echo ':';
            if (stream_get_line($stream, 99, '--') === false) { echo 'false'; }

            $unlimited = fopen('php://memory', 'w+');
            fwrite($unlimited, 'abcdef'); rewind($unlimited);
            echo ':['; echo stream_get_line($unlimited, 0); echo ']';
            echo ':'; echo feof($unlimited);

            $overlap = fopen('php://memory', 'w+');
            fwrite($overlap, 'ababaX'); rewind($overlap);
            echo ':['; echo stream_get_line($overlap, 99, 'aba'); echo ']';
            echo ':'; echo ftell($overlap);

            $nul = fopen('php://memory', 'w+');
            fwrite($nul, 'abc' . chr(0) . 'def'); rewind($nul);
            echo ':['; echo stream_get_line($nul, 99, chr(0)); echo ']';
            echo ':'; echo ftell($nul);
            "
        ),
        "[ab]:4:[cd]:8:[ef]:1:false:[abcdef]:1:[]:3:[abc]:4"
    );
}

#[test]
#[cfg(feature = "stream-line")]
fn stream_line_matches_php_argument_conversion_and_errors() {
    assert_eq!(
        run_php(
            "<?php
            try { stream_get_line(false, 1); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';

            $stream = fopen('php://memory', 'w+');
            fwrite($stream, 'abcdef'); rewind($stream);
            try { stream_get_line($stream, -1); }
            catch (ValueError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_get_line($stream, []); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_get_line($stream, 1, []); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_get_line($stream, 1, new stdClass()); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';

            class Ending {
                public function __toString(): string { return 'bc'; }
            }
            rewind($stream); echo '['; echo stream_get_line($stream, 99, new Ending()); echo ']';
            echo ':'; echo ftell($stream); echo '|';
            fclose($stream);
            try { stream_get_line($stream, 1); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            $writeOnly = fopen('php://memory', 'w');
            if (stream_get_line($writeOnly, 2) === false) { echo 'false'; }
            "
        ),
        concat!(
            "TypeError:stream_get_line(): Argument #1 ($stream) must be of type resource, false given|",
            "ValueError:stream_get_line(): Argument #2 ($length) must be greater than or equal to 0|",
            "TypeError:stream_get_line(): Argument #2 ($length) must be of type int, array given|",
            "TypeError:stream_get_line(): Argument #3 ($ending) must be of type string, array given|",
            "TypeError:stream_get_line(): Argument #3 ($ending) must be of type string, stdClass given|",
            "[a]:3|",
            "TypeError:stream_get_line(): supplied resource is not a valid stream resource|false"
        )
    );
}

#[test]
#[cfg(feature = "stream-truncate")]
fn stream_truncate_matches_backend_cursor_eof_and_growth_rules() {
    assert_eq!(
        run_php(
            "<?php
            $memory = fopen('php://memory', 'w+');
            fwrite($memory, 'abcdef');
            echo ftruncate($memory, 4); echo ':'; echo ftell($memory); echo ':';
            rewind($memory); echo '['; echo fread($memory, 8); echo ']';
            fread($memory, 1); echo ':'; echo feof($memory); echo ':';
            echo ftruncate($memory, 8); echo ':'; echo ftell($memory); echo ':'; echo feof($memory);
            rewind($memory); echo ':['; echo fread($memory, 8); echo ']';

            $memoryPast = fopen('php://memory', 'w+');
            fwrite($memoryPast, 'abcdef'); ftruncate($memoryPast, 2);
            echo ':'; echo fwrite($memoryPast, 'Z'); echo ':'; echo ftell($memoryPast);
            rewind($memoryPast); echo ':['; echo fread($memoryPast, 7); echo ']';

            $tempPast = fopen('php://temp/maxmemory:2', 'w+');
            fwrite($tempPast, 'abcdef'); ftruncate($tempPast, 2);
            fwrite($tempPast, 'Z'); rewind($tempPast);
            echo ':['; echo fread($tempPast, 7); echo ']';
            "
        ),
        concat!(
            "1:6:[abcd]:1:1:4:1:[abcd\0\0\0\0]",
            ":1:7:[abZ]:[ab\0\0\0\0Z]"
        )
    );
}

#[test]
#[cfg(feature = "stream-truncate")]
fn stream_truncate_matches_php_argument_errors_and_real_files() {
    let path = TemporaryPath::unique("stream-truncate");
    let source = format!(
        "<?php
        $stream = fopen('{}', 'w+');
        fwrite($stream, 'abcdef');
        echo ftruncate($stream, 2); echo ':'; echo ftell($stream); echo ':';
        fwrite($stream, 'Z'); rewind($stream); echo '['; echo fread($stream, 7); echo ']|';

        try {{ ftruncate(false, 1); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ ftruncate($stream, -1); }}
        catch (ValueError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        try {{ ftruncate($stream, []); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        fclose($stream);
        try {{ ftruncate($stream, 1); }}
        catch (TypeError $error) {{ echo get_class($error); echo ':'; echo $error->getMessage(); }}
        echo '|';
        $readOnly = fopen('php://memory', 'r');
        if (ftruncate($readOnly, 1) === false) {{ echo 'false'; }}
        ",
        path.php_literal()
    );
    assert_eq!(
        run_php(&source),
        concat!(
            "1:6:[ab\0\0\0\0Z]|",
            "TypeError:ftruncate(): Argument #1 ($stream) must be of type resource, false given|",
            "ValueError:ftruncate(): Argument #2 ($size) must be greater than or equal to 0|",
            "TypeError:ftruncate(): Argument #2 ($size) must be of type int, array given|",
            "TypeError:ftruncate(): supplied resource is not a valid stream resource|false"
        )
    );
}

#[test]
#[cfg(feature = "stream-context")]
fn stream_context_mutators_merge_context_and_stream_state() {
    assert_eq!(
        run_php(
            "<?php
            $context = stream_context_create(
                ['file' => ['a' => 1, 'keep' => 2]],
                ['notification' => 'strlen']
            );
            echo stream_context_set_option($context, 'file', 'a', 9); echo ':';
            echo stream_context_set_options($context, [
                'file' => ['extra' => 3],
                'http' => ['method' => 'POST'],
            ]); echo ':';
            echo stream_context_set_params($context, [
                'notification' => 'trim',
                'options' => ['http' => ['timeout' => 4]],
                'ignored' => true,
            ]); echo ':';
            $options = stream_context_get_options($context);
            echo $options['file']['a']; echo ':';
            echo $options['file']['keep']; echo ':';
            echo $options['file']['extra']; echo ':';
            echo $options['http']['method']; echo ':';
            echo $options['http']['timeout']; echo ':';
            $params = stream_context_get_params($context);
            echo $params['notification']; echo ':';
            try {
                stream_context_set_params($context, [
                    'notification' => 'strlen',
                    'options' => ['broken' => 1],
                ]);
            } catch (ValueError $error) {
                echo get_class($error); echo ':';
            }
            $params = stream_context_get_params($context);
            echo $params['notification']; echo ':';

            $stream = fopen('php://memory', 'w+');
            stream_context_set_options($stream, ['file' => ['stream' => 5]]);
            stream_context_set_params($stream, [
                'notification' => 'strlen',
                'options' => ['http' => ['stream' => 6]],
            ]);
            $streamParams = stream_context_get_params($stream);
            echo $streamParams['notification']; echo ':';
            echo $streamParams['options']['file']['stream']; echo ':';
            echo $streamParams['options']['http']['stream'];
            "
        ),
        "1:1:1:9:2:3:POST:4:trim:ValueError:strlen:strlen:5:6"
    );
}

#[test]
#[cfg(feature = "stream-context")]
fn stream_context_argument_errors_match_php_classes_and_messages() {
    assert_eq!(
        run_php(
            "<?php
            try { stream_context_create(false); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_context_create([], false); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_context_create(['file' => 1]); }
            catch (ValueError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_context_get_options(false); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { fopen('php://memory', 'r', [], null); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { fopen('php://memory', 'r', false, false); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            $stream = fopen('php://memory', 'w+');
            try { fopen('php://memory', 'r', false, $stream); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            fclose($stream);
            try { stream_context_get_params($stream); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_context_set_option(false, 'file', 'x', 1); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            $context = stream_context_create();
            try { stream_context_set_options($context, false); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_context_set_options($context, ['file' => 1]); }
            catch (ValueError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_context_set_option($context, 'file', 'x'); }
            catch (ValueError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_context_set_option($context, ['file' => ['x' => 1]], null, 1); }
            catch (ValueError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_context_set_params($context, ['notification' => 'missing_stream_context_callback']); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_context_set_params($context, ['options' => false]); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_context_set_params($stream, []); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            "
        ),
        concat!(
            "TypeError:stream_context_create(): Argument #1 ($options) must be of type ?array, false given",
            "|TypeError:stream_context_create(): Argument #2 ($params) must be of type ?array, false given",
            "|ValueError:Options should have the form [\"wrappername\"][\"optionname\"] = $value",
            "|TypeError:stream_context_get_options(): Argument #1 ($stream_or_context) must be of type resource, false given",
            "|TypeError:fopen(): Argument #3 ($use_include_path) must be of type bool, array given",
            "|TypeError:fopen(): Argument #4 ($context) must be of type resource or null, false given",
            "|TypeError:fopen(): supplied resource is not a valid Stream-Context resource",
            "|TypeError:stream_context_get_params(): Argument #1 ($context) must be a valid stream/context",
            "|TypeError:stream_context_set_option(): Argument #1 ($context) must be of type resource, false given",
            "|TypeError:stream_context_set_options(): Argument #2 ($options) must be of type array, false given",
            "|ValueError:Options should have the form [\"wrappername\"][\"optionname\"] = $value",
            "|ValueError:stream_context_set_option(): Argument #4 ($value) must be provided when argument #2 ($wrapper_or_options) is a string",
            "|ValueError:stream_context_set_option(): Argument #4 ($value) cannot be provided when argument #2 ($wrapper_or_options) is an array",
            "|TypeError:stream_context_set_params(): Argument #1 ($context) must be an array with valid callbacks as values, function \"missing_stream_context_callback\" not found or invalid function name",
            "|TypeError:Invalid stream/context parameter",
            "|TypeError:stream_context_set_params(): Argument #1 ($context) must be a valid stream/context"
        )
    );
}

#[test]
#[cfg(feature = "stream-contents")]
fn stream_contents_preserves_length_offset_cursor_and_eof() {
    assert_eq!(
        run_php(
            "<?php
            $stream = fopen('php://memory', 'w+');
            fwrite($stream, 'abcdef'); rewind($stream);
            echo '['; echo stream_get_contents($stream, 2); echo ']';
            echo ':'; echo ftell($stream);
            echo ':'; if (feof($stream)) { echo 'eof'; } else { echo 'open'; }
            echo ':['; echo stream_get_contents($stream, null, -99); echo ']';
            echo ':'; echo ftell($stream);
            echo ':'; if (feof($stream)) { echo 'eof'; } else { echo 'open'; }
            echo ':['; echo stream_get_contents($stream, 0, 3); echo ']';
            echo ':'; echo ftell($stream);
            echo ':'; if (feof($stream)) { echo 'eof'; } else { echo 'open'; }
            echo ':['; echo stream_get_contents($stream, -1, 1); echo ']';
            echo ':['; echo stream_get_contents($stream, 2, 10); echo ']';
            echo ':'; echo ftell($stream);
            echo ':'; if (feof($stream)) { echo 'eof'; } else { echo 'open'; }

            $temp = fopen('php://temp/maxmemory:4', 'w+');
            fwrite($temp, 'abcdefghij');
            echo ':['; echo stream_get_contents($temp, 5, 3); echo ']';
            echo ':'; echo ftell($temp);

            $exact = fopen('php://memory', 'w+');
            fwrite($exact, 'xyz'); rewind($exact);
            echo ':['; echo stream_get_contents($exact, 3); echo ']';
            echo ':'; if (feof($exact)) { echo 'eof'; } else { echo 'exact-open'; }

            $write_only = fopen('php://memory', 'w');
            echo ':';
            if (stream_get_contents($write_only) === false) { echo 'unreadable'; }
            "
        ),
        "[ab]:2:open:[cdef]:6:eof:[]:3:open:[bcdef]:[]:10:eof:[defgh]:8:[xyz]:exact-open:unreadable"
    );
}

#[test]
#[cfg(feature = "stream-contents")]
fn stream_contents_argument_errors_match_php_classes_and_messages() {
    assert_eq!(
        run_php(
            "<?php
            $stream = fopen('php://memory', 'w+');
            try { stream_get_contents(false); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_get_contents($stream, -2); }
            catch (ValueError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_get_contents($stream, []); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_get_contents($stream, 1, new stdClass()); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            fclose($stream); echo '|';
            try { stream_get_contents($stream); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            "
        ),
        concat!(
            "TypeError:stream_get_contents(): Argument #1 ($stream) must be of type resource, false given",
            "|ValueError:stream_get_contents(): Argument #2 ($length) must be greater than or equal to -1",
            "|TypeError:stream_get_contents(): Argument #2 ($length) must be of type ?int, array given",
            "|TypeError:stream_get_contents(): Argument #3 ($offset) must be of type int, stdClass given",
            "|TypeError:stream_get_contents(): Argument #1 ($stream) must be an open stream resource"
        )
    );
}

#[test]
#[cfg(feature = "stream-copy")]
fn stream_copy_preserves_php_length_offset_cursor_eof_and_failure_rules() {
    assert_eq!(
        run_php(
            "<?php
            $source = fopen('php://memory', 'w+');
            fwrite($source, 'abcdef'); fseek($source, 2);
            $destination = fopen('php://temp/maxmemory:2', 'w+');
            echo stream_copy_to_stream($source, $destination);
            echo ':'; echo ftell($source);
            echo ':'; if (feof($source)) { echo 'eof'; }
            echo ':'; echo ftell($destination); echo ':';
            rewind($destination); echo fread($destination, 100);

            $exact = fopen('php://memory', 'w+');
            fwrite($exact, 'abcdef'); fseek($exact, 4);
            $exact_destination = fopen('php://memory', 'w+');
            echo ':'; echo stream_copy_to_stream($exact, $exact_destination, 2, 0);
            echo ':'; echo ftell($exact);
            echo ':'; if (feof($exact)) { echo 'eof'; } else { echo 'exact-open'; }
            echo ':'; rewind($exact_destination); echo fread($exact_destination, 100);

            $offset = fopen('php://memory', 'w+');
            fwrite($offset, 'abcdef'); fseek($offset, 4);
            $offset_destination = fopen('php://memory', 'w+');
            echo ':'; echo stream_copy_to_stream($offset, $offset_destination, -9, 1);
            echo ':'; echo ftell($offset);
            echo ':'; if (feof($offset)) { echo 'eof'; }
            echo ':'; rewind($offset_destination); echo fread($offset_destination, 100);

            $beyond = fopen('php://memory', 'w+');
            fwrite($beyond, 'abc');
            echo ':'; echo stream_copy_to_stream($beyond, $offset_destination, null, 10);
            echo ':'; echo ftell($beyond);
            echo ':'; if (feof($beyond)) { echo 'eof'; }

            $same = fopen('php://memory', 'w+');
            fwrite($same, 'abcdef'); rewind($same);
            echo ':'; echo stream_copy_to_stream($same, $same);
            echo ':'; echo ftell($same); echo ':';
            rewind($same); echo fread($same, 100);

            $write_failure_source = fopen('php://memory', 'w+');
            fwrite($write_failure_source, 'xyz'); rewind($write_failure_source);
            $read_only = fopen('php://memory', 'r');
            echo ':';
            if (stream_copy_to_stream($write_failure_source, $read_only) === false) {
                echo 'write-failed';
            }
            echo ':'; echo ftell($write_failure_source);
            "
        ),
        concat!(
            "4:6:eof:4:cdef",
            ":2:6:exact-open:ef",
            ":5:6:eof:bcdef",
            ":0:10:eof",
            ":6:12:abcdefabcdef",
            ":write-failed:3"
        )
    );
}

#[test]
#[cfg(feature = "stream-copy")]
fn stream_copy_crosses_fixed_chunks_between_real_files() {
    let source_path = TemporaryPath::unique("stream-copy-source");
    let destination_path = TemporaryPath::unique("stream-copy-destination");
    let unreadable_path = TemporaryPath::unique("stream-copy-unreadable");
    let payload: Vec<u8> = (0..20_000).map(|index| (index % 251) as u8).collect();
    std::fs::write(&source_path.0, &payload).unwrap();
    std::fs::write(&unreadable_path.0, b"unreadable").unwrap();
    let source = format!(
        "<?php
        $source = fopen('{}', 'r');
        $destination = fopen('{}', 'w');
        echo stream_copy_to_stream($source, $destination); echo ':';
        echo ftell($source); echo ':'; echo ftell($destination); echo ':';
        if (feof($source)) {{ echo 'eof'; }}

        $unreadable = fopen('{}', 'w');
        echo ':';
        if (stream_copy_to_stream($unreadable, $destination) === false) {{
            echo 'read-failed';
        }}
        echo ':'; echo ftell($unreadable);
        ",
        source_path.php_literal(),
        destination_path.php_literal(),
        unreadable_path.php_literal()
    );
    assert_eq!(run_php(&source), "20000:20000:20000:eof:read-failed:0");
    assert_eq!(std::fs::read(&destination_path.0).unwrap(), payload);
}

#[test]
#[cfg(feature = "stream-copy")]
fn stream_copy_argument_errors_match_php_classes_and_messages() {
    assert_eq!(
        run_php(
            "<?php
            $stream = fopen('php://memory', 'w+');
            try { stream_copy_to_stream(false, $stream); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_copy_to_stream($stream, false); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_copy_to_stream($stream, $stream, []); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            echo '|';
            try { stream_copy_to_stream($stream, $stream, 1, new stdClass()); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            $closed = fopen('php://memory', 'w+'); fclose($closed); echo '|';
            try { stream_copy_to_stream($stream, $closed); }
            catch (TypeError $error) { echo get_class($error); echo ':'; echo $error->getMessage(); }
            "
        ),
        concat!(
            "TypeError:stream_copy_to_stream(): Argument #1 ($from) must be of type resource, false given",
            "|TypeError:stream_copy_to_stream(): Argument #2 ($to) must be of type resource, false given",
            "|TypeError:stream_copy_to_stream(): Argument #3 ($length) must be of type ?int, array given",
            "|TypeError:stream_copy_to_stream(): Argument #4 ($offset) must be of type int, stdClass given",
            "|TypeError:stream_copy_to_stream(): Argument #2 ($to) must be an open stream resource"
        )
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
#[cfg(feature = "stream-contents")]
fn stream_contents_reads_real_files_from_an_absolute_offset() {
    let path = TemporaryPath::unique("stream-contents");
    std::fs::write(&path.0, b"0123456789").unwrap();
    let source = format!(
        "<?php
        $stream = fopen('{}', 'r');
        echo stream_get_contents($stream, 4, 3); echo ':';
        echo ftell($stream); echo ':';
        echo stream_get_contents($stream); echo ':';
        if (feof($stream)) {{ echo 'eof'; }}
        ",
        path.php_literal()
    );
    assert_eq!(run_php(&source), "3456:7:789:eof");
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

#[test]
fn csv_records_preserve_multiline_quotes_empty_lines_and_eof() {
    assert_eq!(
        run_php(
            r#"<?php
            $stream = fopen('php://temp/maxmemory:4', 'w+');
            fwrite($stream, "a,\"two\nlines\",\"b\"\"c\"\r\n\r\n");
            rewind($stream);
            $row = fgetcsv($stream);
            echo count($row); echo ':'; echo $row[0]; echo ':';
            echo $row[1]; echo ':'; echo $row[2];
            $blank = fgetcsv($stream);
            echo ':';
            if ($blank[0] === null) { echo 'null'; }
            echo ':';
            if (fgetcsv($stream) === false) { echo 'eof'; }
            "#,
        ),
        "3:a:two\nlines:b\"c:null:eof"
    );
}

#[test]
fn csv_length_and_custom_controls_preserve_exact_cursor() {
    assert_eq!(
        run_php(
            r#"<?php
            $stream = fopen('php://memory', 'w+');
            fwrite($stream, "\"abcdef\";x\nnext;row\n");
            rewind($stream);
            $bounded = fgetcsv($stream, 8, ';', '"', '');
            echo count($bounded); echo ':'; echo $bounded[0];
            echo ':'; echo ftell($stream);
            $tail = fgetcsv($stream, null, ';', '"', '');
            echo ':'; echo count($tail); echo ':'; echo $tail[0];
            echo ':'; echo $tail[1]; echo ':'; echo ftell($stream);

            rewind($stream);
            $continued = fgetcsv($stream, 4, ';', '"', '');
            echo ':'; echo $continued[0]; echo ':'; echo $continued[1];
            echo ':'; echo ftell($stream);

            rewind($stream);
            try {
                if (fgetcsv($stream, null, '::', '"', '') === false) { echo ':invalid'; }
            }
            catch (ValueError $error) { echo ':invalid'; }
            try {
                if (fgetcsv($stream, null, ',', 'xx', '') === false) { echo ':enclosure'; }
            }
            catch (ValueError $error) { echo ':enclosure'; }
            try {
                if (fgetcsv($stream, null, ',', '"', 'xx') === false) { echo ':escape'; }
            }
            catch (ValueError $error) { echo ':escape'; }
            echo ':'; echo ftell($stream);
            "#,
        ),
        "1:abcdef:8:2::x:11:abcdef:x:11:invalid:enclosure:escape:0"
    );
}

#[test]
#[cfg(feature = "csv-errors")]
fn csv_read_argument_errors_match_php_classes_and_messages() {
    assert_eq!(
        run_php(
            r#"<?php
            try { fgetcsv('not-a-stream'); }
            catch (TypeError $error) {
                echo get_class($error); echo ':'; echo $error->getMessage();
            }

            $stream = fopen('php://memory', 'w+');
            try { fgetcsv($stream, -1); }
            catch (Error $error) {
                echo '|'; echo get_class($error); echo ':'; echo $error->getMessage();
            }
            try { fgetcsv($stream, null, []); }
            catch (TypeError $error) {
                echo '|'; echo get_class($error); echo ':'; echo $error->getMessage();
            }
            fclose($stream);
            try { fgetcsv($stream); }
            catch (TypeError $error) {
                echo '|'; echo get_class($error); echo ':'; echo $error->getMessage();
            }
            "#,
        ),
        concat!(
            "TypeError:fgetcsv(): Argument #1 ($stream) must be of type resource, string given",
            "|ValueError:fgetcsv(): Argument #2 ($length) must be between 0 and 9223372036854775806",
            "|TypeError:fgetcsv(): Argument #3 ($separator) must be of type string, array given",
            "|TypeError:fgetcsv(): Argument #1 ($stream) must be an open stream resource"
        )
    );
}

#[test]
fn csv_quote_edges_match_php_byte_rules() {
    assert_eq!(
        run_php(
            r#"<?php
            $stream = fopen('php://memory', 'w+');
            fwrite($stream, "  \"a,b\",c\nx\"y,z\n\"a\"tail,b\n\"a\"  ,b\n\"a\" \"b\",c\n");
            rewind($stream);

            $one = fgetcsv($stream, null, ',', '"', '');
            echo '['; echo $one[0]; echo ']['; echo $one[1]; echo ']';
            $two = fgetcsv($stream, null, ',', '"', '');
            echo ':['; echo $two[0]; echo ']['; echo $two[1]; echo ']';
            $three = fgetcsv($stream, null, ',', '"', '');
            echo ':['; echo $three[0]; echo ']['; echo $three[1]; echo ']';
            $four = fgetcsv($stream, null, ',', '"', '');
            echo ':['; echo $four[0]; echo ']['; echo $four[1]; echo ']';
            $five = fgetcsv($stream, null, ',', '"', '');
            echo ':['; echo $five[0]; echo ']['; echo $five[1]; echo ']';

            $escaped = fopen('php://memory', 'w+');
            fwrite($escaped, 'a,"b\"c",d' . "\n");
            rewind($escaped);
            $row = fgetcsv($escaped, null, ',', '"', '\\');
            echo ':['; echo $row[1]; echo ']';
            "#,
        ),
        "[a,b][c]:[x\"y][z]:[atail][b]:[a  ][b]:[a \"b\"][c]:[b\\\"c]"
    );
}

#[test]
#[cfg(feature = "csv-write")]
fn csv_writes_quote_php_special_bytes_and_report_length() {
    assert_eq!(
        run_php(
            r#"<?php
            $stream = fopen('php://memory', 'w+');
            $length = fputcsv(
                $stream,
                ['plain', 'a b', "a\tb", 'a,b', 'a"b', 'a\\b', "a\nb", ''],
                ',',
                '"',
                '\\'
            );
            echo $length; echo ':'; echo ftell($stream); echo ':';
            rewind($stream); echo fread($stream, 1000);
            "#,
        ),
        "44:44:plain,\"a b\",\"a\tb\",\"a,b\",\"a\"\"b\",\"a\\b\",\"a\nb\",\n"
    );
}

#[test]
#[cfg(feature = "csv-write")]
fn csv_writes_custom_controls_eol_scalars_and_empty_records() {
    assert_eq!(
        run_php(
            r#"<?php
            $stream = fopen('php://temp/maxmemory:4', 'w+');
            echo fputcsv($stream, [null, false, true, 0, 1, 1.5, ''], ';', '~', '', '<EOL>');
            echo ':';
            echo fputcsv($stream, [], ';', '~', '', '');
            echo ':';
            try { fputcsv($stream, ['x'], '::', '~', '', "\n"); }
            catch (ValueError $error) { echo 'separator'; }
            try { fputcsv($stream, ['x'], ';', 'xx', '', "\n"); }
            catch (ValueError $error) { echo ':enclosure'; }
            try { fputcsv($stream, ['x'], ';', '~', 'xx', "\n"); }
            catch (ValueError $error) { echo ':escape'; }
            echo ':'; echo fputcsv($stream, ['z'], ';', '~', '', null);
            echo ':'; echo ftell($stream); echo ':';
            rewind($stream); echo fread($stream, 1000);
            "#,
        ),
        "17:0:separator:enclosure:escape:2:19:;;1;0;1;1.5;<EOL>z\n"
    );
}

#[test]
#[cfg(feature = "csv-write")]
fn csv_writes_preserve_legacy_escape_byte_behavior() {
    assert_eq!(
        run_php(
            r#"<?php
            $stream = fopen('php://memory', 'w+');
            fputcsv($stream, ['a\\"b', 'a\\\\b', 'a"b'], ',', '"', '\\', '<EOL>');
            fputcsv($stream, ['a\\"b', 'a\\\\b', 'a"b'], ',', '"', '', '');
            rewind($stream); echo fread($stream, 1000);
            "#,
        ),
        "\"a\\\"b\",\"a\\\\b\",\"a\"\"b\"<EOL>\"a\\\"\"b\",a\\\\b,\"a\"\"b\""
    );
}

#[test]
#[cfg(feature = "csv-write")]
fn csv_writes_use_array_order_and_reject_unwritable_streams() {
    assert_eq!(
        run_php(
            r#"<?php
            $stream = fopen('php://memory', 'w+');
            $fields = ['second' => 'two', 10 => 'ten', 'last' => 'x,y'];
            echo fputcsv($stream, $fields, ',', '"', '', "\r\n");
            echo ':';
            rewind($stream); echo fread($stream, 1000);

            $read_only = fopen('php://memory', 'r');
            if (fputcsv($read_only, ['no']) === false) { echo ':readonly'; }
            try { fputcsv($stream, 'not-an-array'); }
            catch (TypeError $error) { echo ':fields'; }
            "#,
        ),
        "15:two,ten,\"x,y\"\r\n:readonly:fields"
    );
}

#[test]
#[cfg(feature = "csv-write")]
fn csv_write_argument_errors_match_php_classes_and_messages() {
    assert_eq!(
        run_php(
            r#"<?php
            try { fputcsv('not-a-stream', ['x'], ',', '"', ''); }
            catch (TypeError $error) {
                echo get_class($error); echo ':'; echo $error->getMessage();
            }

            $stream = fopen('php://memory', 'w+');
            try { fputcsv($stream, 'not-an-array', ',', '"', ''); }
            catch (TypeError $error) {
                echo '|'; echo get_class($error); echo ':'; echo $error->getMessage();
            }
            try { fputcsv($stream, ['x'], ',', '"', 'xx'); }
            catch (ValueError $error) {
                echo '|'; echo get_class($error); echo ':'; echo $error->getMessage();
            }
            try { fputcsv($stream, ['x'], ',', '"', '', []); }
            catch (TypeError $error) {
                echo '|'; echo get_class($error); echo ':'; echo $error->getMessage();
            }
            "#,
        ),
        concat!(
            "TypeError:fputcsv(): Argument #1 ($stream) must be of type resource, string given",
            "|TypeError:fputcsv(): Argument #2 ($fields) must be of type array, string given",
            "|ValueError:fputcsv(): Argument #5 ($escape) must be empty or a single character",
            "|TypeError:fputcsv(): Argument #6 ($eol) must be of type ?string, array given"
        )
    );
}
