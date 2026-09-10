<?php
function proof_add($a, $b) { return $a + $b; }
function proof_sub($a, $b) { return $a - $b; }
function proof_mul($a, $b) { return $a * $b; }
function proof_div($a, $b) { return intdiv($a, $b); }
function proof_mod($a, $b) { return $a % $b; }
function proof_cmp($a, $b) { return $a <=> $b; }
function proof_and($a, $b) { return $a & $b; }
function proof_or($a, $b) { return $a | $b; }
function proof_xor($a, $b) { return $a ^ $b; }
function proof_dependency($a, $b) { return ($a + 3) * ($b - 2); }
function proof_select($a, $b) { return $a < $b ? $a + 3 : $b - 2; }
function proof_result($body) {
    try {
        $value = $body();
        echo gettype($value), is_int($value) ? ':' . $value : '', ';';
    } catch (Throwable $error) { echo get_class($error), ';'; }
}
foreach ([[PHP_INT_MAX,1],[PHP_INT_MIN,-1],[-17,0],[23,7],[1.5,2],[3,4]] as [$a,$b]) {
    proof_result(fn() => proof_add($a,$b));
    proof_result(fn() => proof_sub($a,$b));
    proof_result(fn() => proof_mul($a,$b));
    proof_result(fn() => proof_div($a,$b));
    proof_result(fn() => proof_mod($a,$b));
    proof_result(fn() => proof_cmp($a,$b));
    if (is_int($a)) {
        proof_result(fn() => proof_and($a,$b));
        proof_result(fn() => proof_or($a,$b));
        proof_result(fn() => proof_xor($a,$b));
    }
    proof_result(fn() => proof_dependency($a,$b));
    proof_result(fn() => proof_select($a,$b));
    echo "\n";
}
$original = 11;
$alias =& $original;
echo proof_add(b: 4, a: $alias), ':', $original, "\n";
