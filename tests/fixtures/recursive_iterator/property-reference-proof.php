<?php
class ConstraintProof {
    public int $a = 1;
    public int $b = 2;
    public string $text = 'old';
    public $plain = 3;
}
function proof_write_int($object, $value) { $object->a = $value; return $object->a; }
function proof_write_plain($object, $value) { $object->plain = $value; return $object->plain; }
$object = new ConstraintProof;
foreach ([4,5] as $value) {
    echo proof_write_int($object, $value), ':', proof_write_plain($object, $value), "\n";
}
$alias =& $object->a;
$object->b =& $object->a;
$object->plain =& $object->text;
echo proof_write_int($object, '12'), ':', $alias, ':', $object->b, "\n";
$assigned = proof_write_plain($object, 17);
echo gettype($assigned), ':', $assigned, ':', $object->text, "\n";
foreach (['proof_write_int','proof_write_plain'] as $writer) {
    try { $writer($object, []); }
    catch (Throwable $error) { echo get_class($error), "\n"; }
}
echo $object->a, ':', $object->b, ':', $object->text, "\n";
unset($object->a, $object->plain);
echo proof_write_int($object, 23), ':', proof_write_plain($object, 29), "\n";
echo $alias, ':', $object->b, ':', $object->text, "\n";
