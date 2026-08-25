mod common;

use common::run_php;

#[test]
fn euc_jp_and_big5_recovery_preserves_restart_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function () { return true; });
$sets = [
    'EUC-JP' => [
        'standalone' => "\x80<&",
        'valid' => "\x8E\xA1\x8F\xA1\xA1\xA1\xA1Z",
        'consume' => "\x8F\xA0\xA1!",
        'restart' => "\x8F\xA1!",
        'pair' => "\xB2\xFF!",
    ],
    'BIG5' => [
        'standalone' => "\x80\xFF<&",
        'valid' => "\x81\x40\xFE\xFEZ",
        'below' => "\x81\x3F!",
        'gap' => "\x81\x7F!",
        'restart' => "\x81\x82\x40!",
    ],
];
$flags = [
    'reject' => ENT_QUOTES,
    'substitute' => ENT_QUOTES | ENT_SUBSTITUTE,
    'ignore' => ENT_QUOTES | ENT_IGNORE,
    'both' => ENT_QUOTES | ENT_SUBSTITUTE | ENT_IGNORE,
];
foreach ($sets as $encoding => $inputs) {
    foreach ($inputs as $name => $input) {
        foreach ($flags as $flagName => $flag) {
            $special = htmlspecialchars($input, $flag, $encoding);
            $entities = htmlentities($input, $flag, $encoding);
            echo $encoding, '/', $name, '/', $flagName, '=',
                bin2hex($special), '|', bin2hex($entities), "\n";
        }
    }
}
"#,
        ),
        concat!(
            "EUC-JP/standalone/reject=80266c743b26616d703b|80266c743b26616d703b\n",
            "EUC-JP/standalone/substitute=80266c743b26616d703b|80266c743b26616d703b\n",
            "EUC-JP/standalone/ignore=80266c743b26616d703b|80266c743b26616d703b\n",
            "EUC-JP/standalone/both=80266c743b26616d703b|80266c743b26616d703b\n",
            "EUC-JP/valid/reject=8ea18fa1a1a1a15a|8ea18fa1a1a1a15a\n",
            "EUC-JP/valid/substitute=8ea18fa1a1a1a15a|8ea18fa1a1a1a15a\n",
            "EUC-JP/valid/ignore=8ea18fa1a1a1a15a|8ea18fa1a1a1a15a\n",
            "EUC-JP/valid/both=8ea18fa1a1a1a15a|8ea18fa1a1a1a15a\n",
            "EUC-JP/consume/reject=|\n",
            "EUC-JP/consume/substitute=262378464646443b262378464646443b21|262378464646443b262378464646443b21\n",
            "EUC-JP/consume/ignore=21|21\n",
            "EUC-JP/consume/both=21|21\n",
            "EUC-JP/restart/reject=|\n",
            "EUC-JP/restart/substitute=262378464646443b262378464646443b21|262378464646443b262378464646443b21\n",
            "EUC-JP/restart/ignore=21|21\n",
            "EUC-JP/restart/both=21|21\n",
            "EUC-JP/pair/reject=|\n",
            "EUC-JP/pair/substitute=262378464646443b21|262378464646443b21\n",
            "EUC-JP/pair/ignore=21|21\n",
            "EUC-JP/pair/both=21|21\n",
            "BIG5/standalone/reject=80ff266c743b26616d703b|80ff266c743b26616d703b\n",
            "BIG5/standalone/substitute=80ff266c743b26616d703b|80ff266c743b26616d703b\n",
            "BIG5/standalone/ignore=80ff266c743b26616d703b|80ff266c743b26616d703b\n",
            "BIG5/standalone/both=80ff266c743b26616d703b|80ff266c743b26616d703b\n",
            "BIG5/valid/reject=8140fefe5a|8140fefe5a\n",
            "BIG5/valid/substitute=8140fefe5a|8140fefe5a\n",
            "BIG5/valid/ignore=8140fefe5a|8140fefe5a\n",
            "BIG5/valid/both=8140fefe5a|8140fefe5a\n",
            "BIG5/below/reject=|\n",
            "BIG5/below/substitute=262378464646443b3f21|262378464646443b3f21\n",
            "BIG5/below/ignore=3f21|3f21\n",
            "BIG5/below/both=3f21|3f21\n",
            "BIG5/gap/reject=|\n",
            "BIG5/gap/substitute=262378464646443b7f21|262378464646443b7f21\n",
            "BIG5/gap/ignore=7f21|7f21\n",
            "BIG5/gap/both=7f21|7f21\n",
            "BIG5/restart/reject=|\n",
            "BIG5/restart/substitute=262378464646443b824021|262378464646443b824021\n",
            "BIG5/restart/ignore=824021|824021\n",
            "BIG5/restart/both=824021|824021\n",
        )
    );
}

#[test]
fn multibyte_aliases_share_basic_entities() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) {
    echo 'diag=', $level, ':', $message, "\n";
    return true;
});
foreach ([
    'EUC-JP' => "\xA1\xA1&amp;<",
    'EUCJP' => "\x8E\xA1&amp;<",
    'eucJP-win' => "\x8F\xA1\xA1&amp;<",
    'BIG5' => "\x81\x40&amp;<",
] as $encoding => $input) {
    $output = htmlentities($input, ENT_QUOTES, $encoding, false);
    echo $encoding, '=', bin2hex($output), "\n";
}
"#,
        ),
        concat!(
            "diag=8:htmlentities(): Only basic entities substitution is supported for multi-byte encodings other than UTF-8; functionality is equivalent to htmlspecialchars\n",
            "EUC-JP=a1a126616d703b266c743b\n",
            "diag=8:htmlentities(): Only basic entities substitution is supported for multi-byte encodings other than UTF-8; functionality is equivalent to htmlspecialchars\n",
            "EUCJP=8ea126616d703b266c743b\n",
            "diag=8:htmlentities(): Only basic entities substitution is supported for multi-byte encodings other than UTF-8; functionality is equivalent to htmlspecialchars\n",
            "eucJP-win=8fa1a126616d703b266c743b\n",
            "diag=8:htmlentities(): Only basic entities substitution is supported for multi-byte encodings other than UTF-8; functionality is equivalent to htmlspecialchars\n",
            "BIG5=814026616d703b266c743b\n",
        )
    );
}
