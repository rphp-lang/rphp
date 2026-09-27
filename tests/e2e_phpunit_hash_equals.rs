mod common;

use common::run_php;

#[test]
fn hash_equals_preserves_binary_values_exact_types_and_sensitive_metadata() {
    assert_eq!(
        run_php(
            r#"<?php
$function = new ReflectionFunction('hash_equals');
echo $function->getExtensionName(), ':', $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), ':', $function->getReturnType(), '|';
foreach ($function->getParameters() as $parameter) {
    echo $parameter->getName(), ':', $parameter->getType(), ':', count($parameter->getAttributes(SensitiveParameter::class)), '|';
}
foreach ([
    ['', ''], ['same', 'same'], ['not1same', 'not2same'], ['short', 'longer'],
    ["a\0\x80\xff", "a\0\x80\xff"], ["a\0\x80\xff", "a\0\x80\xfe"],
] as [$known, $user]) {
    echo (int) hash_equals($known, $user);
}
echo '|';
foreach ([[123, 'x'], ['x', 123], [null, '']] as $arguments) {
    try {
        hash_equals(...$arguments);
    } catch (TypeError $error) {
        echo $error->getMessage(), '|';
    }
}
"#,
        ),
        concat!(
            "hash:2/2:bool|known_string:string:1|user_string:string:1|",
            "110010|",
            "hash_equals(): Argument #1 ($known_string) must be of type string, int given|",
            "hash_equals(): Argument #2 ($user_string) must be of type string, int given|",
            "hash_equals(): Argument #1 ($known_string) must be of type string, null given|",
        ),
    );
}

#[test]
fn hash_equals_redacts_both_arguments_in_throwable_traces() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    hash_equals('known-secret', null);
} catch (Throwable $error) {
    $trace = $error->getTrace();
    echo get_debug_type($trace[0]['args'][0]), ':', get_debug_type($trace[0]['args'][1]);
}
"#,
        ),
        "SensitiveParameterValue:SensitiveParameterValue",
    );
}
