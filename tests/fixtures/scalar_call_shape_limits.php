<?php
function scalarIdentity($x) { return $x; }
function scalarOne($x) { return $x + 3; }
function scalarTwo($x) { return ($x + 3) * 2; }
function scalarSeven($x) { return (((((($x + 3) * 2) - 5) + 7) * 3) - 11) + 13; }
function scalarEight($x) { return ((((((($x + 3) * 2) - 5) + 7) * 3) - 11) + 13) * 2; }
function scalarNine($x) { return (((((((($x + 3) * 2) - 5) + 7) * 3) - 11) + 13) * 2) - 17; }
function scalarBranch($x) {
    if ($x < 0) { return (($x - 3) * 2) + 5; }
    return (($x + 3) * 2) - 5;
}
foreach ([0, -3, 7, 1.25, '9', PHP_INT_MAX, PHP_INT_MIN] as $input) {
    for ($warm = 0; $warm < 128; $warm++) {
        $result = [scalarIdentity($input), scalarOne($input), scalarTwo($input),
            scalarSeven($input), scalarEight($input), scalarNine($input), scalarBranch($input)];
    }
    echo json_encode($result), "\n";
}
