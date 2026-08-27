mod common;

use common::run_php;

#[test]
fn append_overflow_preserves_evaluation_order_references_cow_and_state() {
    assert_eq!(
        run_php(
            r#"<?php
class Recorder {
    public static array $events = [];
}
function attempt(string $label, callable $operation): void {
    Recorder::$events = [];
    echo $label, ':';
    try {
        $result = $operation();
        echo 'ok:', get_debug_type($result), ':', json_encode($result);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage();
    }
    echo ':events=', implode(',', Recorder::$events), "\n";
}
function effect(string $name, mixed $value): mixed {
    Recorder::$events[] = $name;
    return $value;
}
function by_value(): int {
    Recorder::$events[] = 'by-value';
    return 41;
}
class DefaultAppendValue {
    public function __construct() {
        Recorder::$events[] = 'default-value';
    }
}
function use_default($value = [PHP_INT_MAX => 'edge', new DefaultAppendValue()]): void {
    echo 'body';
}

$array = [PHP_INT_MAX => 'edge'];
$copy = $array;
attempt('ordinary', function () use (&$array) {
    return $array[] = effect('rhs', ['nested' => 1]);
});
echo 'ordinary-state:', count($array), ':', count($copy), ':', $copy[PHP_INT_MAX], "\n";

$slot = 12;
$alias = &$slot;
attempt('reference-variable', function () use (&$array, &$alias) {
    return $array[] =& $alias;
});
attempt('reference-value', function () use (&$array) {
    return $array[] =& by_value();
});
echo 'reference-state:', count($array), ':', $slot, "\n";

$references = [];
$value = 5;
$references[] =& $value;
$value = 8;
set_error_handler(function (int $level, string $message): bool {
    Recorder::$events[] = 'notice:' . $message;
    return true;
});
attempt('valid-reference-value', function () use (&$references) {
    return $references[] =& by_value();
});
restore_error_handler();
echo 'valid-reference-state:', json_encode($references), "\n";
$notice_throw = [];
set_error_handler(function (int $level, string $message): never {
    Recorder::$events[] = 'notice-throw';
    throw new Exception('notice-handler');
});
attempt('valid-reference-notice-throw', function () use (&$notice_throw) {
    return $notice_throw[] =& by_value();
});
restore_error_handler();
echo 'valid-reference-notice-state:', json_encode($notice_throw), "\n";

attempt('literal', function () {
    return [effect('key', PHP_INT_MAX) => effect('first', 'edge'), effect('later', 'value')];
});
attempt('default', function () {
    use_default();
    return 'returned';
});

$removed = [PHP_INT_MAX => 'edge'];
unset($removed[PHP_INT_MAX]);
attempt('removed-max', function () use (&$removed) {
    return $removed[] = effect('rhs', 9);
});
echo 'removed-state:', json_encode($removed), "\n";
"#,
        ),
        concat!(
            "ordinary:Error:Cannot add element to the array as the next element is already occupied:events=rhs\n",
            "ordinary-state:1:1:edge\n",
            "reference-variable:Error:Cannot add element to the array as the next element is already occupied:events=\n",
            "reference-value:Error:Cannot add element to the array as the next element is already occupied:events=by-value\n",
            "reference-state:1:12\n",
            "valid-reference-value:ok:int:41:events=by-value,notice:Only variables should be assigned by reference\n",
            "valid-reference-state:[8,41]\n",
            "valid-reference-notice-throw:Exception:notice-handler:events=by-value,notice-throw\n",
            "valid-reference-notice-state:[null]\n",
            "literal:Error:Cannot add element to the array as the next element is already occupied:events=key,first,later\n",
            "default:Error:Cannot add element to the array as the next element is already occupied:events=default-value\n",
            "removed-max:ok:int:9:events=rhs\n",
            "removed-state:{\"9223372036854775807\":9}\n",
        )
    );
}

#[test]
fn append_compound_and_increment_targets_share_the_overflow_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
class Recorder {
    public static array $events = [];
}
function attempt(string $label, callable $operation): void {
    Recorder::$events = [];
    echo $label, ':';
    try {
        $result = $operation();
        echo 'ok:', get_debug_type($result), ':', json_encode($result);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage();
    }
    echo ':events=', implode(',', Recorder::$events), "\n";
}
function effect(string $name, mixed $value): mixed {
    Recorder::$events[] = $name;
    return $value;
}

$full = [PHP_INT_MAX => 'edge'];
attempt('add', function () use (&$full) { return $full[] += effect('rhs', 3); });
attempt('concat', function () use (&$full) { return $full[] .= effect('rhs', 'x'); });
attempt('preinc', function () use (&$full) { return ++$full[]; });
attempt('postinc', function () use (&$full) { return $full[]++; });
echo 'full-state:', count($full), ':', $full[PHP_INT_MAX], "\n";

$valid = [-2 => 'negative'];
attempt('valid-add', function () use (&$valid) { return $valid[] += effect('rhs', 2); });
attempt('valid-preinc', function () use (&$valid) { return ++$valid[]; });
attempt('valid-postinc', function () use (&$valid) { return $valid[]++; });
echo 'valid-state:', json_encode($valid), "\n";
"#,
        ),
        concat!(
            "add:Error:Cannot add element to the array as the next element is already occupied:events=rhs\n",
            "concat:Error:Cannot add element to the array as the next element is already occupied:events=rhs\n",
            "preinc:Error:Cannot add element to the array as the next element is already occupied:events=\n",
            "postinc:Error:Cannot add element to the array as the next element is already occupied:events=\n",
            "full-state:1:edge\n",
            "valid-add:ok:int:2:events=rhs\n",
            "valid-preinc:ok:int:1:events=\n",
            "valid-postinc:ok:null:null:events=\n",
            "valid-state:{\"-2\":\"negative\",\"-1\":2,\"0\":1,\"1\":1}\n",
        )
    );
}

#[test]
fn incomplete_and_scalar_receivers_fail_without_property_or_array_mutation() {
    assert_eq!(
        run_php(
            r#"<?php
class Recorder {
    public static array $events = [];
}
function attempt(string $label, callable $operation): void {
    Recorder::$events = [];
    echo $label, ':';
    try {
        $result = $operation();
        echo 'ok:', get_debug_type($result), ':', json_encode($result);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage();
    }
    echo ':events=', implode(',', Recorder::$events), "\n";
}
function effect(string $name, mixed $value): mixed {
    Recorder::$events[] = $name;
    return $value;
}

$object = unserialize('O:13:"MissingTarget":1:{s:4:"keep";i:7;}');
attempt('assign', function () use ($object) { return $object->added = effect('rhs', 1); });
attempt('compound', function () use ($object) { return $object->keep += effect('rhs', 2); });
attempt('preinc', function () use ($object) { return ++$object->keep; });
attempt('postinc', function () use ($object) { return $object->keep++; });
echo 'incomplete-state:', json_encode((array) $object), "\n";

$truth = true;
attempt('scalar-index-assign', function () use (&$truth) { return $truth[3] = effect('rhs', 4); });
attempt('scalar-index-compound', function () use (&$truth) { return $truth[3] += effect('rhs', 4); });
attempt('scalar-property-assign', function () use (&$truth) { return $truth->item = effect('rhs', 4); });
attempt('scalar-property-compound', function () use (&$truth) { return $truth->item += effect('rhs', 4); });
echo 'scalar-state:', get_debug_type($truth), ':', json_encode($truth), "\n";
"#,
        ),
        concat!(
            "assign:Error:The script tried to modify a property on an incomplete object. Please ensure that the class definition \"MissingTarget\" of the object you are trying to operate on was loaded _before_ unserialize() gets called or provide an autoloader to load the class definition:events=rhs\n",
            "compound:Error:The script tried to modify a property on an incomplete object. Please ensure that the class definition \"MissingTarget\" of the object you are trying to operate on was loaded _before_ unserialize() gets called or provide an autoloader to load the class definition:events=rhs\n",
            "preinc:Error:The script tried to modify a property on an incomplete object. Please ensure that the class definition \"MissingTarget\" of the object you are trying to operate on was loaded _before_ unserialize() gets called or provide an autoloader to load the class definition:events=\n",
            "postinc:Error:The script tried to modify a property on an incomplete object. Please ensure that the class definition \"MissingTarget\" of the object you are trying to operate on was loaded _before_ unserialize() gets called or provide an autoloader to load the class definition:events=\n",
            "incomplete-state:{\"__PHP_Incomplete_Class_Name\":\"MissingTarget\",\"keep\":7}\n",
            "scalar-index-assign:Error:Cannot use a scalar value as an array:events=rhs\n",
            "scalar-index-compound:Error:Cannot use a scalar value as an array:events=rhs\n",
            "scalar-property-assign:Error:Attempt to assign property \"item\" on true:events=rhs\n",
            "scalar-property-compound:Error:Attempt to assign property \"item\" on true:events=rhs\n",
            "scalar-state:bool:true\n",
        )
    );
}
