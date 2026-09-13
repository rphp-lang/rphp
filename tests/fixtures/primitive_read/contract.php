<?php
function change_read_source(&$value, $replacement) {
    $value = $replacement;
    return 3;
}
switch (getenv('RPHP_PRIMITIVE_READ_CASE')) {
case 'scalars':
    foreach ([null, false, true, -7, 2.5, -0.0] as $source) {
        $snapshot = $source;
        $answer = $source + change_read_source($source, 19);
        var_dump($snapshot, $answer, $source);
    }
    break;
case 'aliases':
    $source = 4.5;
    $alias =& $source;
    var_dump($source + change_read_source($alias, 8), $source, $alias);
    $source = null;
    var_dump($alias + change_read_source($source, false), $alias);
    $a = 7;
    $b =& $a;
    $copy = $b;
    $b = 12;
    var_dump($copy, $a);
    break;
case 'float_bits':
    $values = [-0.0, 0.0, INF, -INF, NAN];
    foreach ($values as $value) {
        $snapshot = $value;
        var_dump($snapshot);
    }
    $limit = PHP_INT_MAX;
    $other = 1;
    var_dump($limit + $other, $limit);
    break;
case 'undefined':
    set_error_handler(function ($severity, $message) {
        global $missing;
        echo $severity, ':', $message, "\n";
        $missing = 41;
    });
    $answer = $missing + 2;
    var_dump($answer, $missing);
    unset($missing);
    $answer = @$missing;
    var_dump($answer, $missing);
    restore_error_handler();
    break;
case 'throw_order':
    function consume_read_arguments($first, $second) { return $first + $second; }
    $side = 0;
    set_error_handler(function ($severity, $message) {
        echo "warning\n";
        throw new Exception('read stopped');
    });
    try {
        $result = consume_read_arguments($missing, change_read_source($side, 9));
    } catch (Exception $e) {
        echo $e->getMessage(), "\n";
    }
    var_dump($side, isset($result), isset($missing));
    restore_error_handler();
    break;
case 'unpack':
    function read_unpack(&$first, ...$rest) {
        $first = 77;
        var_dump($rest);
    }
    $items = [1, 2, 3];
    read_unpack(...$items);
    var_dump($items);
    $alias =& $items;
    read_unpack(...$alias);
    var_dump($items === $alias);
    break;
case 'owners':
    class ReadSnapshotOwner {
        public function __destruct() { echo "owner released\n"; }
    }
    $values = ['text', ['key' => 7], new ReadSnapshotOwner(), function () { return 5; }, 1.25, null];
    foreach ($values as $value) {
        $snapshot = $value;
        $value = false;
        echo gettype($snapshot), "\n";
    }
    unset($snapshot, $value, $values);
    echo "done\n";
    break;
case 'retirement':
    class TempRetirementOwner {
        public function __construct(private int $id) {}
        public function __destruct() {
            $nested = fopen('php://memory', 'w+');
            echo 'drop:', $this->id, ':', (int)fclose($nested), "\n";
        }
    }
    function build_temp_root($id) {
        $stream = fopen('php://memory', 'w+');
        $alias = $stream;
        $root = [new TempRetirementOwner($id), [$alias], $stream];
        fclose($stream);
        return $root;
    }
    for ($i = 0; $i < 3; ++$i) {
        $next = build_temp_root($i);
        $kept = $next;
        $next[1][] = 9;
        var_dump(count($kept[1]), count($next[1]));
        unset($kept, $next);
    }
    echo "finished\n";
    break;
case 'large_frame':
    $body = 'function read_large_frame() {';
    for ($i = 0; $i < 72; $i++) {
        $body .= '$padding' . $i . ' = ' . $i . ';';
    }
    $body .= '$source = -0.5; $alias =& $source; $result = $alias; $source = 8;';
    $body .= 'var_dump($result, $alias, $padding71); } read_large_frame();';
    eval($body);
    break;
default:
    throw new Exception('unknown primitive-read case');
}
