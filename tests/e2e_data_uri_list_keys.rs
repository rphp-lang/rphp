mod common;

use common::run_php;

#[test]
fn runtime_data_uri_constants_drive_long_short_nested_and_reference_list_keys() {
    assert_eq!(
        run_php(
            r#"<?php
class TraceLog {
    public static array $events = [];
}
function source_value(string $label, mixed $value): mixed {
    TraceLog::$events[] = 'source:' . $label;
    return $value;
}
function source_key(string $label, mixed $value): mixed {
    TraceLog::$events[] = 'key:' . $label;
    return $value;
}
define('DATA_KEY', file_get_contents('data:text/plain,2'));
define('NEST_KEY', file_get_contents('data:text/plain,nest'));
$values = [1 => 'one', 2 => 'two', 3 => 'three', 'nest' => [2 => 'deep']];

list(1 => $one, DATA_KEY => $two, 3 => $three) = source_value('long', $values);
echo 'long:', json_encode([$one, $two, $three, DATA_KEY]), ':', implode(',', TraceLog::$events), "\n";

TraceLog::$events = [];
[source_key('left', DATA_KEY) => $two, source_key('right', 3) => $three]
    = source_value('short', $values);
echo 'short:', json_encode([$two, $three]), ':', implode(',', TraceLog::$events), "\n";

TraceLog::$events = [];
[NEST_KEY => [DATA_KEY => $deep]] = source_value('nested', $values);
echo 'nested:', json_encode($deep), ':', implode(',', TraceLog::$events), "\n";

$referenceSource = [2 => 'before'];
$referenceCopy = $referenceSource;
[DATA_KEY => &$reference] = $referenceSource;
$reference = 'after';
echo 'reference:', json_encode([$reference, $referenceSource, $referenceCopy]), "\n";
"#,
        ),
        concat!(
            "long:[\"one\",\"two\",\"three\",\"2\"]:source:long\n",
            "short:[\"two\",\"three\"]:source:short,key:left,key:right\n",
            "nested:\"deep\":source:nested\n",
            "reference:[\"after\",{\"2\":\"after\"},{\"2\":\"before\"}]\n",
        )
    );
}

#[test]
fn runtime_constant_list_keys_preserve_diagnostic_order_and_target_state() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
class TraceLog {
    public static array $events = [];
}
function source_value(string $label, mixed $value): mixed {
    TraceLog::$events[] = 'source:' . $label;
    return $value;
}
$values = [2 => 'two'];
$slot = 'old';
try {
    [ORIGINAL_MISSING_CONSTANT => $slot] = source_value('undefined-constant', $values);
} catch (Error $error) {
    TraceLog::$events[] = 'caught:' . $error->getMessage();
}
echo 'constant:', json_encode($slot), ':', implode(',', TraceLog::$events), "\n";

define('MISSING_ARRAY_KEY', file_get_contents('data:text/plain,9'));
define('DATA_KEY', file_get_contents('data:text/plain,2'));
TraceLog::$events = [];
set_error_handler(function (int $severity, string $message): never {
    TraceLog::$events[] = 'throw:' . $message;
    throw new RuntimeException('stop');
});
$first = 'first-old';
$second = 'second-old';
try {
    [MISSING_ARRAY_KEY => $first, DATA_KEY => $second] = source_value('missing-key', $values);
} catch (RuntimeException $error) {
    TraceLog::$events[] = 'caught:' . $error->getMessage();
}
restore_error_handler();
echo 'key:', json_encode([$first, $second]), ':', implode(',', TraceLog::$events), "\n";
"#,
        ),
        concat!(
            "constant:\"old\":source:undefined-constant,caught:Undefined constant \"ORIGINAL_MISSING_CONSTANT\"\n",
            "key:[\"first-old\",\"second-old\"]:source:missing-key,throw:Undefined array key 9,caught:stop\n",
        )
    );
}

#[test]
fn data_uri_contents_decode_binary_payloads_and_preserve_handler_suppression() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
$values = [
    file_get_contents('data://text/plain,hello%20world%00%2B+'),
    file_get_contents('data:text/plain;base64,AAECYWJj'),
    file_get_contents('data:,plain%2Ctext'),
    file_get_contents('data:text/plain,a%GGb%2'),
];
echo implode('|', array_map('bin2hex', $values)), "\n";

set_error_handler(function (int $severity, string $message): bool {
    echo 'warning:', $severity, ':', $message, ':mask=', error_reporting(), "\n";
    return true;
});
var_dump(file_get_contents('data:text/plain;BASE64,YQ=='));
@var_dump(file_get_contents('data:text/plain;base64,YQ==='));
restore_error_handler();

$slot = 'old';
set_error_handler(function (int $severity, string $message): never {
    throw new RuntimeException($message);
});
try {
    $slot = file_get_contents('data:text/plain');
} catch (RuntimeException $error) {
    echo 'caught:', $error->getMessage(), "\n";
}
restore_error_handler();
var_dump($slot);
"#,
        ),
        concat!(
            "68656c6c6f20776f726c64002b20|000102616263|706c61696e2c74657874|61254747622532\n",
            "warning:2:file_get_contents(data:text/plain;BASE64,YQ==): Failed to open stream: rfc2397: illegal parameter:mask=30719\n",
            "bool(false)\n",
            "warning:2:file_get_contents(data:text/plain;base64,YQ===): Failed to open stream: rfc2397: unable to decode:mask=4437\n",
            "bool(false)\n",
            "caught:file_get_contents(data:text/plain): Failed to open stream: rfc2397: no comma in URL\n",
            "string(3) \"old\"\n",
        )
    );
}

#[cfg(feature = "file-contents")]
#[test]
fn data_uri_contents_honor_positive_negative_and_failed_seek_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function (int $severity, string $message): bool {
    echo 'warning:', $message, "\n";
    return true;
});
foreach ([[2, 3], [-2, null], [1, 0], [99, null], [-99, null]] as [$offset, $length]) {
    $value = $length === null
        ? file_get_contents('data:text/plain,abcdef', false, null, $offset)
        : file_get_contents('data:text/plain,abcdef', false, null, $offset, $length);
    var_dump($value);
}
"#,
        ),
        concat!(
            "string(3) \"cde\"\n",
            "string(2) \"ef\"\n",
            "string(0) \"\"\n",
            "string(0) \"\"\n",
            "warning:file_get_contents(): Failed to seek to position -99 in the stream\n",
            "bool(false)\n",
        )
    );
}
