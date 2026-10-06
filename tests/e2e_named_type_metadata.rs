mod common;
use common::run_php;

#[test]
fn named_type_misses_aliases_intersections_and_properties_remain_observable() {
    assert_eq!(
        run_php(
            r#"<?php
interface LeftType {}
interface RightType {}
class BothTypes implements LeftType, RightType {}
class OtherType {}
class Holder { public ?LaterType $value = null; }
function named(LaterType $value): LaterType { return $value; }
function joined(LeftType&RightType $value): LeftType|OtherType { return $value; }
spl_autoload_register(function ($name) { echo 'autoload:', $name, '|'; });
$object = new BothTypes;
$holder = new Holder;
try { named($object); } catch (TypeError $e) { echo 'miss|'; }
try { $holder->value = $object; } catch (TypeError $e) { echo 'property-miss|'; }
class_alias(LeftType::class, 'LaterType');
for ($i = 0; $i < 3; $i++) {
    $holder->value = named(joined($object));
    echo (int) ($holder->value === $object);
}
try { named(new OtherType); } catch (TypeError $e) { echo '|argument'; }
try { $holder->value = new OtherType; } catch (TypeError $e) { echo '|property'; }
echo '|', (new ReflectionFunction('named'))->getReturnType();
"#
        ),
        "miss|property-miss|111|argument|property|LaterType"
    );
}

#[test]
fn named_type_metadata_keeps_relative_closure_and_late_static_scopes_distinct() {
    assert_eq!(
        run_php(
            r#"<?php
class FirstRoot {}
class SecondRoot {}
class FirstScope extends FirstRoot {
    static function factory() { return function (self $value, parent $root): self { return $value; }; }
    function same(): static { return $this; }
    function wrong(): static { return new FirstScope; }
}
class SecondScope extends SecondRoot {}
class ChildScope extends FirstScope {}
$first = FirstScope::factory();
$second = $first->bindTo(null, SecondScope::class);
$a = new FirstScope;
$b = new SecondScope;
for ($i = 0; $i < 3; $i++) {
    echo (int) ($first($a, new FirstRoot) === $a);
    echo (int) ($second($b, new SecondRoot) === $b);
}
try { $second($a, new SecondRoot); } catch (TypeError $e) { echo '|self'; }
try { $second($b, new FirstRoot); } catch (TypeError $e) { echo '|parent'; }
$child = new ChildScope;
echo '|', (int) ($child->same() === $child);
try { $child->wrong(); } catch (TypeError $e) { echo '|static'; }
"#
        ),
        "111111|self|parent|1|static"
    );
}

#[test]
fn named_builtin_hints_keep_closures_iterables_literals_and_failures() {
    assert_eq!(
        run_php(
            r#"<?php
function sequence(iterable $value): iterable { return $value; }
function closure(Closure $value): object { return $value; }
function literal(false|null $value): false|null { return $value; }
$callback = function () { return 7; };
echo closure($callback)(), '|';
foreach ([[], new ArrayIterator([1]), []] as $value) {
    foreach (sequence($value) as $item) { echo $item; }
    echo ':';
}
echo (int) (literal(false) === false), (int) (literal(null) === null);
try { sequence(new stdClass); } catch (TypeError $e) { echo '|iterable'; }
try { closure(new stdClass); } catch (TypeError $e) { echo '|closure'; }
try { literal(true); } catch (TypeError $e) { echo '|literal'; }
"#
        ),
        "7|:1::11|iterable|closure|literal"
    );
}
