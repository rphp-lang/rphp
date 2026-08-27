mod common;

use common::run_php;

#[test]
fn unterminated_object_property_counts_report_the_consumed_input_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function (int $level, string $message): bool {
    echo "warning:$message\n";
    return true;
});

foreach ([
    'O:9:"000000000":10000000',
    'a:2:{i:0;O:9:"000000000":10000000',
] as $wire) {
    echo 'case:', strlen($wire), "\n";
    var_dump(unserialize($wire));
}

$valid = unserialize('O:8:"stdClass":0:{}');
echo get_class($valid), "\n";
"#,
        ),
        concat!(
            "case:24\n",
            "warning:unserialize(): Error at offset 24 of 24 bytes\n",
            "bool(false)\n",
            "case:33\n",
            "warning:unserialize(): Error at offset 33 of 33 bytes\n",
            "bool(false)\n",
            "stdClass\n",
        )
    );
}

#[test]
fn malformed_unserialize_reports_the_first_general_parser_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function (int $level, string $message): bool {
    echo "warning:$message\n";
    return true;
});

foreach ([
    'a:2:{i:0;N;}',
    'a:1:{i:0;r:1;}',
    'O:8:"stdClass":1:{s:1:"x";N;',
    'R:1;',
] as $wire) {
    echo 'case:', strlen($wire), "\n";
    var_dump(unserialize($wire));
}
"#,
        ),
        concat!(
            "case:12\n",
            "warning:unserialize(): Unexpected end of serialized data\n",
            "warning:unserialize(): Error at offset 11 of 12 bytes\n",
            "bool(false)\n",
            "case:14\n",
            "warning:unserialize(): Error at offset 13 of 14 bytes\n",
            "bool(false)\n",
            "case:28\n",
            "warning:unserialize(): Error at offset 28 of 28 bytes\n",
            "bool(false)\n",
            "case:4\n",
            "warning:unserialize(): Error at offset 4 of 4 bytes\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn legacy_payload_errors_keep_callback_and_outer_diagnostics_ordered() {
    assert_eq!(
        run_php(
            r#"<?php
class LegacyPacket implements Serializable {
    public static int $calls = 0;
    public function serialize(): string { return ''; }
    public function unserialize(string $wire): void {
        self::$calls++;
        unserialize($wire);
    }
    public function __serialize(): array { return []; }
    public function __unserialize(array $data): void {}
}

set_error_handler(function (int $level, string $message): bool {
    echo "warning:$message\n";
    return true;
});

$inner = 'a:2:{i:0;N;}';
$outer = 'a:2:{i:0;C:12:"LegacyPacket":'.strlen($inner).':{'.$inner.'}i:1;R:4;}';
echo 'lengths:', strlen($inner), ':', strlen($outer), "\n";
var_dump(unserialize($outer));
echo 'calls:', LegacyPacket::$calls, "\n";
"#,
        ),
        concat!(
            "lengths:12:55\n",
            "warning:unserialize(): Unexpected end of serialized data\n",
            "warning:unserialize(): Error at offset 11 of 12 bytes\n",
            "warning:unserialize(): Error at offset 54 of 55 bytes\n",
            "bool(false)\n",
            "calls:1\n",
        )
    );
}

#[test]
fn uppercase_wire_references_share_one_storage_cell_and_visible_owner() {
    assert_eq!(
        run_php(
            r#"<?php
$wire = 'a:2:{i:0;O:8:"stdClass":1:{s:1:"p";a:1:{i:0;s:1:"x";}}i:1;R:3;}';
$value = unserialize($wire);
$value[1][0] = 'changed';
echo $value[0]->p[0], "\n";
debug_zval_dump($value[1]);
"#,
        ),
        concat!(
            "changed\n",
            "array(1) refcount(2){\n",
            "  [0]=>\n",
            "  string(7) \"changed\" interned\n",
            "}\n",
        )
    );
}

#[test]
fn throwable_formatting_reads_reference_backed_and_unserialized_properties() {
    assert_eq!(
        run_php(
            r#"<?php
class LinkedFailure extends Exception {
    public function __construct(string &$message) {
        $this->message =& $message;
    }
}

$message = 'linked';
$linked = new LinkedFailure($message);
echo str_contains((string) $linked, 'linked') ? "linked\n" : "missing\n";

$roundTrip = unserialize(serialize(new Exception('roundtrip')));
echo str_contains((string) $roundTrip, 'roundtrip') ? "roundtrip\n" : "missing\n";
"#,
        ),
        "linked\nroundtrip\n"
    );
}

#[test]
fn typed_object_materialization_resolves_visibility_and_uses_strict_assignment() {
    assert_eq!(
        run_php(
            r#"<?php
class AncestorPacket {
    private int $value;
    public function ancestorValue(): int { return $this->value; }
}
class VisibilityPacket extends AncestorPacket {
    private int $value;
    protected float $ratio;
    public int $public;
    public function values(): string {
        return $this->ancestorValue().':'.$this->value.':'.gettype($this->ratio).':'.$this->ratio.':'.$this->public;
    }
}
class IntPacket { public int $value; }

function member(string $name, string $value): string {
    return 's:'.strlen($name).':"'.$name.'";'.$value;
}

$wire = 'O:16:"VisibilityPacket":4:{'
    .member("\0AncestorPacket\0value", 'i:11;')
    .member("\0VisibilityPacket\0value", 'i:13;')
    .member("\0*\0ratio", 'i:7;')
    .member('public', 'i:17;')
    .'}';
echo unserialize($wire)->values(), "\n";

try {
    unserialize('O:9:"IntPacket":1:{s:5:"value";s:3:"bad";}');
    echo "not-reached\n";
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "11:13:double:7:17\n",
            "Cannot assign string to property IntPacket::$value of type int\n",
        )
    );
}

#[test]
fn typed_references_hooks_and_cycles_survive_object_materialization() {
    assert_eq!(
        run_php(
            r#"<?php
class RefPacket { public int $value; }
class PairPacket { public int $int; public float $float; }
class HookPacket {
    public int $value;
    public function __unserialize(array $data): void {
        echo 'hook:', gettype($data['value']), "\n";
        $this->value = 7;
    }
}
class CyclePacket { public ?CyclePacket $next; }

try {
    unserialize('O:10:"PairPacket":2:{s:3:"int";i:7;s:5:"float";R:2;}');
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}

$graph = unserialize('a:2:{i:0;O:9:"RefPacket":1:{s:5:"value";i:7;}i:1;R:3;}');
$graph[1] = 9;
echo $graph[0]->value, ':', $graph[1], "\n";
try {
    $graph[1] = 'bad';
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
echo $graph[0]->value, ':', $graph[1], "\n";

$hook = unserialize('O:10:"HookPacket":1:{s:5:"value";s:3:"bad";}');
echo $hook->value, "\n";

$cycle = new CyclePacket();
$cycle->next = $cycle;
$copy = unserialize(serialize($cycle));
echo $copy === $copy->next ? "cycle\n" : "broken\n";
"#,
        ),
        concat!(
            "Reference with value of type int held by property PairPacket::$int of type int is not compatible with property PairPacket::$float of type float\n",
            "9:9\n",
            "Cannot assign string to reference held by property RefPacket::$value of type int\n",
            "9:9\n",
            "hook:string\n",
            "7\n",
            "cycle\n",
        )
    );
}

#[test]
fn internal_throwable_previous_is_validated_before_unserialize_returns() {
    assert_eq!(
        run_php(
            r#"<?php
$key = "\0Exception\0previous";
$invalid = 'O:9:"Exception":1:{s:'.strlen($key).':"'.$key.'";O:8:"stdClass":0:{}}';
try {
    unserialize($invalid);
    echo "not-reached\n";
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}

$previous = new Exception('root');
$copy = unserialize(serialize(new Exception('leaf', 0, $previous)));
echo $copy->getMessage(), ':', $copy->getPrevious()->getMessage(), "\n";
"#,
        ),
        concat!(
            "Cannot assign stdClass to property Exception::$previous of type ?Throwable\n",
            "leaf:root\n",
        )
    );
}
