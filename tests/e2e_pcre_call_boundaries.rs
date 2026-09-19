mod common;

use common::run_php;

#[test]
fn pcre_delimiters_follow_php_spacing_escaping_nesting_and_nul_rules() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function(int $level, string $message): bool {
    echo "warning:", $message, "\n";
    return true;
});
foreach ([
    ["   ", ""],
    ["@@", "anything"],
    ["{a{1}b}", "ab"],
    ["@a\\@b@", "a@b"],
    ["/a/  S\r\n", "a"],
    ["{", ""],
    ["//\0i", ""],
] as [$pattern, $subject]) {
    var_dump(preg_match($pattern, $subject));
}
"#,
        ),
        concat!(
            "warning:preg_match(): Empty regular expression\n",
            "bool(false)\n",
            "int(1)\n",
            "int(1)\n",
            "int(1)\n",
            "int(1)\n",
            "warning:preg_match(): No ending matching delimiter '}' found\n",
            "bool(false)\n",
            "warning:preg_match(): NUL byte is not a valid modifier\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn pcre_string_pattern_boundaries_reject_arrays_and_objects_without_writeback() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['preg_match', 'preg_match_all', 'preg_split'] as $function) {
    foreach ([[], new stdClass()] as $pattern) {
        $matches = 'kept';
        try {
            if ($function === 'preg_match') {
                $function($pattern, 'abc', $matches);
            } elseif ($function === 'preg_match_all') {
                $function($pattern, 'abc', $matches);
            } else {
                $function($pattern, 'abc');
            }
        } catch (Throwable $error) {
            echo $error->getMessage(), "|", $matches, "\n";
        }
    }
}
"#,
        ),
        concat!(
            "preg_match(): Argument #1 ($pattern) must be of type string, array given|kept\n",
            "preg_match(): Argument #1 ($pattern) must be of type string, stdClass given|kept\n",
            "preg_match_all(): Argument #1 ($pattern) must be of type string, array given|kept\n",
            "preg_match_all(): Argument #1 ($pattern) must be of type string, stdClass given|kept\n",
            "preg_split(): Argument #1 ($pattern) must be of type string, array given|kept\n",
            "preg_split(): Argument #1 ($pattern) must be of type string, stdClass given|kept\n",
        )
    );
}

#[test]
fn preg_replace_validates_conditional_array_and_union_contracts_before_matching() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    static fn() => preg_replace('/a/', [], 'a'),
    static fn() => preg_replace(['/a/'], new stdClass(), 'a'),
    static fn() => preg_replace(['/a/'], ['x'], new stdClass()),
] as $operation) {
    try {
        var_dump($operation());
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "TypeError:preg_replace(): Argument #1 ($pattern) must be of type array when argument #2 ($replacement) is an array, string given\n",
            "TypeError:preg_replace(): Argument #2 ($replacement) must be of type array|string, stdClass given\n",
            "TypeError:preg_replace(): Argument #3 ($subject) must be of type array|string, stdClass given\n",
        )
    );
}

#[test]
fn preg_replace_callback_maps_pattern_and_subject_arrays_in_order() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function(int $level, string $message): bool {
    echo "warning:", $message, "\n";
    return true;
});
$count = -1;
var_dump(preg_replace_callback(
    ['/@/', '/b/'],
    static fn(array $match): string => strtoupper($match[0]),
    ['k' => 'a@b', 7 => ['b']],
    -1,
    $count,
), $count);
try {
    preg_replace_callback(['/a/', new stdClass()], static fn() => 'x', 'a');
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "warning:Array to string conversion\n",
            "array(2) {\n",
            "  [\"k\"]=>\n  string(3) \"a@B\"\n",
            "  [7]=>\n  string(5) \"Array\"\n",
            "}\n",
            "int(2)\n",
            "Error:Object of class stdClass could not be converted to string\n",
        )
    );
}
