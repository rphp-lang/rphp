mod common;

use common::run_php;

#[test]
fn shared_frame_roots_preserve_nested_owners_and_reference_aliases() {
    assert_eq!(
        run_php(
            r#"<?php
gc_disable();
class SharedReleaseLeaf {
    public function __construct(public string $name) {}
    public function __destruct() { echo 'drop:', $this->name, '|'; }
}
function inspectShared($owner, $array, $closure, &$reference, $fail) {
    $same = $owner;
    $copy = $array;
    $alias =& $reference;
    echo $closure()->name, ':', $copy[0]->name, ':', $alias->name, '|';
    if ($fail) { throw new Exception('expected'); }
}
$owner = new SharedReleaseLeaf('retained');
$array = [$owner];
$closure = static fn () => $owner;
$reference = $owner;
$weak = WeakReference::create($owner);
inspectShared($owner, $array, $closure, $reference, false);
try { inspectShared($owner, $array, $closure, $reference, true); }
catch (Exception $error) { echo 'caught|'; }
unset($owner, $array, $closure);
echo $weak->get()->name, '|';
unset($reference);
echo (int) ($weak->get() === null);
"#,
        ),
        concat!(
            "retained:retained:retained|retained:retained:retained|",
            "caught|retained|drop:retained|1"
        )
    );
}

#[test]
fn local_aliases_do_not_prove_an_external_owner() {
    // Every alias belongs to this frame. Vary the number of owners while
    // requiring the final callback to precede the caller's next statement.
    for aliases in [1, 2, 4, 8, 9] {
        let mut source = String::from(
            "<?php class LocalReleaseLeaf { function __destruct() { echo 'drop|'; } } \
             function retire() { $owner = new LocalReleaseLeaf;",
        );
        for index in 0..aliases {
            source.push_str(&format!("$alias{index} = $owner;"));
        }
        source.push_str("echo 'body|'; } retire(); echo 'after';");
        assert_eq!(run_php(&source), "body|drop|after", "aliases={aliases}");
    }
}

#[test]
fn final_local_container_keeps_destructor_order_with_shared_direct_roots() {
    assert_eq!(
        run_php(
            r#"<?php
class ContainerReleaseLeaf {
    public function __construct(public string $name) {}
    public function __destruct() { echo 'drop:', $this->name, '|'; }
}
function inspectAndCreate($retained) {
    $alias = $retained;
    $local = [new ContainerReleaseLeaf('nested')];
    echo 'body|';
}
$retained = new ContainerReleaseLeaf('outer');
inspectAndCreate($retained);
echo 'after|';
unset($retained);
"#,
        ),
        "body|drop:nested|after|drop:outer|"
    );
}

#[test]
fn wide_frames_preserve_shared_owners_and_final_nested_callbacks() {
    let mut source = String::from(
        "<?php class WideReleaseLeaf { \
         function __construct(public string $name) {} \
         function __destruct() { echo 'drop:', $this->name, '|'; } } \
         function inspectWide($retained, $nested) {",
    );
    // Force the non-bitmap frame path while keeping few heap owners. Neither
    // aliases nor a final plain outer object can hide a nested PHP callback.
    for index in 0..70 {
        source.push_str(&format!("$scalar{index} = {index};"));
    }
    source.push_str(
        "$alias = $retained; $plain = new stdClass; $plain->number = 1; \
         if ($nested) { $leaf = new WideReleaseLeaf('nested'); \
             $plain->leaf = $leaf; } \
         echo $alias->name, ':', $scalar69, '|'; } \
         $owner = new WideReleaseLeaf('outer'); \
         inspectWide($owner, false); echo 'after|'; \
         inspectWide($owner, true); echo 'after|'; unset($owner);",
    );
    assert_eq!(
        run_php(&source),
        "outer:69|after|outer:69|drop:nested|after|drop:outer|"
    );
}

#[test]
fn final_plain_weak_map_key_still_releases_its_callback_value() {
    assert_eq!(
        run_php(
            r#"<?php
class WeakReleaseLeaf {
    public function __destruct() { echo 'drop|'; }
}
function retireKey($map) {
    $key = new stdClass;
    $key->number = 1;
    $map[$key] = new WeakReleaseLeaf;
    echo 'body|';
}
$map = new WeakMap;
retireKey($map);
echo 'after:', count($map);
"#,
        ),
        "body|drop|after:0"
    );
}
