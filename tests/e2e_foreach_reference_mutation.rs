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
            "rphp_foreach_reference_mutation_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("mutation.php");
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
fn direct_unset_translates_only_positions_after_the_removed_bucket() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['future', 'next', 'previous', 'current'] as $mode) {
    $array = ['a', 'b', 'c', 'd'];
    $visited = [];
    foreach ($array as $key => &$value) {
        $visited[] = "$key:$value";
        if ($mode === 'future' && $key === 0) unset($array[2]);
        if ($mode === 'next' && $key === 0) unset($array[1]);
        if ($mode === 'previous' && $key === 1) unset($array[0]);
        if ($mode === 'current' && $key === 1) unset($array[1]);
    }
    unset($value);
    echo $mode, ':', implode(',', $visited), '|', json_encode($array), ';';
}
"#,
        ),
        concat!(
            "future:0:a,1:b,3:d|{\"0\":\"a\",\"1\":\"b\",\"3\":\"d\"};",
            "next:0:a,2:c,3:d|{\"0\":\"a\",\"2\":\"c\",\"3\":\"d\"};",
            "previous:0:a,1:b,2:c,3:d|{\"1\":\"b\",\"2\":\"c\",\"3\":\"d\"};",
            "current:0:a,1:b,2:c,3:d|{\"0\":\"a\",\"2\":\"c\",\"3\":\"d\"};",
        )
    );
}

#[test]
fn nested_reference_loops_keep_independent_live_positions() {
    assert_eq!(
        run_php(
            r#"<?php
$array = ['A', 'B', 'C', 'D', 'E'];
$visited = [];
foreach ($array as $outerKey => &$outerValue) {
    foreach ($array as $innerKey => &$innerValue) {
        $visited[] = "$outerKey$innerKey";
        if ($outerKey === 0 && $innerKey === 2) {
            unset($array[3]);
            unset($array[1]);
        }
    }
}
unset($outerValue, $innerValue);
echo implode(';', $visited), '|', json_encode($array);
"#,
        ),
        "00;01;02;04;20;22;24;40;42;44|{\"0\":\"A\",\"2\":\"C\",\"4\":\"E\"}"
    );
}

#[test]
fn append_and_string_insertion_remain_visible_after_cursor_translation() {
    assert_eq!(
        run_php(
            r#"<?php
$array = ['a', 'b', 'c'];
$visited = [];
foreach ($array as $key => &$value) {
    $visited[] = "$key:$value";
    if ($key === 0) {
        unset($array[1]);
        $array[] = 'd';
    }
    if ($key === 2) {
        unset($array[0]);
        $array['tail'] = 'e';
    }
}
unset($value);
echo implode(',', $visited), '|', json_encode($array);
"#,
        ),
        "0:a,2:c,3:d,tail:e|{\"2\":\"c\",\"3\":\"d\",\"tail\":\"e\"}"
    );
}

#[test]
fn cow_aliases_and_exception_cleanup_preserve_the_final_loop_binding() {
    assert_eq!(
        run_php(
            r#"<?php
$array = ['a', 'b', 'c'];
$copy = $array;
$alias = &$array;
$visited = [];
foreach ($alias as $key => &$value) {
    $visited[] = "$key:$value";
    if ($key === 1) unset($array[1]);
    if ($key === 2) $value = 'C';
}
$value = 'final';
unset($value);
echo implode(',', $visited), '|', json_encode($array), '|', json_encode($alias), '|', json_encode($copy), ';';

$array = ['a', 'b', 'c', 'd'];
$visited = [];
try {
    foreach ($array as $key => &$value) {
        $visited[] = "$key:$value";
        if ($key === 1) {
            unset($array[0]);
            throw new Exception('stop');
        }
    }
} catch (Exception $error) {
    echo $error->getMessage(), '|';
}
$value = 'after';
unset($value);
echo implode(',', $visited), '|', json_encode($array);
"#,
        ),
        concat!(
            "0:a,1:b,2:c|{\"0\":\"a\",\"2\":\"final\"}|{\"0\":\"a\",\"2\":\"final\"}|[\"a\",\"b\",\"c\"];",
            "stop|0:a,1:b|{\"1\":\"after\",\"2\":\"c\",\"3\":\"d\"}",
        )
    );
}

#[test]
fn user_function_unset_translates_a_callers_live_reference_cursor() {
    assert_eq!(
        run_php(
            r#"<?php
function removeFirst(array &$target): void {
    unset($target[0]);
}
$array = ['a', 'b', 'c'];
$visited = [];
foreach ($array as $key => &$value) {
    $visited[] = "$key:$value";
    if ($key === 1) removeFirst($array);
}
unset($value);
echo implode(',', $visited), '|', json_encode($array);
"#,
        ),
        "0:a,1:b,2:c|{\"1\":\"b\",\"2\":\"c\"}"
    );
}

#[test]
fn direct_include_and_eval_sources_share_reference_cursor_translation() {
    let fixture = IncludeFixture::new(
        r#"<?php
return static function (): string {
    $array = [0, 1, 2];
    $seen = [];
    foreach ($array as $key => &$value) {
        $seen[] = "$key:$value";
        if ($key === 0) unset($array[1]);
    }
    unset($value);
    return implode(',', $seen) . '|' . json_encode($array);
};
"#,
    );
    let source = format!(
        r#"<?php
$direct = static function (): string {{
    $array = [0, 1, 2];
    $seen = [];
    foreach ($array as $key => &$value) {{
        $seen[] = "$key:$value";
        if ($key === 0) unset($array[1]);
    }}
    unset($value);
    return implode(',', $seen) . '|' . json_encode($array);
}};
$included = include {include:?};
$evaluated = eval(<<<'PHP'
return static function (): string {{
    $array = [0, 1, 2];
    $seen = [];
    foreach ($array as $key => &$value) {{
        $seen[] = "$key:$value";
        if ($key === 0) unset($array[1]);
    }}
    unset($value);
    return implode(',', $seen) . '|' . json_encode($array);
}};
PHP);
echo $direct(), ';', $included(), ';', $evaluated();
"#,
        include = fixture.file.to_string_lossy(),
    );
    assert_eq!(
        run_php_with_source_context(&source, "/virtual/reference-mutation.php", "/virtual"),
        "0:0,2:2|{\"0\":0,\"2\":2};0:0,2:2|{\"0\":0,\"2\":2};0:0,2:2|{\"0\":0,\"2\":2}"
    );
}
