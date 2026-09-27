mod common;

use common::{run_php, run_php_with_functions};

#[test]
fn ini_get_all_projects_admitted_registry_values_and_extension_groups() {
    assert_eq!(
        run_php(
            r#"<?php
$function = new ReflectionFunction('ini_get_all');
echo $function->getExtensionName(), ':', $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), ':', $function->getReturnType(), '|';
foreach ($function->getParameters() as $parameter) {
    echo $parameter->getName(), ':', $parameter->getType(), ':';
    var_export($parameter->getDefaultValue());
    echo '|';
}

$all = ini_get_all();
echo count($all), ':', array_key_first($all), ':', array_key_last($all), '|';
foreach (['allow_url_fopen', 'auto_prepend_file', 'precision'] as $name) {
    $entry = $all[$name];
    echo $name, ':';
    var_export($entry['global_value']);
    echo ':';
    var_export($entry['local_value']);
    echo ':', $entry['access'], '|';
}

ini_set('precision', '3');
$precision = ini_get_all(null, true)['precision'];
echo $precision['global_value'], ':', $precision['local_value'], ':', $precision['access'], '|';
echo ini_get_all(null, false)['precision'], '|';
foreach (['pcre', 'standard', 'iconv', 'json'] as $extension) {
    $entries = ini_get_all($extension, false);
    echo $extension, ':', implode(',', array_keys($entries)), '|';
}
echo 'iconv-values:', count(ini_get_all('iconv', false)), ':', implode(',', ini_get_all('iconv', false)), '|';
"#,
        ),
        concat!(
            "standard:0/2:array|false|extension:?string:NULL|details:bool:true|",
            "44:allow_url_fopen:zend.exception_string_param_max_len|",
            "allow_url_fopen:'1':'1':4|",
            "auto_prepend_file:NULL:NULL:6|",
            "precision:'14':'14':7|",
            "14:3:7|3|",
            "pcre:pcre.backtrack_limit,pcre.recursion_limit|",
            "standard:assert.exception|",
            "iconv:iconv.input_encoding,iconv.internal_encoding,iconv.output_encoding|",
            "json:|",
            "iconv-values:3:,,|",
        ),
    );
}

#[test]
fn ini_get_all_keeps_startup_global_value_distinct_and_reports_unknown_extensions() {
    assert_eq!(
        run_php_with_functions(
            r#"<?php
$before = ini_get_all(null, true)['precision'];
ini_set('precision', '9');
$after = ini_get_all(null, true)['precision'];
echo $before['global_value'], ':', $before['local_value'], '|';
echo $after['global_value'], ':', $after['local_value'], '|';
set_error_handler(static function (int $level, string $message): bool {
    echo $level, ':', $message, '|';
    return true;
});
foreach (['', 'Pcre', 'missing'] as $extension) {
    var_export(ini_get_all($extension));
    echo '|';
}
"#,
            |eg| {
                rphp::stdlib::apply_startup_ini_settings(
                    eg,
                    &[("precision".to_string(), "6".to_string())],
                );
            },
        ),
        concat!(
            "6:6|6:9|",
            "2:ini_get_all(): Extension \"\" cannot be found|false|",
            "2:ini_get_all(): Extension \"Pcre\" cannot be found|false|",
            "2:ini_get_all(): Extension \"missing\" cannot be found|false|",
        ),
    );
}

#[test]
fn ini_get_all_follows_nullable_string_and_boolean_call_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function (int $level, string $message): bool {
    echo $message, '|';
    return true;
});
var_export(ini_get_all(123, ''));
"#,
        ),
        "ini_get_all(): Extension \"123\" cannot be found|false",
    );
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
try {
    ini_get_all([], true);
} catch (TypeError $error) {
    echo $error->getMessage(), '|';
}
try {
    ini_get_all(null, []);
} catch (TypeError $error) {
    echo $error->getMessage(), '|';
}
"#,
        ),
        concat!(
            "ini_get_all(): Argument #1 ($extension) must be of type ?string, array given|",
            "ini_get_all(): Argument #2 ($details) must be of type bool, array given|",
        ),
    );
}
