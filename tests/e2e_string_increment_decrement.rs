mod common;

use common::run_php;

#[test]
fn string_steps_cover_carry_borrow_bytes_errors_and_immutability() {
    assert_eq!(
        run_php(
            r#"<?php
function observe_step(string $function, string $value): void {
    try {
        echo $function, '/', bin2hex($value), '=', bin2hex($function($value)), "\n";
    } catch (Throwable $error) {
        echo $function, '/', bin2hex($value), '=', $error->getMessage(), "\n";
    }
}

$values = ['9', 'Z', 'z', 'Z9', '9Z', 'A0', 'a0', '10a', '01', 'A', 'a', '0', '999', 'ZZZ', 'zzz', 'Zz9'];
foreach (['str_increment', 'str_decrement'] as $function) {
    foreach ($values as $value) {
        observe_step($function, $value);
    }
}

$binary = "\x39";
echo 'binary=', bin2hex(str_increment($binary)), ':', bin2hex(str_decrement($binary)), "\n";
foreach (["\0", "\x80"] as $invalid) {
    observe_step('str_increment', $invalid);
    observe_step('str_decrement', $invalid);
}

$source = 'Zz9';
$result = str_increment($source);
echo 'immutable=', $source, ':', $result, "\n";
"#,
        ),
        r#"str_increment/39=3130
str_increment/5a=4141
str_increment/7a=6161
str_increment/5a39=414130
str_increment/395a=313041
str_increment/4130=4131
str_increment/6130=6131
str_increment/313061=313062
str_increment/3031=3032
str_increment/41=42
str_increment/61=62
str_increment/30=31
str_increment/393939=31303030
str_increment/5a5a5a=41414141
str_increment/7a7a7a=61616161
str_increment/5a7a39=41416130
str_decrement/39=38
str_decrement/5a=59
str_decrement/7a=79
str_decrement/5a39=5a38
str_decrement/395a=3959
str_decrement/4130=39
str_decrement/6130=39
str_decrement/313061=397a
str_decrement/3031=str_decrement/3031=str_decrement(): Argument #1 ($string) "01" is out of decrement range
str_decrement/41=str_decrement/41=str_decrement(): Argument #1 ($string) "A" is out of decrement range
str_decrement/61=str_decrement/61=str_decrement(): Argument #1 ($string) "a" is out of decrement range
str_decrement/30=str_decrement/30=str_decrement(): Argument #1 ($string) "0" is out of decrement range
str_decrement/393939=393938
str_decrement/5a5a5a=5a5a59
str_decrement/7a7a7a=7a7a79
str_decrement/5a7a39=5a7a38
binary=3130:38
str_increment/00=str_increment/00=str_increment(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters
str_decrement/00=str_decrement/00=str_decrement(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters
str_increment/80=str_increment/80=str_increment(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters
str_decrement/80=str_decrement/80=str_decrement(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters
immutable=Zz9:AAa0
"#,
    );
}

#[test]
fn string_steps_use_php_weak_coercion_and_polyfill_primitives() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
class AlphaText { public function __toString(): string { return 'Az'; } }
foreach (['str_increment', 'str_decrement'] as $function) {
    foreach ([null, false, true, 0, 1, new AlphaText(), []] as $value) {
        try {
            $result = $function($value);
            echo $function, '/', get_debug_type($value), '=', bin2hex($result), "\n";
        } catch (Throwable $error) {
            echo $function, '/', get_debug_type($value), '=', $error->getMessage(), "\n";
        }
    }
}

$value = 'Z';
@$value++;
echo "suppressed=$value\n";
$text = '5e6';
$text[1] = 'f';
$text[-1] = '7';
echo "offset=$text\n";
"#,
        ),
        r#"diag=8192:str_increment(): Passing null to parameter #1 ($string) of type string is deprecated
str_increment/null=str_increment(): Argument #1 ($string) must not be empty
str_increment/bool=str_increment(): Argument #1 ($string) must not be empty
str_increment/bool=32
str_increment/int=31
str_increment/int=32
str_increment/AlphaText=4261
str_increment/array=str_increment(): Argument #1 ($string) must be of type string, array given
diag=8192:str_decrement(): Passing null to parameter #1 ($string) of type string is deprecated
str_decrement/null=str_decrement(): Argument #1 ($string) must not be empty
str_decrement/bool=str_decrement(): Argument #1 ($string) must not be empty
str_decrement/bool=30
str_decrement/int=str_decrement(): Argument #1 ($string) "0" is out of decrement range
str_decrement/int=30
str_decrement/AlphaText=4179
str_decrement/array=str_decrement(): Argument #1 ($string) must be of type string, array given
diag=8192:Increment on non-numeric string is deprecated, use str_increment() instead
suppressed=AA
offset=5f7
"#,
    );
}

#[test]
fn string_steps_reject_non_strings_at_strict_call_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
foreach (['str_increment', 'str_decrement'] as $function) {
    foreach ([null, false, true, 0, 1, 1.5, [], new stdClass()] as $value) {
        try {
            $function($value);
        } catch (Throwable $error) {
            echo $function, '|', get_debug_type($value), '|', $error::class, '|', $error->getMessage(), "\n";
        }
    }
}
"#,
        ),
        r#"str_increment|null|TypeError|str_increment(): Argument #1 ($string) must be of type string, null given
str_increment|bool|TypeError|str_increment(): Argument #1 ($string) must be of type string, false given
str_increment|bool|TypeError|str_increment(): Argument #1 ($string) must be of type string, true given
str_increment|int|TypeError|str_increment(): Argument #1 ($string) must be of type string, int given
str_increment|int|TypeError|str_increment(): Argument #1 ($string) must be of type string, int given
str_increment|float|TypeError|str_increment(): Argument #1 ($string) must be of type string, float given
str_increment|array|TypeError|str_increment(): Argument #1 ($string) must be of type string, array given
str_increment|stdClass|TypeError|str_increment(): Argument #1 ($string) must be of type string, stdClass given
str_decrement|null|TypeError|str_decrement(): Argument #1 ($string) must be of type string, null given
str_decrement|bool|TypeError|str_decrement(): Argument #1 ($string) must be of type string, false given
str_decrement|bool|TypeError|str_decrement(): Argument #1 ($string) must be of type string, true given
str_decrement|int|TypeError|str_decrement(): Argument #1 ($string) must be of type string, int given
str_decrement|int|TypeError|str_decrement(): Argument #1 ($string) must be of type string, int given
str_decrement|float|TypeError|str_decrement(): Argument #1 ($string) must be of type string, float given
str_decrement|array|TypeError|str_decrement(): Argument #1 ($string) must be of type string, array given
str_decrement|stdClass|TypeError|str_decrement(): Argument #1 ($string) must be of type string, stdClass given
"#,
    );
}
