mod common;

use common::{run_php, run_php_expect_error_with_source_context};

#[test]
fn trim_family_preserves_php_bytes_directions_ranges_and_alias_diagnostics() {
    assert_eq!(
        run_php(
            r#"<?php
$edge = "\0\t\n\v\r \f.left.mid.\x80\f \0\r\n\v\t";
foreach (['trim', 'ltrim', 'rtrim', 'chop'] as $function) {
    echo $function, '=', bin2hex($function($edge)), "\n";
}

$diagnostics = [];
set_error_handler(function (int $level, string $message) use (&$diagnostics): bool {
    $diagnostics[] = $level . ':' . $message;
    return true;
});
$sample = ".azAZ09\0\x1f\x7f\x80\xff";
foreach (['a..z', 'z..a', '..a', 'a..', 'a...z', 'a....z', '.......'] as $characters) {
    foreach (['trim', 'ltrim', 'rtrim', 'chop'] as $function) {
        $diagnostics = [];
        $result = $function($sample, $characters);
        echo $function, '/', bin2hex($characters), '=', implode(',', $diagnostics),
            '|', bin2hex($result), "\n";
    }
}

$all = '';
for ($byte = 0; $byte <= 255; $byte++) {
    $all .= chr($byte);
}
echo 'all=', strlen(trim($all, "\0..\xff")), "\n";
echo 'high=', bin2hex(trim("\xff\x80A\x80\xff", "\x80..\xff")), "\n";
$reflection = new ReflectionFunction('chop');
echo 'alias=', (int) function_exists('chop'), ':',
    (int) in_array('chop', get_defined_functions()['internal'], true), ':',
    $reflection->getName(), ':', $reflection->getNumberOfParameters(), ':',
    $reflection->getNumberOfRequiredParameters(), "\n";
"#,
        ),
        r#"trim=0c2e6c6566742e6d69642e800c
ltrim=0c2e6c6566742e6d69642e800c20000d0a0b09
rtrim=00090a0b0d200c2e6c6566742e6d69642e800c
chop=00090a0b0d200c2e6c6566742e6d69642e800c
trim/612e2e7a=|2e617a415a3039001f7f80ff
ltrim/612e2e7a=|2e617a415a3039001f7f80ff
rtrim/612e2e7a=|2e617a415a3039001f7f80ff
chop/612e2e7a=|2e617a415a3039001f7f80ff
trim/7a2e2e61=2:trim(): Invalid '..'-range, '..'-range needs to be incrementing|415a3039001f7f80ff
ltrim/7a2e2e61=2:ltrim(): Invalid '..'-range, '..'-range needs to be incrementing|415a3039001f7f80ff
rtrim/7a2e2e61=2:rtrim(): Invalid '..'-range, '..'-range needs to be incrementing|2e617a415a3039001f7f80ff
chop/7a2e2e61=2:chop(): Invalid '..'-range, '..'-range needs to be incrementing|2e617a415a3039001f7f80ff
trim/2e2e61=2:trim(): Invalid '..'-range, no character to the left of '..'|7a415a3039001f7f80ff
ltrim/2e2e61=2:ltrim(): Invalid '..'-range, no character to the left of '..'|7a415a3039001f7f80ff
rtrim/2e2e61=2:rtrim(): Invalid '..'-range, no character to the left of '..'|2e617a415a3039001f7f80ff
chop/2e2e61=2:chop(): Invalid '..'-range, no character to the left of '..'|2e617a415a3039001f7f80ff
trim/612e2e=2:trim(): Invalid '..'-range, no character to the right of '..'|7a415a3039001f7f80ff
ltrim/612e2e=2:ltrim(): Invalid '..'-range, no character to the right of '..'|7a415a3039001f7f80ff
rtrim/612e2e=2:rtrim(): Invalid '..'-range, no character to the right of '..'|2e617a415a3039001f7f80ff
chop/612e2e=2:chop(): Invalid '..'-range, no character to the right of '..'|2e617a415a3039001f7f80ff
trim/612e2e2e7a=|001f7f80ff
ltrim/612e2e2e7a=|001f7f80ff
rtrim/612e2e2e7a=|2e617a415a3039001f7f80ff
chop/612e2e2e7a=|2e617a415a3039001f7f80ff
trim/612e2e2e2e7a=|415a3039001f7f80ff
ltrim/612e2e2e2e7a=|415a3039001f7f80ff
rtrim/612e2e2e2e7a=|2e617a415a3039001f7f80ff
chop/612e2e2e2e7a=|2e617a415a3039001f7f80ff
trim/2e2e2e2e2e2e2e=2:trim(): Invalid '..'-range,2:trim(): Invalid '..'-range, no character to the right of '..'|617a415a3039001f7f80ff
ltrim/2e2e2e2e2e2e2e=2:ltrim(): Invalid '..'-range,2:ltrim(): Invalid '..'-range, no character to the right of '..'|617a415a3039001f7f80ff
rtrim/2e2e2e2e2e2e2e=2:rtrim(): Invalid '..'-range,2:rtrim(): Invalid '..'-range, no character to the right of '..'|2e617a415a3039001f7f80ff
chop/2e2e2e2e2e2e2e=2:chop(): Invalid '..'-range,2:chop(): Invalid '..'-range, no character to the right of '..'|2e617a415a3039001f7f80ff
all=0
high=41
alias=1:1:chop:2:1
"#,
    );
}

#[test]
fn trim_family_uses_weak_scalar_and_null_coercion() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function (int $level, string $message): bool {
    echo 'diag=', $level, ':', $message, "\n";
    return true;
});
foreach ([null, false, true, 0, 12, 1.5] as $index => $value) {
    echo 'weak/', $index, '=', bin2hex(trim($value)), "\n";
}
echo 'null-mask=', bin2hex(trim(" x ", null)), "\n";
"#,
        ),
        r#"weak/0=diag=8192:trim(): Passing null to parameter #1 ($string) of type string is deprecated

weak/1=
weak/2=31
weak/3=30
weak/4=3132
weak/5=312e35
null-mask=diag=8192:trim(): Passing null to parameter #2 ($characters) of type string is deprecated
207820
"#,
    );
}

#[test]
fn trim_family_rejects_strict_scalar_arguments_with_alias_names() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
foreach (['trim', 'ltrim', 'rtrim', 'chop'] as $function) {
    try {
        $function(12);
    } catch (TypeError $error) {
        echo $error->getMessage(), "\n";
    }
    try {
        $function('value', 12);
    } catch (TypeError $error) {
        echo $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "trim(): Argument #1 ($string) must be of type string, int given\n",
            "trim(): Argument #2 ($characters) must be of type string, int given\n",
            "ltrim(): Argument #1 ($string) must be of type string, int given\n",
            "ltrim(): Argument #2 ($characters) must be of type string, int given\n",
            "rtrim(): Argument #1 ($string) must be of type string, int given\n",
            "rtrim(): Argument #2 ($characters) must be of type string, int given\n",
            "chop(): Argument #1 ($string) must be of type string, int given\n",
            "chop(): Argument #2 ($characters) must be of type string, int given\n",
        ),
    );
}

#[test]
fn trim_alias_resolves_static_namespace_fallback_and_blocks_redeclaration() {
    assert_eq!(
        run_php(
            r#"<?php
namespace TrimAliasFixture;
set_error_handler(function (int $level, string $message): bool {
    echo $message, "\n";
    return true;
});
var_dump(chop(" x "));
chop(null);
var_dump(\CHOP(" y "));
echo (int) function_exists('\chop'), ':', (new \ReflectionFunction('CHOP'))->getName();
"#,
        ),
        concat!(
            "string(2) \" x\"\n",
            "chop(): Passing null to parameter #1 ($string) of type string is deprecated\n",
            "string(2) \" y\"\n",
            "1:chop",
        ),
    );

    assert_eq!(
        run_php_expect_error_with_source_context(
            "<?php\nfunction CHOP() {}",
            "/virtual/trim-alias-redeclaration.php",
            "/virtual",
        )
        .to_string(),
        "Cannot redeclare function CHOP() in /virtual/trim-alias-redeclaration.php on line 2",
    );
}
