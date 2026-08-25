mod common;

use common::run_php;

#[test]
fn path_decomposition_preserves_php_bytes_components_and_cow() {
    assert_eq!(
        run_php(
            r#"<?php
function bytes(string $label, string $value): void {
    echo $label, '=', strlen($value), ':', bin2hex($value), "\n";
}
function info(string $label, array|string $value): void {
    echo $label, '=';
    if (is_string($value)) {
        echo 's:', strlen($value), ':', bin2hex($value), "\n";
        return;
    }
    echo 'a:', count($value);
    foreach ($value as $key => $item) {
        echo ':', $key, '=', strlen($item), ',', bin2hex($item);
    }
    echo "\n";
}

$paths = [
    'empty' => '',
    'root' => '////',
    'relative' => 'a//b///c/',
    'absolute' => '///a//b///c/',
    'dots' => '.././leaf.',
    'binary' => "A/\0/B.\xff",
    'utf8' => "řeka/žluťoučký.txt",
];
foreach ($paths as $label => $path) {
    bytes("base/$label", basename($path));
    bytes("dir1/$label", dirname($path));
    bytes("dir2/$label", dirname($path, 2));
    bytes("dirmax/$label", dirname($path, PHP_INT_MAX));
    info("info/$label", pathinfo($path));
}

foreach ([
    ['short', '/a/archive.tar.gz///', '.gz'],
    ['equal', 'leaf', 'leaf'],
    ['missing', 'leaf.ext', '.zip'],
    ['binary', "A/na\0me.\xff", ".\xff"],
] as [$label, $path, $suffix]) {
    bytes("suffix/$label", basename($path, $suffix));
}

foreach ([0, 3, 5, 9, 12, 15, -1] as $flags) {
    info("flags/$flags", pathinfo('leaf', $flags));
}
$nulDir = "A/\0/B.ext";
bytes('nul/dirname', dirname($nulDir));
info('nul/pathinfo-dirname', pathinfo($nulDir, PATHINFO_DIRNAME));
info('nul/pathinfo-all', pathinfo($nulDir));

$source = "/a/na\0me.\xff";
$alias =& $source;
$copy = $source;
$base = basename($source);
$all = pathinfo($source);
$base[0] = 'X';
$all['basename'][0] = 'Y';
echo 'cow=', bin2hex($source), ':', bin2hex($alias), ':', bin2hex($copy), ':', bin2hex($base), ':', bin2hex($all['basename']), "\n";
"#,
        ),
        r#"base/empty=0:
dir1/empty=0:
dir2/empty=0:
dirmax/empty=0:
info/empty=a:2:basename=0,:filename=0,
base/root=0:
dir1/root=1:2f
dir2/root=1:2f
dirmax/root=1:2f
info/root=a:3:dirname=1,2f:basename=0,:filename=0,
base/relative=1:63
dir1/relative=4:612f2f62
dir2/relative=1:61
dirmax/relative=1:2e
info/relative=a:3:dirname=4,612f2f62:basename=1,63:filename=1,63
base/absolute=1:63
dir1/absolute=7:2f2f2f612f2f62
dir2/absolute=4:2f2f2f61
dirmax/absolute=1:2f
info/absolute=a:3:dirname=7,2f2f2f612f2f62:basename=1,63:filename=1,63
base/dots=5:6c6561662e
dir1/dots=4:2e2e2f2e
dir2/dots=2:2e2e
dirmax/dots=1:2e
info/dots=a:4:dirname=4,2e2e2f2e:basename=5,6c6561662e:extension=0,:filename=4,6c656166
base/binary=3:422eff
dir1/binary=3:412f00
dir2/binary=1:41
dirmax/binary=1:2e
info/binary=a:4:dirname=2,412f:basename=3,422eff:extension=1,ff:filename=1,42
base/utf8=17:c5be6c75c5a56f75c48d6bc3bd2e747874
dir1/utf8=5:c599656b61
dir2/utf8=1:2e
dirmax/utf8=1:2e
info/utf8=a:4:dirname=5,c599656b61:basename=17,c5be6c75c5a56f75c48d6bc3bd2e747874:extension=3,747874:filename=13,c5be6c75c5a56f75c48d6bc3bd
suffix/short=11:617263686976652e746172
suffix/equal=4:6c656166
suffix/missing=8:6c6561662e657874
suffix/binary=5:6e61006d65
flags/0=s:0:
flags/3=s:1:2e
flags/5=s:1:2e
flags/9=s:1:2e
flags/12=s:4:6c656166
flags/15=a:3:dirname=1,2e:basename=4,6c656166:filename=4,6c656166
flags/-1=s:1:2e
nul/dirname=3:412f00
nul/pathinfo-dirname=s:2:412f
nul/pathinfo-all=a:4:dirname=2,412f:basename=5,422e657874:extension=3,657874:filename=1,42
cow=2f612f6e61006d652eff:2f612f6e61006d652eff:2f612f6e61006d652eff:5861006d652eff:5961006d652eff
"#,
    );
}

#[test]
fn path_decomposition_owns_weak_strict_and_diagnostic_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
function show(mixed $value): string {
    if (is_array($value)) return 'array:' . count($value);
    return 'string:' . strlen($value) . ':' . bin2hex($value);
}
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try { echo show($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
final class PathText {
    public function __construct(private string $value) {}
    public function __toString(): string { return $this->value; }
}
attempt('basename/int', static fn () => basename(12.5));
attempt('basename/null', static fn () => basename(null));
attempt('basename/object', static fn () => basename(new PathText('/a/b.ext')));
attempt('basename/array', static fn () => basename([]));
attempt('basename/suffix-null', static fn () => basename('/a/b.ext', null));
attempt('basename/suffix-array', static fn () => basename('/a/b.ext', []));
attempt('dirname/string-level', static fn () => dirname('/a/b/c', '2'));
attempt('dirname/float-level', static fn () => dirname('/a/b/c', 2.5));
attempt('dirname/null-level', static fn () => dirname('/a/b/c', null));
attempt('dirname/zero', static fn () => dirname('/a/b/c', 0));
attempt('dirname/object-level', static fn () => dirname('/a/b/c', new PathText('2')));
attempt('pathinfo/string-flags', static fn () => pathinfo('/a/b.ext', '4'));
attempt('pathinfo/float-flags', static fn () => pathinfo('/a/b.ext', 4.5));
attempt('pathinfo/false-flags', static fn () => pathinfo('/a/b.ext', false));
attempt('pathinfo/null-flags', static fn () => pathinfo('/a/b.ext', null));
attempt('pathinfo/object-flags', static fn () => pathinfo('/a/b.ext', new PathText('4')));
attempt('pathinfo/array-flags', static fn () => pathinfo('/a/b.ext', []));
restore_error_handler();
"#,
        ),
        r#"basename/int=string:4:31322e35
basename/null=diag=8192:basename(): Passing null to parameter #1 ($path) of type string is deprecated
string:0:
basename/object=string:5:622e657874
basename/array=TypeError:basename(): Argument #1 ($path) must be of type string, array given
basename/suffix-null=diag=8192:basename(): Passing null to parameter #2 ($suffix) of type string is deprecated
string:5:622e657874
basename/suffix-array=TypeError:basename(): Argument #2 ($suffix) must be of type string, array given
dirname/string-level=string:2:2f61
dirname/float-level=diag=8192:Implicit conversion from float 2.5 to int loses precision
string:2:2f61
dirname/null-level=diag=8192:dirname(): Passing null to parameter #2 ($levels) of type int is deprecated
ValueError:dirname(): Argument #2 ($levels) must be greater than or equal to 1
dirname/zero=ValueError:dirname(): Argument #2 ($levels) must be greater than or equal to 1
dirname/object-level=TypeError:dirname(): Argument #2 ($levels) must be of type int, PathText given
pathinfo/string-flags=string:3:657874
pathinfo/float-flags=diag=8192:Implicit conversion from float 4.5 to int loses precision
string:3:657874
pathinfo/false-flags=string:0:
pathinfo/null-flags=diag=8192:pathinfo(): Passing null to parameter #2 ($flags) of type int is deprecated
string:0:
pathinfo/object-flags=TypeError:pathinfo(): Argument #2 ($flags) must be of type int, PathText given
pathinfo/array-flags=TypeError:pathinfo(): Argument #2 ($flags) must be of type int, array given
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function render(mixed $value): string {
    if (is_array($value)) return 'array:' . count($value);
    return 'string:' . strlen($value) . ':' . bin2hex($value);
}
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try { echo render($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
final class PathText { public function __toString(): string { return '/a/b.ext'; } }
attempt('basename/string', static fn () => basename('/a/b.ext', '.ext'));
attempt('basename/path-int', static fn () => basename(123));
attempt('basename/suffix-int', static fn () => basename('/a/b.ext', 123));
attempt('basename/stringable', static fn () => basename(new PathText()));
attempt('dirname/string', static fn () => dirname('/a/b/c', 2));
attempt('dirname/path-int', static fn () => dirname(123));
attempt('dirname/levels-string', static fn () => dirname('/a/b/c', '2'));
attempt('dirname/stringable', static fn () => dirname(new PathText()));
attempt('pathinfo/string', static fn () => pathinfo('/a/b.ext', PATHINFO_EXTENSION));
attempt('pathinfo/path-int', static fn () => pathinfo(123));
attempt('pathinfo/flags-string', static fn () => pathinfo('/a/b.ext', '4'));
attempt('pathinfo/stringable', static fn () => pathinfo(new PathText()));
"#,
        ),
        r#"basename/string=string:1:62
basename/path-int=TypeError:basename(): Argument #1 ($path) must be of type string, int given
basename/suffix-int=TypeError:basename(): Argument #2 ($suffix) must be of type string, int given
basename/stringable=TypeError:basename(): Argument #1 ($path) must be of type string, PathText given
dirname/string=string:2:2f61
dirname/path-int=TypeError:dirname(): Argument #1 ($path) must be of type string, int given
dirname/levels-string=TypeError:dirname(): Argument #2 ($levels) must be of type int, string given
dirname/stringable=TypeError:dirname(): Argument #1 ($path) must be of type string, PathText given
pathinfo/string=string:3:657874
pathinfo/path-int=TypeError:pathinfo(): Argument #1 ($path) must be of type string, int given
pathinfo/flags-string=TypeError:pathinfo(): Argument #2 ($flags) must be of type int, string given
pathinfo/stringable=TypeError:pathinfo(): Argument #1 ($path) must be of type string, PathText given
"#,
    );
}

#[test]
fn path_decomposition_call_shapes_reflection_and_side_effect_order_match() {
    assert_eq!(
        run_php(
            r#"<?php
function show(mixed $value): string {
    if (is_array($value)) {
        $parts = [];
        foreach ($value as $key => $item) $parts[] = "$key:" . bin2hex($item);
        return 'array[' . implode(',', $parts) . ']';
    }
    return 'string:' . bin2hex($value);
}
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try { echo show($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
foreach (['basename', 'dirname', 'pathinfo'] as $name) {
    $callback = $name(...);
    $arg = $name === 'basename' ? '.ext' : ($name === 'dirname' ? 2 : PATHINFO_FILENAME);
    $named = match ($name) {
        'basename' => static fn () => basename(path: '/a/b.ext', suffix: '.ext'),
        'dirname' => static fn () => dirname(path: '/a/b.ext', levels: 2),
        'pathinfo' => static fn () => pathinfo(path: '/a/b.ext', flags: PATHINFO_FILENAME),
    };
    attempt("$name/named", $named);
    attempt("$name/dynamic", static fn () => ($GLOBALS['name'])('/a/b.ext', $GLOBALS['arg']));
    attempt("$name/callback", static fn () => ($GLOBALS['callback'])('/a/b.ext', $GLOBALS['arg']));
    attempt("$name/call-user", static fn () => call_user_func($GLOBALS['name'], '/a/b.ext', $GLOBALS['arg']));
    $second = $name === 'basename' ? 'suffix' : ($name === 'dirname' ? 'levels' : 'flags');
    attempt("$name/call-array", static fn () => call_user_func_array($GLOBALS['name'], ['path' => '/a/b.ext', $GLOBALS['second'] => $GLOBALS['arg']]));
    attempt("$name/missing", static fn () => $GLOBALS['name']());
    attempt("$name/too-many", static fn () => $GLOBALS['name']('/a/b.ext', $GLOBALS['arg'], 3));
    attempt("$name/unknown", static fn () => $GLOBALS['name'](path: '/a/b.ext', extra: 3));
    $reflection = new ReflectionFunction($name);
    echo "reflection/$name=", $reflection->getNumberOfRequiredParameters(), '/', $reflection->getNumberOfParameters(), ':', $reflection->getReturnType(), "\n";
    foreach ($reflection->getParameters() as $parameter) {
        echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':',
            $parameter->isOptional() ? 'optional' : 'required', ':',
            $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
    }
}
final class TracePath {
    public function __construct(private string $label, private string $value) {}
    public function __toString(): string { echo "convert/{$this->label}\n"; return $this->value; }
}
function evaluated(string $label, mixed $value): mixed { echo "evaluate/$label\n"; return $value; }
attempt('order/basename', static fn () => basename(
    evaluated('path', new TracePath('path', '/a/b.ext')),
    evaluated('suffix', new TracePath('suffix', '.ext')),
));
attempt('order/pathinfo-error', static fn () => pathinfo(
    evaluated('path', new TracePath('path', '/a/b.ext')),
    evaluated('flags', []),
));
"#,
        ),
        r#"basename/named=string:62
basename/dynamic=string:62
basename/callback=string:62
basename/call-user=string:62
basename/call-array=string:62
basename/missing=ArgumentCountError:basename() expects at least 1 argument, 0 given
basename/too-many=ArgumentCountError:basename() expects at most 2 arguments, 3 given
basename/unknown=Error:Unknown named parameter $extra
reflection/basename=1/2:string
param=path:string:required:nonnull
param=suffix:string:optional:nonnull
dirname/named=string:2f
dirname/dynamic=string:2f
dirname/callback=string:2f
dirname/call-user=string:2f
dirname/call-array=string:2f
dirname/missing=ArgumentCountError:dirname() expects at least 1 argument, 0 given
dirname/too-many=ArgumentCountError:dirname() expects at most 2 arguments, 3 given
dirname/unknown=Error:Unknown named parameter $extra
reflection/dirname=1/2:string
param=path:string:required:nonnull
param=levels:int:optional:nonnull
pathinfo/named=string:62
pathinfo/dynamic=string:62
pathinfo/callback=string:62
pathinfo/call-user=string:62
pathinfo/call-array=string:62
pathinfo/missing=ArgumentCountError:pathinfo() expects at least 1 argument, 0 given
pathinfo/too-many=ArgumentCountError:pathinfo() expects at most 2 arguments, 3 given
pathinfo/unknown=Error:Unknown named parameter $extra
reflection/pathinfo=1/2:array|string
param=path:string:required:nonnull
param=flags:int:optional:nonnull
order/basename=evaluate/path
evaluate/suffix
convert/path
convert/suffix
string:62
order/pathinfo-error=evaluate/path
evaluate/flags
convert/path
TypeError:pathinfo(): Argument #2 ($flags) must be of type int, array given
"#,
    );
}
