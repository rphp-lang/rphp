mod common;

use common::{run_php, run_php_with_source_context};

#[test]
fn weak_scalar_coercions_report_before_calls_returns_and_typed_writes_commit() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function (int $level, string $message): bool {
    echo "$level:$message|";
    return true;
});
function acceptInt(int $value): void { echo "int=$value\n"; }
function acceptUnion(int|string $value): void { echo get_debug_type($value), "=$value\n"; }
function returnInt(): int { return 4.5; }
class ScalarHolder { public int $value; }
acceptInt(1.5);
acceptInt('2.5');
acceptUnion(NAN);
var_dump(returnInt());
$holder = new ScalarHolder;
$holder->value = 6.5;
var_dump($holder->value);
restore_error_handler();
set_error_handler(function (int $level, string $message): never {
    throw new RuntimeException("stop:$message");
});
try { acceptInt(7.5); echo "bad-body\n"; }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "8192:Implicit conversion from float 1.5 to int loses precision|int=1\n",
            "8192:Implicit conversion from float-string \"2.5\" to int loses precision|int=2\n",
            "2:unexpected NAN value was coerced to string|string=NAN\n",
            "8192:Implicit conversion from float 4.5 to int loses precision|int(4)\n",
            "8192:Implicit conversion from float 6.5 to int loses precision|int(6)\n",
            "RuntimeException:stop:Implicit conversion from float 7.5 to int loses precision\n",
        )
    );
}

#[test]
fn implicit_integer_diagnostics_use_the_consuming_expression_line() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
set_error_handler(function (int $level, string $message, string $file, int $line): bool {
    echo "$level@$line:$message\n";
    return true;
});
var_dump(~1.5);
var_dump(7 % 2.5);
var_dump(3 << 1.5);
$array = ['a', 'b', 'c'];
var_dump($array[1.5]);
$string = 'abc';
var_dump($string[1.5]);
"#,
            "/virtual/scalar-coercion-lines.php",
            "/virtual",
        ),
        concat!(
            "8192@6:Implicit conversion from float 1.5 to int loses precision\n",
            "int(-2)\n",
            "8192@7:Implicit conversion from float 2.5 to int loses precision\n",
            "int(1)\n",
            "8192@8:Implicit conversion from float 1.5 to int loses precision\n",
            "int(6)\n",
            "8192@10:Implicit conversion from float 1.5 to int loses precision\n",
            "string(1) \"b\"\n",
            "2@12:String offset cast occurred\n",
            "string(1) \"b\"\n",
        )
    );
}

#[test]
fn omitted_constant_defaults_are_validated_before_the_function_body() {
    assert_eq!(
        run_php(
            r#"<?php
const BAD_INT_DEFAULT = 'text';
const NULL_INT_DEFAULT = null;
function badDefault(int $value = BAD_INT_DEFAULT): void { echo "bad-body\n"; }
function nullDefault(int $value = NULL_INT_DEFAULT): void { echo "null-body\n"; }
function nullableDefault(?int $value = NULL_INT_DEFAULT): void { var_dump($value); }
foreach (['badDefault', 'nullDefault', 'nullableDefault'] as $callback) {
    try { $callback(); }
    catch (Throwable $error) {
        echo get_class($error), ':', explode(', called', $error->getMessage())[0], "\n";
    }
}
"#,
        ),
        concat!(
            "TypeError:badDefault(): Argument #1 ($value) must be of type int, string given\n",
            "TypeError:nullDefault(): Argument #1 ($value) must be of type int, null given\n",
            "NULL\n",
        )
    );
}

#[test]
fn reentrant_and_nonrepresentable_offsets_preserve_php_operand_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
function dynamicValue(): int { return 10; }
set_error_handler(function (int $level, string $message): bool {
    global $values;
    $values = null;
    echo "$level:$message|";
    return true;
});
$values = [dynamicValue()];
var_dump(array_key_exists(1.0E+42, $values));
var_dump($values);
restore_error_handler();
set_error_handler(function (int $level, string $message): bool {
    echo "$level:$message|";
    return true;
});
$text = 'abc';
try { var_dump($text[1.0E+42]); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
try { var_dump($text['1.0E+42']); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "2:The float 1.0E+42 is not representable as an int, cast occurred|bool(false)\n",
            "NULL\n",
            "2:String offset cast occurred|string(1) \"a\"\n",
            "TypeError:Cannot access offset of type string on string\n",
        )
    );
}

#[test]
fn weak_numeric_strings_accept_php_vertical_tab_and_form_feed_whitespace() {
    assert_eq!(
        run_php(
            r#"<?php
function acceptWhitespaceInt(int $value): void { var_dump($value); }
function acceptWhitespaceFloat(float $value): void { var_dump($value); }
acceptWhitespaceInt("\v\f123\v\f");
acceptWhitespaceFloat("\v\f123.0\v\f");
"#,
        ),
        "int(123)\nfloat(123)\n"
    );
}
