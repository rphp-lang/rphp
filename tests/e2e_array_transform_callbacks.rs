mod common;
use common::run_php;

#[test]
fn array_column_projects_values_whole_rows_and_index_keys() {
    let output = run_php(
        r#"<?php
$rows = [
    ['name' => 'alpha', 'id' => 'same'],
    ['name' => 'beta', 'id' => 'same'],
    ['name' => 'gamma'],
    ['other' => 4, 'id' => 'ignored'],
];
$selected = array_column($rows, 'name', 'id');
echo $selected['same'], ':', $selected[0], ':', count($selected), "\n";

$whole = array_column([1, 'two', null, [42 => 'answer']], null);
echo get_debug_type($whole[0]), ':', $whole[1], ':', get_debug_type($whole[2]), ':';
echo array_column([$whole[3]], '42')[0], "\n";

$named = array_column(array: [['v' => 1, 'k' => 'a']], column_key: 'v', index_key: 'k');
echo $named['a'], ':';
echo array_column([['v' => 2]], 'v', null)[0];
"#,
    );
    assert_eq!(output, "beta:gamma:2\nint:two:null:answer\n1:2");
}

#[test]
fn array_column_uses_public_and_guarded_magic_object_properties() {
    let output = run_php(
        r#"<?php
class ColumnRow {
    public string $name;
    private string $id;
    public array $log = [];

    public function __construct(string $name, string $id) {
        $this->name = $name;
        $this->id = $id;
    }
    public function __isset(string $property): bool {
        $this->log[] = "isset:$property";
        return $property === 'id';
    }
    public function __get(string $property): mixed {
        $this->log[] = "get:$property";
        return $this->id;
    }
}
$first = new ColumnRow('alpha', 'a');
$second = new ColumnRow('beta', 'b');
$result = array_column([$first, $second], 'name', 'id');
echo $result['a'], ':', $result['b'], "\n";
echo implode(',', $first->log), "\n";
var_dump(array_column([$first], 'missing'));
echo implode(',', $first->log);
"#,
    );
    assert_eq!(
        output,
        "alpha:beta\nisset:id,get:id\narray(0) {\n}\nisset:id,get:id,isset:missing"
    );
}

#[test]
fn array_column_preserves_property_hook_and_exception_order() {
    let output = run_php(
        r#"<?php
class HookColumnRow {
    public int $value { get { echo "hook\n"; return 9; } }
}
var_dump(array_column([new HookColumnRow()], 'value'));

class ThrowingColumnRow {
    public function __isset(string $name): bool { echo "isset:$name\n"; return true; }
    public function __get(string $name): mixed {
        echo "get:$name\n";
        throw new Exception('stop');
    }
}
try { array_column([new ThrowingColumnRow(), new ThrowingColumnRow()], 'value'); }
catch (Exception $error) { echo "caught:", $error->getMessage(); }
"#,
    );
    assert_eq!(
        output,
        "hook\narray(1) {\n  [0]=>\n  int(9)\n}\nisset:value\nget:value\ncaught:stop"
    );
}

#[test]
fn array_column_enforces_php_weak_and_strict_selector_types() {
    let weak = run_php(
        r#"<?php
$rows = [[0 => 'zero', 1 => 'one']];
echo array_column($rows, false)[0], ':', array_column($rows, true)[0], ':';
echo array_column($rows, 1.0)[0], "\n";
try { array_column(null, 'x'); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { array_column([], []); } catch (TypeError $e) { echo $e->getMessage(); }
"#,
    );
    assert_eq!(
        weak,
        "zero:one:one\narray_column(): Argument #1 ($array) must be of type array, null given\narray_column(): Argument #2 ($column_key) must be of type string|int|null, array given"
    );

    let strict = run_php(
        r#"<?php
declare(strict_types=1);
foreach ([false, 1.0] as $key) {
    try { array_column([[0 => 'x']], $key); }
    catch (TypeError $e) { echo $e->getMessage(), "\n"; }
}
try { array_column([['x' => 1]], 'x', false); }
catch (TypeError $e) { echo $e->getMessage(); }
"#,
    );
    assert_eq!(
        strict,
        "array_column(): Argument #2 ($column_key) must be of type string|int|null, false given\narray_column(): Argument #2 ($column_key) must be of type string|int|null, float given\narray_column(): Argument #3 ($index_key) must be of type string|int|null, false given"
    );
}

#[test]
fn null_array_map_retains_reference_cells_and_checks_array_arguments() {
    let output = run_php(
        r#"<?php
$value = 1;
$source = ['key' => &$value];
$single = array_map(null, $source);
$single['key'] = 4;
echo $value, ':', $source['key'], "\n";

$multiple = array_map(null, $source, $source);
$multiple[0][0] = 7;
echo $value, ':', $multiple[0][1], "\n";

try { array_map(fn($item) => $item, null); }
catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { array_map(null, [1], null); }
catch (TypeError $e) { echo $e->getMessage(); }
"#,
    );
    assert_eq!(
        output,
        "4:4\n7:7\narray_map(): Argument #2 ($array) must be of type array, null given\narray_map(): Argument #3 ($arrays) must be of type array, null given"
    );
}

#[test]
fn array_map_reports_specific_invalid_callback_reasons() {
    let output = run_php(
        r#"<?php
class MapVisibility {
    private static function hidden($value) { return $value; }
}
$callbacks = [
    '',
    [],
    ['MissingMapClass', 'run'],
    ['MapVisibility', 'missing'],
    ['MapVisibility', 'hidden'],
    [null, 'run'],
    42,
];
foreach ($callbacks as $callback) {
    try { array_map($callback, [1]); }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
"#,
    );
    assert_eq!(
        output,
        concat!(
            "array_map(): Argument #1 ($callback) must be a valid callback or null, function \"\" not found or invalid function name\n",
            "array_map(): Argument #1 ($callback) must be a valid callback or null, array callback must have exactly two members\n",
            "array_map(): Argument #1 ($callback) must be a valid callback or null, class \"MissingMapClass\" not found\n",
            "array_map(): Argument #1 ($callback) must be a valid callback or null, class MapVisibility does not have a method \"missing\"\n",
            "array_map(): Argument #1 ($callback) must be a valid callback or null, cannot access private method MapVisibility::hidden()\n",
            "array_map(): Argument #1 ($callback) must be a valid callback or null, first array member is not a valid class name or object\n",
            "array_map(): Argument #1 ($callback) must be a valid callback or null, no array or string given\n",
        )
    );
}
