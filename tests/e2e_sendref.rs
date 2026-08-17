/// Tests for pass-by-reference (SendRef) — both user functions and stdlib.
mod common;
use common::run_php;
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;

// ============================================================
// User function &$param tests
// ============================================================

#[test]
fn test_user_ref_basic() {
    let out = run_php(
        r#"<?php
function inc(&$x) { $x = $x + 1; }
$a = 10;
inc($a);
echo $a;
"#,
    );
    assert_eq!(out, "11");
}

#[test]
fn test_user_ref_string() {
    let out = run_php(
        r#"<?php
function append(&$s, $suffix) { $s = $s . $suffix; }
$str = "hello";
append($str, " world");
echo $str;
"#,
    );
    assert_eq!(out, "hello world");
}

#[test]
fn test_user_ref_swap() {
    let out = run_php(
        r#"<?php
function swap(&$a, &$b) {
    $tmp = $a;
    $a = $b;
    $b = $tmp;
}
$x = 1;
$y = 2;
swap($x, $y);
echo $x . "," . $y;
"#,
    );
    assert_eq!(out, "2,1");
}

#[test]
fn test_user_ref_array() {
    let out = run_php(
        r#"<?php
function add_elem(&$arr, $val) { $arr[] = $val; }
$a = [1, 2];
add_elem($a, 3);
echo count($a);
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn test_user_ref_with_default() {
    let out = run_php(
        r#"<?php
function maybe_inc(&$x, $amount = 1) { $x = $x + $amount; }
$a = 5;
maybe_inc($a);
echo $a . ",";
maybe_inc($a, 10);
echo $a;
"#,
    );
    assert_eq!(out, "6,16");
}

#[test]
fn test_user_ref_mixed_params() {
    // Only first param is by-ref, second is by-val
    let out = run_php(
        r#"<?php
function add_to(&$target, $val) { $target = $target + $val; }
$x = 100;
$y = 50;
add_to($x, $y);
echo $x . "," . $y;
"#,
    );
    assert_eq!(out, "150,50");
}

#[test]
fn test_user_ref_nested_calls() {
    let out = run_php(
        r#"<?php
function double(&$x) { $x = $x * 2; }
function double_twice(&$x) { double($x); double($x); }
$a = 3;
double_twice($a);
echo $a;
"#,
    );
    assert_eq!(out, "12");
}

#[test]
fn reassigned_reference_source_uses_its_value_in_scalar_operations() {
    let out = run_php(
        r#"<?php
function expose(&$value) {}
$value = 1;
expose($value);
$value = 'stable';
echo $value . '-ok';
"#,
    );
    assert_eq!(out, "stable-ok");
}

#[test]
fn array_and_property_targets_bind_to_the_source_variable() {
    let out = run_php(
        r#"<?php
$values = ['old', 'nested' => ['old']];
$source = 'first';
$result = ($values[0] = &$source);
echo $result, ':', $values[0], ':', $source, '|';
$source = 'second';
echo $values[0], ':', $source, '|';
$values[0] = 'third';
echo $values[0], ':', $source, '|';

$nested = 'nested-first';
$values['nested'][0] = &$nested;
$nested = 'nested-second';
echo $values['nested'][0], ':', $nested, '|';

class ReferenceBox { public $value = 'old'; }
$box = new ReferenceBox();
$property = 'property-first';
$box->value = &$property;
$box->value = 'property-second';
echo $box->value, ':', $property, '|';

$left = 'left';
$holder = ['right'];
$literal = [&$left, &$holder[0]];
$literal[0] = 'left-updated';
$holder[0] = 'right-updated';
echo $left, ':', $literal[0], ':', $holder[0], ':', $literal[1];
"#,
    );
    assert_eq!(
        out,
        "first:first:first|second:second|third:third|nested-second:nested-second|property-second:property-second|left-updated:left-updated:right-updated:right-updated"
    );
}

// ============================================================
// Stdlib by-ref tests — sort, array_push, array_pop, etc.
// ============================================================

#[test]
fn test_sort_modifies_caller() {
    let out = run_php(
        r#"<?php
$arr = [3, 1, 2];
sort($arr);
echo $arr[0] . "," . $arr[1] . "," . $arr[2];
"#,
    );
    assert_eq!(out, "1,2,3");
}

#[test]
fn test_rsort_modifies_caller() {
    let out = run_php(
        r#"<?php
$arr = [1, 3, 2];
rsort($arr);
echo $arr[0] . "," . $arr[1] . "," . $arr[2];
"#,
    );
    assert_eq!(out, "3,2,1");
}

#[test]
fn test_array_push_modifies_caller() {
    let out = run_php(
        r#"<?php
$arr = [1, 2];
array_push($arr, 3);
echo count($arr) . "," . $arr[2];
"#,
    );
    assert_eq!(out, "3,3");
}

#[test]
fn test_array_pop_modifies_caller() {
    let out = run_php(
        r#"<?php
$arr = [10, 20, 30];
$last = array_pop($arr);
echo $last . "," . count($arr);
"#,
    );
    assert_eq!(out, "30,2");
}

#[test]
fn test_array_shift_modifies_caller() {
    let out = run_php(
        r#"<?php
$arr = [10, 20, 30];
$first = array_shift($arr);
echo $first . "," . count($arr);
"#,
    );
    assert_eq!(out, "10,2");
}

#[test]
fn test_array_unshift_modifies_caller() {
    let out = run_php(
        r#"<?php
$arr = [2, 3];
array_unshift($arr, 1);
echo $arr[0] . "," . count($arr);
"#,
    );
    assert_eq!(out, "1,3");
}

#[test]
fn test_shuffle_modifies_caller() {
    // shuffle is random, but the array should still have 3 elements
    let out = run_php(
        r#"<?php
$arr = [1, 2, 3];
shuffle($arr);
echo count($arr);
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn test_settype_modifies_caller() {
    let out = run_php(
        r#"<?php
$x = "42";
settype($x, "integer");
echo $x + 8;
"#,
    );
    assert_eq!(out, "50");
}

#[test]
fn test_array_splice_modifies_caller() {
    let out = run_php(
        r#"<?php
$arr = [1, 2, 3, 4, 5];
array_splice($arr, 1, 2);
echo count($arr) . "," . $arr[0] . "," . $arr[1] . "," . $arr[2];
"#,
    );
    assert_eq!(out, "3,1,4,5");
}

#[test]
fn test_ref_after_multiple_calls() {
    // Verify the caller variable stays correct after multiple by-ref calls
    let out = run_php(
        r#"<?php
$arr = [];
array_push($arr, 1);
array_push($arr, 2);
array_push($arr, 3);
echo count($arr) . "," . $arr[0] . "," . $arr[1] . "," . $arr[2];
"#,
    );
    assert_eq!(out, "3,1,2,3");
}

#[test]
fn test_user_ref_return_value() {
    // Function with &$param can still return a value
    let out = run_php(
        r#"<?php
function pop_and_count(&$arr) {
    array_pop($arr);
    return count($arr);
}
$a = [1, 2, 3];
$n = pop_and_count($a);
echo $n . "," . count($a);
"#,
    );
    assert_eq!(out, "2,2");
}

#[test]
fn test_ref_parser_ampersand() {
    // Verify &$param parses correctly with variadic
    let out = run_php(
        r#"<?php
function fill(&$arr, ...$vals) {
    for ($i = 0; $i < count($vals); $i++) {
        $arr[] = $vals[$i];
    }
}
$a = [];
fill($a, 10, 20, 30);
echo count($a) . "," . $a[0] . "," . $a[2];
"#,
    );
    assert_eq!(out, "3,10,30");
}

#[test]
fn test_positional_reference_argument_can_append_a_fresh_array_slot() {
    let out = run_php(
        r#"<?php
function bind_fresh_slot(&$slot, $value) {
    $slot = $value;
}

$values = [];
$nested = ["slots" => []];
class SlotHolder { public $slots = []; }
$holder = new SlotHolder;
bind_fresh_slot($values[], "first");
bind_fresh_slot($values[], "second");
bind_fresh_slot($nested["slots"][], "nested");
bind_fresh_slot($holder->slots[], "property");
echo count($values), ":", $values[0], "|", $values[1],
     ":", $nested["slots"][0], ":", $holder->slots[0];
"#,
    );
    assert_eq!(out, "2:first|second:nested:property");
}

#[test]
fn test_positional_value_argument_still_rejects_an_empty_dimension_read() {
    let source = "<?php\nfunction observe($value) {}\nobserve($values[]);";
    let tokens = Lexer::new(source).tokenize().expect("source must lex");
    let statements = Parser::new(tokens).parse().expect("source must parse");
    let error = match Compiler::new().compile(&statements) {
        Ok(_) => panic!("value argument must not turn [] into an append target"),
        Err(error) => error.message,
    };
    assert_eq!(error, "Cannot use [] for reading on line 3");
}

#[test]
fn test_positional_reference_argument_supports_intermediate_append_dimensions() {
    let out = run_php(
        r#"<?php
function bind_fresh_slot(&$slot, $value) {
    $slot = $value;
}
function first_key() {
    global $items;
    echo count($items), ":";
    return "first";
}
function second_key() {
    echo "second:";
    return "second";
}

$items = [];
bind_fresh_slot($items[][first_key()][second_key()], "nested");
class NestedSlotHolder { public $items = []; }
$holder = new NestedSlotHolder;
bind_fresh_slot($holder->items[]["property"], "stored");
echo $items[0]["first"]["second"], ":", $holder->items[0]["property"];
"#,
    );
    assert_eq!(out, "0:second:nested:stored");
}

#[test]
fn test_append_reference_writeback_survives_a_throwing_callee() {
    let out = run_php(
        r#"<?php
function bind_then_throw(&$slot) {
    $slot = "retained";
    throw new Exception("stop");
}
class ThrowingSlotHolder { public $items = []; }
$holder = new ThrowingSlotHolder;
try {
    bind_then_throw($holder->items[]);
} catch (Exception $error) {
    echo $holder->items[0];
}
"#,
    );
    assert_eq!(out, "retained");
}

#[test]
fn test_intermediate_append_value_argument_raises_a_catchable_read_error() {
    let out = run_php(
        r#"<?php
function observe_slot($value) {}
function observed_key() {
    echo "key:";
    return "slot";
}
function call_observer($input) {
    try {
        observe_slot($input[][observed_key()]);
    } catch (Error $error) {
        echo $error->getMessage(), ":", count($input);
    }
}
call_observer([]);
"#,
    );
    assert_eq!(out, "key:Cannot use [] for reading:0");
}

// ============================================================
// Method call by-ref (SendVarEx runtime check)
// ============================================================

#[test]
fn test_method_ref_basic() {
    let out = run_php(
        r#"<?php
class Counter {
    public $val = 0;
    function increment(&$x) {
        $x = $x + 1;
    }
}
$c = new Counter();
$a = 10;
$c->increment($a);
echo $a;
"#,
    );
    assert_eq!(out, "11");
}

#[test]
fn test_method_ref_multiple() {
    let out = run_php(
        r#"<?php
class Modifier {
    function double(&$x) { $x = $x * 2; }
    function addOne(&$x) { $x = $x + 1; }
}
$m = new Modifier();
$a = 5;
$m->double($a);
$m->addOne($a);
echo $a;
"#,
    );
    assert_eq!(out, "11");
}

#[test]
fn test_method_ref_mixed_params() {
    let out = run_php(
        r#"<?php
class Math {
    function addTo(&$target, $amount) {
        $target = $target + $amount;
    }
}
$m = new Math();
$x = 100;
$m->addTo($x, 42);
echo $x;
"#,
    );
    assert_eq!(out, "142");
}

#[test]
fn runtime_resolved_method_reference_updates_property_lvalue() {
    let out = run_php(
        r#"<?php
class Collector {
    public array $calls = [];

    function collect(array &$calls): array {
        $calls['service'] = [1, true];
        return $calls;
    }

    function observe(array $calls): void {
        $calls['detached'] = true;
    }

    function run(): array {
        $returned = $this->collect($this->calls);
        $this->observe($this->calls);
        return [$this->calls, $returned];
    }
}

$collector = new Collector();
echo json_encode($collector->run());
"#,
    );
    assert_eq!(out, r#"[{"service":[1,true]},{"service":[1,true]}]"#);
}

// ============================================================
// Static call by-ref (SendVarEx)
// ============================================================

#[test]
fn test_static_ref_basic() {
    let out = run_php(
        r#"<?php
class Utils {
    static function inc(&$x) { $x = $x + 1; }
}
$a = 7;
Utils::inc($a);
echo $a;
"#,
    );
    assert_eq!(out, "8");
}

// ============================================================
// Dynamic/closure call by-ref (SendVarEx)
// ============================================================

#[test]
fn test_dynamic_call_ref() {
    let out = run_php(
        r#"<?php
function inc(&$x) { $x = $x + 1; }
$f = "inc";
$a = 20;
$f($a);
echo $a;
"#,
    );
    assert_eq!(out, "21");
}

#[test]
fn test_closure_ref() {
    let out = run_php(
        r#"<?php
$inc = function(&$x) { $x = $x + 1; };
$a = 30;
$inc($a);
echo $a;
"#,
    );
    assert_eq!(out, "31");
}
