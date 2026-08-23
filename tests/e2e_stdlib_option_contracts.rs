mod common;

use common::run_php;

#[test]
fn array_unique_preserves_first_keys_live_references_and_enum_identity() {
    assert_eq!(
        run_php(
            r#"<?php
enum Backed: string { case A = "a"; case B = "b"; }
enum Pure { case A; case B; }

$alias = Backed::A;
$backed = [
    "a" => Backed::A,
    "dup" => Backed::A,
    "b" => Backed::B,
    "ref" => &$alias,
];
var_dump(array_unique(array: $backed, flags: SORT_REGULAR));
var_dump(array_unique([Pure::A, Pure::A, Pure::B], SORT_REGULAR));
var_dump(array_unique(["a", "a", "b", "a"]));
$many = [];
for ($index = 0; $index < 200; $index++) { $many[] = "value-$index"; }
$many = array_unique($many);
var_dump(count($many), array_key_last($many));

$value = "same";
$values = [
    "kept" => &$value,
    "removed" => "same",
    4 => 1,
    5 => "1",
    6 => null,
    7 => false,
];
$unique = array_unique($values);
$value = "changed";
var_dump($unique);
var_dump(array_unique([1, "1.0", 2, "2.0"], SORT_NUMERIC));

try { array_unique("bad"); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "array(2) {\n",
            "  [\"a\"]=>\n  enum(Backed::A)\n",
            "  [\"b\"]=>\n  enum(Backed::B)\n",
            "}\n",
            "array(2) {\n",
            "  [0]=>\n  enum(Pure::A)\n",
            "  [2]=>\n  enum(Pure::B)\n",
            "}\n",
            "array(2) {\n",
            "  [0]=>\n  string(1) \"a\"\n",
            "  [2]=>\n  string(1) \"b\"\n",
            "}\n",
            "int(200)\n",
            "int(199)\n",
            "array(3) {\n",
            "  [\"kept\"]=>\n  &string(7) \"changed\"\n",
            "  [4]=>\n  int(1)\n",
            "  [6]=>\n  NULL\n",
            "}\n",
            "array(2) {\n",
            "  [0]=>\n  int(1)\n",
            "  [2]=>\n  int(2)\n",
            "}\n",
            "array_unique(): Argument #1 ($array) must be of type array, string given\n",
        )
    );
}

#[test]
fn str_pad_matches_php_85_directions_bytes_and_value_errors() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    ["x", 6, "ab", STR_PAD_LEFT],
    ["x", 6, "ab", STR_PAD_RIGHT],
    ["x", 6, "ab", STR_PAD_BOTH],
    ["already", -1, "", 99],
] as $arguments) {
    var_dump(str_pad(...$arguments));
}
var_dump(bin2hex(str_pad("\0\xff", 7, "\x80a", STR_PAD_BOTH)));

try { str_pad("x", 2, ""); }
catch (ValueError $error) { echo $error->getMessage(), "\n"; }
try { str_pad("x", 3, "+", 99); }
catch (ValueError $error) { echo $error->getMessage(), "\n"; }
try { str_pad([], 3); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { str_pad("x", []); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "string(6) \"ababax\"\n",
            "string(6) \"xababa\"\n",
            "string(6) \"abxaba\"\n",
            "string(7) \"already\"\n",
            "string(14) \"806100ff806180\"\n",
            "str_pad(): Argument #3 ($pad_string) must not be empty\n",
            "str_pad(): Argument #4 ($pad_type) must be STR_PAD_LEFT, STR_PAD_RIGHT, or STR_PAD_BOTH\n",
            "str_pad(): Argument #1 ($string) must be of type string, array given\n",
            "str_pad(): Argument #2 ($length) must be of type int, array given\n",
        )
    );
}

#[test]
fn html_encoders_honor_quote_document_and_double_encode_controls() {
    assert_eq!(
        run_php(
            r#"<?php
$input = "<&>\"' &lt; &copy; &#60; &bogus;";
var_dump(htmlspecialchars($input));
var_dump(htmlspecialchars($input, ENT_NOQUOTES, "UTF-8", false));
var_dump(htmlspecialchars($input, ENT_QUOTES | ENT_HTML5, "UTF-8", false));
var_dump(htmlentities("The < character is encoded as &lt;", double_encode: false));
var_dump(htmlentities("<&>\"'", flags: ENT_COMPAT, encoding: null, double_encode: true));

try { htmlspecialchars([], ENT_QUOTES); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { htmlentities("x", flags: []); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { htmlspecialchars("x", encoding: []); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { htmlentities("x", double_encode: []); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "string(67) \"&lt;&amp;&gt;&quot;&#039; &amp;lt; &amp;copy; &amp;#60; &amp;bogus;\"\n",
            "string(45) \"&lt;&amp;&gt;\"' &lt; &copy; &#60; &amp;bogus;\"\n",
            "string(55) \"&lt;&amp;&gt;&quot;&apos; &lt; &copy; &#60; &amp;bogus;\"\n",
            "string(37) \"The &lt; character is encoded as &lt;\"\n",
            "string(20) \"&lt;&amp;&gt;&quot;'\"\n",
            "htmlspecialchars(): Argument #1 ($string) must be of type string, array given\n",
            "htmlentities(): Argument #2 ($flags) must be of type int, array given\n",
            "htmlspecialchars(): Argument #3 ($encoding) must be of type ?string, array given\n",
            "htmlentities(): Argument #4 ($double_encode) must be of type bool, array given\n",
        )
    );
}

#[test]
fn stdlib_option_contracts_respect_strict_scalar_types() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
try { array_unique([1], "2"); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { str_pad(123, 4); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { str_pad("x", 4, 1); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { htmlspecialchars(123); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { htmlentities("x", double_encode: 1); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "array_unique(): Argument #2 ($flags) must be of type int, string given\n",
            "str_pad(): Argument #1 ($string) must be of type string, int given\n",
            "str_pad(): Argument #3 ($pad_string) must be of type string, int given\n",
            "htmlspecialchars(): Argument #1 ($string) must be of type string, int given\n",
            "htmlentities(): Argument #4 ($double_encode) must be of type bool, int given\n",
        )
    );
}
