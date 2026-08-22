mod common;
use common::{run_php, run_php_expect_error};
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;

#[test]
fn function_static_can_rebind_a_global_cv_without_overwriting_the_global() {
    assert_eq!(
        run_php(
            r#"<?php
function update() {
    global $value;
    $value = 42;
    echo $value, ':';
    static $value = 41;
    echo $value, ':';
}
update(); echo $value, ':';
update(); echo $value;
"#
        ),
        "42:41:42:42:41:42"
    );
}

// list() / destructuring tests

#[test]
fn test_list_basic() {
    assert_eq!(
        run_php(
            r#"<?php
list($a, $b, $c) = [10, 20, 30];
echo "$a $b $c";
"#
        ),
        "10 20 30"
    );
}

#[test]
fn test_short_destructuring() {
    assert_eq!(
        run_php(
            r#"<?php
[$a, $b] = [1, 2];
echo "$a $b";
"#
        ),
        "1 2"
    );
}

#[test]
fn keyed_destructuring_evaluates_the_source_then_each_key_and_writable_target() {
    assert_eq!(
        run_php(
            r#"<?php
class DestructureSink { public $slot; }
function keyedSource(&$trace) {
    $trace .= 'S';
    return ['left' => 10, 'right' => 20];
}
function keyedOffset(&$trace, $name) {
    $trace .= 'K' . $name;
    return $name;
}
$trace = '';
$sink = new DestructureSink();
$values = [];
list(
    keyedOffset($trace, 'left') => $values[],
    keyedOffset($trace, 'right') => $sink->slot,
) = keyedSource($trace);
echo $trace, ':', $values[0], ':', $sink->slot;
"#
        ),
        "SKleftKright:10:20"
    );
}

#[test]
fn keyed_destructuring_accepts_magic_compile_time_and_runtime_keys() {
    assert_eq!(
        run_php(
            r#"<?php
const FIRST_DESTRUCTURE_KEY = 'first';
define('SECOND_DESTRUCTURE_KEY', 'second');
$thirdKey = 'third';
list(
    FIRST_DESTRUCTURE_KEY => $first,
    SECOND_DESTRUCTURE_KEY => $second,
    $thirdKey => $third,
) = ['first' => 1, 'second' => 2, 'third' => 3];
list(__FILE__ => $fileValue) = [__FILE__ => 4];
echo $first, $second, $third, $fileValue;
"#
        ),
        "1234"
    );
}

#[test]
fn keyed_destructuring_preserves_integer_and_numeric_string_array_access_keys() {
    assert_eq!(
        run_php(
            r#"<?php
class DestructureKeyProbe implements ArrayAccess {
    public function offsetGet($offset): mixed {
        echo gettype($offset), ':';
        return $offset;
    }
    public function offsetSet($offset, $value): void {}
    public function offsetExists($offset): bool { return true; }
    public function offsetUnset($offset): void {}
}
$source = new DestructureKeyProbe();
list(123 => $integer, '123' => $string) = $source;
var_dump($integer, $string);
"#
        ),
        "integer:string:int(123)\nstring(3) \"123\"\n"
    );
}

#[test]
fn list_assignment_expressions_capture_keys_sources_and_member_targets_in_arrows() {
    assert_eq!(
        run_php(
            r#"<?php
class ArrowDestructureSink { public $slot; }
$key = 'selected';
$source = ['selected' => 42];
$sink = new ArrowDestructureSink();
$assign = fn() => list($key => $sink->slot) = $source;
$returned = $assign();
echo $sink->slot, ':', $returned['selected'];
"#
        ),
        "42:42"
    );
}

#[test]
fn legacy_list_assignment_is_a_value_and_only_reference_lists_are_referenceable() {
    assert_eq!(
        run_php(
            r#"<?php
function replaceListValue(&$value) { $value = ['changed']; }
$source = [7];
$returned = LIST($item) = $source;
echo $returned[0], ':', $item, '|';
try {
    replaceListValue(list($item) = $source);
} catch (Error $error) {
    echo $error->getMessage(), '|';
}
$replace = 'replaceListValue';
try {
    $replace(value: list($item) = $source);
} catch (Error $error) {
    echo $error->getMessage(), '|';
}
replaceListValue(list(&$item) = $source);
echo $source[0];
"#
        ),
        "7:7|replaceListValue(): Argument #1 ($value) could not be passed by reference|replaceListValue(): Argument #1 ($value) could not be passed by reference|changed"
    );
}

#[test]
fn destructuring_pattern_errors_are_compile_time_and_do_not_run_the_source() {
    for (pattern, message) in [
        (
            "[42]",
            "Assignments can only happen to writable values on line 1",
        ),
        (
            "[array($value)]",
            "Cannot assign to array(), use [] instead on line 1",
        ),
        (
            "list($first, 1 => $second)",
            "Cannot mix keyed and unkeyed array entries in assignments on line 1",
        ),
        ("list([$value])", "Cannot mix [] and list() on line 1"),
        ("list(,,,,)", "Cannot use empty list on line 1"),
        (
            "['key' => $value, ,]",
            "Cannot use empty array entries in keyed array assignment on line 1",
        ),
    ] {
        let source = format!(
            "<?php function destructureMustNotRun() {{ echo 'ran'; return []; }} {pattern} = destructureMustNotRun();"
        );
        assert_eq!(run_php_expect_error(&source).to_string(), message);
    }
}

#[test]
fn destructuring_null_yields_null_elements_without_offset_warnings() {
    assert_eq!(
        run_php(
            r#"<?php
$source = null;
var_dump(!([$first, $second] = $source));
var_dump($first, $second);
if (([$third, $fourth] = $source)) echo "truthy\n";
var_dump($third, $fourth);
"#
        ),
        "bool(true)\nNULL\nNULL\nNULL\nNULL\n"
    );
}

#[test]
fn destructuring_non_array_scalars_yields_null_without_offset_warnings() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([1, true, 1.5, 'text'] as $source) {
    [$first, $second] = $source;
    var_dump($first, $second);
}
"#
        ),
        "NULL\nNULL\nNULL\nNULL\nNULL\nNULL\nNULL\nNULL\n"
    );
}

#[test]
fn destructuring_spread_is_a_located_compile_error() {
    let source = r#"<?php
$marker = 'must-not-run';
[$primary, ...$overflow] = makeValues();
"#;
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens)
        .with_source_name("/fixture/destructuring-spread.php")
        .parse()
        .unwrap();
    let error = match Compiler::new()
        .with_source_context("/fixture/destructuring-spread.php", "/fixture")
        .compile(&statements)
    {
        Ok(_) => panic!("destructuring spread must fail during compilation"),
        Err(error) => error.message,
    };

    assert_eq!(
        error,
        "Spread operator is not supported in assignments in /fixture/destructuring-spread.php on line 3"
    );
}

#[test]
fn test_list_skip_elements() {
    assert_eq!(
        run_php(
            r#"<?php
[, $b, , $d] = [1, 2, 3, 4];
echo "$b $d";
"#
        ),
        "2 4"
    );
}

#[test]
fn test_list_from_function() {
    assert_eq!(
        run_php(
            r#"<?php
function pair() { return [42, "hello"]; }
[$num, $str] = pair();
echo "$num $str";
"#
        ),
        "42 hello"
    );
}

// global keyword tests

#[test]
fn test_global_read() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 10;
function foo() {
    global $x;
    echo $x;
}
foo();
"#
        ),
        "10"
    );
}

#[test]
fn test_global_write() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 10;
function foo() {
    global $x;
    $x = 20;
}
foo();
echo $x;
"#
        ),
        "20"
    );
}

#[test]
fn test_global_multiple() {
    assert_eq!(
        run_php(
            r#"<?php
$a = 1;
$b = 2;
function swap() {
    global $a, $b;
    $tmp = $a;
    $a = $b;
    $b = $tmp;
}
swap();
echo "$a $b";
"#
        ),
        "2 1"
    );
}

// Transitive global access tests — A calls B, B uses `global`
// These verify that the `needs_globals_sync` guard correctly syncs
// caller scope even when the immediate callee doesn't use `global`.

#[test]
fn test_global_transitive_one_level() {
    // A() has no `global`, A() calls B(), B() has `global $x`
    assert_eq!(
        run_php(
            r#"<?php
$x = 42;
function A() {
    B();
}
function B() {
    global $x;
    echo $x;
}
A();
"#
        ),
        "42"
    );
}

#[test]
fn test_global_transitive_two_levels() {
    // A() calls B(), B() calls C(), C() has `global $x`
    assert_eq!(
        run_php(
            r#"<?php
$x = 99;
function A() {
    B();
}
function B() {
    C();
}
function C() {
    global $x;
    echo $x;
}
A();
"#
        ),
        "99"
    );
}

#[test]
fn test_global_transitive_write() {
    // A() calls B(), B() writes `global $x`, verify main scope sees the change
    assert_eq!(
        run_php(
            r#"<?php
$x = 1;
function A() {
    B();
}
function B() {
    global $x;
    $x = 200;
}
A();
echo $x;
"#
        ),
        "200"
    );
}

#[test]
fn test_global_transitive_closure() {
    // Closure calls a function that uses `global`
    assert_eq!(
        run_php(
            r#"<?php
$x = 77;
function reader() {
    global $x;
    return $x;
}
$f = function() {
    return reader();
};
echo $f();
"#
        ),
        "77"
    );
}

#[test]
fn test_global_transitive_method() {
    // Method calls a function that uses `global`
    assert_eq!(
        run_php(
            r#"<?php
$x = 55;
function get_global_x() {
    global $x;
    return $x;
}
class Foo {
    function bar() {
        return get_global_x();
    }
}
$obj = new Foo();
echo $obj->bar();
"#
        ),
        "55"
    );
}

#[test]
fn test_global_after_modification_transitive() {
    // Modify $x in main scope, then A() → B() reads it
    assert_eq!(
        run_php(
            r#"<?php
$x = 1;
function A() { B(); }
function B() { global $x; echo $x . " "; }
A();
$x = 2;
A();
$x = 3;
A();
"#
        ),
        "1 2 3 "
    );
}

#[test]
fn test_globals_dimension_tracks_the_global_symbol_table() {
    assert_eq!(
        run_php(
            r#"<?php
$shade = "amber";
function recolor() {
    $GLOBALS["shade"] = "blue";
}
function erase_global($name) {
    unset($GLOBALS[$name]);
}
recolor();
echo $shade, "|", $GLOBALS["shade"], "\n";
erase_global("shade");
var_dump(isset($shade), isset($GLOBALS["shade"]));
$GLOBALS[3] = "numeric";
echo $GLOBALS["3"], "|";
$shape = 9;
$GLOBALS["shape"] = "round";
echo $shape, "|";
$GLOBALS["counter"] = 2;
$GLOBALS["counter"]++;
$GLOBALS["counter"] += 4;
$GLOBALS["bag"] = [];
$GLOBALS["bag"][] = "first";
$GLOBALS["fallback"] ??= "ready";
$GLOBALS["fallback"] ??= "ignored";
$source = 5;
$GLOBALS["linked"] =& $source;
$source++;
$alias =& $GLOBALS["counter"];
$alias++;
echo $GLOBALS["counter"], "|", $GLOBALS["bag"][0], "|", $linked, "|", $fallback;
"#
        ),
        "blue|blue\nbool(false)\nbool(false)\nnumeric|round|8|first|6|ready"
    );
}

#[test]
fn globals_root_is_readable_but_not_a_mutation_target() {
    fn compile_error(source: &str) -> String {
        let tokens = Lexer::new(source).tokenize().expect("source must lex");
        let statements = Parser::new(tokens).parse().expect("source must parse");
        match Compiler::new().compile(&statements) {
            Ok(_) => panic!("forbidden GLOBALS target must fail during compilation"),
            Err(error) => error.message,
        }
    }

    let mutation_error =
        "$GLOBALS can only be modified using the $GLOBALS[$name] = $value syntax on line 2";
    for source in [
        "<?php\n$GLOBALS = [];",
        "<?php\nif (false) { $GLOBALS = []; }",
        "<?php\n$GLOBALS += [];",
        "<?php\nunset($GLOBALS);",
        "<?php\n[$GLOBALS] = [1];",
        "<?php\n[[[$GLOBALS]]] = [[[1]]];",
        "<?php\nforeach ([1] as $GLOBALS) {}",
        "<?php\nforeach ([1] as $key => $GLOBALS) {}",
        "<?php\nforeach ([1] as $GLOBALS => $value) {}",
        "<?php\n++$GLOBALS;",
        "<?php\n$GLOBALS++;",
        "<?php\n$GLOBALS ??= [];",
        "<?php\n$replacement = []; $GLOBALS =& $replacement;",
    ] {
        assert_eq!(compile_error(source), mutation_error, "{source}");
    }

    assert_eq!(
        compile_error("<?php\n$GLOBALS[] = 1;"),
        "Cannot append to $GLOBALS on line 2"
    );
    assert_eq!(
        compile_error("<?php\n$alias =& $GLOBALS;"),
        "Cannot acquire reference to $GLOBALS on line 2"
    );
    assert_eq!(
        compile_error("<?php\nfunction make_probe() { return function () use ($GLOBALS) {}; }",),
        "Cannot use auto-global as lexical variable on line 2"
    );

    assert_eq!(
        run_php(
            r#"<?php
$tone = "warm";
$snapshot = $GLOBALS;
$before = $snapshot["tone"];
$snapshot["tone"] = "cool";
$globals = 3;
$globals += 4;
echo $before, "|", $tone, "|", $snapshot["tone"], "|", $globals, "|";
$GLOBALS["tone"] = "bright";
$GLOBALS["palette"]["accent"] = "violet";
unset($GLOBALS["palette"]["accent"]);
unset($GLOBALS["missing"]["leaf"]);
echo $tone, "|", isset($GLOBALS["palette"]["accent"]) ? "yes" : "no", "|",
    isset($GLOBALS["missing"]) ? "yes" : "no";
"#
        ),
        "warm|warm|cool|7|bright|no|no"
    );
}

#[test]
fn globals_root_cannot_satisfy_a_reference_parameter() {
    assert_eq!(
        run_php(
            r#"<?php
function borrow_global(&$slot) {}
try {
    borrow_global($GLOBALS);
} catch (Error $error) {
    echo $error->getMessage();
}
echo "|";
try {
    borrow_later($GLOBALS);
} catch (Error $error) {
    echo $error->getMessage();
}
function borrow_later(&$future) {}
"#
        ),
        "borrow_global(): Argument #1 ($slot) could not be passed by reference|borrow_later(): Argument #1 ($future) could not be passed by reference"
    );
}

// static variable tests

#[test]
fn test_static_counter() {
    assert_eq!(
        run_php(
            r#"<?php
function counter() {
    static $count = 0;
    $count++;
    return $count;
}
echo counter() . " " . counter() . " " . counter();
"#
        ),
        "1 2 3"
    );
}

#[test]
fn test_static_multiple_vars() {
    assert_eq!(
        run_php(
            r#"<?php
function test() {
    static $a = 0, $b = 10;
    $a++;
    $b--;
    echo "$a:$b ";
}
test();
test();
"#
        ),
        "1:9 2:8 "
    );
}

#[test]
fn test_static_default_null() {
    assert_eq!(
        run_php(
            r#"<?php
function test() {
    static $x;
    if ($x === null) {
        $x = "initialized";
    }
    echo $x . " ";
    $x = "modified";
}
test();
test();
"#
        ),
        "initialized modified "
    );
}

#[test]
fn test_method_local_static_variable() {
    assert_eq!(
        run_php(
            r#"<?php
class Counter {
    public function next() {
        static $count = 0;
        return ++$count;
    }
}
$counter = new Counter();
echo $counter->next(), '|', $counter->next();
"#,
        ),
        "1|2"
    );
}

#[test]
fn recursive_call_observes_method_static_mutation_immediately() {
    assert_eq!(
        run_php(
            r#"<?php
class RecursingStatic {
    public function run() {
        static $first = true;
        echo $first ? 'first:' : 'again:';
        if ($first) {
            $first = false;
            $this->run();
        }
    }
}
(new RecursingStatic())->run();
"#,
        ),
        "first:again:"
    );
}

#[test]
fn static_initializers_are_lazy_and_recursive_initialization_commits_once() {
    assert_eq!(
        run_php(
            r#"<?php
function seed($value) {
    echo "seed:$value\n";
    return $value;
}
function lazy() {
    static $value = seed(4);
    echo "$value\n";
}
lazy();
lazy();

function recursive($depth) {
    static $value = $depth < 2 ? recursive($depth + 1) : "done";
    echo "$value\n";
    return $depth;
}
recursive(0);
"#,
        ),
        "seed:4\n4\n4\ndone\ndone\ndone\n"
    );
}

#[test]
fn duplicate_static_declarations_are_compile_errors() {
    let error =
        run_php_expect_error("<?php function invalid() { static $value = 1; static $value = 2; }");
    assert_eq!(
        error.to_string(),
        "Duplicate declaration of static variable $value on line 1"
    );
}

#[test]
fn test_destructuring_assignment_is_value_producing_and_allows_skips() {
    assert_eq!(
        run_php(
            "<?php if ([$first, , $third] = ['a', 'ignored', 'c']) { echo $first . $third . ':' . implode(',', [$first, $third]); }"
        ),
        "ac:a,c"
    );
}

#[test]
fn test_destructuring_expression_preserves_a_cv_rhs_before_overwriting_it() {
    assert_eq!(
        run_php(
            "<?php $values = ['a', 'b']; if ([$values, $second] = $values) { echo $values . $second; }"
        ),
        "ab"
    );
}

#[test]
fn reference_destructuring_aliases_positional_keyed_and_nested_elements() {
    assert_eq!(
        run_php(
            r#"<?php
$source = [10, 'inner' => [20]];
[0 => &$first, 'inner' => [&$second]] = $source;
$first = 11;
$source['inner'][0] = 21;
echo $source[0], ':', $second;
"#,
        ),
        "11:21"
    );
}

#[test]
fn reference_destructuring_preserves_rhs_when_a_target_reuses_its_name() {
    assert_eq!(
        run_php(
            r#"<?php
$values = ['left', 'right'];
[&$values, &$tail] = $values;
$values = 'changed';
$tail = 'updated';
echo $values, ':', $tail;
"#,
        ),
        "changed:updated"
    );
}

#[test]
fn reference_destructuring_tracks_reference_returning_calls_and_properties() {
    assert_eq!(
        run_php(
            r#"<?php
function &borrow_list(&$value) { return $value; }
class ReferenceHolder { public $items = [3]; }
$holder = new ReferenceHolder();
[&$fromCall] = borrow_list($holder->items);
[&$fromProperty] = $holder->items;
$fromCall = 4;
$fromProperty = 5;
echo $holder->items[0];
"#,
        ),
        "5"
    );
}

#[test]
fn foreach_reference_destructuring_mutates_only_referenced_members() {
    assert_eq!(
        run_php(
            r#"<?php
$rows = [[1, 10], [2, 20]];
foreach ($rows as [&$mutable, $copy]) {
    $mutable += 3;
    $copy += 100;
}
echo $rows[0][0], ':', $rows[0][1], '|', $rows[1][0], ':', $rows[1][1];
"#,
        ),
        "4:10|5:20"
    );
}
