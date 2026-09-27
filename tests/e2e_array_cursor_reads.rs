mod common;

use common::run_php;

/// The internal array pointer indexes storage directly for every layout
/// (packed, small, linear and general hash), including after deletions,
/// copies made mid-iteration and `array_pop` rewinds.
#[test]
fn array_cursor_reads_follow_php_for_every_storage_layout() {
    assert_eq!(
        run_php(
            r#"<?php
function walk(array $a): string {
    $out = [];
    $out[] = var_export(current($a), true) . '/' . var_export(key($a), true);
    while (($v = next($a)) !== false) { $out[] = var_export($v, true) . '/' . var_export(key($a), true); }
    $out[] = var_export(key($a), true);
    $out[] = var_export(reset($a), true) . '/' . var_export(end($a), true) . '/' . var_export(key($a), true) . '/' . var_export(prev($a), true) . '/' . var_export(key($a), true);
    return implode(' ', $out);
}
echo walk([10, 20, 30]), "\n";
echo walk(['a' => 1, 'b' => 2]), "\n";
echo walk(['a' => 1, 'b' => 2, 'c' => 3, 'd' => 4, 5 => 'e', 'f' => 6]), "\n";
$big = []; for ($i = 0; $i < 40; $i++) { $big["k$i"] = $i; } unset($big['k3'], $big['k20']); $big[] = 'tail';
echo walk($big), "\n";
echo walk([]), "\n";
echo walk([3 => 'x', 1 => 'y']), "\n";
$s = [1, 2, 3]; next($s); next($s); $t = $s; $t[] = 4; echo current($s), current($t), key($t), "\n";
$stack = range(1, 5); while (($x = array_pop($stack)) !== null) { echo $x; } echo "\n";
$h = ['x' => 1, 'y' => 2, 'z' => 3]; next($h); array_pop($h); echo key($h), var_export(current($h), true), "\n";
$m = [5 => 'a', 9 => 'b']; array_pop($m); $m[] = 'c'; echo json_encode($m), "\n";
"#
        ),
        r#"10/0 20/1 30/2 NULL 10/30/2/20/1
1/'a' 2/'b' NULL 1/2/'b'/1/'a'
1/'a' 2/'b' 3/'c' 4/'d' 'e'/5 6/'f' NULL 1/6/'f'/'e'/5
0/'k0' 1/'k1' 2/'k2' 4/'k4' 5/'k5' 6/'k6' 7/'k7' 8/'k8' 9/'k9' 10/'k10' 11/'k11' 12/'k12' 13/'k13' 14/'k14' 15/'k15' 16/'k16' 17/'k17' 18/'k18' 19/'k19' 21/'k21' 22/'k22' 23/'k23' 24/'k24' 25/'k25' 26/'k26' 27/'k27' 28/'k28' 29/'k29' 30/'k30' 31/'k31' 32/'k32' 33/'k33' 34/'k34' 35/'k35' 36/'k36' 37/'k37' 38/'k38' 39/'k39' 'tail'/0 NULL 0/'tail'/0/39/'k39'
false/NULL NULL false/false/NULL/false/NULL
'x'/3 'y'/1 NULL 'x'/'y'/1/'x'/3
332
54321
x1
{"5":"a","9":"c"}
"#
    );
}
