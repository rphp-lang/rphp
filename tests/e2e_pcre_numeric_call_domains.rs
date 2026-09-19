mod common;

use common::run_php;

#[test]
fn match_numeric_domain_errors_follow_compile_and_writeback_order() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function(int $level, string $message): bool {
    echo "warning:$message\n";
    return true;
});
foreach (['preg_match', 'preg_match_all'] as $name) {
    foreach ([
        ['/a/', 0, PHP_INT_MIN],
        ['/a/', 4095, PHP_INT_MIN],
        ['/a/', 4095, 0],
        ['bad', 4095, 0],
    ] as [$pattern, $flags, $offset]) {
        $matches = ['kept'];
        try { var_dump($name($pattern, 'a', $matches, $flags, $offset)); }
        catch (Throwable $error) { echo $error->getMessage(), "\n"; }
        echo json_encode($matches), "\n";
    }
}
"#,
        ),
        concat!(
            "preg_match(): Argument #5 ($offset) must be greater than -9223372036854775808\n",
            "[\"kept\"]\n",
            "preg_match(): Argument #5 ($offset) must be greater than -9223372036854775808\n",
            "[\"kept\"]\n",
            "preg_match(): Argument #4 ($flags) must be a PREG_* constant\n",
            "[]\n",
            "warning:preg_match(): Delimiter must not be alphanumeric, backslash, or NUL byte\n",
            "bool(false)\n[\"kept\"]\n",
            "preg_match_all(): Argument #5 ($offset) must be greater than -9223372036854775808\n",
            "[\"kept\"]\n",
            "preg_match_all(): Argument #5 ($offset) must be greater than -9223372036854775808\n",
            "[\"kept\"]\n",
            "preg_match_all(): Argument #4 ($flags) must be a PREG_* constant\n",
            "[]\n",
            "warning:preg_match_all(): Delimiter must not be alphanumeric, backslash, or NUL byte\n",
            "bool(false)\n[\"kept\"]\n",
        )
    );
}

#[test]
fn valid_match_flag_combinations_remain_accepted() {
    assert_eq!(
        run_php(
            r#"<?php
$matches = [];
echo preg_match('/a/', 'a', $matches, PREG_OFFSET_CAPTURE), '|';
echo preg_match('/a/', 'a', $matches, PREG_UNMATCHED_AS_NULL), '|';
echo preg_match_all('/a/', 'a', $matches, PREG_PATTERN_ORDER), '|';
echo preg_match_all('/a/', 'a', $matches, PREG_SET_ORDER), "\n";
"#,
        ),
        "1|1|1|1\n"
    );
}
