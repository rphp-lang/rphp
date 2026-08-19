mod common;

use common::run_php;

#[test]
fn weak_reference_caches_wrappers_and_clears_objects_and_closures() {
    assert_eq!(
        run_php(
            r#"<?php
class TrackedWeakTarget {
    public WeakReference $weak;
    public function __construct() {
        $this->weak = WeakReference::create($this);
    }
    public function __destruct() {
        echo 'dtor:', $this->weak->get() === $this ? 'live' : 'dead', "\n";
    }
}

$object = new TrackedWeakTarget;
$first = WeakReference::create($object);
$second = WeakReference::create($object);
echo 'cache:', $first === $second ? 'yes' : 'no', "\n";
unset($object);
echo 'object:', $first->get() === null ? 'dead' : 'live', "\n";

$closure = function () {};
$closureRef = WeakReference::create($closure);
unset($closure);
echo 'closure:', $closureRef->get() === null ? 'dead' : 'live', "\n";
"#,
        ),
        "cache:yes\ndtor:live\nobject:dead\nclosure:dead\n"
    );
}

#[test]
fn weak_map_preserves_aliases_and_clones_unaliased_entries() {
    assert_eq!(
        run_php(
            r#"<?php
$map = new WeakMap;
$first = new stdClass;
$second = new stdClass;
$map[$first] = 10;
$map[$second] = 20;
$alias =& $map[$first];
$alias++;

$clone = clone $map;
$alias = 12;
$map[$second]++;
echo 'values:', $map[$first], ',', $clone[$first], ',',
     $map[$second], ',', $clone[$second], "\n";

unset($first);
echo 'counts:', count($map), ',', count($clone), "\n";
$map[$second] = null;
echo 'probe:', isset($map[$second]) ? 1 : 0, ',', empty($map[$second]) ? 1 : 0, "\n";
unset($map[$second]);
echo 'final:', count($map), "\n";
"#,
        ),
        "values:12,12,21,20\ncounts:1,1\nprobe:0,1\nfinal:0\n"
    );
}

#[test]
fn weak_map_iteration_removes_dead_keys_and_allows_reference_values() {
    assert_eq!(
        run_php(
            r#"<?php
$map = new WeakMap;
$first = new stdClass;
$middle = new stdClass;
$last = new stdClass;
$map[$first] = 1;
$map[$middle] = 2;
$map[$last] = 3;

foreach ($map as $key => $value) {
    echo $value;
    if (isset($middle) && $key === $middle) unset($middle);
}
echo "\ncount:", count($map), "\n";

foreach ($map as &$value) {
    $value += 10;
}
unset($value);
echo 'values:', $map[$first], ',', $map[$last], "\n";
"#,
        ),
        "123\ncount:2\nvalues:11,13\n"
    );
}

#[test]
fn weak_map_releases_values_after_the_key_destructor() {
    assert_eq!(
        run_php(
            r#"<?php
class WeakMapKeyWithDestructor {
    public WeakReference $weak;
    public function __construct() {
        $this->weak = WeakReference::create($this);
    }
    public function __destruct() {
        echo 'key-dtor:', $this->weak->get() === $this ? 'live' : 'dead', "\n";
    }
}
class WeakMapPayloadWithDestructor {
    public function __destruct() {
        echo "value-dtor\n";
    }
}

$map = new WeakMap;
$key = new WeakMapKeyWithDestructor;
$weak = $key->weak;
$map[$key] = new WeakMapPayloadWithDestructor;
unset($key);
echo 'after:', $weak->get() === null ? 'dead' : 'live',
     ',count=', count($map), "\n";
"#,
        ),
        "key-dtor:live\nvalue-dtor\nafter:dead,count=0\n"
    );
}

#[test]
fn weak_objects_reject_append_dynamic_properties_and_serialization() {
    assert_eq!(
        run_php(
            r#"<?php
$map = new WeakMap;
try { $map[] = 1; } catch (Error $error) { echo $error->getMessage(), "\n"; }
try { $map[][1] = 1; } catch (Error $error) { echo $error->getMessage(), "\n"; }
try { $map[null] = 1; } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { $map->extra = 1; } catch (Error $error) { echo $error->getMessage(), "\n"; }
try { serialize($map); } catch (Exception $error) { echo $error->getMessage(), "\n"; }
try { unserialize('C:7:"WeakMap":0:{}'); } catch (Exception $error) { echo $error->getMessage(), "\n"; }
try { new WeakReference; } catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "Cannot append to WeakMap\n",
            "Cannot append to WeakMap\n",
            "WeakMap key must be an object\n",
            "Cannot create dynamic property WeakMap::$extra\n",
            "Serialization of 'WeakMap' is not allowed\n",
            "Unserialization of 'WeakMap' is not allowed\n",
            "Direct instantiation of WeakReference is not allowed, ",
            "use WeakReference::create instead\n",
        )
    );
}

#[test]
fn weak_notification_precedes_destructors_nested_inside_properties() {
    assert_eq!(
        run_php(
            r#"<?php
class WeakOwnerWithNestedHandler {
    public array $handlers = [];
    public function __construct() {
        $this->handlers[] = new class($this) {
            private WeakReference $owner;
            public function __construct(object $owner) {
                $this->owner = WeakReference::create($owner);
            }
            public function __destruct() {
                echo $this->owner->get() === null ? "cleared\n" : "live\n";
            }
        };
    }
}

new WeakOwnerWithNestedHandler;
echo "done\n";
"#,
        ),
        "cleared\ndone\n"
    );
}
