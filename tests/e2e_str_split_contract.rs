mod common;

use common::run_php;

#[test]
fn str_split_chunks_php_bytes_and_validates_length() {
    assert_eq!(
        run_php(
            r#"<?php
function show_split(string $label, string $source, int $length): void {
    try {
        $parts = str_split($source, $length);
        echo $label, '=', count($parts);
        foreach ($parts as $part) {
            echo ':', bin2hex($part);
        }
        echo "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}

show_split('empty', '', 1);
show_split('ascii', 'abcde', 2);
show_split('wide', 'abc', PHP_INT_MAX);
show_split('nul', "a\0bc", 2);
show_split('utf8-byte', "éZ", 1);
show_split('utf8-pair', "éZ", 2);
show_split('invalid', "\x80A\xff", 2);
show_split('zero', 'abc', 0);
show_split('negative', 'abc', -1);

$default = str_split('abc');
echo 'default=', count($default), ':', implode(',', $default), "\n";
$source = "\x80Z";
$copy = $source;
str_split($source, 1);
echo 'immutable=', bin2hex($source), ':', bin2hex($copy), "\n";

$function = new ReflectionFunction('str_split');
foreach ($function->getParameters() as $parameter) {
    echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':', $parameter->isOptional() ? 'optional' : 'required', "\n";
}
echo 'return=', $function->getReturnType(), "\n";
"#,
        ),
        r#"empty=0
ascii=3:6162:6364:65
wide=1:616263
nul=2:6100:6263
utf8-byte=3:c3:a9:5a
utf8-pair=2:c3a9:5a
invalid=2:8041:ff
zero=ValueError:str_split(): Argument #2 ($length) must be greater than 0
negative=ValueError:str_split(): Argument #2 ($length) must be greater than 0
default=3:a,b,c
immutable=805a:805a
param=string:string:required
param=length:int:optional
return=array
"#,
    );
}

#[test]
fn str_split_uses_php_weak_scalar_coercion() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
class SplitText { public function __toString(): string { return 'xy'; } }

function weak_split(string $label, mixed $source, mixed $length): void {
    try {
        $parts = str_split($source, $length);
        echo $label, '=', count($parts);
        foreach ($parts as $part) echo ':', bin2hex($part);
        echo "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}

weak_split('source-null', null, 1);
weak_split('source-false', false, 1);
weak_split('source-true', true, 1);
weak_split('source-int', 12, 1);
weak_split('source-float', 1.5, 1);
weak_split('source-object', new SplitText(), 1);
weak_split('source-array', [], 1);
weak_split('length-null', 'abcd', null);
weak_split('length-false', 'abcd', false);
weak_split('length-true', 'abcd', true);
weak_split('length-zero', 'abcd', 0);
weak_split('length-float', 'abcd', 2.5);
weak_split('length-string', 'abcd', '2');
weak_split('length-float-string', 'abcd', '2.5');
weak_split('length-nan', 'abcd', NAN);
weak_split('length-inf', 'abcd', INF);
weak_split('length-array', 'abcd', []);
"#,
        ),
        r#"diag=8192:str_split(): Passing null to parameter #1 ($string) of type string is deprecated
source-null=0
source-false=0
source-true=1:31
source-int=2:31:32
source-float=3:31:2e:35
source-object=2:78:79
source-array=TypeError:str_split(): Argument #1 ($string) must be of type string, array given
diag=8192:str_split(): Passing null to parameter #2 ($length) of type int is deprecated
length-null=ValueError:str_split(): Argument #2 ($length) must be greater than 0
length-false=ValueError:str_split(): Argument #2 ($length) must be greater than 0
length-true=4:61:62:63:64
length-zero=ValueError:str_split(): Argument #2 ($length) must be greater than 0
diag=8192:Implicit conversion from float 2.5 to int loses precision
length-float=2:6162:6364
length-string=2:6162:6364
diag=8192:Implicit conversion from float-string "2.5" to int loses precision
length-float-string=2:6162:6364
length-nan=TypeError:str_split(): Argument #2 ($length) must be of type int, float given
length-inf=TypeError:str_split(): Argument #2 ($length) must be of type int, float given
length-array=TypeError:str_split(): Argument #2 ($length) must be of type int, array given
"#,
    );
}

#[test]
fn str_split_rejects_non_strings_and_non_integers_in_strict_calls() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function strict_split(string $label, mixed $source, mixed $length): void {
    try {
        $parts = str_split($source, $length);
        echo $label, '=', count($parts), "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}

strict_split('valid', 'abcd', 2);
strict_split('source-null', null, 1);
strict_split('source-false', false, 1);
strict_split('source-true', true, 1);
strict_split('source-int', 1, 1);
strict_split('source-float', 1.5, 1);
strict_split('source-array', [], 1);
strict_split('source-object', new stdClass(), 1);
strict_split('length-null', 'abcd', null);
strict_split('length-false', 'abcd', false);
strict_split('length-true', 'abcd', true);
strict_split('length-float', 'abcd', 1.5);
strict_split('length-string', 'abcd', '2');
strict_split('length-array', 'abcd', []);
"#,
        ),
        r#"valid=2
source-null=TypeError:str_split(): Argument #1 ($string) must be of type string, null given
source-false=TypeError:str_split(): Argument #1 ($string) must be of type string, false given
source-true=TypeError:str_split(): Argument #1 ($string) must be of type string, true given
source-int=TypeError:str_split(): Argument #1 ($string) must be of type string, int given
source-float=TypeError:str_split(): Argument #1 ($string) must be of type string, float given
source-array=TypeError:str_split(): Argument #1 ($string) must be of type string, array given
source-object=TypeError:str_split(): Argument #1 ($string) must be of type string, stdClass given
length-null=TypeError:str_split(): Argument #2 ($length) must be of type int, null given
length-false=TypeError:str_split(): Argument #2 ($length) must be of type int, false given
length-true=TypeError:str_split(): Argument #2 ($length) must be of type int, true given
length-float=TypeError:str_split(): Argument #2 ($length) must be of type int, float given
length-string=TypeError:str_split(): Argument #2 ($length) must be of type int, string given
length-array=TypeError:str_split(): Argument #2 ($length) must be of type int, array given
"#,
    );
}
