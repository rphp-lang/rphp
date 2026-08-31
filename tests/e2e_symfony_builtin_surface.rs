mod common;

use common::{run_php, run_php_with_source_context};

#[test]
fn symfony_surface_functions_expose_php_85_signatures_defaults_and_extensions() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'array_replace', 'array_replace_recursive', 'filter_var', 'getenv', 'hash',
    'headers_sent', 'http_build_query', 'libxml_disable_entity_loader',
    'register_shutdown_function',
] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, '|', $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), '|',
        (string) $function->getReturnType(), '|',
        $function->isDeprecated() ? 'deprecated' : 'current', "\n";
    foreach ($function->getParameters() as $parameter) {
        echo $parameter->getName(), ':',
            ($parameter->hasType() ? (string) $parameter->getType() : '-'), ':',
            $parameter->isPassedByReference() ? 'ref' : 'val', ':',
            $parameter->isVariadic() ? 'variadic' : 'fixed', ':';
        if ($parameter->isDefaultValueAvailable()) {
            echo var_export($parameter->getDefaultValue(), true), ':',
                $parameter->isDefaultValueConstant()
                    ? $parameter->getDefaultValueConstantName()
                    : '-';
        } else {
            echo '-:-';
        }
        echo "\n";
    }
}
echo 'query-constants|', PHP_QUERY_RFC1738, '|', PHP_QUERY_RFC3986, "\n";
"#,
        ),
        concat!(
            "array_replace|1/2|array|current\n",
            "array:array:val:fixed:-:-\n",
            "replacements:array:val:variadic:-:-\n",
            "array_replace_recursive|1/2|array|current\n",
            "array:array:val:fixed:-:-\n",
            "replacements:array:val:variadic:-:-\n",
            "filter_var|1/3|mixed|current\n",
            "value:mixed:val:fixed:-:-\n",
            "filter:int:val:fixed:516:FILTER_DEFAULT\n",
            "options:array|int:val:fixed:0:-\n",
            "getenv|0/2|array|string|false|current\n",
            "name:?string:val:fixed:NULL:-\n",
            "local_only:bool:val:fixed:false:-\n",
            "hash|2/4|string|current\n",
            "algo:string:val:fixed:-:-\n",
            "data:string:val:fixed:-:-\n",
            "binary:bool:val:fixed:false:-\n",
            "options:array:val:fixed:array (\n):-\n",
            "headers_sent|0/2|bool|current\n",
            "filename:-:ref:fixed:NULL:-\n",
            "line:-:ref:fixed:NULL:-\n",
            "http_build_query|1/4|string|current\n",
            "data:object|array:val:fixed:-:-\n",
            "numeric_prefix:string:val:fixed:'':-\n",
            "arg_separator:?string:val:fixed:NULL:-\n",
            "encoding_type:int:val:fixed:1:PHP_QUERY_RFC1738\n",
            "libxml_disable_entity_loader|0/1|bool|deprecated\n",
            "disable:bool:val:fixed:true:-\n",
            "register_shutdown_function|1/2|void|current\n",
            "callback:callable:val:fixed:-:-\n",
            "args:mixed:val:variadic:-:-\n",
            "query-constants|1|2\n",
        )
    );
}

#[test]
fn array_replace_validates_every_array_and_preserves_cow_and_references() {
    assert_eq!(
        run_php(
            r#"<?php
$slot = 10;
$source = ['ref' => &$slot, 'nested' => ['value' => 1], 0 => 'old'];
$result = array_replace($source, ['nested' => ['value' => 2], 0 => 'new'], ['tail' => 3]);
$slot = 11;
$result['nested']['value'] = 4;
echo json_encode($result), '|', $source['nested']['value'], '|', $result['ref'], "\n";
echo json_encode(array_replace(array: ['x' => 1])), "\n";
foreach ([
    fn() => array_replace(null),
    fn() => array_replace([], [], 7),
    fn() => array_replace(array: [], replacements: ['x' => 1]),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "{\"ref\":11,\"nested\":{\"value\":4},\"0\":\"new\",\"tail\":3}|1|11\n",
            "{\"x\":1}\n",
            "TypeError:array_replace(): Argument #1 ($array) must be of type array, null given\n",
            "TypeError:array_replace(): Argument #3 must be of type array, int given\n",
            "ArgumentCountError:array_replace() does not accept unknown named parameters\n",
        )
    );
}

#[test]
fn array_replace_recursive_keeps_nested_semantics_and_exact_type_errors() {
    assert_eq!(
        run_php(
            r#"<?php
$result = array_replace_recursive(
    ['a' => ['x' => 1, 'z' => 0], 0 => ['left' => 1]],
    ['a' => ['x' => 2, 'y' => 3], 0 => ['right' => 2]],
);
echo json_encode($result), "\n";
try { array_replace_recursive([], 'bad'); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
try { array_replace_recursive(array: [], replacements: []); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "{\"a\":{\"x\":2,\"z\":0,\"y\":3},\"0\":{\"left\":1,\"right\":2}}\n",
            "TypeError:array_replace_recursive(): Argument #2 must be of type array, string given\n",
            "ArgumentCountError:array_replace_recursive() does not accept unknown named parameters\n",
        )
    );
}

#[test]
fn filter_var_defaults_to_filter_default_and_converts_php_scalars() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([123, 1.25, true, false, null, 'raw', ['x']] as $value) {
    var_dump(filter_var($value));
}
var_dump(
    filter_var('yes', FILTER_VALIDATE_BOOL),
    filter_var('wat', FILTER_VALIDATE_BOOL),
    filter_var('wat', FILTER_VALIDATE_BOOL, FILTER_NULL_ON_FAILURE),
    filter_var('0123', FILTER_VALIDATE_INT),
    filter_var('0123', FILTER_VALIDATE_INT, FILTER_FLAG_ALLOW_OCTAL),
    filter_var('0x10', FILTER_VALIDATE_INT, FILTER_FLAG_ALLOW_HEX),
    filter_var(true, FILTER_VALIDATE_INT),
    filter_var(12.0, FILTER_VALIDATE_INT),
    filter_var('17', FILTER_VALIDATE_INT, [
        'options' => ['min_range' => 20],
        'flags' => FILTER_NULL_ON_FAILURE,
    ]),
    filter_var('17', FILTER_VALIDATE_INT, [
        'options' => ['min_range' => 20, 'default' => 'fallback'],
        'flags' => FILTER_NULL_ON_FAILURE,
    ]),
);
"#,
        ),
        concat!(
            "string(3) \"123\"\n",
            "string(4) \"1.25\"\n",
            "string(1) \"1\"\n",
            "string(0) \"\"\n",
            "string(0) \"\"\n",
            "string(3) \"raw\"\n",
            "bool(false)\n",
            "bool(true)\n",
            "bool(false)\n",
            "NULL\n",
            "bool(false)\n",
            "int(83)\n",
            "int(16)\n",
            "int(1)\n",
            "int(12)\n",
            "NULL\n",
            "string(8) \"fallback\"\n",
        )
    );
}

#[test]
fn filter_var_validates_filter_and_options_before_dispatching() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function (int $level, string $message): bool {
    echo "$level:$message\n";
    return true;
});
var_dump(filter_var('17', '257', '0'));
var_dump(filter_var('x', 999999));
var_dump(filter_var('x', null));
"#,
        ),
        concat!(
            "int(17)\n",
            "2:filter_var(): Unknown filter with ID 999999\n",
            "bool(false)\n",
            "8192:filter_var(): Passing null to parameter #2 ($filter) of type int is deprecated\n",
            "2:filter_var(): Unknown filter with ID 0\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn filter_var_strict_types_enforces_its_declared_union() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach ([
    fn() => filter_var('17', '257'),
    fn() => filter_var('17', FILTER_VALIDATE_INT, '0'),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "TypeError:filter_var(): Argument #2 ($filter) must be of type int, string given\n",
            "TypeError:filter_var(): Argument #3 ($options) must be of type array|int, string given\n",
        )
    );
}

#[test]
fn hash_supports_binary_output_and_xxh128_seed_options() {
    assert_eq!(
        run_php(
            r#"<?php
echo hash('md5', 'abc'), "\n";
echo bin2hex(hash('md5', 'abc', true)), "\n";
echo hash('xxh128', 'abc'), "\n";
echo hash(algo: 'xxh128', data: 'abc', binary: false, options: ['seed' => 1]), "\n";
echo hash('md5', 'abc', false, ['seed' => 1]), "\n";
foreach ([
    fn() => hash('md5', 'abc', false, 1),
    fn() => hash('unknown', 'abc'),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "900150983cd24fb0d6963f7d28e17f72\n",
            "900150983cd24fb0d6963f7d28e17f72\n",
            "06b05ab6733a618578af5f94892f3950\n",
            "7577b06fae9ee3ed6b4467b443c76228\n",
            "900150983cd24fb0d6963f7d28e17f72\n",
            "TypeError:hash(): Argument #4 ($options) must be of type array, int given\n",
            "ValueError:hash(): Argument #1 ($algo) must be a valid hashing algorithm\n",
        )
    );
}

#[test]
fn hash_strict_types_validates_all_four_declared_parameters() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach ([
    fn() => hash(1, 'abc'),
    fn() => hash('md5', 1),
    fn() => hash('md5', 'abc', 1),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "TypeError:hash(): Argument #1 ($algo) must be of type string, int given\n",
            "TypeError:hash(): Argument #2 ($data) must be of type string, int given\n",
            "TypeError:hash(): Argument #3 ($binary) must be of type bool, int given\n",
        )
    );
}

#[test]
fn hash_deprecates_and_ignores_non_integer_xxhash_seeds() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function (int $level, string $message): bool {
    echo "$level:$message\n";
    return true;
});
foreach (['1', 1.5, []] as $seed) {
    echo hash('xxh128', 'abc', false, ['seed' => $seed]), "\n";
}
"#,
        ),
        concat!(
            "8192:hash(): Passing a seed of a type other than int is deprecated because it is ignored\n",
            "06b05ab6733a618578af5f94892f3950\n",
            "8192:hash(): Passing a seed of a type other than int is deprecated because it is ignored\n",
            "06b05ab6733a618578af5f94892f3950\n",
            "8192:hash(): Passing a seed of a type other than int is deprecated because it is ignored\n",
            "06b05ab6733a618578af5f94892f3950\n",
        )
    );
}

#[test]
fn getenv_supports_omitted_nullable_name_and_local_only() {
    assert_eq!(
        run_php(
            r#"<?php
$all = getenv();
$explicit = getenv(null);
var_dump(is_array($all), is_array($explicit));
var_dump(getenv('__RPHP_SURFACE_ABSENT_85__'));
var_dump(getenv(name: '__RPHP_SURFACE_ABSENT_85__', local_only: true));
"#,
        ),
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "bool(false)\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn getenv_strict_types_enforces_nullable_string_and_bool() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach ([fn() => getenv(1), fn() => getenv('PATH', 1)] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "TypeError:getenv(): Argument #1 ($name) must be of type ?string, int given\n",
            "TypeError:getenv(): Argument #2 ($local_only) must be of type bool, int given\n",
        )
    );
}

#[test]
fn http_build_query_distinguishes_rfc1738_and_rfc3986() {
    assert_eq!(
        run_php(
            r#"<?php
$data = [
    'a b' => 'c d~', 2 => true, 'false' => false, 'null' => null,
    'nested' => ['x y' => 0],
];
echo http_build_query($data), "\n";
echo http_build_query($data, 'n_', ';', PHP_QUERY_RFC3986), "\n";
echo http_build_query(['x' => 'a b~'], '', '&', 99), "\n";
echo http_build_query(data: ['x y' => 'z~'], encoding_type: PHP_QUERY_RFC3986), "\n";
ini_set('arg_separator.output', ';');
echo http_build_query(['a' => 1, 'b' => 2]), "\n";
"#,
        ),
        concat!(
            "a+b=c+d%7E&2=1&false=0&nested%5Bx+y%5D=0\n",
            "a%20b=c%20d~;n_2=1;false=0;nested%5Bx%20y%5D=0\n",
            "x=a+b%7E\n",
            "x%20y=z~\n",
            "a=1;b=2\n",
        )
    );
}

#[test]
fn http_build_query_projects_public_object_properties_and_validates_data() {
    assert_eq!(
        run_php(
            r#"<?php
class QueryData {
    public string $visible = 'a b~';
    private string $hidden = 'no';
}
echo http_build_query(new QueryData), "\n";
try { http_build_query('bad'); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "visible=a+b%7E\n",
            "TypeError:http_build_query(): Argument #1 ($data) must be of type array, string given\n",
        )
    );
}

#[test]
fn http_build_query_strict_types_validates_optional_scalars() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach ([
    fn() => http_build_query([], 1),
    fn() => http_build_query([], '', 1),
    fn() => http_build_query([], '', null, '2'),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "TypeError:http_build_query(): Argument #2 ($numeric_prefix) must be of type string, int given\n",
            "TypeError:http_build_query(): Argument #3 ($arg_separator) must be of type ?string, int given\n",
            "TypeError:http_build_query(): Argument #4 ($encoding_type) must be of type int, string given\n",
        )
    );
}

#[test]
fn headers_sent_reports_the_first_unbuffered_output_origin_by_reference() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
$beforeFile = 'seed';
$beforeLine = -1;
$before = headers_sent($beforeFile, $beforeLine);
echo 'before=', (int) $before, ':', $beforeFile, ':', $beforeLine, "\n";
$afterFile = 'seed';
$afterLine = -1;
$after = headers_sent($afterFile, $afterLine);
echo 'after=', (int) $after, ':', $afterFile, ':', $afterLine, "\n";
$namedFile = 'seed';
$namedLine = -1;
$named = headers_sent(line: $namedLine, filename: $namedFile);
echo 'named=', (int) $named, ':', $namedFile, ':', $namedLine, "\n";
"#,
            "/virtual/symfony-builtin-surface.php",
            "/virtual",
        ),
        concat!(
            "before=0::0\n",
            "after=1:/virtual/symfony-builtin-surface.php:5\n",
            "named=1:/virtual/symfony-builtin-surface.php:5\n",
        )
    );
}

#[test]
fn headers_sent_rejects_a_non_writable_reference_argument() {
    assert_eq!(
        run_php(
            r#"<?php
try { headers_sent('literal'); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        "Error:headers_sent(): Argument #1 ($filename) could not be passed by reference\n"
    );
}

#[test]
fn headers_sent_attributes_internal_output_to_the_user_callsite() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function emit_from_internal(): void {
    printf("inside\n");
}
emit_from_internal();
$file = '';
$line = 0;
var_dump(headers_sent($file, $line));
echo $file, ':', $line, "\n";
"#,
            "/virtual/symfony-internal-output.php",
            "/virtual",
        ),
        concat!(
            "inside\n",
            "bool(true)\n",
            "/virtual/symfony-internal-output.php:3\n",
        )
    );
}

#[test]
fn headers_sent_ignores_empty_and_buffered_output_until_flush() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
echo '';
ob_start();
echo 'buffered';
$insideFile = 'seed';
$insideLine = -1;
$inside = headers_sent($insideFile, $insideLine);
$record = 'inside=' . (int) $inside . ':' . $insideFile . ':' . $insideLine . "\n";
ob_end_flush();
$afterFile = 'seed';
$afterLine = -1;
$after = headers_sent($afterFile, $afterLine);
echo $record, 'after=', (int) $after, ':', $afterFile, ':', $afterLine, "\n";
"#,
            "/virtual/symfony-buffered-output.php",
            "/virtual",
        ),
        concat!(
            "bufferedinside=0::0\n",
            "after=1:/virtual/symfony-buffered-output.php:9\n",
        )
    );
}

#[test]
fn libxml_disable_entity_loader_is_deprecated_and_request_local() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function (int $level, string $message): bool {
    echo "$level:$message\n";
    return true;
});
var_dump(libxml_disable_entity_loader());
var_dump(libxml_disable_entity_loader(false));
var_dump(libxml_disable_entity_loader(true));
var_dump(libxml_disable_entity_loader(null));
"#,
        ),
        concat!(
            "8192:Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default\n",
            "bool(false)\n",
            "8192:Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default\n",
            "bool(true)\n",
            "8192:Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default\n",
            "bool(false)\n",
            "8192:Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default\n",
            "8192:libxml_disable_entity_loader(): Passing null to parameter #1 ($disable) of type bool is deprecated\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn libxml_disable_entity_loader_strict_types_rejects_non_bool_after_deprecation() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
set_error_handler(function (int $level, string $message): bool {
    echo "$level:$message\n";
    return true;
});
try { libxml_disable_entity_loader(1); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "8192:Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default\n",
            "TypeError:libxml_disable_entity_loader(): Argument #1 ($disable) must be of type bool, int given\n",
        )
    );
}

#[test]
fn register_shutdown_function_retains_positional_args_fifo_and_rejects_named_variadics() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(register_shutdown_function(function ($a, $b) {
    echo "first:$a:$b\n";
}, 'A', 2));
register_shutdown_function(function () { echo "second\n"; });
try { register_shutdown_function(callback: fn() => null, args: 'x'); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
echo "body\n";
"#,
        ),
        concat!(
            "NULL\n",
            "ArgumentCountError:register_shutdown_function() does not accept unknown named parameters\n",
            "body\n",
            "first:A:2\n",
            "second\n",
        )
    );
}

#[test]
fn register_shutdown_function_reports_exact_invalid_callback_details() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['missing_shutdown', ['MissingClass', 'method'], 42, []] as $callback) {
    try { register_shutdown_function($callback); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "TypeError:register_shutdown_function(): Argument #1 ($callback) must be a valid callback, function \"missing_shutdown\" not found or invalid function name\n",
            "TypeError:register_shutdown_function(): Argument #1 ($callback) must be a valid callback, class \"MissingClass\" not found\n",
            "TypeError:register_shutdown_function(): Argument #1 ($callback) must be a valid callback, no array or string given\n",
            "TypeError:register_shutdown_function(): Argument #1 ($callback) must be a valid callback, array callback must have exactly two members\n",
        )
    );
}
