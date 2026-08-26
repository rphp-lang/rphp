mod common;

use common::run_php;

#[test]
fn substr_null_and_signed_boundaries_slice_php_bytes() {
    assert_eq!(
        run_php(
            r#"<?php
$source = pack('C*', 0, 0x41, 0x80, 0xff, 0x42, 0x43);
foreach ([
    [0, null], [2, null], [-3, null], [99, null], [-99, null],
    [1, 0], [1, 2], [1, -1], [-4, -1], [-1, -99],
    [PHP_INT_MAX, null], [PHP_INT_MIN, null],
] as [$offset, $length]) {
    echo $offset, '/', $length === null ? 'null' : $length, '=';
    echo bin2hex(substr($source, $offset, $length)), "\n";
}
"#,
        ),
        concat!(
            "0/null=004180ff4243\n",
            "2/null=80ff4243\n",
            "-3/null=ff4243\n",
            "99/null=\n",
            "-99/null=004180ff4243\n",
            "1/0=\n",
            "1/2=4180\n",
            "1/-1=4180ff42\n",
            "-4/-1=80ff42\n",
            "-1/-99=\n",
            "9223372036854775807/null=\n",
            "-9223372036854775808/null=004180ff4243\n",
        ),
    );
}

#[test]
fn substr_generated_offset_length_matrix_matches_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
$digest = '';
foreach (range(-10, 10) as $offset) {
    foreach ([null, -12, -5, -1, 0, 1, 5, 12] as $length) {
        $digest .= bin2hex(substr('abcdef', $offset, $length)) . ';';
    }
}
echo md5($digest), "\n";
"#,
        ),
        "983681d73484636ac4cffc3e255ab69c\n",
    );
}

#[test]
fn substr_reflection_call_shapes_and_weak_conversion_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
echo new ReflectionFunction('substr');
$dynamic = 'substr';
echo 'dynamic=', $dynamic('abcdef', 2, null), "\n";
echo 'named=', substr(string: 'abcdef', offset: 2, length: null), "\n";
$first = substr(...);
echo 'first=', $first('abcdef', -3), "\n";
echo 'callback=', call_user_func('substr', 'abcdef', 1, null), "\n";
class SliceStringable {
    public function __toString(): string { echo "stringable\n"; return 'abcdef'; }
}
echo 'weak=', substr(12345, '1', null), "\n";
echo 'object=', substr(new SliceStringable(), 1, null), "\n";
try { substr('abc', new SliceStringable(), null); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "Function [ <internal:standard> function substr ] {\n\n",
            "  - Parameters [3] {\n",
            "    Parameter #0 [ <required> string $string ]\n",
            "    Parameter #1 [ <required> int $offset ]\n",
            "    Parameter #2 [ <optional> ?int $length = null ]\n",
            "  }\n",
            "  - Return [ string ]\n",
            "}\n",
            "dynamic=cdef\n",
            "named=cdef\n",
            "first=def\n",
            "callback=bcdef\n",
            "weak=2345\n",
            "object=stringable\n",
            "bcdef\n",
            "substr(): Argument #2 ($offset) must be of type int, SliceStringable given\n",
        ),
    );
}

#[test]
fn substr_strict_types_and_cow_leave_inputs_unchanged() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
class StrictSliceStringable { public function __toString(): string { return 'abc'; } }
try { substr(123, 0, null); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
try { substr('abc', '0', null); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
try { substr('abc', 0, '1'); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
try { substr(new StrictSliceStringable(), 0, null); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
$source = pack('C*', 0, 0x80, 0xff, 0x41);
$alias =& $source;
$copy = $source;
echo bin2hex(substr($alias, 1, null)), '/';
echo $source === $copy ? 'cow/' : 'changed/';
echo $alias === $source ? "ref\n" : "split\n";
"#,
        ),
        concat!(
            "substr(): Argument #1 ($string) must be of type string, int given\n",
            "substr(): Argument #2 ($offset) must be of type int, string given\n",
            "substr(): Argument #3 ($length) must be of type ?int, string given\n",
            "substr(): Argument #1 ($string) must be of type string, StrictSliceStringable given\n",
            "80ff41/cow/ref\n",
        ),
    );
}
