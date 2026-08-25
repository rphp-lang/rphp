mod common;

use common::{run_php, run_php_expect_error};
use rphp::vm::execute::VmError;

#[test]
fn split_and_join_use_php_bytes_without_mutating_inputs() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
function show(string $label, mixed $value): void {
    echo $label, '=';
    if (is_string($value)) { echo 's:', bin2hex($value), "\n"; return; }
    echo 'a:', count($value), ':';
    foreach ($value as $part) echo bin2hex($part), ',';
    echo "\n";
}
class Piece {
    public int $calls = 0;
    public function __toString(): string { $this->calls++; echo "piece-call={$this->calls}\n"; return "O\0"; }
}
$source = "A\0" . chr(128) . "é--B--" . chr(255);
$alias =& $source;
show('split-all', explode('--', $source));
show('split-zero', explode('--', $source, 0));
show('split-two', explode('--', $source, 2));
show('split-negative', explode('--', $source, -1));
show('split-min', explode('--', $source, PHP_INT_MIN));
show('split-high', explode(chr(128), $source));
show('source', $source);
show('alias', $alias);

$piece = new Piece();
$nested = [1];
$pieces = [10 => "A\0", 2 => chr(128), null, false, true, 7, 1.25, $nested, $piece, $piece];
$copy = $pieces;
show('joined', implode(chr(255), $pieces));
show('joined-empty', join($pieces));
echo 'calls=', $piece->calls, "\n";
echo 'piece-count=', count($pieces), ':copy-count=', count($copy), "\n";
restore_error_handler();
"#,
        ),
        r#"split-all=a:3:410080c3a9,42,ff,
split-zero=a:1:410080c3a92d2d422d2dff,
split-two=a:2:410080c3a9,422d2dff,
split-negative=a:2:410080c3a9,42,
split-min=a:0:
split-high=a:2:4100,c3a92d2d422d2dff,
source=s:410080c3a92d2d422d2dff
alias=s:410080c3a92d2d422d2dff
diag=2:Array to string conversion
piece-call=1
piece-call=2
joined=s:4100ff80ffffff31ff37ff312e3235ff4172726179ff4f00ff4f00
diag=2:Array to string conversion
piece-call=3
piece-call=4
joined-empty=s:4100803137312e323541727261794f004f00
calls=4
piece-count=10:copy-count=10
"#,
    );
}

#[test]
fn split_join_weak_and_strict_boundaries_match_php() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
function show(mixed $value): void {
    if (is_string($value)) echo 's:', bin2hex($value), "\n";
    elseif (is_array($value)) {
        echo 'a:', count($value), ':';
        foreach ($value as $part) echo bin2hex($part), ',';
        echo "\n";
    } else var_dump($value);
}
function attempt(string $label, callable $call): void {
    echo '[', $label, "]\n";
    try { show($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
class TextValue { public function __toString(): string { echo "convert\n"; return ':'; } }
attempt('explode-null-separator', static fn () => explode(null, 'abc'));
attempt('explode-null-string', static fn () => explode('a', null));
attempt('explode-float-limit', static fn () => explode(':', 'a:b:c', 2.9));
attempt('explode-stringable', static fn () => explode(new TextValue(), 'a:b'));
attempt('explode-array', static fn () => explode([], 'a'));
attempt('implode-null-glue', static fn () => implode(null, ['a', 'b']));
attempt('join-numeric-glue', static fn () => join(2.5, ['a', 'b']));
attempt('join-resource-glue', static function () { $r = fopen('php://memory', 'r'); return join($r, ['a', 'b']); });
attempt('implode-plain-object', static fn () => implode(',', [new stdClass()]));
attempt('set-limit-null', static fn () => set_time_limit(null));
restore_error_handler();
"#,
        ),
        r#"[explode-null-separator]
diag=8192:explode(): Passing null to parameter #1 ($separator) of type string is deprecated
ValueError:explode(): Argument #1 ($separator) must not be empty
[explode-null-string]
diag=8192:explode(): Passing null to parameter #2 ($string) of type string is deprecated
a:1:,
[explode-float-limit]
diag=8192:Implicit conversion from float 2.9 to int loses precision
a:2:61,623a63,
[explode-stringable]
convert
a:2:61,62,
[explode-array]
TypeError:explode(): Argument #1 ($separator) must be of type string, array given
[implode-null-glue]
diag=8192:implode(): Passing null to parameter #1 ($separator) of type array|string is deprecated
s:6162
[join-numeric-glue]
s:61322e3562
[join-resource-glue]
TypeError:join(): Argument #1 ($separator) must be of type array|string, resource given
[implode-plain-object]
Error:Object of class stdClass could not be converted to string
[set-limit-null]
diag=8192:set_time_limit(): Passing null to parameter #1 ($seconds) of type int is deprecated
bool(true)
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function attempt(string $label, callable $call): void {
    echo '[', $label, "]\n";
    try { var_dump($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
class StrictText { public function __toString(): string { echo "unexpected\n"; return ':'; } }
attempt('explode-separator', static fn () => explode(1, 'a1b'));
attempt('explode-string', static fn () => explode('1', true));
attempt('explode-limit', static fn () => explode(':', 'a:b', '2'));
attempt('explode-object', static fn () => explode(new StrictText(), 'a:b'));
attempt('implode-glue', static fn () => implode(1, ['a', 'b']));
attempt('implode-null', static fn () => implode(null, ['a', 'b']));
attempt('join-array', static fn () => join(',', 'ab'));
attempt('set-limit', static fn () => set_time_limit('0'));
"#,
        ),
        r#"[explode-separator]
TypeError:explode(): Argument #1 ($separator) must be of type string, int given
[explode-string]
TypeError:explode(): Argument #2 ($string) must be of type string, true given
[explode-limit]
TypeError:explode(): Argument #3 ($limit) must be of type int, string given
[explode-object]
TypeError:explode(): Argument #1 ($separator) must be of type string, StrictText given
[implode-glue]
TypeError:implode(): Argument #1 ($separator) must be of type string, int given
[implode-null]
TypeError:implode(): Argument #1 ($separator) must be of type string, null given
[join-array]
TypeError:join(): Argument #2 ($array) must be of type ?array, string given
[set-limit]
TypeError:set_time_limit(): Argument #1 ($seconds) must be of type int, string given
"#,
    );
}

#[test]
fn split_join_call_shapes_reflection_and_execution_timer_share_the_contract() {
    assert_eq!(
        run_php(
            r#"<?php
function show(string $label, mixed $value): void {
    echo $label, '=';
    if (is_string($value)) echo 's:', bin2hex($value), "\n";
    else { echo 'a:'; foreach ($value as $part) echo bin2hex($part), ','; echo "\n"; }
}
function attempt(string $label, callable $call): void {
    echo '[', $label, "]\n";
    try { var_dump($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
$dynamic = 'explode';
show('dynamic', $dynamic(separator: ':', string: 'a:b:c', limit: -1));
show('named', implode(array: ['a', 'b'], separator: ':'));
show('callback', call_user_func('join', '|', ['a', 'b']));
show('callback-array', call_user_func_array('explode', ['string' => 'a:b', 'separator' => ':']));
attempt('explode-too-many', static fn () => explode(':', 'a:b', 2, 3));
attempt('join-too-few', static fn () => join());
attempt('unknown-named', static fn () => implode(array: ['a'], extra: ':'));
foreach (['explode', 'implode', 'join', 'set_time_limit'] as $name) {
    $reflection = new ReflectionFunction($name);
    echo 'reflection=', $name, ':', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':', $reflection->getReturnType(), "\n";
    foreach ($reflection->getParameters() as $parameter) {
        echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':',
            $parameter->isOptional() ? 'optional' : 'required', ':',
            $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
    }
}
var_dump(set_time_limit(0));
"#,
        ),
        r#"dynamic=a:61,62,
named=s:613a62
callback=s:617c62
callback-array=a:61,62,
[explode-too-many]
ArgumentCountError:explode() expects at most 3 arguments, 4 given
[join-too-few]
ArgumentCountError:join() expects at least 1 argument, 0 given
[unknown-named]
Error:Unknown named parameter $extra
reflection=explode:2/3:array
param=separator:string:required:nonnull
param=string:string:required:nonnull
param=limit:int:optional:nonnull
reflection=implode:1/2:string
param=separator:array|string:required:nonnull
param=array:?array:optional:nullable
reflection=join:1/2:string
param=separator:array|string:required:nonnull
param=array:?array:optional:nullable
reflection=set_time_limit:1/1:bool
param=seconds:int:required:nonnull
bool(true)
"#,
    );

    let started = std::time::Instant::now();
    let error = run_php_expect_error("<?php set_time_limit(1); while (true) {} ");
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    assert!(matches!(
        error,
        VmError::Fatal(message) if message == "Maximum execution time exceeded"
    ));
    assert_eq!(
        run_php(
            "<?php set_time_limit(1); set_time_limit(0); for ($i = 0; $i < 1000; $i++) {} echo 'done';"
        ),
        "done"
    );
}
