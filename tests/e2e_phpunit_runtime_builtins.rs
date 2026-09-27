mod common;

use common::run_php;

#[test]
fn getrusage_matches_php_shape_reflection_and_mode_contract() {
    assert_eq!(
        run_php(
            r#"<?php
$function = new ReflectionFunction('getrusage');
$parameter = $function->getParameters()[0];
echo $function->getExtensionName(), ':', $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), ':', $function->getReturnType(), '|';
echo $parameter->getName(), ':', $parameter->getType(), ':', var_export($parameter->getDefaultValue(), true), '|';
$expected = ['ru_oublock', 'ru_inblock', 'ru_msgsnd', 'ru_msgrcv', 'ru_maxrss', 'ru_ixrss', 'ru_idrss', 'ru_minflt', 'ru_majflt', 'ru_nsignals', 'ru_nvcsw', 'ru_nivcsw', 'ru_nswap', 'ru_utime.tv_usec', 'ru_utime.tv_sec', 'ru_stime.tv_usec', 'ru_stime.tv_sec'];
foreach ([0, 1, -1, 99] as $mode) {
    $usage = getrusage($mode);
    echo (int) is_array($usage), ':', (int) (array_keys($usage) === $expected), ':', (int) (count(array_filter($usage, 'is_int')) === 17), '|';
}
try {
    getrusage([]);
} catch (TypeError $error) {
    echo $error->getMessage();
}
"#,
        ),
        "standard:0/1:array|false|mode:int:0|1:1:1|1:1:1|1:1:1|1:1:1|getrusage(): Argument #1 ($mode) must be of type int, array given",
    );
}

#[test]
fn php_extra_version_completes_the_public_version_tuple() {
    assert_eq!(
        run_php(
            r#"<?php
echo gettype(PHP_EXTRA_VERSION), ':', strlen(PHP_EXTRA_VERSION), '|';
echo PHP_MAJOR_VERSION, '.', PHP_MINOR_VERSION, '.', PHP_RELEASE_VERSION, ':', PHP_VERSION_ID, ':', PHP_VERSION;
"#,
        ),
        "string:0|8.5.0:80500:8.5.0",
    );
}
