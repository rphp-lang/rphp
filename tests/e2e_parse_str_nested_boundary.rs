mod common;

use common::run_php;

#[test]
fn parse_str_normalizes_numeric_malformed_binary_and_collision_keys() {
    assert_eq!(
        run_php(
            r#"<?php
function emit_shape(mixed $value, string $prefix = ''): void {
    if (is_array($value)) {
        echo $prefix, 'A', count($value), "\n";
        foreach ($value as $key => $child) {
            echo $prefix, is_int($key) ? "I$key\n" : 'S' . bin2hex($key) . "\n";
            emit_shape($child, $prefix . '.');
        }
    } else {
        echo $prefix, 'V', bin2hex($value), "\n";
    }
}

foreach ([
    'numeric' => '7=seven&07=leading&-2=minus&+7=space&0=zero&-0=negative-zero',
    'nested' => 'root[2]=two&root[02]=leading&root[-3]=minus&root[]=append&root[5]tail=tail&root[ok][bad=malformed',
    'broken' => 'first[=one&second[x][y=two&third[x]tail[z]=three&four[x]]]=four',
    'collision' => 'a=scalar&a[x]=nested&a[]=append&a=reset&a[y][z]=deep&a[y]=last',
] as $name => $query) {
    echo "CASE:$name\n";
    parse_str($query, $output);
    emit_shape($output);
}

$binary = "\xFF=%80&n[%FF]=%FE&nul%00tail=a%00b";
echo "CASE:binary\n";
parse_str($binary, $output);
emit_shape($output);

$input = 'item[4]=x&item[]=y';
$inputAlias =& $input;
$inputCopy = $input;
$output = ['old' => true];
$outputAlias =& $output;
$outputCopy = $output;
parse_str($inputAlias, $outputAlias);
echo $input === $inputCopy ? 'input-cow:' : 'input-mutated:';
echo $input === $inputAlias ? 'input-ref:' : 'input-split:';
echo $output === $outputAlias ? 'output-ref:' : 'output-split:';
echo isset($outputCopy['old']) ? "copy-old\n" : "copy-mutated\n";
emit_shape($output);
"#,
        ),
        r#"CASE:numeric
A5
I7
.V7370616365
S3037
.V6c656164696e67
I-2
.V6d696e7573
I0
.V7a65726f
S2d30
.V6e656761746976652d7a65726f
CASE:nested
A1
S726f6f74
.A6
.I2
..V74776f
.S3032
..V6c656164696e67
.I-3
..V6d696e7573
.I3
..V617070656e64
.I5
..V7461696c
.S6f6b
..V6d616c666f726d6564
CASE:broken
A4
S66697273745f
.V6f6e65
S7365636f6e64
.A1
.S78
..V74776f
S7468697264
.A1
.S78
..V7468726565
S666f7572
.A1
.S78
..V666f7572
CASE:collision
A1
S61
.A1
.S79
..V6c617374
CASE:binary
A3
Sff
.V80
S6e
.A1
.Sff
..Vfe
S6e756c
.V610062
input-cow:input-ref:output-ref:copy-old
A1
S6974656d
.A2
.I4
..V78
.I5
..V79
"#,
    );
}

#[test]
fn parse_str_generated_numeric_malformed_and_high_byte_sweeps_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
$parts = [];
for ($index = 0; $index <= 64; ++$index) {
    $key = match ($index % 4) {
        0 => (string) $index,
        1 => '0' . $index,
        2 => (string) -$index,
        default => '+' . $index,
    };
    $parts[] = 'n[' . $key . ']=v' . $index;
}
$parts[] = 'n[]=tail';
parse_str(implode('&', $parts), $numeric);
$intKeys = 0;
$stringKeys = 0;
$digest = '';
foreach ($numeric['n'] as $key => $value) {
    is_int($key) ? ++$intKeys : ++$stringKeys;
    $digest .= (is_int($key) ? 'i' : 's') . $key . '=' . $value . ';';
}
echo 'numeric=', count($numeric['n']), '/', $intKeys, '/', $stringKeys, '/', md5($digest), "\n";

$parts = [];
for ($index = 0; $index < 33; ++$index) {
    $parts[] = 'broken' . $index . '[key' . $index . '=x' . $index;
    $parts[] = 'valid' . $index . '[key' . $index . '][tail' . $index . '=y' . $index;
}
parse_str(implode('&', $parts), $malformed);
$digest = '';
foreach ($malformed as $key => $value) {
    $digest .= $key . '=';
    $digest .= is_array($value) ? json_encode($value) : $value;
    $digest .= ';';
}
echo 'malformed=', count($malformed), '/', md5($digest), "\n";

$parts = [];
for ($byte = 0x80; $byte <= 0xFF; ++$byte) {
    $hex = strtoupper(str_pad(dechex($byte), 2, '0', STR_PAD_LEFT));
    $parts[] = 'b[%'. $hex . ']=%' . $hex;
}
parse_str(implode('&', $parts), $binary);
$digest = '';
foreach ($binary['b'] as $key => $value) {
    $digest .= bin2hex($key) . '=' . bin2hex($value) . ';';
}
echo 'binary=', count($binary['b']), '/', md5($digest), "\n";
"#,
        ),
        concat!(
            "numeric=66/34/32/79906de792d4a4f40820ede2eb301961\n",
            "malformed=66/e14ae5bcc11b6f23ded0011f887af4c6\n",
            "binary=128/d72e340c9c24d189ee91ce83af7faca7\n",
        ),
    );
}

#[test]
fn parse_str_call_shapes_reflection_and_conversion_side_effects_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
$reflection = new ReflectionFunction('parse_str');
echo (string) $reflection;
echo 'return=', (string) $reflection->getReturnType(), "\n";
foreach ($reflection->getParameters() as $parameter) {
    echo $parameter->getName(), ':', (string) $parameter->getType(), ':';
    echo $parameter->isPassedByReference() ? 'ref' : 'value', "\n";
}

$dynamic = 'parse_str';
$dynamic('a=1', $out);
echo 'dynamic=', json_encode($out), "\n";
$dynamic(string: 'b=2', result: $out);
echo 'named=', json_encode($out), "\n";
$first = parse_str(...);
$first('c=3', $out);
echo 'first=', json_encode($out), "\n";
$args = ['d=4', &$out];
call_user_func_array('parse_str', $args);
echo 'callback=', json_encode($out), "\n";

class QueryStringable {
    public function __construct(private string $text) {}
    public function __toString(): string {
        echo 'stringable:', $this->text, "\n";
        return $this->text;
    }
}
parse_str(123, $out);
echo 'weak-int=', json_encode($out), "\n";
parse_str(new QueryStringable('e=5'), $out);
echo 'weak-object=', json_encode($out), "\n";
try { parse_str('x=1', []); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
$out = ['before' => true];
try {
    parse_str(new class {
        public function __toString(): string { throw new Exception('string-stop'); }
    }, $out);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
echo 'throw-result=', json_encode($out), "\n";
"#,
        ),
        concat!(
            "Function [ <internal:standard> function parse_str ] {\n\n",
            "  - Parameters [2] {\n",
            "    Parameter #0 [ <required> string $string ]\n",
            "    Parameter #1 [ <required> &$result ]\n",
            "  }\n",
            "  - Return [ void ]\n",
            "}\n",
            "return=void\n",
            "string:string:value\n",
            "result::ref\n",
            "dynamic={\"a\":\"1\"}\n",
            "named={\"b\":\"2\"}\n",
            "first={\"c\":\"3\"}\n",
            "callback={\"d\":\"4\"}\n",
            "weak-int={\"123\":\"\"}\n",
            "stringable:e=5\n",
            "weak-object={\"e\":\"5\"}\n",
            "Error:parse_str(): Argument #2 ($result) could not be passed by reference\n",
            "Exception:string-stop\n",
            "throw-result={\"before\":true}\n",
        ),
    );
}

#[test]
fn parse_str_strict_string_boundary_rejects_scalars_and_stringable_objects() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
class StrictQueryStringable {
    public function __toString(): string { return 'x=1'; }
}
foreach ([123, new StrictQueryStringable()] as $value) {
    try {
        $out = ['old' => true];
        parse_str($value, $out);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), '/', json_encode($out), "\n";
    }
}
$out = null;
parse_str('x=1', $out);
echo json_encode($out), "\n";
"#,
        ),
        concat!(
            "TypeError:parse_str(): Argument #1 ($string) must be of type string, int given/{\"old\":true}\n",
            "TypeError:parse_str(): Argument #1 ($string) must be of type string, StrictQueryStringable given/{\"old\":true}\n",
            "{\"x\":\"1\"}\n",
        ),
    );
}
