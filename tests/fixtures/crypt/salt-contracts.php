<?php
// Independent parser boundaries, bounded work factors only.
foreach (['5', '6'] as $id) {
    foreach (['1000', '01000', '001000', '+1000', ' 1000', "\t1000", '-1000', '-1', '+0', '0', '000', '1', '1000x', 'x1000', '', 'no', '1000 ', '01000x'] as $rounds) {
        $salt = '$'.$id.'$rounds='.$rounds.'$seed$';
        echo json_encode(['name'=>"sha$id/$rounds", 'input'=>bin2hex('probe'), 'salt'=>bin2hex($salt), 'output'=>bin2hex(crypt('probe', $salt))]), "\n";
    }
}
foreach (['1', '5', '6'] as $id) {
    foreach (['a b', 'a:b', 'a;b', 'a!b', 'a*b', "a\x80\xffb", "a\nb", 'a\\b', 'a=b', 'a"b', 'a.b', '', 'rounds=no', 'rounds=1000', 'rounds=01000'] as $raw) {
        $salt = '$'.$id.'$'.$raw;
        echo json_encode(['name'=>"raw$id/".bin2hex($raw), 'input'=>bin2hex('probe'), 'salt'=>bin2hex($salt), 'output'=>bin2hex(crypt('probe', $salt))]), "\n";
    }
}
