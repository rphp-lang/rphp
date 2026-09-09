mod common;
use common::run_php;

#[test]
fn temporary_assignments_preserve_scalars_reference_returns_and_cow() {
    assert_eq!(
        run_php(
            r#"<?php
function makeValue($value) { return $value; }
function &aliasValue(&$value) { return $value; }
$number = makeValue(7);
$number = makeValue(2.5);
$number = makeValue(null);
var_dump($number);
$original = 14;
$copy = aliasValue($original);
$copy = makeValue(21);
echo $original, ':', $copy, "\n";
$copy =& $original;
$copy = makeValue(28);
echo $original, ':', $copy, "\n";
$array = ['nested' => [4]];
$snapshot = aliasValue($array);
$snapshot['nested'][0] = 9;
echo $array['nested'][0], ':', $snapshot['nested'][0], "\n";
$sum = 0;
for ($i = 0; $i < 300; $i++) { $sum = $sum + makeValue($i); }
echo $sum, "\n";
"#
        ),
        "NULL\n14:21\n28:28\n4:9\n44850\n"
    );
}

#[test]
fn temporary_assignment_keeps_typed_reference_checks_and_expression_values() {
    assert_eq!(
        run_php(
            r#"<?php
class TemporaryCounter { public int $count = 3; }
function resultValue($value) { return $value; }
$owner = new TemporaryCounter;
$alias =& $owner->count;
$alias = resultValue(8);
try { $alias = resultValue([]); } catch (TypeError $error) { echo "blocked\n"; }
echo $owner->count, ':', $alias, "\n";
$left = $right = resultValue(['v' => 1]);
$left['v'] = 2;
echo $left['v'], ':', $right['v'], "\n";
"#
        ),
        "blocked\n8:8\n2:1\n"
    );
}

#[test]
fn temporary_assignment_releases_replaced_and_nested_objects_at_the_same_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
class TemporaryMarker {
    public function __construct(public string $name) {}
    public function __destruct() { echo 'drop:', $this->name, "\n"; }
}
function markerValue($name) { return new TemporaryMarker($name); }
function nestedValue() { return [new TemporaryMarker('nested')]; }
$slot = markerValue('first');
$slot = markerValue('second');
echo "replaced\n";
$slot = nestedValue();
echo "nested\n";
$slot = 0;
echo "done\n";
"#
        ),
        "drop:first\nreplaced\ndrop:second\nnested\ndrop:nested\ndone\n"
    );
}
