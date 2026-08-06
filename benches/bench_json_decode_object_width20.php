<?php
$first = '{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":7,"h":8,"i":9,"j":10,"k":11,"l":12,"m":13,"n":14,"o":15,"p":16,"q":17,"r":18,"s":19,"t":20}';
$second = '{"a":2,"b":3,"c":4,"d":5,"e":6,"f":7,"g":8,"h":9,"i":10,"j":11,"k":12,"l":13,"m":14,"n":15,"o":16,"p":17,"q":18,"r":19,"s":20,"t":21}';
$iterations = 200000;
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < $iterations; $i++) {
    $json = (($i % 2) == 0) ? $first : $second;
    $row = json_decode($json);
    $sum += $row->t;
}
$elapsed = microtime(true) - $start;
echo $sum . '|' . $elapsed;
