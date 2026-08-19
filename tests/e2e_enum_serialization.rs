mod common;

use common::run_php;

#[test]
fn enum_wire_format_round_trips_singletons_and_shared_identity() {
    assert_eq!(
        run_php(
            r#"<?php
enum Mode: string {
    case Ready = 'r';
    case Wait = 'w';
}

$wire = serialize([Mode::Ready, Mode::Ready, Mode::Wait]);
echo $wire, "\n";
$copy = unserialize($wire);
echo ($copy[0] === Mode::Ready ? 'same' : 'different'), ',';
echo ($copy[0] === $copy[1] ? 'shared' : 'split'), ',';
echo ($copy[2] === Mode::Wait ? 'same' : 'different'), "\n";
echo unserialize('E:10:"Mode:Ready";', ['allowed_classes' => false]) === Mode::Ready
    ? "allowed\n"
    : "blocked\n";
"#,
        ),
        concat!(
            "a:3:{i:0;E:10:\"Mode:Ready\";i:1;r:2;i:2;E:9:\"Mode:Wait\";}\n",
            "same,shared,same\nallowed\n",
        )
    );
}

#[test]
fn enum_unserialize_reports_semantic_failures_at_php_offsets() {
    assert_eq!(
        run_php(
            r#"<?php
class Plain {}
enum Flag {
    case On;
    const Alias = self::On;
}

set_error_handler(function (int $level, string $message): bool {
    echo $message, "\n";
    return true;
});

foreach ([
    'E:6:"FlagOn";',
    'E:9:"Plain:One";',
    'E:9:"Ghost:One";',
    'E:10:"Flag:Alias";',
    'E:9:"Flag:Miss";',
] as $wire) {
    var_dump(unserialize($wire));
}
"#,
        ),
        concat!(
            "unserialize(): Invalid enum name 'FlagOn' (missing colon)\n",
            "unserialize(): Error at offset 0 of 13 bytes\n",
            "bool(false)\n",
            "unserialize(): Class 'Plain' is not an enum\n",
            "unserialize(): Error at offset 0 of 16 bytes\n",
            "bool(false)\n",
            "unserialize(): Class 'Ghost' not found\n",
            "unserialize(): Error at offset 0 of 16 bytes\n",
            "bool(false)\n",
            "unserialize(): Flag::Alias is not an enum case\n",
            "unserialize(): Error at offset 18 of 18 bytes\n",
            "bool(false)\n",
            "unserialize(): Undefined constant Flag::Miss\n",
            "unserialize(): Error at offset 16 of 16 bytes\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn enum_unserialize_autoloads_class_names_and_keeps_case_names_exact() {
    assert_eq!(
        run_php(
            r#"<?php
spl_autoload_register(function (string $class): void {
    echo "autoload:$class\n";
    if ($class === 'RemoteState') {
        eval('enum RemoteState { case Ready; }');
    }
});

var_dump(unserialize('E:17:"RemoteState:Ready";') === RemoteState::Ready);
set_error_handler(function (int $level, string $message): bool {
    echo $message, "\n";
    return true;
});
var_dump(unserialize('E:17:"remotestate:ready";'));
"#,
        ),
        concat!(
            "autoload:RemoteState\n",
            "bool(true)\n",
            "unserialize(): Undefined constant remotestate::ready\n",
            "unserialize(): Error at offset 25 of 25 bytes\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn malformed_enum_wire_data_fails_without_constructing_an_object() {
    assert_eq!(
        run_php(
            r#"<?php
enum State { case On; }
set_error_handler(function (int $level, string $message): bool {
    echo $message, "\n";
    return true;
});
var_dump(unserialize('E:9:"State:On";'));
"#,
        ),
        concat!(
            "unserialize(): Error at offset 14 of 15 bytes\n",
            "bool(false)\n",
        )
    );
}
