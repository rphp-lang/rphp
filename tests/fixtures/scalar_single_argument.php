<?php
function adjusted($value) { return $value + 7; }
function produced() { global $calls; $calls++; return 11; }
class Adjuster {
    public function method($value) { return $value + 7; }
    public static function fixed($value) { return $value + 7; }
}
$value = 13;
$alias =& $value;
$calls = 0;
$reader = new Adjuster();
$closure = static function ($value) { return $value + 7; };
$rows = [];
for ($iteration = 0; $iteration < 128; $iteration++) {
    $rows = [adjusted(3), adjusted($value), adjusted($alias), adjusted($value * 2),
        adjusted(produced()), adjusted('5'), adjusted(2.5),
        is_float(adjusted(PHP_INT_MAX)), adjusted(PHP_INT_MIN) === PHP_INT_MIN + 7,
        $reader->method(3), Adjuster::fixed(3), $closure(3)];
}
try { adjusted([]); } catch (TypeError $error) { $rows[] = 'array-error'; }
try { adjusted('not-a-number'); } catch (TypeError $error) { $rows[] = 'string-error'; }
echo json_encode([$rows, $value, $calls]), "\n";
