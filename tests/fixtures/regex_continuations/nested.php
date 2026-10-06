<?php
$cases = [
 ['~^((?:ab|a)(?:bc|b))c$~', 'abc'],
 ['~^(?:(?:ab|a)(?:bc|b))c$~', 'abcc'],
 ['~^(a(?:(b|bc)d|b))(d|cd)$~', 'abcd'],
 ['~^(a(?:(b|bc)d|b))(d|cd)$~', 'abdd'],
 ['~^((?:ab|a)+)(b)$~', 'abab'],
 ['~^((?:ab|a)+?)(b)$~', 'abab'],
 ['~^((?:ab|a)*)(c)?$~', 'aba'],
 ['~^((?:ab|a)*)$~', ''],
 ['~^(?:(a|ab))(?=(b|bc))(bc)$~', 'abc'],
 ['~^(?<outer>(?:ab|a)(?<inner>b|bc))c$~', 'abcc'],
 ['~^(a|ab)(?1)b$~', 'aab'],
 ['~^(?(DEFINE)(?<pair>\((?:[0-9]+|(?&pair)),(?:[0-9]+|(?&pair))\)))(?&pair)!$~', '(1,(2,3))!'],
 ['~^(a|b)(*MARK:outer)(?:(c|cd))d$~', 'acdd'],
 ['~^(?:(a(*THEN)b)|(ac))d$~', 'acd'],
 ['~^(?:(a(*PRUNE)b)|(ac))d$~', 'acd'],
 ['~^(?:(a|ab)(*ACCEPT))c$~', 'ab'],
 ['~^(?:a(*THEN)|b)+c$~', 'abbc'],
 ['~^(?:a(*THEN)|b)+c$~', 'abbd'],
 ['~^(?<x>é|éé)(?:x|xy)(é)$~u', 'éxyé'],
];
foreach ($cases as $case) {
    $groups = [];
    $matched = preg_match($case[0], $case[1], $groups, PREG_OFFSET_CAPTURE | PREG_UNMATCHED_AS_NULL);
    echo json_encode([$matched, $groups, preg_last_error()]), "\n";
}
