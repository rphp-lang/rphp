/// E2E tests: foreach loops — value only, key-value, nested, break/continue, edge cases.
mod common;
use common::{run_php, run_php_with_source_context};
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::opcode::OpCode;

fn foreach_opcodes(source: &str, function_name: &str) -> Vec<OpCode> {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    result
        .functions
        .iter()
        .find(|(name, _)| name == function_name)
        .unwrap()
        .1
        .op_array
        .instructions
        .iter()
        .map(|instruction| instruction.opcode)
        .collect()
}

#[test]
fn foreach_specialization_keeps_reference_capable_targets_canonical() {
    let source = r#"<?php
function plain($values) {
    foreach ($values as $value) {}
}
function parameter(&$value, $values) {
    foreach ($values as $value) {}
}
function captured($values) {
    $value = null;
    $closure = function () use (&$value) {};
    foreach ($values as $value) {}
}
"#;

    assert!(foreach_opcodes(source, "plain").contains(&OpCode::ForeachNextPlain));
    assert!(foreach_opcodes(source, "parameter").contains(&OpCode::ForeachNext));
    assert!(foreach_opcodes(source, "captured").contains(&OpCode::ForeachNext));
}

#[test]
fn iterator_aggregate_resolves_nested_aggregates_and_generator_keys() {
    assert_eq!(
        run_php(
            r#"<?php
class GeneratorAggregate implements IteratorAggregate {
    public function getIterator(): Traversable {
        yield 'first' => 10;
        yield 'second' => 20;
    }
}
class NestedAggregate implements IteratorAggregate {
    public function getIterator(): Traversable {
        return new GeneratorAggregate();
    }
}
foreach (new NestedAggregate() as $key => $value) {
    echo $key, ':', $value, '|';
}
"#,
        ),
        "first:10|second:20|"
    );
}

#[test]
fn iterator_aggregate_rejects_non_traversable_results_with_foreach_origin() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class InvalidAggregateResult implements IteratorAggregate {
    #[ReturnTypeWillChange]
    public function getIterator() {
        echo 'called|';
        return 42;
    }
}
try {
    foreach (new InvalidAggregateResult() as $value) {}
} catch (Exception $error) {
    echo $error->getMessage(), '|', $error->getLine();
}
"#,
            "/virtual/iterator-aggregate.php",
            "/virtual",
        ),
        "called|Objects returned by InvalidAggregateResult::getIterator() must be traversable or implement interface Iterator|10"
    );
}

#[test]
fn user_iterator_protocol_preserves_order_exceptions_aggregates_and_by_ref_error() {
    assert_eq!(
        run_php(
            r#"<?php
class TraceIterator implements Iterator {
    public int $position = 0;
    public function __construct(public ?string $trap = null) {}
    private function visit(string $method): void {
        echo $method[0];
        if ($this->trap === $method) throw new Exception($method);
    }
    public function rewind(): void { $this->visit('rewind'); $this->position = 0; }
    public function valid(): bool { $this->visit('valid'); return $this->position < 2; }
    public function current(): mixed { $this->visit('current'); return $this->position + 10; }
    public function key(): mixed { $this->visit('key'); return 'k' . $this->position; }
    public function next(): void { $this->visit('next'); $this->position++; }
}
foreach (['rewind', 'valid', 'current', 'key', 'next', null] as $trap) {
    try {
        foreach (new TraceIterator($trap) as $key => $value) echo "=$key:$value;";
    } catch (Exception $error) {
        echo '!' . $error->getMessage();
    }
    echo '|';
}
class TraceAggregate implements IteratorAggregate {
    public function getIterator(): Traversable { echo 'G'; return new TraceIterator(); }
}
foreach (new TraceAggregate() as $key => $value) { echo "=$key:$value;"; }
try {
    $iterator = new TraceIterator();
    foreach ($iterator as &$value) {}
} catch (Error $error) {
    echo '|' . $error->getMessage();
}
"#
        ),
        "r!rewind|rv!valid|rvc!current|rvck!key|rvck=k0:10;n!next|rvck=k0:10;nvck=k1:11;nv|Grvck=k0:10;nvck=k1:11;nv|An iterator cannot be used with foreach by reference"
    );
}

#[test]
fn iterator_protocol_callbacks_share_live_globals_with_the_suspended_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class GlobalIterator implements Iterator {
    private int $position = 0;
    public function rewind(): void { global $indent; echo "R$indent|"; $indent .= 'r'; $this->position = 0; }
    public function valid(): bool { global $indent; echo "V$indent|"; return $this->position < 1; }
    public function current(): mixed { global $indent; return $indent; }
    public function key(): mixed { return 0; }
    public function next(): void { $this->position++; }
}
$indent = 'a';
foreach (new GlobalIterator() as $value) echo "=$value|";
$indent = 'b';
foreach (new GlobalIterator() as $value) echo "=$value|";
echo "M$indent";
"#,
        ),
        "Ra|Var|=ar|Var|Rb|Vbr|=br|Vbr|Mbr"
    );
}

#[test]
fn iterator_aggregate_generator_closure_preserves_lexical_visibility_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class ScopedGeneratorAggregate implements IteratorAggregate {
    private $factory;
    public function __construct($factory) {
        $this->factory = $factory;
    }
    public function getIterator(): Traversable {
        $factory = $this->factory;
        return $factory();
    }
}
class ScopedGeneratorOwner {
    protected function load(): string {
        return 'visible';
    }
    public static function aggregate(self $owner): IteratorAggregate {
        return new ScopedGeneratorAggregate(function () use ($owner) {
            yield $owner->load();
        });
    }
}
foreach (ScopedGeneratorOwner::aggregate(new ScopedGeneratorOwner()) as $value) {
    echo $value;
}
"#,
        ),
        "visible"
    );
}

// === Basic foreach ($arr as $val) ===

#[test]
fn test_e2e_foreach_value_only() {
    assert_eq!(
        run_php("<?php $a = [10, 20, 30]; foreach ($a as $v) { echo $v; }"),
        "102030"
    );
}

#[test]
fn test_e2e_foreach_string_values() {
    assert_eq!(
        run_php("<?php $a = ['hello', 'world']; foreach ($a as $v) { echo $v . ' '; }"),
        "hello world "
    );
}

// === foreach ($arr as $key => $val) ===

#[test]
fn test_e2e_foreach_key_value() {
    assert_eq!(
        run_php(
            "<?php $a = ['a' => 1, 'b' => 2, 'c' => 3]; foreach ($a as $k => $v) { echo $k . $v; }"
        ),
        "a1b2c3"
    );
}

#[test]
fn test_e2e_foreach_int_keys() {
    assert_eq!(
        run_php("<?php $a = [10, 20, 30]; foreach ($a as $k => $v) { echo $k . ':' . $v . ' '; }"),
        "0:10 1:20 2:30 "
    );
}

#[test]
fn test_e2e_foreach_assigns_keys_and_values_to_object_properties() {
    assert_eq!(
        run_php(
            "<?php class ForeachCursor { public $key; public $value; } $cursor = new ForeachCursor(); foreach (['a' => 1, 'b' => 2] as $cursor->key => $cursor->value) { echo $cursor->key, $cursor->value; } echo '|', $cursor->key, $cursor->value;"
        ),
        "a1b2|b2"
    );
}

// === Empty array ===

#[test]
fn test_e2e_foreach_empty_array() {
    assert_eq!(
        run_php("<?php $a = []; foreach ($a as $v) { echo $v; } echo 'done';"),
        "done"
    );
}

// === foreach with break ===

#[test]
fn test_e2e_foreach_break() {
    assert_eq!(
        run_php(
            "<?php $a = [1, 2, 3, 4, 5]; foreach ($a as $v) { if ($v == 3) { break; } echo $v; }"
        ),
        "12"
    );
}

// === foreach with continue ===

#[test]
fn test_e2e_foreach_continue() {
    assert_eq!(
        run_php(
            "<?php $a = [1, 2, 3, 4, 5]; foreach ($a as $v) { if ($v == 3) { continue; } echo $v; }"
        ),
        "1245"
    );
}

// === foreach preserves insertion order ===

#[test]
fn test_e2e_foreach_order_preserved() {
    assert_eq!(
        run_php(
            "<?php $a = ['z' => 1, 'a' => 2, 'm' => 3]; $r = ''; foreach ($a as $k => $v) { $r .= $k; } echo $r;"
        ),
        "zam"
    );
}

// === foreach with mixed keys ===

#[test]
fn test_e2e_foreach_mixed_keys() {
    assert_eq!(
        run_php(
            "<?php $a = [0 => 'x', 'name' => 'y', 1 => 'z']; foreach ($a as $k => $v) { echo $k . $v; }"
        ),
        "0xnamey1z"
    );
}

// === foreach accumulator ===

#[test]
fn test_e2e_foreach_sum() {
    assert_eq!(
        run_php(
            "<?php $a = [1, 2, 3, 4, 5]; $sum = 0; foreach ($a as $v) { $sum += $v; } echo $sum;"
        ),
        "15"
    );
}

// === foreach with array() syntax ===

#[test]
fn test_e2e_foreach_array_syntax() {
    assert_eq!(
        run_php("<?php $a = array('x', 'y', 'z'); foreach ($a as $v) { echo $v; }"),
        "xyz"
    );
}

// === Nested foreach ===

#[test]
fn test_e2e_foreach_nested() {
    assert_eq!(
        run_php(
            "<?php $a = [[1, 2], [3, 4]]; foreach ($a as $row) { foreach ($row as $v) { echo $v; } }"
        ),
        "1234"
    );
}

// === foreach with function result ===

#[test]
fn test_e2e_foreach_function_result() {
    assert_eq!(
        run_php(
            "<?php function nums() { return [10, 20, 30]; } foreach (nums() as $v) { echo $v; }"
        ),
        "102030"
    );
}

// === foreach doesn't modify original (copy semantics) ===

#[test]
fn test_e2e_foreach_copy_semantics() {
    // PHP foreach iterates over a copy of the array
    assert_eq!(
        run_php(
            "<?php $a = [1, 2, 3]; foreach ($a as $v) { $a[] = $v * 10; } echo $a[0]; echo $a[3]; echo $a[4]; echo $a[5];"
        ),
        "1102030"
    );
}

#[test]
fn test_e2e_foreach_by_reference_writes_values_back() {
    assert_eq!(
        run_php(
            "<?php $values = [1, 2, 3]; foreach ($values as &$value) { $value += 10; } echo $values[0], ',', $values[1], ',', $values[2];"
        ),
        "11,12,13"
    );
}

#[test]
fn foreach_by_reference_observes_live_appends_and_element_overwrites() {
    assert_eq!(
        run_php(
            r#"<?php
$values = [1, 2, 3];
foreach ($values as $key => &$value) {
    echo "$key:$value|";
    if ($value == 2) {
        $values[] = 4;
    }
    if ($value == 3) {
        $values[$key] = 30;
    } else {
        $value *= 10;
    }
}
echo $values[0], ',', $values[1], ',', $values[2], ',', $values[3];
"#
        ),
        "0:1|1:2|2:3|3:4|10,20,30,40"
    );
}

#[test]
fn foreach_by_reference_accepts_a_temporary_array() {
    assert_eq!(
        run_php("<?php foreach ([1, 2, 3] as &$value) { $value *= 2; echo $value; }"),
        "246"
    );
}

#[test]
fn foreach_element_references_are_transparent_to_array_identity() {
    assert_eq!(
        run_php(
            "<?php $values = [1, 2]; foreach ($values as &$value) { var_dump($values === [1, 2]); }"
        ),
        "bool(true)\nbool(true)\n"
    );
}

#[test]
fn test_e2e_foreach_by_reference_flushes_break_value() {
    assert_eq!(
        run_php(
            "<?php $values = [1, 2, 3]; foreach ($values as &$value) { $value *= 2; if ($value == 4) { break; } } echo $values[0], ',', $values[1], ',', $values[2];"
        ),
        "2,4,3"
    );
}

#[test]
fn test_e2e_foreach_by_reference_nested_object_array() {
    assert_eq!(
        run_php(
            "<?php class Store { public $groups = [[1, 2], [3]]; } $store = new Store(); foreach ($store->groups[0] as &$value) { $value += 5; } echo $store->groups[0][0], ',', $store->groups[0][1], ',', $store->groups[1][0];"
        ),
        "6,7,3"
    );
}

#[test]
fn by_value_foreach_updates_a_reference_parameter() {
    assert_eq!(
        run_php(
            r#"<?php
function overwrite(&$slot) {
    foreach (['after'] as $slot) {
    }
}

$value = 'before';
overwrite($value);
echo $value;
"#,
        ),
        "after"
    );
}

#[test]
fn test_e2e_nested_object_array_append() {
    assert_eq!(
        run_php(
            "<?php class Store { public $listeners = []; } $store = new Store(); $store->listeners['event'][10][] = 'first'; $store->listeners['event'][10][] = 'second'; echo $store->listeners['event'][10][0], ',', $store->listeners['event'][10][1];"
        ),
        "first,second"
    );
}

#[test]
fn test_e2e_bind_appended_nested_array_element_reference() {
    assert_eq!(
        run_php(
            "<?php class Store { public $items = []; } function build() { $store = new Store(); $slot = &$store->items['group'][]; $store->items['group'][] = 'next'; $slot = 'bound'; return $store->items; } $items = build(); echo $items['group'][0], ',', $items['group'][1];"
        ),
        "bound,next"
    );
}

#[test]
fn binding_an_appended_element_rebinds_a_reference_parameter_locally() {
    assert_eq!(
        run_php(
            r#"<?php
function appendThrough(&$slot, &$items) {
    $slot = &$items[];
    $slot = 'new';
}

$outside = 'old';
$items = [];
appendThrough($outside, $items);
echo $outside, '|', $items[0];
"#,
        ),
        "old|new"
    );
}

#[test]
fn compiler_append_reference_cvs_do_not_create_visible_aliases() {
    assert_eq!(
        run_php(
            r#"<?php
function setReference(&$value) {
    $value = 1;
}

$flat = [];
setReference($flat[]);
var_dump($flat);

$nested = [];
setReference($nested[][0]);
var_dump($nested);"#
        ),
        "array(1) {\n  [0]=>\n  int(1)\n}\narray(1) {\n  [0]=>\n  array(1) {\n    [0]=>\n    int(1)\n  }\n}\n"
    );
}

#[test]
fn by_reference_foreach_rebinds_lazy_listener_captures() {
    assert_eq!(
        run_php(
            r#"<?php
final class Listener {
    public function __construct(private string $name) {}
    public function invoke(object $event): void { echo $this->name, "\n"; }
}

final class Dispatcher {
    private array $listeners = [];
    private array $optimized;

    public function __construct() {
        $this->optimized = [];
        $this->listeners['event'][32][] = [fn () => new Listener('one'), 'invoke'];
        $this->listeners['event'][16][] = [fn () => new Listener('two'), 'invoke'];
        $this->listeners['event'][100][] = [fn () => new Listener('three'), 'invoke'];
    }

    public function dispatch(object $event, string $eventName): void {
        $listeners = $this->optimized[$eventName]
            ?? $this->optimizeListeners($eventName);
        foreach ($listeners as $listener) {
            $listener($event, $eventName, $this);
        }
    }

    private function optimizeListeners(string $eventName): array {
        krsort($this->listeners[$eventName]);
        $this->optimized[$eventName] = [];
        foreach ($this->listeners[$eventName] as &$listeners) {
            foreach ($listeners as &$listener) {
                $closure = &$this->optimized[$eventName][];
                $closure = static function (...$args) use (&$listener, &$closure): void {
                    if ($listener[0] instanceof Closure) {
                        $listener[0] = $listener[0]();
                    }
                    ($closure = $listener(...))(...$args);
                };
            }
        }
        return $this->optimized[$eventName];
    }
}

(new Dispatcher())->dispatch(new stdClass(), 'event');
"#,
        ),
        "three\none\ntwo\n",
    );
}

#[test]
fn test_e2e_by_reference_builtin_writes_nested_lvalue_back() {
    assert_eq!(
        run_php(
            "<?php class Store { public $groups = ['event' => [-10 => 'low', 10 => 'high']]; } $store = new Store(); krsort($store->groups['event']); foreach ($store->groups['event'] as $value) { echo $value, '>'; }"
        ),
        "high>low>"
    );
}

// === foreach with sparse array ===

#[test]
fn test_e2e_foreach_sparse() {
    assert_eq!(
        run_php(
            "<?php $a = []; $a[0] = 'a'; $a[5] = 'b'; $a[100] = 'c'; foreach ($a as $k => $v) { echo $k . ':' . $v . ' '; }"
        ),
        "0:a 5:b 100:c "
    );
}

// === foreach single element ===

#[test]
fn test_e2e_foreach_single_element() {
    assert_eq!(
        run_php("<?php $a = [42]; foreach ($a as $v) { echo $v; }"),
        "42"
    );
}

// === foreach with break 2 from nested ===

#[test]
fn test_e2e_foreach_break_2_nested() {
    assert_eq!(
        run_php(
            "<?php $a = [[1, 2], [3, 4], [5, 6]]; foreach ($a as $row) { foreach ($row as $v) { if ($v == 3) { break 2; } echo $v; } }"
        ),
        "12"
    );
}

// === foreach with continue in nested ===

#[test]
fn test_e2e_foreach_continue_nested() {
    assert_eq!(
        run_php(
            "<?php $a = [1, 2, 3]; $b = ['a', 'b']; foreach ($a as $n) { foreach ($b as $l) { if ($n == 2 && $l == 'a') { continue; } echo $n . $l; } }"
        ),
        "1a1b2b3a3b"
    );
}

// === foreach building new array ===

#[test]
fn test_e2e_foreach_build_array() {
    assert_eq!(
        run_php(
            "<?php $a = [1, 2, 3]; $b = []; foreach ($a as $v) { $b[] = $v * $v; } echo $b[0]; echo $b[1]; echo $b[2];"
        ),
        "149"
    );
}

// === foreach with key-value on int-keyed array ===

#[test]
fn test_e2e_foreach_int_key_value_sum() {
    assert_eq!(
        run_php(
            "<?php $a = [10, 20, 30]; $sum = 0; foreach ($a as $k => $v) { $sum += $k + $v; } echo $sum;"
        ),
        "63"
    );
}

// === CR11 regression: foreach on non-array emits warning ===

#[test]
fn test_e2e_foreach_int_warns() {
    assert_eq!(
        run_php("<?php foreach (42 as $v) { echo $v; } echo 'after';"),
        "\nWarning: foreach() argument must be of type array|object, int given in <main> on line 1\nafter"
    );
}

#[test]
fn test_e2e_foreach_null_warns() {
    assert_eq!(
        run_php("<?php foreach (null as $v) { echo $v; } echo 'after';"),
        "\nWarning: foreach() argument must be of type array|object, null given in <main> on line 1\nafter"
    );
}

#[test]
fn test_e2e_foreach_string_warns() {
    assert_eq!(
        run_php("<?php foreach ('abc' as $v) { echo $v; } echo 'after';"),
        "\nWarning: foreach() argument must be of type array|object, string given in <main> on line 1\nafter"
    );
}

#[test]
fn test_e2e_foreach_bool_warns() {
    assert_eq!(
        run_php("<?php foreach (true as $v) { echo $v; } echo 'after';"),
        "\nWarning: foreach() argument must be of type array|object, bool given in <main> on line 1\nafter"
    );
}

#[test]
fn test_e2e_quick_foreach_declared_object_property_accumulation() {
    assert_eq!(
        run_php(
            "<?php
class QuickForeachRow {
    public $value;
    public $name;
    public function __construct($value, $name) {
        $this->value = $value;
        $this->name = $name;
    }
}
$rows = [];
for ($i = 0; $i < 64; $i++) {
    $rows[] = new QuickForeachRow(($i % 4) + 1, 'alpha');
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"
        ),
        "480|4|alpha"
    );
}

#[test]
fn test_e2e_quick_foreach_dynamic_object_property_accumulation_on_hash_array() {
    assert_eq!(
        run_php(
            "<?php
$rows = [];
for ($i = 0; $i < 64; $i++) {
    $rows[$i * 3] = json_decode('{\"value\":11,\"name\":\"alpha\"}');
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"
        ),
        "1024|11|alpha"
    );
}

#[test]
fn test_e2e_quick_foreach_dynamic_property_pair_handles_mixed_insertion_order() {
    assert_eq!(
        run_php(
            "<?php
$rows = [];
for ($i = 0; $i < 64; $i++) {
    if (($i % 2) == 0) {
        $rows[] = json_decode('{\"value\":11,\"name\":\"alpha\"}');
    } else {
        $rows[] = json_decode('{\"name\":\"alpha\",\"value\":11}');
    }
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"
        ),
        "1024|11|alpha"
    );
}

#[test]
fn test_e2e_quick_foreach_dynamic_property_pair_handles_linear_storage() {
    assert_eq!(
        run_php(
            "<?php
$rows = [];
for ($i = 0; $i < 64; $i++) {
    $rows[] = json_decode('{\"value\":11,\"name\":\"alpha\",\"x\":1,\"y\":2}');
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"
        ),
        "1024|11|alpha"
    );
}

#[test]
fn test_e2e_quick_foreach_dynamic_property_pair_handles_indexed_storage() {
    assert_eq!(
        run_php(
            "<?php
$rows = [];
for ($i = 0; $i < 64; $i++) {
    $rows[] = json_decode('{\"value\":11,\"name\":\"alpha\",\"x\":1,\"y\":2,\"a\":3,\"b\":4,\"c\":5,\"d\":6,\"e\":7}');
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"
        ),
        "1024|11|alpha"
    );
}

#[test]
fn test_e2e_quick_foreach_indexed_property_pair_handles_mixed_insertion_order() {
    assert_eq!(
        run_php(
            "<?php
$rows = [];
for ($i = 0; $i < 64; $i++) {
    if (($i % 2) == 0) {
        $rows[] = json_decode('{\"value\":11,\"name\":\"alpha\",\"x\":1,\"y\":2,\"a\":3,\"b\":4,\"c\":5,\"d\":6,\"e\":7}');
    } else {
        $rows[] = json_decode('{\"e\":7,\"d\":6,\"c\":5,\"b\":4,\"a\":3,\"y\":2,\"x\":1,\"name\":\"alpha\",\"value\":11}');
    }
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"
        ),
        "1024|11|alpha"
    );
}

#[test]
fn test_e2e_dynamic_property_cache_survives_linear_to_indexed_promotion() {
    assert_eq!(
        run_php(
            "<?php
$row = json_decode('{\"value\":11,\"name\":\"alpha\",\"x\":1,\"y\":2}');
$sum = 0;
for ($i = 0; $i < 80; $i++) {
    $sum += $row->value + strlen($row->name);
    if ($i == 40) {
        $row->a = 5;
        $row->b = 6;
        $row->c = 7;
        $row->d = 8;
        $row->e = 9;
        $row->value = 13;
    }
}
echo $sum . '|' . $row->value . '|' . $row->name . '|' . $row->e;
"
        ),
        "1358|13|alpha|9"
    );
}

#[test]
fn test_e2e_quick_foreach_dynamic_second_projection_side_exit_is_exact() {
    assert_eq!(
        run_php(
            "<?php
$rows = [];
for ($i = 0; $i < 64; $i++) {
    if ($i == 40) {
        $rows[] = json_decode('{\"value\":11,\"name\":5}');
    } else {
        $rows[] = json_decode('{\"value\":11,\"name\":\"alpha\"}');
    }
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"
        ),
        "1020|11|alpha"
    );
}

#[test]
fn test_e2e_quick_foreach_object_class_guard_side_exit() {
    assert_eq!(
        run_php(
            "<?php
class QuickForeachLeft { public $value = 2; public $name = 'x'; }
class QuickForeachRight { public $value = 4; public $name = 'x'; }
$rows = [];
for ($i = 0; $i < 70; $i++) {
    if (($i % 2) == 0) {
        $rows[] = new QuickForeachLeft();
    } else {
        $rows[] = new QuickForeachRight();
    }
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value;
"
        ),
        "280|4"
    );
}

#[test]
fn test_e2e_quick_foreach_single_long_property_projection() {
    assert_eq!(
        run_php(
            "<?php
class QuickForeachLongRow {
    public $value;
    public function __construct($value) { $this->value = $value; }
}
$rows = [];
for ($i = 0; $i < 64; $i++) {
    $rows[] = new QuickForeachLongRow(($i % 4) + 1);
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value;
}
echo $sum . '|' . $row->value;
"
        ),
        "160|4"
    );
}

#[test]
fn test_e2e_quick_foreach_property_type_side_exit() {
    assert_eq!(
        run_php(
            "<?php
class QuickForeachMixedValueRow {
    public $value;
    public function __construct($value) { $this->value = $value; }
}
$rows = [];
for ($i = 0; $i < 64; $i++) {
    if ($i == 40) {
        $rows[] = new QuickForeachMixedValueRow(1.5);
    } else {
        $rows[] = new QuickForeachMixedValueRow(1);
    }
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value;
}
echo $sum;
"
        ),
        "64.5"
    );
}

#[test]
fn foreach_accepts_legacy_list_destructuring() {
    assert_eq!(
        run_php(
            "<?php $rows = [[1, 2], [3, 4]]; foreach ($rows as list($a, $b)) echo $a + $b; foreach ($rows as $key => list($a,)) echo $key . $a;"
        ),
        "370113"
    );
}

#[test]
fn foreach_rejects_list_keys_and_empty_lists() {
    for (source, expected) in [
        (
            "<?php foreach ([[1]] as list($key) => list($value)) {}",
            "Cannot use list as key element",
        ),
        (
            "<?php foreach ([[1]] as $key => list()) {}",
            "Cannot use empty list",
        ),
    ] {
        let error = common::run_php_expect_error(source);
        assert!(format!("{error:?}").contains(expected));
    }
}

#[test]
fn foreach_iterates_visible_object_properties_and_updates_them_by_reference() {
    assert_eq!(
        run_php(
            r#"<?php
class ObjectForeachParent {
    private $shadow = 'parent';
    protected $guarded = 2;
    public $open = 3;
    public function values(): void {
        foreach ($this as $key => $value) echo "$key=$value;";
    }
    public function update(): void {
        foreach ($this as $key => &$value) if (is_int($value)) $value *= 10;
        unset($value);
    }
}
#[AllowDynamicProperties]
class ObjectForeachChild extends ObjectForeachParent {
    public $shadow = 'child';
    public $child = 4;
}
$object = new ObjectForeachChild;
$object->{12} = 5;
foreach ($object as $key => $value) echo "$key=$value;";
echo '|';
$object->values();
echo '|';
$object->update();
$after = get_object_vars($object);
echo $object->open, ':', $object->child, ':', $after[12];
"#,
        ),
        concat!(
            "open=3;shadow=child;child=4;12=5;|",
            "shadow=parent;guarded=2;open=3;child=4;12=5;|",
            "30:40:50",
        )
    );
}

#[test]
fn foreach_by_reference_rejects_readonly_object_properties() {
    let error = common::run_php_expect_error(
        r#"<?php
readonly class ReadonlyObjectForeach {
    public int $value;
    public function __construct() { $this->value = 1; }
    public function update(): void { foreach ($this as &$value) {} }
}
(new ReadonlyObjectForeach)->update();
"#,
    );
    assert!(
        format!("{error:?}").contains(
            "Cannot acquire reference to readonly property ReadonlyObjectForeach::$value"
        )
    );
}

#[test]
fn inherited_private_property_and_dynamic_shadow_keep_distinct_storage() {
    assert_eq!(
        run_php(
            r#"<?php
class ParentPrivateCollision {
    private $value = 'parent';
    public function inspect(): void {
        echo get_object_vars($this)['value'], '|';
        foreach ($this as &$item) $item .= '!';
        unset($item);
        echo $this->value, '|';
    }
}
#[AllowDynamicProperties]
class ChildPrivateCollision extends ParentPrivateCollision {}
$object = new ChildPrivateCollision;
$object->value = 'dynamic';
echo $object->value, '|', get_object_vars($object)['value'], '|';
$object->inspect();
echo $object->value, '|', isset($object->value) ? 'set' : 'unset', '|';
unset($object->value);
echo isset($object->value) ? 'set' : 'unset', '|';
$alias =& $object->value;
$alias = 'reference';
echo $object->value;
"#,
        ),
        "dynamic|dynamic|parent|parent!|dynamic|set|unset|reference"
    );
}
