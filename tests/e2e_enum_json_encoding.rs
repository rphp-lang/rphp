mod common;

use common::run_php;

#[test]
fn enum_json_defaults_distinguish_unit_and_backed_cases() {
    assert_eq!(
        run_php(
            r#"<?php
enum UnitState { case Ready; }
enum IntState: int { case Zero = 0; }
enum TextState: string { case Ready = 'ready'; }

foreach ([UnitState::Ready, IntState::Zero, TextState::Ready] as $case) {
    var_dump(json_encode($case));
    echo json_last_error(), ':', json_last_error_msg(), "\n";
}
"#,
        ),
        concat!(
            "bool(false)\n",
            "11:Non-backed enums have no default serialization\n",
            "string(1) \"0\"\n",
            "0:No error\n",
            "string(7) \"\"ready\"\"\n",
            "0:No error\n",
        )
    );
}

#[test]
fn enum_json_throw_and_partial_flags_preserve_php_error_state_ordering() {
    assert_eq!(
        run_php(
            r#"<?php
enum UnitState { case Ready; }
enum IntState: int { case Zero = 0; }

json_encode(UnitState::Ready);
echo 'failed=', json_last_error(), ';';
echo 'throw-success=', json_encode(IntState::Zero, JSON_THROW_ON_ERROR), ':', json_last_error(), ';';
try {
    json_encode(UnitState::Ready, JSON_THROW_ON_ERROR);
} catch (JsonException $error) {
    echo 'throw-error=', $error->getCode(), ':', $error->getMessage(), ':', json_last_error(), ';';
}
echo 'partial=', json_encode(
    [IntState::Zero, UnitState::Ready],
    JSON_THROW_ON_ERROR | JSON_PARTIAL_OUTPUT_ON_ERROR,
), ':', json_last_error(), ';';
echo 'reset=', json_encode(['ok' => true]), ':', json_last_error_msg();
"#,
        ),
        concat!(
            "failed=11;",
            "throw-success=0:11;",
            "throw-error=11:Non-backed enums have no default serialization:11;",
            "partial=[0,0]:11;",
            "reset={\"ok\":true}:No error",
        )
    );
}

#[test]
fn json_serializable_precedes_enum_default_and_keeps_callback_order() {
    assert_eq!(
        run_php(
            r#"<?php
final class Trace { public static $events = []; }
enum UnitState { case Ready; }
enum CustomState implements JsonSerializable {
    case Ready;
    public function jsonSerialize(): mixed {
        Trace::$events[] = 'custom:' . $this->name;
        return ['name' => $this->name];
    }
}
enum NestedState implements JsonSerializable {
    case Ready;
    public function jsonSerialize(): mixed {
        Trace::$events[] = 'nested';
        return UnitState::Ready;
    }
}

echo json_encode([CustomState::Ready, NestedState::Ready], JSON_PARTIAL_OUTPUT_ON_ERROR), ';';
echo implode(',', Trace::$events), ';', json_last_error_msg();
"#,
        ),
        "[{\"name\":\"Ready\"},0];custom:Ready,nested;Non-backed enums have no default serialization"
    );
}

#[test]
fn repeated_json_serializable_enum_calls_release_the_recursion_guard() {
    assert_eq!(
        run_php(
            r#"<?php
enum CustomState implements JsonSerializable {
    case Ready;
    public function jsonSerialize(): mixed { return 'custom:' . $this->name; }
}
$case = CustomState::Ready;
echo json_encode($case), ';';
echo json_encode($case, JSON_THROW_ON_ERROR), ';';
echo json_last_error_msg();
"#,
        ),
        "\"custom:Ready\";\"custom:Ready\";No error"
    );
}

#[test]
fn json_serializable_self_return_uses_ordinary_enum_projection() {
    assert_eq!(
        run_php(
            r#"<?php
enum SelfState implements JsonSerializable {
    case Ready;
    public function jsonSerialize(): mixed { return $this; }
}
enum BackedSelfState: int implements JsonSerializable {
    case Ready = 3;
    public function jsonSerialize(): mixed { return $this; }
}
echo json_encode(SelfState::Ready), ':', json_encode(BackedSelfState::Ready), ':', json_last_error_msg();
"#,
        ),
        "{\"name\":\"Ready\"}:{\"name\":\"Ready\",\"value\":3}:No error"
    );
}

#[test]
fn json_callback_exception_and_recursive_values_preserve_state_without_mutation() {
    assert_eq!(
        run_php(
            r#"<?php
enum UnitState { case Ready; }
enum ThrowingState implements JsonSerializable {
    case Ready;
    public function jsonSerialize(): mixed { throw new RuntimeException('callback'); }
}

json_encode(UnitState::Ready);
try { json_encode(ThrowingState::Ready); }
catch (RuntimeException $error) {
    echo $error->getMessage(), ':', json_last_error_msg(), ';';
}

$recursive = ['value' => 7];
$recursive['self'] =& $recursive;
$copy = $recursive;
var_dump(json_encode($recursive));
echo json_last_error(), ':', json_last_error_msg(), ';';
echo json_encode($recursive, JSON_PARTIAL_OUTPUT_ON_ERROR), ';';
echo $copy['value'], ':', $recursive['value'];
"#,
        ),
        concat!(
            "callback:No error;",
            "bool(false)\n",
            "6:Recursion detected;",
            "{\"value\":7,\"self\":null};7:7",
        )
    );
}

#[test]
fn nested_json_encode_shares_the_json_serializable_recursion_guard() {
    assert_eq!(
        run_php(
            r#"<?php
final class ReentrantJson implements JsonSerializable {
    public $value = 7;
    public static $events = [];
    public function jsonSerialize(): mixed {
        self::$events[] = 'enter';
        self::$events[] = 'nested=' . var_export(json_encode($this), true);
        return $this;
    }
}
$object = new ReentrantJson;
echo json_encode($object), ';', implode(',', ReentrantJson::$events), ';', json_last_error_msg(), ';', $object->value;
"#,
        ),
        "{\"value\":7};enter,nested=false;No error;7"
    );
}

#[test]
fn enum_json_callback_keeps_the_referenced_input_snapshot_alive() {
    assert_eq!(
        run_php(
            r#"<?php
enum DetachingJson implements JsonSerializable {
    case Ready;
    public function jsonSerialize(): mixed {
        global $owners;
        unset($owners[0]);
        return [$this->name, count($owners)];
    }
}
$owners = [DetachingJson::Ready];
echo json_encode([&$owners]), ':', count($owners), ':', DetachingJson::Ready->name;
"#,
        ),
        "[[[\"Ready\",0]]]:0:Ready"
    );
}
