mod common;

use common::run_php;

#[test]
fn randomizer_get_bytes_from_string_exposes_the_php_85_contract() {
    assert_eq!(
        run_php(
            r#"<?php
$reflection = new ReflectionMethod(Random\Randomizer::class, 'getBytesFromString');
echo $reflection->getExtensionName(), ':', $reflection->getNumberOfRequiredParameters(), '/',
    $reflection->getNumberOfParameters(), ':', $reflection->getReturnType(), '|';
$value = (new Random\Randomizer())->getBytesFromString('ab', 32);
echo strlen($value), ':', (int) (strspn($value, 'ab') === 32), "\n";
foreach ([['', 1], ['a', 0]] as [$alphabet, $length]) {
    try {
        (new Random\Randomizer())->getBytesFromString($alphabet, $length);
    } catch (ValueError $error) {
        echo $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "random:2/2:string|32:1\n",
            "Random\\Randomizer::getBytesFromString(): Argument #1 ($string) must not be empty\n",
            "Random\\Randomizer::getBytesFromString(): Argument #2 ($length) must be greater than 0\n",
        ),
    );
}
