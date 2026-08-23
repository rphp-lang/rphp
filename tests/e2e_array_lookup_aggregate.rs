mod common;

use common::run_php;

#[test]
fn lookup_signatures_filters_references_and_types_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['in_array', 'array_search', 'array_key_exists', 'key_exists', 'array_sum', 'array_product'] as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, ':', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':';
    foreach ($reflection->getParameters() as $parameter) {
        echo $parameter->getName(), ';';
    }
    echo "\n";
}

$slot = 42;
$haystack = ['reference' => &$slot, 'string' => '42', 'empty' => [], 'zero' => '0'];
echo 'in-loose:', in_array(42, $haystack) ? 'yes' : 'no', "\n";
echo 'in-strict:', in_array(42, $haystack, true) ? 'yes' : 'no', "\n";
echo 'search-loose:', array_search('42', $haystack), "\n";
echo 'search-strict:', array_search('42', $haystack, true), "\n";
echo 'array-strict:', array_search([], $haystack, true), "\n";
echo 'false-loose:', array_search(false, $haystack), "\n";
$large = [9007199254740992, 9007199254740993];
echo 'large-in:', in_array('9007199254740993', [$large[0]]) ? 'yes' : 'no', "\n";
echo 'large-search:', array_search('9007199254740993', $large), "\n";
echo 'large-strings:', array_search(
    '9007199254740993',
    ['9007199254740992', '9007199254740993']
), "\n";

set_error_handler(function ($level, $message) {
    echo 'warning:', $level, ':', $message, "\n";
    return true;
});
echo 'null-strict:', in_array('42', $haystack, null) ? 'yes' : 'no', "\n";
restore_error_handler();

foreach (['in_array', 'array_search'] as $name) {
    try {
        $name(1, new stdClass());
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "in_array:2/3:needle;haystack;strict;\n",
            "array_search:2/3:needle;haystack;strict;\n",
            "array_key_exists:2/2:key;array;\n",
            "key_exists:2/2:key;array;\n",
            "array_sum:1/1:array;\n",
            "array_product:1/1:array;\n",
            "in-loose:yes\n",
            "in-strict:yes\n",
            "search-loose:reference\n",
            "search-strict:string\n",
            "array-strict:empty\n",
            "false-loose:empty\n",
            "large-in:no\n",
            "large-search:1\n",
            "large-strings:1\n",
            "null-strict:warning:8192:in_array(): Passing null to parameter #3 ($strict) of type bool is deprecated\n",
            "yes\n",
            "TypeError:in_array(): Argument #2 ($haystack) must be of type array, stdClass given\n",
            "TypeError:array_search(): Argument #2 ($haystack) must be of type array, stdClass given\n",
        )
    );
}

#[test]
fn lookup_recursion_and_cv_operator_diagnostics_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
$left = [];
$left['self'] = &$left;
$right = [];
$right['self'] = &$right;
foreach (['in_array', 'array_search'] as $name) {
    foreach ([false, true] as $strict) {
        try {
            $name($left, [$right], $strict);
        } catch (Throwable $error) {
            echo $name, ':', $strict ? 'strict' : 'loose', ':', get_class($error),
                ':', $error->getMessage(), "\n";
        }
    }
}
try {
    array_keys([$right], $left, true);
} catch (Throwable $error) {
    echo 'array_keys:strict:', get_class($error), ':', $error->getMessage(), "\n";
}

function multiplyOperands(mixed $left, mixed $right): mixed {
    return $left * $right;
}
$resource = fopen('php://memory', 'r+');
try {
    multiplyOperands(10, $resource);
} catch (Throwable $error) {
    echo 'cv-cv:', $error->getMessage(), "\n";
}
try {
    10 * $resource;
} catch (Throwable $error) {
    echo 'const-cv:', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "in_array:loose:Error:Nesting level too deep - recursive dependency?\n",
            "in_array:strict:Error:Nesting level too deep - recursive dependency?\n",
            "array_search:loose:Error:Nesting level too deep - recursive dependency?\n",
            "array_search:strict:Error:Nesting level too deep - recursive dependency?\n",
            "array_keys:strict:Error:Nesting level too deep - recursive dependency?\n",
            "cv-cv:Unsupported operand types: int * resource\n",
            "const-cv:Unsupported operand types: resource * int\n",
        )
    );
}

#[test]
fn lookup_strict_bool_arguments_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);

foreach (['in_array', 'array_search'] as $name) {
    foreach ([1, '1', null, new stdClass()] as $strict) {
        try {
            $name(1, [1], $strict);
        } catch (Throwable $error) {
            echo get_class($error), ':', $error->getMessage(), "\n";
        }
    }
}
"#,
        ),
        concat!(
            "TypeError:in_array(): Argument #3 ($strict) must be of type bool, int given\n",
            "TypeError:in_array(): Argument #3 ($strict) must be of type bool, string given\n",
            "TypeError:in_array(): Argument #3 ($strict) must be of type bool, null given\n",
            "TypeError:in_array(): Argument #3 ($strict) must be of type bool, stdClass given\n",
            "TypeError:array_search(): Argument #3 ($strict) must be of type bool, int given\n",
            "TypeError:array_search(): Argument #3 ($strict) must be of type bool, string given\n",
            "TypeError:array_search(): Argument #3 ($strict) must be of type bool, null given\n",
            "TypeError:array_search(): Argument #3 ($strict) must be of type bool, stdClass given\n",
        )
    );
}

#[test]
fn array_key_exists_coercion_diagnostics_and_reentrancy_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
$array = ['' => 'empty', 0 => 'zero', 1 => 'one', -2 => 'minus'];
$warnings = [];
set_error_handler(function ($level, $message) use (&$warnings) {
    $message = preg_replace('/Resource ID#[0-9]+/', 'Resource ID#@', $message);
    $message = preg_replace('/\([0-9]+\)$/', '(@)', $message);
    $warnings[] = $level . ':' . $message;
    return true;
});
foreach ([null, false, true, 1.75, -2.0, STDERR] as $key) {
    echo get_debug_type($key), ':', array_key_exists($key, $array) ? 'yes' : 'no', "\n";
}
echo 'alias:', key_exists(null, $array) ? 'yes' : 'no', "\n";
restore_error_handler();
foreach ($warnings as $warning) {
    echo 'warning:', $warning, "\n";
}

foreach ([[], new stdClass()] as $key) {
    try {
        array_key_exists($key, $array);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}

set_error_handler(function ($level, $message) use (&$array) {
    $array = [];
    return true;
});
echo 'snapshot:', array_key_exists(null, $array) ? 'yes' : 'no', ':', count($array), "\n";
restore_error_handler();
"#,
        ),
        concat!(
            "null:yes\n",
            "bool:yes\n",
            "bool:yes\n",
            "float:yes\n",
            "float:yes\n",
            "resource (stream):no\n",
            "alias:yes\n",
            "warning:8192:Using null as the key parameter for array_key_exists() is deprecated, use an empty string instead\n",
            "warning:8192:Implicit conversion from float 1.75 to int loses precision\n",
            "warning:2:Resource ID#@ used as offset, casting to integer (@)\n",
            "warning:8192:Using null as the key parameter for array_key_exists() is deprecated, use an empty string instead\n",
            "TypeError:Cannot access offset of type array on array\n",
            "TypeError:Cannot access offset of type stdClass on array\n",
            "snapshot:yes:0\n",
        )
    );
}

#[test]
fn array_aggregates_match_php_85_conversion_warning_overflow_and_error_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
$reference = '10';
$reference .= '0';
$referenced = [&$reference, 100];
var_dump(array_sum($referenced), array_product($referenced), $reference);
var_dump(
    array_sum([1, 2, 3, 4]),
    array_product([1, -1, 1, -1]),
    array_sum([1.25, 2.5, -0.5]),
    array_product([1.25, 2.0, 4.0])
);

$values = [2, '3', '4.5', true, null, '12tail', 'word', [], new stdClass()];
$warnings = [];
set_error_handler(function ($level, $message) use (&$warnings) {
    $warnings[] = $message;
    return true;
});
var_dump(array_sum($values), array_product($values));
echo 'resource-sum:', array_sum([10, STDERR]) === 10 + get_resource_id(STDERR) ? 'yes' : 'no', "\n";
echo 'resource-product:', array_product([10, STDERR]) === 10 * get_resource_id(STDERR) ? 'yes' : 'no', "\n";
restore_error_handler();
foreach ($warnings as $warning) {
    echo 'warning:', $warning, "\n";
}

foreach ([[PHP_INT_MAX, 1], [PHP_INT_MIN, -1], [PHP_INT_MAX, 2]] as $input) {
    var_dump(array_sum($input), array_product($input));
}

foreach (['array_sum', 'array_product'] as $name) {
    set_error_handler(function ($level, $message) {
        throw new RuntimeException('stop:' . $message);
    });
    try {
        $name([2, [], 3]);
    } catch (Throwable $error) {
        echo $name, ':', $error->getMessage(), "\n";
    }
    restore_error_handler();
}

foreach (['array_sum', 'array_product'] as $name) {
    try {
        $name(null);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "int(200)\n",
            "int(10000)\n",
            "string(3) \"100\"\n",
            "int(10)\n",
            "int(1)\n",
            "float(3.25)\n",
            "float(10)\n",
            "float(22.5)\n",
            "float(0)\n",
            "resource-sum:yes\n",
            "resource-product:yes\n",
            "warning:A non-numeric value encountered\n",
            "warning:array_sum(): Addition is not supported on type string\n",
            "warning:array_sum(): Addition is not supported on type array\n",
            "warning:array_sum(): Addition is not supported on type stdClass\n",
            "warning:A non-numeric value encountered\n",
            "warning:array_product(): Multiplication is not supported on type string\n",
            "warning:array_product(): Multiplication is not supported on type array\n",
            "warning:array_product(): Multiplication is not supported on type stdClass\n",
            "warning:array_sum(): Addition is not supported on type resource\n",
            "warning:array_product(): Multiplication is not supported on type resource\n",
            "float(9.223372036854776E+18)\n",
            "int(9223372036854775807)\n",
            "float(-9.223372036854776E+18)\n",
            "float(9.223372036854776E+18)\n",
            "float(9.223372036854776E+18)\n",
            "float(1.8446744073709552E+19)\n",
            "array_sum:stop:array_sum(): Addition is not supported on type array\n",
            "array_product:stop:array_product(): Multiplication is not supported on type array\n",
            "TypeError:array_sum(): Argument #1 ($array) must be of type array, null given\n",
            "TypeError:array_product(): Argument #1 ($array) must be of type array, null given\n",
        )
    );
}
