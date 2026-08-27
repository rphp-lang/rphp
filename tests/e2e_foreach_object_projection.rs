mod common;

use common::{run_php, run_php_with_source_context};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

static INCLUDE_COUNTER: AtomicU64 = AtomicU64::new(0);

struct IncludeFixture {
    directory: std::path::PathBuf,
    file: std::path::PathBuf,
}

impl IncludeFixture {
    fn new(source: &str) -> Self {
        let identity = INCLUDE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let directory = std::env::temp_dir().join(format!(
            "rphp_foreach_object_projection_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("projection.php");
        let mut output = std::fs::File::create(&file).unwrap();
        output.write_all(source.as_bytes()).unwrap();
        Self { directory, file }
    }
}

impl Drop for IncludeFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn stdclass_foreach_projects_mangled_keys_without_collapsing_collisions() {
    assert_eq!(
        run_php(
            r#"<?php
$object = (object) [
    "\0Owner\0shared" => 'private',
    'shared' => 'public',
    "\0*\0shared" => 'protected',
    12 => 'numeric',
    "bin\x7f" => 'binary',
];
foreach ($object as $key => $value) {
    echo bin2hex($key), '=', $value, ';';
}
"#,
        ),
        "736861726564=private;736861726564=public;736861726564=protected;3132=numeric;62696e7f=binary;"
    );
}

#[test]
fn reference_foreach_updates_raw_storage_and_round_trip_aliases() {
    assert_eq!(
        run_php(
            r#"<?php
$private = "\0Owner\0value";
$protected = "\0*\0guarded";
$alias = 5;
$source = [$private => &$alias, $protected => 9, 'plain' => 11];
$copy = $source;
$object = (object) $source;
foreach ($object as $key => &$value) {
    echo bin2hex($key), '=', $value, ';';
    $value += 100;
}
unset($value);
echo '|', $alias, '|', $source[$private], '|', $copy[$private], '|';
echo $source[$protected], '|', $source['plain'], '|', implode(',', (array) $object), '|';

$roundTrip = (array) $object;
$roundTrip[$private] = 700;
$roundTrip[$protected] = 800;
$roundTrip['plain'] = 900;
echo $alias, '|', $source[$private], '|', $copy[$private], '|';
echo implode(',', $roundTrip), '|', implode(',', (array) $object);

$single = 3;
$singleObject = new stdClass();
$singleObject->value = &$single;
unset($single);
$singleArray = (array) $singleObject;
$singleObject->value = 4;
echo '|', $singleArray['value'], ':', $singleObject->value;

$single = 5;
$singleArray = ['value' => &$single];
unset($single);
$singleObject = (object) $singleArray;
$singleObject->value = 6;
echo '|', $singleArray['value'], ':', $singleObject->value;
"#,
        ),
        concat!(
            "76616c7565=5;67756172646564=9;706c61696e=11;",
            "|105|105|105|9|11|105,109,111|",
            "700|700|700|700,800,900|700,109,111|3:4|5:6",
        )
    );
}

#[test]
fn malformed_mangled_keys_report_before_loop_variable_assignment() {
    assert_eq!(
        run_php(
            r#"<?php
function label($value): string {
    return isset($value) ? gettype($value) . ':' . (is_string($value) ? bin2hex($value) : $value) : 'unset';
}
$key = 'old-key';
$value = 'old-value';
$object = (object) ["\0Owner" => 1, "\0\0empty" => 2, "\0*\0" => 3, "\0A\0b\0tail" => 4];
set_error_handler(function ($severity, $message) use (&$key, &$value) {
    echo 'notice=', $severity, ':', $message, ';key=', label($key), ';value=', label($value), '|';
    return true;
});
foreach ($object as $key => $value) {
    echo 'body=', bin2hex($key), ':', $value, '|';
}
restore_error_handler();
"#,
        ),
        concat!(
            "notice=8:Illegal member variable name;key=string:6f6c642d6b6579;value=string:6f6c642d76616c7565|",
            "body=004f776e6572:1|",
            "notice=8:Illegal member variable name;key=string:004f776e6572;value=integer:1|",
            "body=0000656d707479:2|",
            "notice=8:Corrupt member variable name;key=string:0000656d707479;value=integer:2|",
            "body=002a00:3|body=7461696c:4|",
        )
    );
}

#[test]
fn value_foreach_tracks_live_object_mutation_and_nested_positions() {
    assert_eq!(
        run_php(
            r#"<?php
$object = (object) ["\0Owner\0first" => 1, 'second' => 2];
foreach ($object as $key => $value) {
    echo $key, '=', $value, ';';
    if ($value === 1) {
        unset($object->second);
        $object->third = 3;
    }
}
echo '|';

$nested = (object) ['a' => 1, 'b' => 2];
foreach ($nested as $outer) {
    foreach ($nested as $inner) {
        echo $outer, $inner, ';';
        if ($outer === 1 && $inner === 1) {
            $nested->c = 3;
        }
    }
    if ($outer === 2) {
        break;
    }
}
echo '|';

try {
    foreach ($object as $key => &$value) {
        $value *= 10;
        throw new Exception('stop');
    }
} catch (Exception $error) {
    echo $error->getMessage(), '|';
}
unset($value);
foreach ($object as $key => $value) echo $key, '=', $value, ';';
"#,
        ),
        concat!(
            "first=1;third=3;|",
            "11;12;13;21;22;23;|",
            "stop|first=10;third=3;",
        )
    );
}

#[test]
fn user_class_visibility_remains_scope_sensitive() {
    assert_eq!(
        run_php(
            r#"<?php
class VisibleProjection {
    public $open = 1;
    protected $guarded = 2;
    private $hidden = 3;
    public function inside(): void {
        foreach ($this as $key => $value) echo $key, '=', $value, ';';
    }
}
$object = new VisibleProjection();
foreach ($object as $key => $value) echo $key, '=', $value, ';';
echo '|';
$object->inside();
"#,
        ),
        "open=1;|open=1;guarded=2;hidden=3;"
    );
}

#[test]
fn direct_include_and_eval_sources_share_object_key_projection() {
    let fixture =
        IncludeFixture::new(r#"<?php return (object) ["\0Included\0value" => 'include'];"#);
    let source = format!(
        r#"<?php
$direct = (object) ["\0Direct\0value" => 'direct'];
$included = include {include:?};
$evaluated = eval('return (object) ["\\0Evaluated\\0value" => "eval"];');
foreach ([$direct, $included, $evaluated] as $object) {{
    foreach ($object as $key => $value) echo $key, '=', $value, ';';
}}
"#,
        include = fixture.file.to_string_lossy(),
    );
    assert_eq!(
        run_php_with_source_context(&source, "/virtual/object-projection.php", "/virtual"),
        "value=direct;value=include;value=eval;"
    );
}
