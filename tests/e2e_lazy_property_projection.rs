mod common;

use common::run_php;

#[test]
fn a_lazy_dynamic_write_detaches_the_existing_projection_not_the_declared_slots() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class SnapshotLedger {
    public int $count = 12;
    public ?string $label = null;
    public function update(int $count) { $this->count = $count; }
}
function showSnapshotLedger($object) {
    echo $object->count, ':', $object->label, ':', json_encode(get_object_vars($object)), '|';
}
$reflection = new ReflectionClass(SnapshotLedger::class);
$object = $reflection->newLazyGhost(function($object) {
    $object->extra = 3; $object->count = 21; $object->label = 'ready';
});
ob_start(); var_dump($object); ob_end_clean();
$object->extra += 2;
showSnapshotLedger($object);
$object->count = 33; showSnapshotLedger($object);
$object->update(44); showSnapshotLedger($object);
$copy = clone $object; showSnapshotLedger($copy);
echo json_encode((array) $object), '|';
foreach ($object as $name => $value) echo $name, ':', $value, '|';
"#
        ),
        concat!(
            "21:ready:{\"count\":12,\"label\":null,\"extra\":5}|",
            "33:ready:{\"count\":12,\"label\":null,\"extra\":5}|",
            "44:ready:{\"count\":12,\"label\":null,\"extra\":5}|",
            "44:ready:{\"count\":12,\"label\":null,\"extra\":5}|",
            "{\"count\":12,\"label\":null,\"extra\":5}|count:12|label:|extra:5|",
        )
    );
}

#[test]
fn lazy_projection_capture_depends_on_write_order_and_preinitialization_enumeration() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class OrderedProjection { public int $count = 12; }
$reflection = new ReflectionClass(OrderedProjection::class);
foreach (['before', 'after', 'none', 'inside'] as $mode) {
    $object = $reflection->newLazyGhost(function($object) use ($mode) {
        if ($mode === 'inside') { ob_start(); var_dump($object); ob_end_clean(); }
        if ($mode === 'before') $object->extra = 3;
        $object->count = 21;
        if ($mode !== 'before') $object->extra = 3;
    });
    if ($mode === 'before' || $mode === 'after') {
        ob_start(); var_dump($object); ob_end_clean();
    }
    $reflection->initializeLazyObject($object);
    $object->count = 33;
    echo $mode, ':', $object->count, ':', json_encode(get_object_vars($object)), '|';
}
$object = $reflection->newLazyGhost(function($object) { $object->count = 21; });
ob_start(); var_dump($object); ob_end_clean();
$reflection->initializeLazyObject($object);
$object->count = 44; $object->extra = 5;
echo json_encode(get_object_vars($object));
"#
        ),
        concat!(
            "before:33:{\"count\":12,\"extra\":3}|after:33:{\"count\":21,\"extra\":3}|",
            "none:33:{\"count\":33,\"extra\":3}|inside:33:{\"count\":33,\"extra\":3}|",
            "{\"count\":44,\"extra\":5}",
        )
    );
}

#[test]
fn lazy_projection_rollback_and_declared_references_preserve_their_separate_state() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class RetriedProjection { public int $count = 12; public ?string $label = null; }
$reflection = new ReflectionClass(RetriedProjection::class);
$attempt = 0;
$object = $reflection->newLazyGhost(function($object) use (&$attempt) {
    $object->extra = ++$attempt; $object->count = 21;
    if ($attempt === 1) throw new Exception('retry');
    $object->label = 'ready';
});
ob_start(); var_dump($object); ob_end_clean();
try { $reflection->initializeLazyObject($object); }
catch (Exception $error) { echo $error->getMessage(), '|'; }
echo (int) $reflection->isUninitializedLazyObject($object), '|';
$reflection->initializeLazyObject($object);
echo $object->count, ':', json_encode(get_object_vars($object)), '|';
$object = $reflection->newLazyGhost(function($object) {
    $reference =& $object->count; $reference = 21;
    $object->extra = 3; $reference = 33; $object->label = 'ready';
});
ob_start(); var_dump($object); ob_end_clean();
$reflection->initializeLazyObject($object);
echo $object->count, ':', json_encode(get_object_vars($object)), '|';
"#
        ),
        "retry|1|21:{\"count\":12,\"label\":null,\"extra\":2}|33:{\"count\":33,\"label\":null,\"extra\":3}|"
    );
}

#[test]
fn missing_dynamic_unset_detaches_but_isset_and_proxy_initialization_do_not() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class AccessProjection { public int $count = 12; }
$reflection = new ReflectionClass(AccessProjection::class);
foreach (['unset', 'isset', 'reference', 'proxy'] as $mode) {
    $initializer = function($object) use ($mode) {
        if ($mode === 'proxy') $object = new AccessProjection;
        if ($mode === 'unset') unset($object->extra);
        if ($mode === 'isset') isset($object->extra);
        if ($mode === 'reference') { $reference =& $object->extra; $reference = 3; }
        $object->count = 21;
        if ($mode === 'proxy') return $object;
    };
    $object = $mode === 'proxy' ? $reflection->newLazyProxy($initializer) : $reflection->newLazyGhost($initializer);
    ob_start(); var_dump($object); ob_end_clean();
    $reflection->initializeLazyObject($object);
    echo $mode, ':', $object->count, ':', json_encode(get_object_vars($object)), '|';
}
"#
        ),
        "unset:21:{\"count\":12}|isset:21:{\"count\":21}|reference:21:{\"count\":12,\"extra\":3}|proxy:21:{\"count\":21}|"
    );
}
