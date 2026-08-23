mod common;

use common::run_php;

#[test]
fn cardinality_signatures_constants_countable_and_errors_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['count', 'sizeof', 'min', 'max'] as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, ':', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':';
    foreach ($reflection->getParameters() as $parameter) {
        echo $parameter->getName(), ',',
            $parameter->isVariadic() ? 'variadic' : 'fixed', ';';
    }
    echo "\n";
}
echo 'constants:', COUNT_NORMAL, ',', COUNT_RECURSIVE, "\n";
echo 'ordinary:', count([1, [2]]), ':', count([1, [2]], COUNT_RECURSIVE),
    ':', sizeof([1, [2]], 1), "\n";

class CardinalityBox implements Countable {
    public function count(): int {
        return 7;
    }
}
echo 'countable:', count(new CardinalityBox(), 1), ':',
    sizeof(new CardinalityBox(), 0), "\n";

class ExplodingCountable implements Countable {
    public function count(): int {
        throw new RuntimeException('boom');
    }
}
try {
    sizeof(new ExplodingCountable());
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}

foreach ([null, true, new stdClass()] as $value) {
    try {
        sizeof($value);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
try {
    count(null, 2);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "count:1/2:value,fixed;mode,fixed;\n",
            "sizeof:1/2:value,fixed;mode,fixed;\n",
            "min:1/2:value,fixed;values,variadic;\n",
            "max:1/2:value,fixed;values,variadic;\n",
            "constants:0,1\n",
            "ordinary:2:3:3\n",
            "countable:7:7\n",
            "RuntimeException:boom\n",
            "TypeError:sizeof(): Argument #1 ($value) must be of type Countable|array, null given\n",
            "TypeError:sizeof(): Argument #1 ($value) must be of type Countable|array, true given\n",
            "TypeError:sizeof(): Argument #1 ($value) must be of type Countable|array, stdClass given\n",
            "ValueError:count(): Argument #2 ($mode) must be either COUNT_NORMAL or COUNT_RECURSIVE\n",
        )
    );
}

#[test]
fn recursive_count_modes_reentrancy_and_depth_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
$sharedChild = [1];
$shared = [&$sharedChild, &$sharedChild];
echo 'shared:', count($shared, COUNT_RECURSIVE), "\n";

$self = [];
$self['self'] = &$self;
set_error_handler(function ($level, $message) use (&$self) {
    echo 'handled:', $level, ':', $message, "\n";
    $self = ['changed'];
    return true;
});
echo 'reentrant:', count($self, COUNT_RECURSIVE), ':', json_encode($self), "\n";
restore_error_handler();

$self = [];
$self[] = &$self;
set_error_handler(function ($level, $message) {
    throw new RuntimeException('stop:' . $message);
});
try {
    sizeof($self, COUNT_RECURSIVE);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
restore_error_handler();

set_error_handler(function ($level, $message) {
    echo 'mode-diag:', $level, ':', $message, "\n";
    return true;
});
foreach ([false, true, '1', 0.0, null] as $mode) {
    echo 'mode:', count([1, [2]], $mode), "\n";
}
restore_error_handler();

$deep = [];
for ($index = 0; $index < 600; $index++) {
    $deep = [$deep];
}
echo 'deep:', count($deep, COUNT_RECURSIVE), "\n";
"#,
        ),
        concat!(
            "shared:4\n",
            "reentrant:handled:2:count(): Recursion detected\n",
            "1:[\"changed\"]\n",
            "RuntimeException:stop:sizeof(): Recursion detected\n",
            "mode:2\n",
            "mode:3\n",
            "mode:3\n",
            "mode:2\n",
            "mode:mode-diag:8192:count(): Passing null to parameter #2 ($mode) of type int is deprecated\n",
            "2\n",
            "deep:600\n",
        )
    );
}

#[test]
fn cardinality_strict_mode_types_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);

foreach ([true, '1', 1.0, null, [], new stdClass()] as $mode) {
    try {
        count([], $mode);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "TypeError:count(): Argument #2 ($mode) must be of type int, true given\n",
            "TypeError:count(): Argument #2 ($mode) must be of type int, string given\n",
            "TypeError:count(): Argument #2 ($mode) must be of type int, float given\n",
            "TypeError:count(): Argument #2 ($mode) must be of type int, null given\n",
            "TypeError:count(): Argument #2 ($mode) must be of type int, array given\n",
            "TypeError:count(): Argument #2 ($mode) must be of type int, stdClass given\n",
        )
    );
}

#[test]
fn extrema_array_variadic_comparison_reference_and_recursion_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['min', 'max'] as $name) {
    echo $name, '-empty:';
    try {
        $name([]);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
    echo $name, '-scalar:';
    try {
        $name(1);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
    echo $name, '-many:';
    var_dump($name(4, -2, 7, 1));
    echo $name, '-one-array:';
    var_dump($name([4, -2, 7, 1]));
    echo $name, '-large:';
    var_dump($name(9007199254740992, '9007199254740993'));
    echo $name, '-ties:';
    var_dump(
        $name(1, 1.0),
        $name(1.0, 1),
        $name(null, ''),
        $name(false, 0)
    );
    echo $name, '-nan:';
    var_dump(is_nan($name(NAN, 1.0)), is_nan($name(1.0, NAN)));
    echo $name, '-null-zero:';
    var_dump($name(null, '0'), $name('0', null));
}

$slot = 3;
$input = [5, &$slot, 4];
$result = min($input);
$result = 99;
echo 'reference:', $slot, "\n";

$first = (object) ['x' => 2];
$second = (object) ['x' => 1];
echo 'objects:';
var_dump(min($first, $second) === $second, max($first, $second) === $first);
echo 'arrays:';
var_dump(min([1], [1, 2]), max([1], [1, 2]));

$left = [];
$left['self'] = &$left;
$right = [];
$right['self'] = &$right;
foreach (['min', 'max'] as $name) {
    try {
        $name([$left, $right]);
    } catch (Throwable $error) {
        echo $name, ':', get_class($error), ':', $error->getMessage(), "\n";
    }
    echo $name, '-same:', $name([$left, $left]) === $left ? 'yes' : 'no', "\n";
}
"#,
        ),
        concat!(
            "min-empty:ValueError:min(): Argument #1 ($value) must contain at least one element\n",
            "min-scalar:TypeError:min(): Argument #1 ($value) must be of type array, int given\n",
            "min-many:int(-2)\n",
            "min-one-array:int(-2)\n",
            "min-large:int(9007199254740992)\n",
            "min-ties:int(1)\n",
            "float(1)\n",
            "NULL\n",
            "bool(false)\n",
            "min-nan:bool(true)\n",
            "bool(false)\n",
            "min-null-zero:NULL\n",
            "NULL\n",
            "max-empty:ValueError:max(): Argument #1 ($value) must contain at least one element\n",
            "max-scalar:TypeError:max(): Argument #1 ($value) must be of type array, int given\n",
            "max-many:int(7)\n",
            "max-one-array:int(7)\n",
            "max-large:string(16) \"9007199254740993\"\n",
            "max-ties:int(1)\n",
            "float(1)\n",
            "NULL\n",
            "bool(false)\n",
            "max-nan:bool(true)\n",
            "bool(false)\n",
            "max-null-zero:string(1) \"0\"\n",
            "string(1) \"0\"\n",
            "reference:3\n",
            "objects:bool(true)\n",
            "bool(true)\n",
            "arrays:array(1) {\n",
            "  [0]=>\n",
            "  int(1)\n",
            "}\n",
            "array(2) {\n",
            "  [0]=>\n",
            "  int(1)\n",
            "  [1]=>\n",
            "  int(2)\n",
            "}\n",
            "min:Error:Nesting level too deep - recursive dependency?\n",
            "min-same:yes\n",
            "max:Error:Nesting level too deep - recursive dependency?\n",
            "max-same:yes\n",
        )
    );
}

#[test]
fn direct_binary_extrema_and_dynamic_ties_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(
    min(1, 1.0),
    min(1.0, 1),
    min(null, ''),
    min(false, 0),
    min(NAN, 1.0),
    min(1.0, NAN),
    min(9007199254740992, 9007199254740993.0)
);
var_dump(
    max(1, 1.0),
    max(1.0, 1),
    max(null, ''),
    max(false, 0),
    max(NAN, 1.0),
    max(1.0, NAN)
);

$function = 'min';
var_dump($function(1, 1.0), $function(NAN, 1.0));

$left = [];
$left['self'] = &$left;
$right = [];
$right['self'] = &$right;
try {
    min($left, $right);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "float(1)\n",
            "int(1)\n",
            "string(0) \"\"\n",
            "int(0)\n",
            "float(1)\n",
            "float(NAN)\n",
            "float(9007199254740992)\n",
            "int(1)\n",
            "float(1)\n",
            "NULL\n",
            "bool(false)\n",
            "float(1)\n",
            "float(NAN)\n",
            "int(1)\n",
            "float(NAN)\n",
            "Error:Nesting level too deep - recursive dependency?\n",
        )
    );
}
