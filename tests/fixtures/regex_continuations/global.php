<?php
$cases = [
 ['~((?:ab|a)+)(b)~', 'abab aab abab'],
 ['~((a|ab)*)~', 'ab x aa'],
 ['~(?<outer>(?:ab|a)(?<inner>b|bc))c~', 'abcc abc'],
 ['~(?=(a|ab))((?:a|ab)b)~', 'ab abbb'],
];
foreach ($cases as $case) {
    $groups = [];
    $matched = preg_match_all($case[0], $case[1], $groups, PREG_SET_ORDER | PREG_OFFSET_CAPTURE | PREG_UNMATCHED_AS_NULL);
    echo json_encode([$matched, $groups, preg_last_error()]), "\n";
    echo preg_replace_callback($case[0], function ($m) { return '[' . implode(':', $m) . ']'; }, $case[1]), "\n";
    echo json_encode(preg_split($case[0], $case[1], -1, PREG_SPLIT_DELIM_CAPTURE | PREG_SPLIT_OFFSET_CAPTURE)), "\n";
}
