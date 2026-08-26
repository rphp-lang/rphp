mod common;

use std::sync::atomic::{AtomicU64, Ordering};

use common::run_php;

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TemporaryPath(std::path::PathBuf);

impl TemporaryPath {
    fn unique(extension: &str) -> Self {
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "rphp-get-meta-tags-{}-{sequence}.{extension}",
            std::process::id()
        )))
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
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn get_meta_tags_extracts_general_and_malformed_html_boundaries() {
    let fixture = TemporaryPath::unique("html");
    let path = fixture.php_literal();
    assert_eq!(
        run_php(&format!(
            r#"<?php
$path = '{path}';
$cases = [
    '<meta name="Author" content="Ada"><meta name="geo.position" content="1;2">',
    '<META CONTENT="first" NAME="X.Y"><meta content=second name=z>',
    '<meta name=a content=one><meta name=A content=two>',
    '<meta http-equiv="refresh" content="5"><meta property="og:title" content="ignored">',
    '<meta name=only><meta content=value><meta name="" content=empty>',
    '<meta name="a&amp;b" content="x&amp;y&#33;">',
    '<html><head><meta name=a content=one></head><meta name=b content=two></html>',
    '<!-- <meta name=a content=bad> --><script>"<meta name=b content=bad>"</script><meta name=c content=good>',
    "<meta name=\"author\" content=\"name\"\n<meta name=\"keywords\" content=\"words\">",
    '<meta <meta name="keywords" content="words">',
    '<meta name=x content="a>b"><meta name=y content=c>',
    '<meta name= x content=y>',
    '<meta name="a b" content="c d">',
    '<meta name="A.B-C:D/E[F] G" content=v>',
    '<metadata name=bad content=x><meta name=good content=y>',
    '<meta name=first name=second content=one content=two>',
    '<meta name=a content=one',
];
foreach ($cases as $html) {{
    file_put_contents($path, $html);
    echo json_encode(get_meta_tags($path), JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE), "\n";
}}
unlink($path);
"#
        )),
        concat!(
            "{\"author\":\"Ada\",\"geo_position\":\"1;2\"}\n",
            "{\"x_y\":\"first\",\"z\":\"second\"}\n",
            "{\"a\":\"two\"}\n",
            "[]\n",
            "{\"only\":\"\",\"\":\"empty\"}\n",
            "{\"a&amp;b\":\"x&amp;y&#33;\"}\n",
            "{\"a\":\"one\"}\n",
            "{\"a\":\"bad\",\"b\":\"bad\",\"c\":\"good\"}\n",
            "{\"keywords\":\"words\"}\n",
            "{\"keywords\":\"words\"}\n",
            "{\"x\":\"a\",\"y\":\"c\"}\n",
            "[]\n",
            "{\"a_b\":\"c d\"}\n",
            "{\"a_b-c:d/e_f__g\":\"v\"}\n",
            "{\"good\":\"y\"}\n",
            "{\"second\":\"two\"}\n",
            "[]\n",
        )
    );
}

#[test]
fn get_meta_tags_preserves_calls_references_files_and_reflection() {
    let fixture = TemporaryPath::unique("html");
    std::fs::write(
        &fixture.0,
        b"<meta content=first name=Alpha><meta name=beta content=second>",
    )
    .unwrap();
    let path = fixture.php_literal();
    let missing = format!("{path}.missing");
    assert_eq!(
        run_php(&format!(
            r#"<?php
$path = '{path}';
$copy = $path;
$reference =& $path;
$dynamic = 'get_meta_tags';
$firstClass = get_meta_tags(...);
$calls = [
    get_meta_tags($path),
    $dynamic($path),
    $firstClass($path),
    call_user_func('get_meta_tags', $path),
    get_meta_tags(filename: $path),
    get_meta_tags(...[$path, false]),
    get_meta_tags('file://' . $path),
];
foreach ($calls as $value) {{
    echo json_encode($value), "\n";
}}
echo ($path === $copy && $path === $reference) ? "stable\n" : "mutated\n";

$reflection = new ReflectionFunction('get_meta_tags');
echo $reflection->getName(), '|', $reflection->getNumberOfRequiredParameters(), '/',
    $reflection->getNumberOfParameters(), '|', $reflection->getReturnType(), "\n";
foreach ($reflection->getParameters() as $parameter) {{
    echo $parameter->getName(), ':', $parameter->getType(), ':',
        $parameter->isDefaultValueAvailable() ? var_export($parameter->getDefaultValue(), true) : '-', "\n";
}}

set_error_handler(static function (int $level, string $message): bool {{
    echo "diag=$level:$message\n";
    return true;
}});
var_dump(get_meta_tags('{missing}'));
restore_error_handler();
set_error_handler(static function (): never {{ throw new RuntimeException('warning-stop'); }});
try {{ get_meta_tags('{missing}'); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
restore_error_handler();
"#
        )),
        format!(
            concat!(
                "{{\"alpha\":\"first\",\"beta\":\"second\"}}\n",
                "{{\"alpha\":\"first\",\"beta\":\"second\"}}\n",
                "{{\"alpha\":\"first\",\"beta\":\"second\"}}\n",
                "{{\"alpha\":\"first\",\"beta\":\"second\"}}\n",
                "{{\"alpha\":\"first\",\"beta\":\"second\"}}\n",
                "{{\"alpha\":\"first\",\"beta\":\"second\"}}\n",
                "{{\"alpha\":\"first\",\"beta\":\"second\"}}\n",
                "stable\n",
                "get_meta_tags|1/2|array|false\n",
                "filename:string:-\n",
                "use_include_path:bool:false\n",
                "diag=2:get_meta_tags({missing}): Failed to open stream: No such file or directory\n",
                "bool(false)\n",
                "RuntimeException:warning-stop\n",
            ),
            missing = missing,
        )
    );
}

#[test]
fn get_meta_tags_matches_weak_and_strict_argument_boundaries() {
    let fixture = TemporaryPath::unique("html");
    std::fs::write(&fixture.0, b"<meta name=a content=b>").unwrap();
    let path = fixture.php_literal();
    assert_eq!(
        run_php(&format!(
            r#"<?php
class MetaPath {{
    public function __construct(private string $path) {{}}
    public function __toString(): string {{ echo "cast|"; return $this->path; }}
}}
class ThrowingMetaPath {{
    public function __toString(): string {{ echo "throw-cast|"; throw new Exception('string-stop'); }}
}}
set_error_handler(static function (int $level, string $message): bool {{
    echo "diag=$level:$message|";
    return true;
}});
echo json_encode(get_meta_tags(new MetaPath('{path}'), false)), "\n";
echo json_encode(get_meta_tags('{path}', null)), "\n";
try {{ var_dump(get_meta_tags(null, false)); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
try {{ var_dump(get_meta_tags(new ThrowingMetaPath, false)); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
try {{ var_dump(get_meta_tags([], false)); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
try {{ var_dump(get_meta_tags('', [])); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
try {{ var_dump(get_meta_tags("a" . chr(0) . "b", false)); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
restore_error_handler();
"#
        )),
        concat!(
            "cast|{\"a\":\"b\"}\n",
            "diag=8192:get_meta_tags(): Passing null to parameter #2 ($use_include_path) of type bool is deprecated|{\"a\":\"b\"}\n",
            "diag=8192:get_meta_tags(): Passing null to parameter #1 ($filename) of type string is deprecated|ValueError:Path must not be empty\n",
            "throw-cast|Exception:string-stop\n",
            "TypeError:get_meta_tags(): Argument #1 ($filename) must be of type string, array given\n",
            "TypeError:get_meta_tags(): Argument #2 ($use_include_path) must be of type bool, array given\n",
            "ValueError:get_meta_tags(): Argument #1 ($filename) must not contain any null bytes\n",
        )
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
class StrictMetaPath { public function __toString(): string { return '/tmp/no'; } }
try { var_dump(get_meta_tags(1, false)); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
try { var_dump(get_meta_tags(new StrictMetaPath, false)); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
try { var_dump(get_meta_tags('', 1)); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "TypeError:get_meta_tags(): Argument #1 ($filename) must be of type string, int given\n",
            "TypeError:get_meta_tags(): Argument #1 ($filename) must be of type string, StrictMetaPath given\n",
            "TypeError:get_meta_tags(): Argument #2 ($use_include_path) must be of type bool, int given\n",
        )
    );
}

#[test]
#[cfg(feature = "include-path")]
fn get_meta_tags_honors_include_path_only_when_requested() {
    let directory_guard = TemporaryPath::unique("dir");
    std::fs::create_dir_all(&directory_guard.0).unwrap();
    std::fs::write(
        directory_guard.0.join("metadata.html"),
        b"<meta name=source content=include>",
    )
    .unwrap();
    let directory = directory_guard.php_literal();
    assert_eq!(
        run_php(&format!(
            r#"<?php
set_include_path('{directory}');
echo json_encode(get_meta_tags('metadata.html', true)), '|';
echo @get_meta_tags('metadata.html', false) === false ? 'missing' : 'bad';
"#
        )),
        "{\"source\":\"include\"}|missing"
    );
}
