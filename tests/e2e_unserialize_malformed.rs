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
