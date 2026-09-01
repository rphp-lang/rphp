mod common;

use common::run_php;

const CTYPE_FUNCTIONS: &str = r#"[
    'ctype_alnum', 'ctype_alpha', 'ctype_cntrl', 'ctype_digit',
    'ctype_graph', 'ctype_lower', 'ctype_print', 'ctype_punct',
    'ctype_space', 'ctype_upper', 'ctype_xdigit',
]"#;

#[test]
fn ctype_surface_exposes_all_php_85_signatures() {
    assert_eq!(
        run_php(&format!(
            r#"<?php
foreach ({CTYPE_FUNCTIONS} as $name) {{
    $function = new ReflectionFunction($name);
    $parameter = $function->getParameters()[0];
    echo $function->getName(), ':', $function->getNumberOfRequiredParameters(), ':',
        $function->getNumberOfParameters(), ':', $parameter->getName(), ':',
        $parameter->getType()?->getName(), ':', (int) $parameter->allowsNull(), ':',
        $function->getReturnType()?->getName(), '|';
}}
"#
        )),
        concat!(
            "ctype_alnum:1:1:text:mixed:1:bool|ctype_alpha:1:1:text:mixed:1:bool|",
            "ctype_cntrl:1:1:text:mixed:1:bool|ctype_digit:1:1:text:mixed:1:bool|",
            "ctype_graph:1:1:text:mixed:1:bool|ctype_lower:1:1:text:mixed:1:bool|",
            "ctype_print:1:1:text:mixed:1:bool|ctype_punct:1:1:text:mixed:1:bool|",
            "ctype_space:1:1:text:mixed:1:bool|ctype_upper:1:1:text:mixed:1:bool|",
            "ctype_xdigit:1:1:text:mixed:1:bool|",
        )
    );
}

#[test]
fn ctype_full_byte_sets_match_the_c_locale_contract() {
    assert_eq!(
        run_php(&format!(
            r#"<?php
foreach ({CTYPE_FUNCTIONS} as $name) {{
    $hits = [];
    for ($byte = 0; $byte <= 255; $byte++) {{
        if ($name(chr($byte))) $hits[] = $byte;
    }}
    echo $name, '=', implode(',', $hits), "\n";
}}
"#
        )),
        concat!(
            "ctype_alnum=48,49,50,51,52,53,54,55,56,57,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,97,98,99,100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122\n",
            "ctype_alpha=65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,97,98,99,100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122\n",
            "ctype_cntrl=0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,127\n",
            "ctype_digit=48,49,50,51,52,53,54,55,56,57\n",
            "ctype_graph=33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,97,98,99,100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126\n",
            "ctype_lower=97,98,99,100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122\n",
            "ctype_print=32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,97,98,99,100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126\n",
            "ctype_punct=33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,58,59,60,61,62,63,64,91,92,93,94,95,96,123,124,125,126\n",
            "ctype_space=9,10,11,12,13,32\n",
            "ctype_upper=65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90\n",
            "ctype_xdigit=48,49,50,51,52,53,54,55,56,57,65,66,67,68,69,70,97,98,99,100,101,102\n",
        )
    );
}

#[test]
fn ctype_requires_every_byte_and_rejects_empty_or_non_ascii_strings() {
    assert_eq!(
        run_php(&format!(
            r#"<?php
$values = ['', 'AA', 'A!', "A\0", "A\n", "\x80", "\xff", 'é'];
foreach ({CTYPE_FUNCTIONS} as $name) {{
    echo $name, '=';
    foreach ($values as $value) echo (int) $name($value);
    echo "\n";
}}
"#
        )),
        concat!(
            "ctype_alnum=01000000\n",
            "ctype_alpha=01000000\n",
            "ctype_cntrl=00000000\n",
            "ctype_digit=00000000\n",
            "ctype_graph=01100000\n",
            "ctype_lower=00000000\n",
            "ctype_print=01100000\n",
            "ctype_punct=00000000\n",
            "ctype_space=00000000\n",
            "ctype_upper=01000000\n",
            "ctype_xdigit=01000000\n",
        )
    );
}

#[test]
fn ctype_integer_legacy_mapping_covers_boundaries_and_large_values() {
    assert_eq!(
        run_php(&format!(
            r#"<?php
$values = [-129,-128,-1,0,9,10,11,12,13,32,33,47,48,57,58,64,65,70,71,90,91,96,97,102,103,122,123,126,127,128,255,256,1000,PHP_INT_MAX,PHP_INT_MIN];
foreach ({CTYPE_FUNCTIONS} as $name) {{
    echo $name, '=';
    foreach ($values as $value) echo (int) @$name($value);
    echo "\n";
}}
"#
        )),
        concat!(
            "ctype_alnum=00000000000011001111001111000001110\n",
            "ctype_alpha=00000000000000001111001111000000000\n",
            "ctype_cntrl=00011111100000000000000000001000000\n",
            "ctype_digit=00000000000011000000000000000001110\n",
            "ctype_graph=10000000001111111111111111110001111\n",
            "ctype_lower=00000000000000000000001111000000000\n",
            "ctype_print=10000000011111111111111111110001111\n",
            "ctype_punct=00000000001100110000110000110000000\n",
            "ctype_space=00001111110000000000000000000000000\n",
            "ctype_upper=00000000000000001111000000000000000\n",
            "ctype_xdigit=00000000000011001100001100000001110\n",
        )
    );
}

#[test]
fn ctype_non_string_types_deprecate_without_coercion() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $severity, string $message): bool {
    echo $severity, ':', $message, "\n";
    return true;
});
final class CtypeStringable {
    public function __toString(): string { echo "unexpected conversion\n"; return 'A'; }
}
$stream = fopen('php://temp', 'r');
foreach ([null, false, true, 65.0, [], new stdClass(), $stream,
          static fn () => null, new CtypeStringable()] as $value) {
    var_dump(ctype_alpha($value));
}
fclose($stream);
"#,
        ),
        concat!(
            "8192:ctype_alpha(): Argument of type null will be interpreted as string in the future\n",
            "bool(false)\n",
            "8192:ctype_alpha(): Argument of type bool will be interpreted as string in the future\n",
            "bool(false)\n",
            "8192:ctype_alpha(): Argument of type bool will be interpreted as string in the future\n",
            "bool(false)\n",
            "8192:ctype_alpha(): Argument of type float will be interpreted as string in the future\n",
            "bool(false)\n",
            "8192:ctype_alpha(): Argument of type array will be interpreted as string in the future\n",
            "bool(false)\n",
            "8192:ctype_alpha(): Argument of type stdClass will be interpreted as string in the future\n",
            "bool(false)\n",
            "8192:ctype_alpha(): Argument of type resource will be interpreted as string in the future\n",
            "bool(false)\n",
            "8192:ctype_alpha(): Argument of type Closure will be interpreted as string in the future\n",
            "bool(false)\n",
            "8192:ctype_alpha(): Argument of type CtypeStringable will be interpreted as string in the future\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn every_ctype_integer_path_uses_its_canonical_deprecation_name() {
    assert_eq!(
        run_php(&format!(
            r#"<?php
set_error_handler(static function (int $severity, string $message): bool {{
    echo $severity, ':', $message, '|';
    return true;
}});
foreach ({CTYPE_FUNCTIONS} as $name) echo (int) $name(65), "\n";
"#
        )),
        concat!(
            "8192:ctype_alnum(): Argument of type int will be interpreted as string in the future|1\n",
            "8192:ctype_alpha(): Argument of type int will be interpreted as string in the future|1\n",
            "8192:ctype_cntrl(): Argument of type int will be interpreted as string in the future|0\n",
            "8192:ctype_digit(): Argument of type int will be interpreted as string in the future|0\n",
            "8192:ctype_graph(): Argument of type int will be interpreted as string in the future|1\n",
            "8192:ctype_lower(): Argument of type int will be interpreted as string in the future|0\n",
            "8192:ctype_print(): Argument of type int will be interpreted as string in the future|1\n",
            "8192:ctype_punct(): Argument of type int will be interpreted as string in the future|0\n",
            "8192:ctype_space(): Argument of type int will be interpreted as string in the future|0\n",
            "8192:ctype_upper(): Argument of type int will be interpreted as string in the future|1\n",
            "8192:ctype_xdigit(): Argument of type int will be interpreted as string in the future|1\n",
        )
    );
}

#[test]
fn ctype_deprecation_is_reentrant_snapshots_the_argument_and_can_throw() {
    assert_eq!(
        run_php(
            r#"<?php
$value = 48;
set_error_handler(function (int $severity, string $message) use (&$value): bool {
    echo $value, ':', (int) ctype_alpha('A'), '|';
    $value = 65;
    return true;
});
var_dump(ctype_digit($value), $value);
restore_error_handler();
set_error_handler(static function (): never { throw new RuntimeException('stop'); });
try { ctype_alpha(65); }
catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
"#,
        ),
        "48:1|bool(true)\nint(65)\nRuntimeException:stop\n"
    );
}

#[test]
fn ctype_mixed_contract_is_identical_under_strict_types() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
set_error_handler(static function (int $severity, string $message): bool {
    echo $severity, ':', $message, '|';
    return true;
});
foreach ([65, 65.0, null, true, 'A'] as $value) {
    echo (int) ctype_alpha($value), "\n";
}
"#,
        ),
        concat!(
            "8192:ctype_alpha(): Argument of type int will be interpreted as string in the future|1\n",
            "8192:ctype_alpha(): Argument of type float will be interpreted as string in the future|0\n",
            "8192:ctype_alpha(): Argument of type null will be interpreted as string in the future|0\n",
            "8192:ctype_alpha(): Argument of type bool will be interpreted as string in the future|0\n",
            "1\n",
        )
    );
}

#[test]
fn ctype_functions_share_named_dynamic_first_class_and_callback_dispatch() {
    assert_eq!(
        run_php(
            r#"<?php
$dynamic = 'ctype_digit';
$first = ctype_xdigit(...);
echo (int) ctype_alpha(text: 'Az'), '|';
echo (int) $dynamic('09'), '|';
echo (int) $first('aF09'), '|';
echo (int) call_user_func('ctype_punct', '!?'), '|';
echo (int) call_user_func_array('ctype_space', ['text' => " \t"]), '|';
echo implode('', array_map('ctype_upper', ['ABC', 'AbC'])), "\n";
"#,
        ),
        "1|1|1|1|1|1\n"
    );
}

#[test]
fn ctype_registration_owns_arity_and_named_argument_errors() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    static fn () => ctype_alpha(),
    static fn () => ctype_alpha('a', 'b'),
    static fn () => ctype_alpha(nope: 'a'),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "ArgumentCountError:ctype_alpha() expects exactly 1 argument, 0 given\n",
            "ArgumentCountError:ctype_alpha() expects exactly 1 argument, 2 given\n",
            "Error:Unknown named parameter $nope\n",
        )
    );
}

#[test]
fn ctype_inventory_is_case_insensitive_and_available_to_namespaced_fallback() {
    assert_eq!(
        run_php(&format!(
            r#"<?php
namespace CtypeProbe;
echo (int) \function_exists('CTYPE_ALPHA'), ':',
    (new \ReflectionFunction('CTYPE_ALPHA'))->getName(), ':';
echo (int) ctype_alpha('Az'), ':', (int) \CTYPE_DIGIT('09'), '|';
$internal = \get_defined_functions()['internal'];
foreach ({CTYPE_FUNCTIONS} as $name) echo (int) \in_array($name, $internal, true);
echo "\n";
"#
        )),
        "1:ctype_alpha:1:1|11111111111\n"
    );
}

#[test]
fn ctype_reads_references_without_mutating_or_detaching_them() {
    assert_eq!(
        run_php(&format!(
            r#"<?php
$text = 'Az09';
$alias =& $text;
foreach ({CTYPE_FUNCTIONS} as $name) echo (int) $name($alias);
echo '|', $text, ':', $alias, '|';
$text[0] = '!';
echo $text, ':', $alias, "\n";
"#
        )),
        "10001010000|Az09:Az09|!z09:!z09\n"
    );
}
