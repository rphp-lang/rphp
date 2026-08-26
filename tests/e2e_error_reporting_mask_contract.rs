mod common;

use common::run_php;

#[test]
fn php_85_error_masks_e_strict_reads_reflection_and_call_shapes_match() {
    assert_eq!(
        run_php(
            r#"<?php
echo 'mask=', E_ALL, ':', error_reporting(), "\n";
foreach (['set_error_handler', 'error_reporting'] as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, ':', $reflection->getReturnType(), ':';
    foreach ($reflection->getParameters() as $index => $parameter) {
        if ($index) echo ',';
        echo $parameter->getName(), '=', $parameter->getType(), '=';
        echo $parameter->isDefaultValueAvailable()
            ? var_export($parameter->getDefaultValue(), true)
            : '-';
    }
    echo "\n";
}
set_error_handler(static function (int $level, string $message): bool {
    echo 'diag=', $level, ':', $message, ':mask=', error_reporting(), "\n";
    return true;
});
echo 'direct=', E_STRICT, "\n";
echo 'fq=', \E_STRICT, "\n";
echo 'dynamic=', constant('E_STRICT'), "\n";
echo 'namespace=', eval('namespace Probe; return E_STRICT;'), "\n";
echo 'quiet=', defined('E_STRICT') ? 1 : 0, ':', get_defined_constants()['E_STRICT'], "\n";
echo 'suppressed=', @E_STRICT, "\n";
restore_error_handler();
$dynamic = 'error_reporting';
echo 'calls=', error_reporting(), ':', $dynamic(), ':',
    call_user_func('error_reporting'), "\n";
$fiber = new Fiber(static fn () => error_reporting());
var_dump($fiber->start());
"#,
        ),
        concat!(
            "mask=30719:30719\n",
            "set_error_handler::callback=?callable=-,error_levels=int=30719\n",
            "error_reporting:int:error_level=?int=NULL\n",
            "direct=diag=8192:Constant E_STRICT is deprecated since 8.4, the error level was removed:mask=30719\n",
            "2048\n",
            "fq=diag=8192:Constant E_STRICT is deprecated since 8.4, the error level was removed:mask=30719\n",
            "2048\n",
            "dynamic=diag=8192:Constant E_STRICT is deprecated since 8.4, the error level was removed:mask=30719\n",
            "2048\n",
            "namespace=diag=8192:Constant E_STRICT is deprecated since 8.4, the error level was removed:mask=30719\n",
            "2048\n",
            "quiet=1:2048\n",
            "suppressed=diag=8192:Constant E_STRICT is deprecated since 8.4, the error level was removed:mask=4437\n",
            "2048\n",
            "calls=30719:30719:30719\n",
            "NULL\n",
        )
    );
}

#[test]
fn error_mask_arguments_match_weak_and_strict_internal_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo 'diag=', $level, ':', $message, "\n";
    return true;
});
function attempt(string $label, callable $call): void {
    try {
        $result = $call();
        echo $label, '=ok:', var_export($result, true), "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}
attempt('report-null', static fn () => error_reporting(null)); error_reporting(E_ALL);
attempt('report-false', static fn () => error_reporting(false)); error_reporting(E_ALL);
attempt('report-true', static fn () => error_reporting(true)); error_reporting(E_ALL);
attempt('report-float', static fn () => error_reporting(2.5)); error_reporting(E_ALL);
attempt('report-string', static fn () => error_reporting('2')); error_reporting(E_ALL);
attempt('report-array', static fn () => error_reporting([]));
$probe = static fn () => null;
attempt('handler-null', static function () use ($probe) {
    $old = set_error_handler($probe, null);
    restore_error_handler();
    return $old !== null;
});
attempt('handler-float', static function () use ($probe) {
    $old = set_error_handler($probe, 2.5);
    restore_error_handler();
    return $old !== null;
});
attempt('handler-array', static fn () => set_error_handler($probe, []));
echo get_error_handler() instanceof Closure ? "outer\n" : "lost\n";
"#,
        ),
        concat!(
            "report-null=ok:30719\n",
            "report-false=ok:30719\n",
            "report-true=ok:30719\n",
            "diag=8192:Implicit conversion from float 2.5 to int loses precision\n",
            "report-float=ok:30719\n",
            "report-string=ok:30719\n",
            "report-array=TypeError:error_reporting(): Argument #1 ($error_level) must be of type ?int, array given\n",
            "diag=8192:set_error_handler(): Passing null to parameter #2 ($error_levels) of type int is deprecated\n",
            "handler-null=ok:true\n",
            "diag=8192:Implicit conversion from float 2.5 to int loses precision\n",
            "handler-float=ok:true\n",
            "handler-array=TypeError:set_error_handler(): Argument #2 ($error_levels) must be of type int, array given\n",
            "outer\n",
        )
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function attempt(string $label, callable $call): void {
    try {
        $result = $call();
        echo $label, '=ok:', var_export($result, true), "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}
attempt('report-null', static fn () => error_reporting(null));
attempt('report-false', static fn () => error_reporting(false));
attempt('report-float', static fn () => error_reporting(2.5));
attempt('report-string', static fn () => error_reporting('2'));
$probe = static fn () => null;
attempt('handler-null', static function () use ($probe) {
    $old = set_error_handler($probe, null);
    restore_error_handler();
    return $old;
});
attempt('handler-false', static fn () => set_error_handler($probe, false));
attempt('handler-float', static fn () => set_error_handler($probe, 2.5));
attempt('handler-string', static fn () => set_error_handler($probe, '2'));
"#,
        ),
        concat!(
            "report-null=ok:30719\n",
            "report-false=TypeError:error_reporting(): Argument #1 ($error_level) must be of type ?int, false given\n",
            "report-float=TypeError:error_reporting(): Argument #1 ($error_level) must be of type ?int, float given\n",
            "report-string=TypeError:error_reporting(): Argument #1 ($error_level) must be of type ?int, string given\n",
            "handler-null=TypeError:set_error_handler(): Argument #2 ($error_levels) must be of type int, null given\n",
            "handler-false=TypeError:set_error_handler(): Argument #2 ($error_levels) must be of type int, false given\n",
            "handler-float=TypeError:set_error_handler(): Argument #2 ($error_levels) must be of type int, float given\n",
            "handler-string=TypeError:set_error_handler(): Argument #2 ($error_levels) must be of type int, string given\n",
        )
    );
}

#[test]
fn extra_user_arguments_do_not_initialize_locals_and_keep_snapshot_lifetime() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function (int $level, string $message): bool {
    echo $level, ':', $message, '|';
    return true;
});
function zero() { echo $local, 'args=', json_encode(func_get_args()), "\n"; }
function one($declared) {
    echo $tail, 'args=', json_encode(func_get_args()), ':pick=', func_get_arg(1),
        ':count=', func_num_args(), "\n";
}
zero('x', 2);
one('a', 'b');
$captured = 'capture';
$closure = function () use ($captured) {
    echo $captured, ':', $local, json_encode(func_get_args()), "\n";
};
$closure('extra');
class ExtraProbe {
    public function run() { echo $local, 'method=', json_encode(func_get_args()), "\n"; }
}
(new ExtraProbe)->run('m');
$value = 'stable';
function mutate_extra() { $args = func_get_args(); $args[0] = 'changed'; }
mutate_extra($value);
echo $value, "\n";
class Watch { public function __destruct() { echo "drop\n"; } }
function retain_extra() { echo "body\n"; }
retain_extra(new Watch());
echo "after\n";
restore_error_handler();
"#,
        ),
        concat!(
            "2:Undefined variable $local|args=[\"x\",2]\n",
            "2:Undefined variable $tail|args=[\"a\",\"b\"]:pick=b:count=2\n",
            "capture:2:Undefined variable $local|[\"extra\"]\n",
            "2:Undefined variable $local|method=[\"m\"]\n",
            "stable\n",
            "body\n",
            "drop\n",
            "after\n",
        )
    );
}

#[test]
fn e_strict_handler_throw_restores_suppression_and_blocks_recursion() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): never {
    echo 'throw=', $level, ':', error_reporting(), "\n";
    throw new RuntimeException('stop');
});
try {
    echo @E_STRICT;
} catch (Throwable $error) {
    echo $error::class, ':', $error->getMessage(), ':mask=', error_reporting(), "\n";
}
restore_error_handler();
set_error_handler(static function (int $level, string $message): bool {
    echo 'outer=', $level, ':';
    echo E_STRICT, "\n";
    return true;
});
echo E_STRICT, "\n";
"#,
        ),
        concat!(
            "throw=8192:4437\n",
            "RuntimeException:stop:mask=30719\n",
            "outer=8192:2048\n",
            "2048\n",
        )
    );
}

#[test]
fn suppressed_calls_restore_fatal_only_changes_on_return_and_unwind() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
function normal_reporting_change(): void { error_reporting(0); }
@normal_reporting_change();
echo 'normal=', error_reporting(), "\n";

error_reporting(E_ALL);
function throwing_reporting_change(): void {
    @$missing;
    error_reporting(0);
    throw new RuntimeException('stop');
}
try {
    @throwing_reporting_change();
} catch (RuntimeException $error) {
    echo $error->getMessage(), "\n";
}
echo 'unwind=', error_reporting(), "\n";
"#,
        ),
        concat!("normal=30719\n", "stop\n", "unwind=30719\n")
    );
}
