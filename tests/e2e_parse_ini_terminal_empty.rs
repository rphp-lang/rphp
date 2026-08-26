mod common;

use common::run_php;

#[test]
fn parse_ini_terminal_values_and_nul_match_php_85_in_every_scanner_mode() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([INI_SCANNER_NORMAL, INI_SCANNER_RAW, INI_SCANNER_TYPED] as $mode) {
    foreach ([false, true] as $sections) {
        $empty = parse_ini_string("\0ignored=1", $sections, $mode);
        $truncated = parse_ini_string("key=value\0ignored=1", $sections, $mode);
        $nulValue = parse_ini_string("key=\0ignored=1", $sections, $mode);
        $terminal = parse_ini_string("key=value \t", $sections, $mode);
        $terminated = parse_ini_string("key=value \t\r\n", $sections, $mode);
        echo $mode, '/', (int) $sections, '=';
        echo count($empty), '/';
        echo bin2hex((string) $truncated['key']), '/';
        echo bin2hex((string) $nulValue['key']), '/';
        echo bin2hex((string) $terminal['key']), '/';
        echo bin2hex((string) $terminated['key']), "\n";
    }
}
"#,
        ),
        concat!(
            "0/0=0/76616c7565//76616c75652009/76616c7565\n",
            "0/1=0/76616c7565//76616c75652009/76616c7565\n",
            "1/0=0/76616c7565//76616c7565/76616c7565\n",
            "1/1=0/76616c7565//76616c7565/76616c7565\n",
            "2/0=0/76616c7565//76616c75652009/76616c7565\n",
            "2/1=0/76616c7565//76616c75652009/76616c7565\n",
        ),
    );
}

#[test]
fn parse_ini_generated_terminal_matrix_matches_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
$record = '';
foreach ([INI_SCANNER_NORMAL, INI_SCANNER_RAW, INI_SCANNER_TYPED] as $mode) {
    foreach ([["", 0], ["\n", 1], ["\r", 1], ["\r\n", 1]] as [$ending, $terminated]) {
        foreach (["", " ", "\t", " \t"] as $padding) {
            $parsed = parse_ini_string('key=value' . $padding . $ending, false, $mode);
            $record .= $mode . '/' . $terminated . '/' . bin2hex($padding) . ':';
            $record .= bin2hex((string) $parsed['key']) . ';';
        }
    }
}
foreach (["\0", "key=value\0tail=lost", "key=\0tail=lost"] as $source) {
    $record .= bin2hex($source) . ':' . serialize(parse_ini_string($source)) . ';';
}
echo md5($record), "\n";
"#,
        ),
        "a50b77ed25b9d7f21267be37525928d1\n",
    );
}

#[test]
fn parse_ini_call_shapes_sections_and_cow_share_the_terminal_contract() {
    assert_eq!(
        run_php(
            r#"<?php
$source = "[part]\rkey=value \t";
$alias =& $source;
$copy = $source;
$dynamic = 'parse_ini_string';
$named = parse_ini_string(
    ini_string: $alias,
    process_sections: true,
    scanner_mode: INI_SCANNER_NORMAL,
);
$callback = call_user_func('parse_ini_string', $source, true, INI_SCANNER_RAW);
echo bin2hex($named['part']['key']), '/';
echo bin2hex($dynamic($source, true, INI_SCANNER_TYPED)['part']['key']), '/';
echo bin2hex($callback['part']['key']), '/';
echo $source === $copy ? 'cow/' : 'changed/';
echo $alias === $source ? "ref\n" : "split\n";
"#,
        ),
        "76616c75652009/76616c75652009/76616c7565/cow/ref\n",
    );
}
