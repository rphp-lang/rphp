mod common;

use common::run_php;

#[test]
fn slash_escaping_preserves_bytes_ranges_and_decode_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
$input = "\0\x01\x07\x08\t\n\v\f\r\x1f'\"\x7f\x80\xff\\";
$added = addslashes($input);
echo "add=", strlen($added), ":", bin2hex($added), "\n";
echo "round=", strlen(stripslashes($added)), ":", bin2hex(stripslashes($added)), "\n";
$unicode = "žluťoučký'\\";
echo "unicode=", bin2hex(addslashes($unicode)), "\n";

$escapes = [
    "\\", "\\0", "\\0000", "\\377", "\\400", "\\777",
    "\\8", "\\09", "\\x", "\\x0", "\\x000", "\\xFF",
    "\\xfg", "\\xG", "\\a\\b\\t\\n\\v\\f\\r\\q",
];
foreach ($escapes as $index => $escape) {
    echo "decode/", $index, "=", bin2hex(stripcslashes($escape)), ":",
        bin2hex(stripslashes($escape)), "\n";
}

set_error_handler(function (int $level, string $message): bool {
    echo "diag=", $level, ":", $message, "\n";
    return true;
});
$sample = ".azAZ09\0\x1f\x7f\x80\xff";
foreach (['a..z', 'z..a', '..a', 'a..', 'a...z', 'a....z', "\0..\x1f", "\x7f..\xff"] as $index => $characters) {
    echo "mask/", $index, "=", bin2hex(addcslashes($sample, $characters)), "\n";
}

foreach ([null, false, true, 0, 12, 1.5] as $index => $value) {
    echo "weak/", $index, "=", bin2hex(addslashes($value)), "\n";
}
"#,
        ),
        r#"add=20:5c30010708090a0b0c0d1f5c275c227f80ff5c5c
round=16:00010708090a0b0c0d1f27227f80ff5c
unicode=c5be6c75c5a56f75c48d6bc3bd5c275c5c
decode/0=5c:
decode/1=00:00
decode/2=0030:00303030
decode/3=ff:333737
decode/4=00:343030
decode/5=ff:373737
decode/6=38:38
decode/7=0039:0039
decode/8=78:78
decode/9=00:7830
decode/10=0030:78303030
decode/11=ff:784646
decode/12=0f67:786667
decode/13=7847:7847
decode/14=0708090a0b0c0d71:6162746e76667271
mask/0=2e5c615c7a415a3039001f7f80ff
mask/1=diag=2:addcslashes(): Invalid '..'-range, '..'-range needs to be incrementing
5c2e5c615c7a415a3039001f7f80ff
mask/2=diag=2:addcslashes(): Invalid '..'-range, no character to the left of '..'
5c2e5c617a415a3039001f7f80ff
mask/3=diag=2:addcslashes(): Invalid '..'-range, no character to the right of '..'
5c2e5c617a415a3039001f7f80ff
mask/4=5c2e5c615c7a5c415c5a5c305c39001f7f80ff
mask/5=5c2e5c615c7a415a3039001f7f80ff
mask/6=2e617a415a30395c3030305c3033377f80ff
mask/7=2e617a415a3039001f5c3137375c3230305c333737
weak/0=diag=8192:addslashes(): Passing null to parameter #1 ($string) of type string is deprecated

weak/1=
weak/2=31
weak/3=30
weak/4=3132
weak/5=312e35
"#,
    );
}

#[test]
fn slash_escaping_rejects_weak_scalar_coercion_in_strict_calls() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
foreach (['addslashes', 'stripslashes', 'stripcslashes'] as $function) {
    try {
        $function(12);
    } catch (TypeError $error) {
        echo $error->getMessage(), "\n";
    }
}
try {
    addcslashes('value', 12);
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "addslashes(): Argument #1 ($string) must be of type string, int given\n",
            "stripslashes(): Argument #1 ($string) must be of type string, int given\n",
            "stripcslashes(): Argument #1 ($string) must be of type string, int given\n",
            "addcslashes(): Argument #2 ($characters) must be of type string, int given\n",
        ),
    );
}
