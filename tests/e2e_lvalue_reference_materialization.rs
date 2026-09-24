mod common;

use common::run_php;

#[test]
fn nested_array_access_coalesce_reenters_the_write_path_without_repeating_keys() {
    assert_eq!(
        run_php(
            r#"<?php
class CoalesceAccess implements ArrayAccess {
    public array $data;
    public function __construct(array $data = []) { $this->data = $data; }
    public function offsetExists(mixed $key): bool {
        echo "exists:$key\n";
        return array_key_exists($key, $this->data);
    }
    public function &offsetGet(mixed $key): mixed {
        echo "get:$key\n";
        return $this->data[$key];
    }
    public function offsetSet(mixed $key, mixed $value): void {
        echo "set:$key=$value\n";
        $this->data[$key] = $value;
    }
    public function offsetUnset(mixed $key): void {}
}
$keys = 0;
function key_once(string $key): string { global $keys; ++$keys; return $key; }
$value = new CoalesceAccess(['outer' => new CoalesceAccess]);
$value[key_once('outer')][key_once('inner')] ??= 'written';
echo "keys:$keys\n";
$value[key_once('outer')][key_once('inner')] ??= 'ignored';
echo "keys:$keys\n";
"#,
        ),
        concat!(
            "exists:outer\n",
            "get:outer\n",
            "exists:inner\n",
            "get:outer\n",
            "set:inner=written\n",
            "keys:2\n",
            "exists:outer\n",
            "get:outer\n",
            "exists:inner\n",
            "get:inner\n",
            "keys:4\n",
        )
    );
}

#[test]
fn runtime_signatures_materialize_append_and_nested_dimension_references() {
    assert_eq!(
        run_php(
            r#"<?php
class ReferenceCalls {
    public function append(&$value): void { $value = 'method'; }
    public function value($value): void {}
    public static function nested(&$value): void { $value = 'static'; }
    public function __construct(&$value) { $value = 'constructor'; }
}
$seed = null;
$receiver = new ReferenceCalls($seed);
$receiver->append($append[]);
try { $receiver->value($append[]); } catch (Error $error) {
    echo $error->getMessage(), "\n";
}
ReferenceCalls::nested($static[0][1]);
new ReferenceCalls($constructed[0]);
echo json_encode([$seed, $append, $static, $constructed]), "\n";
"#,
        ),
        concat!(
            "Cannot use [] for reading\n",
            "[\"constructor\",[\"method\"],[{\"1\":\"static\"}],[\"constructor\"]]\n",
        )
    );
}

#[test]
fn known_constructor_reference_parameters_initialize_undefined_variables_silently() {
    assert_eq!(
        run_php(
            r#"<?php
class KnownConstructorReference {
    public function __construct($value, &$reference) {
        $reference = 'initialized';
    }
}
set_error_handler(function ($severity, $message) {
    echo $message, "\n";
    return true;
});
unset($byValue, $byReference);
new KnownConstructorReference($byValue, $byReference);
var_dump($byValue, $byReference);
"#,
        ),
        concat!(
            "Undefined variable $byValue\n",
            "Undefined variable $byValue\n",
            "NULL\n",
            "string(11) \"initialized\"\n",
        )
    );
}

#[test]
fn reference_assignment_rebinds_the_persistent_static_cell() {
    assert_eq!(
        run_php(
            r#"<?php
$calls = 0;
function temporary_reference_value() {
    global $calls;
    return 'value:' . ++$calls;
}
function retained_static_reference() {
    static $value;
    if (!isset($value)) {
        $value =& temporary_reference_value();
    }
    return $value;
}
function &activation_local_static_reference() {
    static $value = '1';
    $local = $value;
    $value =& $local;
    return $value;
}
set_error_handler(function ($severity, $message) {
    echo $message, "\n";
    return true;
});
echo retained_static_reference(), "\n";
echo retained_static_reference(), "\n";
echo 'calls:', $calls, "\n";
$first =& activation_local_static_reference();
$first .= '2';
echo $first, ':', activation_local_static_reference();
"#,
        ),
        concat!(
            "Only variables should be assigned by reference\n",
            "value:1\n",
            "value:1\n",
            "calls:1\n",
            "12:1",
        )
    );
}

#[test]
fn runtime_reference_selection_rejects_temporary_and_string_dimensions() {
    assert_eq!(
        run_php(
            r#"<?php
$closure = function (&$value): void {};
try { $closure([0, 1][0]); } catch (Error $error) {
    echo $error->getMessage(), "\n";
}
eval('function late_reference(&$value): void {}');
try { late_reference(chr(0)[0]); } catch (Error $error) {
    echo $error->getMessage(), "\n";
}
function by_value($value): void { echo $value, "\n"; }
function forward_value(): void { by_value(func_get_args()[0]); }
forward_value('value');
"#,
        ),
        concat!(
            "Cannot use temporary expression in write context\n",
            "Cannot create references to/from string offsets\n",
            "value\n",
        )
    );
}

#[test]
fn rejected_negative_string_offset_assignment_returns_null_and_preserves_storage() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) {
    echo "warning:$message\n";
    return true;
});
$value = '';
$result = ($value[-1] = 'x');
var_dump($result, $value);
"#,
        ),
        "warning:Illegal string offset -1\nNULL\nstring(0) \"\"\n"
    );
}
