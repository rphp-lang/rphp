<?php
class BitmapOwner {
    public static int $retired = 0;
    public function __destruct() { ++self::$retired; }
}
function changingScalar($kind) {
    if ($kind === 0) return new BitmapOwner;
    if ($kind === 1) return true;
    if ($kind === 2) return 17;
    return false;
}
function scalarBitmapFrame() {
    // FRAME_LOCALS
    $resource = fopen('php://memory', 'w+');
    $saved = [];
    $sum = 0;
    for ($i = 0; $i < 20; ++$i) {
        $value = changingScalar($i % 4);
        if (is_object($value)) $saved[] = $value;
        $truth = $i < 7;
        $negated = !$truth;
        $length = strlen('four');
        $sum += (int)$truth + (int)$negated + $length;
    }
    echo $sum, ':', count($saved), ':', BitmapOwner::$retired, ':', (int)is_resource($resource), "\n";
    unset($saved);
    echo BitmapOwner::$retired, "\n";
}
scalarBitmapFrame();
$mirror = [fopen('php://memory', 'w+')];
$retained = $mirror;
$mirror = null;
echo (int)is_resource($retained[0]), ':', (int)($GLOBALS['mirror'] === null), "\n";
$mirror = [fopen('php://memory', 'w+')];
$alias =& $mirror;
$mirror = 11;
echo $alias, ':', $GLOBALS['mirror'], "\n";
fclose($retained[0]);
echo (int)is_resource($retained[0]), "\n";
