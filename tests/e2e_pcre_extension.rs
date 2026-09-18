mod common;

use common::run_php;

#[test]
fn pcre_missing_globals_publish_php_85_signatures_and_constants() {
    assert_eq!(
        run_php(
            r#"<?php
$names = ['preg_filter','preg_grep','preg_last_error','preg_last_error_msg','preg_replace_callback_array'];
foreach ($names as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':',
        $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(),
        ':', (string) $function->getReturnType(), '(';
    foreach ($function->getParameters() as $parameter) {
        echo $parameter->getName(), ':', (string) $parameter->getType(),
            ':', (int) $parameter->isPassedByReference();
        if ($parameter->isDefaultValueAvailable()) {
            echo '=', var_export($parameter->getDefaultValue(), true);
        }
        echo ',';
    }
    echo ")\n";
}
foreach ([
    'PREG_GREP_INVERT','PREG_NO_ERROR','PREG_INTERNAL_ERROR',
    'PREG_BACKTRACK_LIMIT_ERROR','PREG_RECURSION_LIMIT_ERROR',
    'PREG_BAD_UTF8_ERROR','PREG_BAD_UTF8_OFFSET_ERROR','PREG_JIT_STACKLIMIT_ERROR',
] as $name) echo $name, '=', constant($name), '|';
echo "\n";
"#,
        ),
        concat!(
            "preg_filter:pcre:3/5:array|string|null(pattern:array|string:0,replacement:array|string:0,subject:array|string:0,limit:int:0=-1,count::1=NULL,)\n",
            "preg_grep:pcre:2/3:array|false(pattern:string:0,array:array:0,flags:int:0=0,)\n",
            "preg_last_error:pcre:0/0:int()\n",
            "preg_last_error_msg:pcre:0/0:string()\n",
            "preg_replace_callback_array:pcre:2/5:array|string|null(pattern:array:0,subject:array|string:0,limit:int:0=-1,count::1=NULL,flags:int:0=0,)\n",
            "PREG_GREP_INVERT=1|PREG_NO_ERROR=0|PREG_INTERNAL_ERROR=1|",
            "PREG_BACKTRACK_LIMIT_ERROR=2|PREG_RECURSION_LIMIT_ERROR=3|",
            "PREG_BAD_UTF8_ERROR=4|PREG_BAD_UTF8_OFFSET_ERROR=5|",
            "PREG_JIT_STACKLIMIT_ERROR=6|\n",
        )
    );
}

#[test]
fn preg_error_state_is_request_local_and_success_resets_it() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function(int $level, string $message): bool {
    echo $level, ':', $message, "\n";
    return true;
});
echo preg_last_error(), ':', preg_last_error_msg(), "\n";
var_dump(preg_grep('abcdef', [123, 'abc']));
echo preg_last_error(), ':', preg_last_error_msg(), "\n";
var_dump(preg_grep('/1/', ['z' => 123, 4 => true, 5 => null, 6 => false, 7 => 1.5]));
var_dump(preg_grep('/1/', ['z' => 123, 4 => true, 5 => null, 6 => false, 7 => 1.5], PREG_GREP_INVERT));
echo preg_last_error(), ':', preg_last_error_msg(), "\n";
$count = 99;
var_dump(preg_filter(['/a/', '/[/'], ['b', 'x'], 'a', -1, $count), $count);
$count = 99;
var_dump(preg_replace_callback_array([
    '/a/' => static fn(): string => 'b',
    '/[/' => static fn(): string => 'x',
], 'a', -1, $count), $count);
"#,
        ),
        concat!(
            "0:No error\n",
            "2:preg_grep(): Delimiter must not be alphanumeric, backslash, or NUL byte\n",
            "bool(false)\n1:Internal error\n",
            "array(3) {\n  [\"z\"]=>\n  int(123)\n  [4]=>\n  bool(true)\n  [7]=>\n  float(1.5)\n}\n",
            "array(2) {\n  [5]=>\n  NULL\n  [6]=>\n  bool(false)\n}\n",
            "0:No error\n",
            "2:preg_filter(): Compilation failed: missing terminating ] for character class at offset 1\n",
            "NULL\nint(1)\n",
            "2:preg_replace_callback_array(): Compilation failed: missing terminating ] for character class at offset 1\n",
            "NULL\nint(99)\n",
        )
    );
}

#[test]
fn preg_grep_preserves_selected_reference_cells_without_mutating_input() {
    assert_eq!(
        run_php(
            r#"<?php
$tracked = 'b';
$input = ['a', &$tracked, null, []];
$selected = preg_grep('/./', $input);
$tracked = 'y';
var_dump($selected, $input);
"#,
        ),
        concat!(
            "\nWarning: Array to string conversion in <main> on line 4\n",
            "array(3) {\n  [0]=>\n  string(1) \"a\"\n  [1]=>\n  &string(1) \"y\"\n  [3]=>\n  array(0) {\n  }\n}\n",
            "array(4) {\n  [0]=>\n  string(1) \"a\"\n  [1]=>\n  &string(1) \"y\"\n  [2]=>\n  NULL\n  [3]=>\n  array(0) {\n  }\n}\n",
        )
    );
}

#[test]
fn preg_filter_applies_sequential_patterns_filters_keys_and_counts_all_replacements() {
    assert_eq!(
        run_php(
            r#"<?php
$count = 99;
var_dump(preg_filter(
    ['/\d/', '/[a-z]/', '/[1a]/'],
    ['A:$0', 'B:$0', 'C:$0'],
    [0 => '1', 1 => 'a', 2 => '2', 3 => 'b', 4 => '3', 7 => '4'],
    -1,
    $count,
), $count);
$count = 99;
var_dump(preg_filter('/x/', 'y', 'abc', -1, $count), $count);
$count = 99;
var_dump(preg_filter('/x/', 'y', 'x x', 1, $count), $count);
"#,
        ),
        concat!(
            "array(6) {\n",
            "  [0]=>\n  string(5) \"A:C:1\"\n",
            "  [1]=>\n  string(5) \"B:C:a\"\n",
            "  [2]=>\n  string(3) \"A:2\"\n",
            "  [3]=>\n  string(3) \"B:b\"\n",
            "  [4]=>\n  string(3) \"A:3\"\n",
            "  [7]=>\n  string(3) \"A:4\"\n}\n",
            "int(8)\nNULL\nint(0)\nstring(3) \"y x\"\nint(1)\n",
        )
    );
}

#[test]
fn preg_replace_callback_array_preserves_order_keys_count_and_capture_flags() {
    assert_eq!(
        run_php(
            r#"<?php
$count = 99;
$result = preg_replace_callback_array([
    '/a/' => static fn(array $matches): string => 'A',
    '/A/' => static fn(array $matches): string => 'Z',
], ['k' => 'a', 'n' => 'x'], -1, $count);
var_dump($result, $count);
$count = 99;
$result = preg_replace_callback_array([
    '/(a)|(b)/' => static function(array $matches): string {
        foreach ($matches as $value) {
            echo '[', var_export($value[0], true), ',', $value[1], ']';
        }
        echo "\n";
        return $matches[0][0];
    },
], 'ab', -1, $count, PREG_OFFSET_CAPTURE | PREG_UNMATCHED_AS_NULL);
var_dump($result, $count);
"#,
        ),
        concat!(
            "array(2) {\n  [\"k\"]=>\n  string(1) \"Z\"\n  [\"n\"]=>\n  string(1) \"x\"\n}\n",
            "int(2)\n",
            "['a',0]['a',0][NULL,-1]\n",
            "['b',1][NULL,-1]['b',1]\n",
            "string(2) \"ab\"\nint(2)\n",
        )
    );
}

#[test]
fn preg_replace_callback_array_rejects_keys_and_callbacks_after_prior_side_effects() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    preg_replace_callback_array([42 => static fn(): string => 'x'], 'a');
} catch (Throwable $error) {
    echo get_class($error), ': ', $error->getMessage(), "\n";
}
$log = [];
try {
    preg_replace_callback_array([
        '/a/' => static function(array $matches) use (&$log): string {
            $log[] = 'called';
            return 'b';
        },
        '/b/' => 'missing',
    ], 'a');
} catch (Throwable $error) {
    echo get_class($error), ': ', $error->getMessage(), "\n";
}
echo implode(',', $log), "\n";
"#,
        ),
        concat!(
            "TypeError: preg_replace_callback_array(): Argument #1 ($pattern) must contain only string patterns as keys\n",
            "TypeError: preg_replace_callback_array(): Argument #1 ($pattern) must contain only valid callbacks\n",
            "called\n",
        )
    );
}

#[test]
fn capture_registers_and_php_projection_follow_the_selected_backtracking_path() {
    assert_eq!(
        run_php(
            r#"<?php
function emit($label, $value) { echo $label, ':', json_encode($value), "\n"; }
preg_match('~(?P<date>(?P<year>(\d{2})?\d{2})-(?P<month>\d{2}|[a-z]{3})-(?P<day>\d{2}))~i', '2006-05-13', $matches);
emit('date', $matches);
preg_match('@^(/([a-z]*))*$@', '//abcde', $matches);
emit('repeat', $matches);
preg_match('/(a)?([a-z]*)(\d*)/', '123', $matches, PREG_UNMATCHED_AS_NULL);
emit('null', $matches);
preg_match('/(?P<size>\d+)m|M/', '4M', $matches);
emit('trailing', $matches);
preg_match('|(?P<name>)(\d+)|', '123', $matches);
emit('named', $matches);
preg_match_all('/(4)?(2)?\d/', '123456', $matches, PREG_SET_ORDER | PREG_UNMATCHED_AS_NULL);
emit('set', $matches);
preg_match_all('/(?<a>4)?(?<b>2)?\d/', '123456', $matches, PREG_UNMATCHED_AS_NULL);
emit('pattern', $matches);
preg_match('/(a)|(b)/', 'b', $matches);
emit('branch', $matches);
preg_match('/(a(b)?)+/', 'aba', $matches);
emit('stale', $matches);
preg_replace_callback('/(?<left>a)|(b)/', function ($matches) {
    emit('callback', $matches);
    return $matches[0];
}, 'ab');
preg_replace_callback('/_(a)(*MARK:A)_|_(b)_/', function ($matches) {
    emit('mark', $matches);
    return $matches[0];
}, '_a__b_');
"#,
        ),
        concat!(
            "date:{\"0\":\"2006-05-13\",\"date\":\"2006-05-13\",\"1\":\"2006-05-13\",\"year\":\"2006\",\"2\":\"2006\",\"3\":\"20\",\"month\":\"05\",\"4\":\"05\",\"day\":\"13\",\"5\":\"13\"}\n",
            "repeat:[\"\\/\\/abcde\",\"\\/abcde\",\"abcde\"]\n",
            "null:[\"123\",null,\"\",\"123\"]\n",
            "trailing:[\"M\"]\n",
            "named:{\"0\":\"123\",\"name\":\"\",\"1\":\"\",\"2\":\"123\"}\n",
            "set:[[\"1\",null,null],[\"23\",null,\"2\"],[\"45\",\"4\",null],[\"6\",null,null]]\n",
            "pattern:{\"0\":[\"1\",\"23\",\"45\",\"6\"],\"a\":[null,null,\"4\",null],\"1\":[null,null,\"4\",null],\"b\":[null,\"2\",null,null],\"2\":[null,\"2\",null,null]}\n",
            "branch:[\"b\",\"\",\"b\"]\n",
            "stale:[\"aba\",\"a\",\"b\"]\n",
            "callback:{\"0\":\"a\",\"left\":\"a\",\"1\":\"a\"}\n",
            "callback:{\"0\":\"b\",\"left\":\"\",\"1\":\"\",\"2\":\"b\"}\n",
            "mark:{\"0\":\"_a_\",\"1\":\"a\",\"MARK\":\"A\"}\n",
            "mark:[\"_b_\",\"\",\"b\"]\n",
        )
    );
}
