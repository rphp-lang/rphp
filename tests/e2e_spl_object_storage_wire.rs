mod common;

use common::run_php;

#[test]
fn enum_storage_uses_the_canonical_alternating_wire_and_singletons() {
    assert_eq!(
        run_php(
            r#"<?php
enum StorageMode: string { case Alpha = 'a'; case Omega = 'o'; }
$storage = new SplObjectStorage;
$storage[StorageMode::Alpha] = 'first';
$storage[StorageMode::Omega] = ['v' => 9];
$storage[StorageMode::Alpha] = 'updated';
$wire = serialize($storage);
echo $wire, "\n";
$copy = unserialize($wire);
echo count($copy), ':';
echo (int) $copy->contains(StorageMode::Alpha), ':', $copy[StorageMode::Alpha], ':';
echo (int) $copy->contains(StorageMode::Omega), ':', $copy[StorageMode::Omega]['v'];
"#,
        ),
        concat!(
            "O:16:\"SplObjectStorage\":2:{i:0;a:4:{",
            "i:0;E:17:\"StorageMode:Alpha\";i:1;s:7:\"updated\";",
            "i:2;E:17:\"StorageMode:Omega\";i:3;a:1:{s:1:\"v\";i:9;}",
            "}i:1;a:0:{}}\n",
            "2:1:updated:1:9",
        )
    );
}

#[test]
fn storage_updates_and_detaches_without_reordering_or_mutating_the_source() {
    assert_eq!(
        run_php(
            r#"<?php
enum OrderedMode { case First; case Middle; case Last; }
$storage = new SplObjectStorage;
$storage[OrderedMode::First] = ['value' => 1];
$storage[OrderedMode::Middle] = null;
$storage[OrderedMode::Last] = 'last';
$storage[OrderedMode::First] = ['value' => 2];
$storage->detach(OrderedMode::Middle);
$wire = serialize($storage);
$copy = unserialize($wire);
$copyInfo = $copy[OrderedMode::First];
$copyInfo['value'] = 7;
foreach ($copy as $case) {
    echo $case->name, '=', json_encode($copy[$case]), ';';
}
echo '|source=', $storage[OrderedMode::First]['value'];
echo ':copy=', $copy[OrderedMode::First]['value'];
echo ':missing=', (int) !$copy->contains(OrderedMode::Middle);
"#,
        ),
        "First={\"value\":2};Last=\"last\";|source=2:copy=2:missing=1"
    );
}

#[test]
fn storage_wire_preserves_object_links_and_nested_reference_cells() {
    assert_eq!(
        run_php(
            r#"<?php
$first = (object) ['name' => 'first'];
$second = (object) ['name' => 'second'];
$shared = 8;
$info = ['left' => &$shared, 'right' => &$shared, 'peer' => $second];
$storage = new SplObjectStorage;
$storage[$first] = $info;
$storage[$second] = ['peer' => $first];
echo serialize($storage), "\n";
$copy = unserialize(serialize($storage));
$objects = iterator_to_array($copy, false);
$copiedInfo = $copy[$objects[0]];
$copiedInfo['left'] = 99;
echo (int) ($copiedInfo['peer'] === $objects[1]), ':';
echo (int) ($copy[$objects[1]]['peer'] === $objects[0]), ':';
echo $copy[$objects[0]]['left'], ':', $copy[$objects[0]]['right'], ':', $shared;
"#,
        ),
        concat!(
            "O:16:\"SplObjectStorage\":2:{i:0;a:4:{",
            "i:0;O:8:\"stdClass\":1:{s:4:\"name\";s:5:\"first\";}",
            "i:1;a:3:{s:4:\"left\";i:8;s:5:\"right\";R:6;",
            "s:4:\"peer\";O:8:\"stdClass\":1:{s:4:\"name\";s:6:\"second\";}}",
            "i:2;r:7;i:3;a:1:{s:4:\"peer\";r:3;}",
            "}i:1;a:0:{}}\n",
            "1:1:99:99:8",
        )
    );
}

#[test]
fn malformed_storage_entries_fail_before_publishing_partial_state() {
    assert_eq!(
        run_php(
            r#"<?php
enum KeptMode { case Value; }
$kept = new SplObjectStorage;
$kept[KeptMode::Value] = 'kept';
foreach ([
    'O:16:"SplObjectStorage":2:{i:0;a:1:{i:0;O:8:"stdClass":0:{}}i:1;a:0:{}}',
    'O:16:"SplObjectStorage":2:{i:0;a:2:{i:0;i:1;i:1;s:1:"x";}i:1;a:0:{}}',
] as $wire) {
    try { unserialize($wire); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), ';'; }
}
echo $kept[KeptMode::Value], ':', count($kept);
"#,
        ),
        concat!(
            "UnexpectedValueException:Odd number of elements;",
            "UnexpectedValueException:Non-object key;kept:1",
        )
    );
}

#[test]
fn storage_member_payload_and_allowed_classes_stay_separate_from_engine_state() {
    assert_eq!(
        run_php(
            r#"<?php
$wire = 'O:16:"SplObjectStorage":2:{i:0;a:0:{}i:1;a:1:{s:6:"marker";s:2:"ok";}}';
$storage = unserialize($wire);
echo $storage->marker, ':', count($storage), ':', serialize($storage), "\n";
$blocked = unserialize($wire, ['allowed_classes' => false]);
echo get_class($blocked);
"#,
        ),
        concat!(
            "ok:0:O:16:\"SplObjectStorage\":2:{i:0;a:0:{}i:1;a:1:{s:6:\"marker\";s:2:\"ok\";}}\n",
            "__PHP_Incomplete_Class",
        )
    );
}

#[test]
fn uppercase_aliases_do_not_shift_following_object_reference_numbers() {
    assert_eq!(
        run_php(
            r#"<?php
$number = 8;
$object = new stdClass;
$wire = serialize([&$number, &$number, $object, $object]);
echo $wire, "\n";
$copy = unserialize($wire);
$copy[0] = 9;
echo $copy[1], ':', (int) ($copy[2] === $copy[3]);
"#,
        ),
        "a:4:{i:0;i:8;i:1;R:2;i:2;O:8:\"stdClass\":0:{}i:3;r:3;}\n9:1"
    );
}

#[test]
fn empty_storage_round_trip_has_no_engine_property_leak() {
    assert_eq!(
        run_php(
            r#"<?php
$storage = new SplObjectStorage;
$wire = serialize($storage);
$copy = unserialize($wire);
echo $wire, ':', count($copy), ':';
echo (int) !str_contains($wire, '__rphp_');
"#,
        ),
        "O:16:\"SplObjectStorage\":2:{i:0;a:0:{}i:1;a:0:{}}:0:1"
    );
}
