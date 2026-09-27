mod common;

use common::run_php;

#[test]
fn random_extension_reflection_matches_php_85() {
    let out = run_php(
        r#"<?php
$names = [
    'srand', 'mt_srand', 'rand', 'mt_rand',
    'getrandmax', 'mt_getrandmax', 'random_bytes', 'random_int',
];
foreach ($names as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, '|', $reflection->getExtensionName(), '|',
        $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), '|',
        $reflection->hasReturnType() ? (string) $reflection->getReturnType() : 'none';
    foreach ($reflection->getParameters() as $parameter) {
        echo '|', $parameter->getName(), ':',
            $parameter->hasType() ? (string) $parameter->getType() : 'none', ':';
        if (!$parameter->isDefaultValueAvailable()) {
            echo '-';
        } elseif ($parameter->isDefaultValueConstant()) {
            echo $parameter->getDefaultValueConstantName();
        } else {
            echo var_export($parameter->getDefaultValue(), true);
        }
    }
    echo "\n";
}
echo MT_RAND_MT19937, '|', MT_RAND_MAX, "\n";
"#,
    );
    assert_eq!(
        out,
        concat!(
            "srand|random|0/2|void|seed:?int:NULL|mode:int:MT_RAND_MT19937\n",
            "mt_srand|random|0/2|void|seed:?int:NULL|mode:int:MT_RAND_MT19937\n",
            "rand|random|0/2|int|min:int:-|max:int:-\n",
            "mt_rand|random|0/2|int|min:int:-|max:int:-\n",
            "getrandmax|random|0/0|int\n",
            "mt_getrandmax|random|0/0|int\n",
            "random_bytes|random|1/1|string|length:int:-\n",
            "random_int|random|2/2|int|min:int:-|max:int:-\n",
            "0|2147483647\n",
        )
    );
}

#[test]
fn mt_srand_reproduces_standard_legacy_and_wide_php_sequences() {
    let out = run_php(
        r#"<?php
mt_srand(1234);
for ($i = 0; $i < 6; $i++) {
    echo mt_rand(), $i === 5 ? "\n" : ',';
}
srand(1234);
echo mt_rand(), '|', rand(), '|', mt_rand(), "\n";
mt_srand(-1);
echo mt_rand(), ',', mt_rand(), "\n";
mt_srand(1234, 2);
echo mt_rand(), "\n";
set_error_handler(function ($level, $message) { echo 'D:', $message, "\n"; });
mt_srand(1234, 1);
restore_error_handler();
for ($i = 0; $i < 5; $i++) {
    echo mt_rand(), $i === 4 ? "\n" : ',';
}
mt_srand(1);
echo mt_rand(PHP_INT_MIN, PHP_INT_MAX), "\n";
"#,
    );
    assert_eq!(
        out,
        concat!(
            "411284887,1068724585,1335968403,1756294682,940013158,1314500282\n",
            "411284887|1068724585|1335968403\n",
            "209663185,239673489\n",
            "411284887\n",
            "D:The MT_RAND_PHP variant of Mt19937 is deprecated\n",
            "1741177057,1068724585,1335968403,400890732,1196196624\n",
            "9171440914760070181\n",
        )
    );
}

#[test]
fn seeded_random_state_is_shared_by_array_and_string_consumers() {
    let out = run_php(
        r#"<?php
mt_srand(1234);
$values = [1, 2, 3, 4, 5];
shuffle($values);
echo implode(',', $values), "\n";
mt_srand(1234);
echo str_shuffle('abcde'), "\n";
mt_srand(1234);
echo implode(',', array_rand(['a' => 1, 'b' => 2, 'c' => 3, 'd' => 4, 'e' => 5], 3)),
    '|', mt_rand(), "\n";
mt_srand(1234);
echo mt_rand(0, 10), '|';
$values = [1, 2, 3, 4];
shuffle($values);
echo implode(',', $values), '|', mt_rand(0, 10), "\n";
"#,
    );
    assert_eq!(out, "3,2,5,4,1\ncbeda\nc,d,e|1335968403\n5|3,2,1,4|3\n");
}

#[test]
fn legacy_random_calls_enforce_php_85_argument_boundaries() {
    let out = run_php(
        r#"<?php
srand(1234);
echo rand(20, 10), "\n";
foreach ([['mt_rand', [1]], ['rand', [1]], ['mt_rand', [2, 1]]] as [$function, $args]) {
    try {
        $function(...$args);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
foreach (['rand', 'mt_rand'] as $function) {
    try {
        $function(max: 2);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
mt_srand('12');
echo mt_rand(), "\n";
set_error_handler(function ($level, $message) { echo 'D:', $message, "\n"; });
echo MT_RAND_PHP, "\n";
restore_error_handler();
"#,
    );
    assert_eq!(
        out,
        concat!(
            "15\n",
            "ArgumentCountError:mt_rand() expects exactly 2 arguments, 1 given\n",
            "ArgumentCountError:rand() expects exactly 2 arguments, 1 given\n",
            "ValueError:mt_rand(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)\n",
            "ArgumentCountError:rand(): Argument #1 ($min) must be passed explicitly, because the default value is not known\n",
            "ArgumentCountError:mt_rand(): Argument #1 ($min) must be passed explicitly, because the default value is not known\n",
            "331062181\n",
            "D:Constant MT_RAND_PHP is deprecated since 8.3, as it uses a biased non-standard variant of Mt19937\n",
            "1\n",
        )
    );
    assert_eq!(
        run_php(
            "<?php declare(strict_types=1); try { mt_srand('12'); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(); }",
        ),
        "TypeError:mt_srand(): Argument #1 ($seed) must be of type ?int, string given",
    );
}
