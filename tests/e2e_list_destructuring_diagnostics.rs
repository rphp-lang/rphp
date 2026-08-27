mod common;

use common::run_php;

#[test]
fn scalar_destructuring_warns_once_per_materialized_element_in_evaluation_order() {
    assert_eq!(
        run_php(
            r#"<?php
class TraceLog {
    public static array $events = [];
}
function source_value(string $label, mixed $value): mixed {
    TraceLog::$events[] = 'source:' . $label;
    return $value;
}
function source_key(string $key): string {
    TraceLog::$events[] = 'key:' . $key;
    return $key;
}
function observe(string $label, callable $operation): void {
    TraceLog::$events = [];
    echo $label, ':';
    $values = $operation();
    echo json_encode($values), ':', implode(',', TraceLog::$events), "\n";
}
set_error_handler(function (int $level, string $message): bool {
    TraceLog::$events[] = 'warning:' . $message;
    return true;
});

observe('long-bool', function () {
    list($first, $second) = source_value('true', true);
    return [$first, $second];
});
observe('short-false', function () {
    [$first, $second] = source_value('false', false);
    return [$first, $second];
});
observe('float', function () {
    [$first, $second] = source_value('float', 2.5);
    return [$first, $second];
});
observe('int', function () {
    [$first, $second] = source_value('int', 7);
    return [$first, $second];
});
observe('string', function () {
    [$first, $second, $third] = source_value('string', 'xy');
    return [$first, $second, $third];
});
observe('skip', function () {
    [$first, , $third] = source_value('skip', true);
    return [$first, $third];
});
observe('keyed', function () {
    [source_key('left') => $first, source_key('right') => $second] = source_value('keyed', 3);
    return [$first, $second];
});
observe('nested-scalar', function () {
    [$first, [$second, $third]] = source_value('nested-scalar', true);
    return [$first, $second, $third];
});
observe('nested-string', function () {
    [$first, [$second, $third]] = source_value('nested-string', [9, 'xy']);
    return [$first, $second, $third];
});
"#,
        ),
        concat!(
            "long-bool:[null,null]:source:true,warning:Cannot use bool as array,warning:Cannot use bool as array\n",
            "short-false:[null,null]:source:false,warning:Cannot use bool as array,warning:Cannot use bool as array\n",
            "float:[null,null]:source:float,warning:Cannot use float as array,warning:Cannot use float as array\n",
            "int:[null,null]:source:int,warning:Cannot use int as array,warning:Cannot use int as array\n",
            "string:[null,null,null]:source:string,warning:Cannot use string as array,warning:Cannot use string as array,warning:Cannot use string as array\n",
            "skip:[null,null]:source:skip,warning:Cannot use bool as array,warning:Cannot use bool as array\n",
            "keyed:[null,null]:source:keyed,key:left,warning:Cannot use int as array,key:right,warning:Cannot use int as array\n",
            "nested-scalar:[null,null,null]:source:nested-scalar,warning:Cannot use bool as array,warning:Cannot use bool as array\n",
            "nested-string:[9,null,null]:source:nested-string,warning:Cannot use string as array,warning:Cannot use string as array\n",
        )
    );
}

#[test]
fn scalar_destructuring_preserves_suppression_handler_and_exception_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
class TraceLog {
    public static array $events = [];
}
function source_value(string $label, mixed $value): mixed {
    TraceLog::$events[] = 'source:' . $label;
    return $value;
}
function observe(string $label, callable $operation): void {
    TraceLog::$events = [];
    echo $label, ':', json_encode($operation()), ':', implode(',', TraceLog::$events), "\n";
}
$recording = function (int $level, string $message): bool {
    TraceLog::$events[] = 'warning:' . $message . ':mask=' . error_reporting();
    return true;
};
set_error_handler($recording);

observe('at-handler', function () {
    @[$first, $second] = source_value('at-handler', true);
    return [$first, $second];
});
observe('zero-handler', function () {
    $previous = error_reporting(0);
    [$first, $second] = source_value('zero-handler', true);
    error_reporting($previous);
    return [$first, $second];
});
observe('throwing', function () {
    $first = 'first-before';
    $second = 'second-before';
    set_error_handler(function (int $level, string $message): never {
        TraceLog::$events[] = 'throw:' . $message;
        throw new RuntimeException('stop');
    });
    try {
        [$first, $second] = source_value('throwing', true);
    } catch (RuntimeException $error) {
        TraceLog::$events[] = 'caught:' . $error->getMessage();
    } finally {
        restore_error_handler();
    }
    return [$first, $second];
});

restore_error_handler();
@[$first, $second] = source_value('builtin-at', true);
$previous = error_reporting(0);
[$third, $fourth] = source_value('builtin-zero', true);
error_reporting($previous);
echo 'builtin:', json_encode([$first, $second, $third, $fourth]), "\n";
"#,
        ),
        concat!(
            "at-handler:[null,null]:source:at-handler,warning:Cannot use bool as array:mask=4437,warning:Cannot use bool as array:mask=4437\n",
            "zero-handler:[null,null]:source:zero-handler,warning:Cannot use bool as array:mask=0,warning:Cannot use bool as array:mask=0\n",
            "throwing:[\"first-before\",\"second-before\"]:source:throwing,throw:Cannot use bool as array,caught:stop\n",
            "builtin:[null,null,null,null]\n",
        )
    );
}

#[test]
fn null_valid_reference_cow_and_prior_string_offset_diagnostics_remain_stable() {
    assert_eq!(
        run_php(
            r#"<?php
class TraceLog {
    public static array $events = [];
}
set_error_handler(function (int $level, string $message): bool {
    TraceLog::$events[] = $message;
    return true;
});

[$nullFirst, $nullSecond] = null;
$source = [1, [2, 3]];
$copy = $source;
[$first, [$second, $third]] = $source;
echo 'valid:', json_encode([$nullFirst, $nullSecond, $first, $second, $third]),
    ':', json_encode($source), ':', json_encode($copy), ':', implode(',', TraceLog::$events), "\n";

$referenceSource = [4, 5];
$referenceCopy = $referenceSource;
[$referenceFirst, &$referenceSecond] = $referenceSource;
$referenceSecond = 8;
echo 'reference:', json_encode([$referenceFirst, $referenceSecond]),
    ':', json_encode($referenceSource), ':', json_encode($referenceCopy), "\n";

TraceLog::$events = [];
$empty = '';
[$emptyFirst, $emptySecond] = $empty[0];
echo 'empty:', json_encode([$emptyFirst, $emptySecond]), ':', implode(',', TraceLog::$events), "\n";
"#,
        ),
        concat!(
            "valid:[null,null,1,2,3]:[1,[2,3]]:[1,[2,3]]:\n",
            "reference:[4,8]:[4,8]:[4,5]\n",
            "empty:[null,null]:Uninitialized string offset 0,Cannot use string as array,Cannot use string as array\n",
        )
    );
}
