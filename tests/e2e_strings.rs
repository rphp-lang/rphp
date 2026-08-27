/// E2E tests: strings, concatenation, escapes, UTF-8, truthiness.
mod common;
use common::{run_php, run_php_with_source_context};

// === String basics ===

#[test]
fn test_e2e_echo_string() {
    assert_eq!(run_php("<?php echo \"hello\";"), "hello");
}

#[test]
fn test_e2e_echo_single_quoted() {
    assert_eq!(run_php("<?php echo 'world';"), "world");
}

#[test]
fn binary_string_prefixes_preserve_quoted_string_behavior() {
    assert_eq!(
        run_php(
            r#"<?php
$name = 'RPHP';
echo b"plain", '|', B'single', '|', b"$name\n", '|', B"\x41";
"#,
        ),
        "plain|single|RPHP\n|A"
    );
}

#[test]
fn test_e2e_concat_strings() {
    assert_eq!(
        run_php("<?php echo \"hello\" . \" \" . \"world\";"),
        "hello world"
    );
}

#[test]
fn test_e2e_concat_string_int() {
    assert_eq!(run_php("<?php echo \"value: \" . 42;"), "value: 42");
}

#[test]
fn test_e2e_concat_variable() {
    assert_eq!(
        run_php("<?php $name = \"PHP\"; echo \"Hello \" . $name;"),
        "Hello PHP"
    );
}

#[test]
fn test_e2e_string_assign_echo() {
    assert_eq!(run_php("<?php $s = \"test\"; echo $s;"), "test");
}

#[test]
fn substr_saturates_a_negative_length_before_the_string_start() {
    assert_eq!(
        run_php("<?php echo '[' . substr('abcdef', 1, -99) . ']';"),
        "[]"
    );
}

// === Concat precedence ===

#[test]
fn test_e2e_concat_plus_precedence() {
    assert_eq!(run_php("<?php echo \"x\" . 1 + 2;"), "x3");
}

#[test]
fn test_e2e_concat_mul_precedence() {
    assert_eq!(run_php("<?php echo \"val\" . 3 * 4;"), "val12");
}

// === String concat in loop (tests Drop correctness) ===

#[test]
fn test_e2e_concat_in_loop() {
    assert_eq!(
        run_php(
            "<?php $s = \"\"; $i = 0; while ($i < 3) { $s = $s . \"x\"; $i = $i + 1; } echo $s;"
        ),
        "xxx"
    );
}

// === Reassign string variable (tests Drop on overwrite) ===

#[test]
fn test_e2e_string_reassign() {
    assert_eq!(
        run_php("<?php $s = \"hello\"; $s = \"world\"; echo $s;"),
        "world"
    );
}

// === UTF-8 ===

#[test]
fn test_e2e_utf8_string() {
    assert_eq!(run_php("<?php echo \"Ahoj světe\";"), "Ahoj světe");
}

#[test]
fn test_e2e_utf8_concat() {
    assert_eq!(run_php("<?php echo \"Č\" . \"esky\";"), "Česky");
}

// === String truthiness ===

#[test]
fn test_e2e_string_truthy() {
    assert_eq!(run_php("<?php if (\"hello\") echo 1;"), "1");
}

#[test]
fn test_e2e_empty_string_falsy() {
    assert_eq!(run_php("<?php if (\"\") echo 1;"), "");
}

#[test]
fn test_e2e_string_zero_falsy() {
    assert_eq!(run_php("<?php if (\"0\") echo 1;"), "");
}

// === Escape sequences ===

#[test]
fn test_e2e_double_quote_newline() {
    assert_eq!(run_php("<?php echo \"a\\nb\";"), "a\nb");
}

#[test]
fn test_e2e_double_quote_tab() {
    assert_eq!(run_php("<?php echo \"a\\tb\";"), "a\tb");
}

#[test]
fn test_e2e_double_quote_escaped_backslash() {
    assert_eq!(run_php("<?php echo \"a\\\\b\";"), "a\\b");
}

#[test]
fn test_e2e_double_quote_escaped_dollar() {
    assert_eq!(run_php("<?php echo \"a\\$b\";"), "a$b");
}

#[test]
fn test_e2e_double_quote_escaped_quote() {
    assert_eq!(run_php(r#"<?php echo "a\"b";"#), "a\"b");
}

#[test]
fn double_quoted_and_heredoc_strings_decode_hex_and_unicode_escapes() {
    assert_eq!(
        run_php(
            r#"<?php
echo 'A=', "\u{41}", '|bytes=', strlen("\u{202A}"), '|face=', "\u{1F642}", '|';
$value = <<<TEXT
heredoc=\u{2069}|hex=\x41\x42
TEXT;
echo $value;
"#,
        ),
        "A=A|bytes=3|face=🙂|heredoc=\u{2069}|hex=AB"
    );
}

#[test]
fn scalar_literals_flow_through_array_selection_and_transform_functions() {
    assert_eq!(
        run_php(
            r#"<?php
$scalars = [TrUe, fAlSe, nUlL];
$filtered = array_filter($scalars);
echo count($filtered), ':', (int) $filtered[0], '|';
echo array_rand([TrUe]), '|';

$escapes = "\e\f\v";
$document = <<<TEXT
\e\f\v
TEXT;
echo bin2hex($escapes), ':', bin2hex($document), '|';
echo count(array_diff([$escapes], ["\x1b\x0c\x0b"])), '|';
echo bin2hex(array_reverse([$escapes])[0]), '|';
$values = [];
array_unshift($values, $escapes);
echo bin2hex($values[0]);
"#,
        ),
        "1:1|0|1b0c0b:1b0c0b|0|1b0c0b|1b0c0b"
    );
}

#[test]
fn test_e2e_single_quote_literal_backslash_n() {
    assert_eq!(run_php("<?php echo 'a\\nb';"), "a\\nb");
}

#[test]
fn test_e2e_single_quote_escaped_backslash() {
    assert_eq!(run_php("<?php echo 'a\\\\b';"), "a\\b");
}

#[test]
fn test_e2e_single_quote_escaped_quote() {
    assert_eq!(run_php("<?php echo 'a\\'b';"), "a'b");
}

// ========== String interpolation ==========

#[test]
fn test_string_interpolation_basic() {
    assert_eq!(
        run_php("<?php $name = 'World'; echo \"Hello $name\";"),
        "Hello World"
    );
}

#[test]
fn test_string_interpolation_multiple_vars() {
    assert_eq!(
        run_php("<?php $a = 'foo'; $b = 'bar'; echo \"$a and $b\";"),
        "foo and bar"
    );
}

#[test]
fn test_string_interpolation_with_number() {
    assert_eq!(
        run_php("<?php $n = 42; echo \"The answer is $n\";"),
        "The answer is 42"
    );
}

#[test]
fn test_string_interpolation_escaped_dollar() {
    assert_eq!(run_php("<?php $x = 5; echo \"Cost: \\$x\";"), "Cost: $x");
}

#[test]
fn test_string_interpolation_curly_brace() {
    assert_eq!(
        run_php("<?php $fruit = 'banana'; echo \"I like {$fruit}s\";"),
        "I like bananas"
    );
}

#[test]
fn deprecated_dollar_brace_interpolation_preserves_both_variable_forms() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
$foo = 'bar';
$name = 'foo';
echo "${foo}|${ $name }";
"#,
            "/virtual/deprecated-interpolation.php",
            "/virtual",
        ),
        concat!(
            "\nDeprecated: Using ${var} in strings is deprecated, use {$var} instead in /virtual/deprecated-interpolation.php on line 4\n",
            "\nDeprecated: Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead in /virtual/deprecated-interpolation.php on line 4\n",
            "bar|bar",
        )
    );

    assert_eq!(
        run_php_with_source_context(
            "<?php\nif (false) { echo \"${missing}\"; }\necho 'ok';",
            "/virtual/dead-interpolation.php",
            "/virtual",
        ),
        concat!(
            "\nDeprecated: Using ${var} in strings is deprecated, use {$var} instead in /virtual/dead-interpolation.php on line 2\n",
            "ok",
        )
    );
}

#[test]
fn nested_same_label_heredoc_is_scanned_inside_dollar_brace_interpolation() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
${"inner"} = "resolved";
echo <<<OUT
    start
    ${<<<OUT
        inner
        OUT}
    end
    OUT;
"#,
            "/virtual/nested-heredoc.php",
            "/virtual",
        ),
        concat!(
            "\nDeprecated: Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead in /virtual/nested-heredoc.php on line 4\n",
            "start\nresolved\nend",
        )
    );
}

#[test]
fn simple_array_interpolation_accepts_php_index_forms() {
    assert_eq!(
        run_php(
            r#"<?php
$values = [
    'word' => 'alpha',
    0 => 'zero',
    '-0' => 'negative-zero',
    '00' => 'leading-zero',
    '0x0' => 'hex-shaped',
    '-0x0' => 'negative-hex-shaped',
    '9223372036854775808' => 'overflow',
];
$index = 'word';
echo "$values[word]|$values[0]|$values[$index]\n";
echo "$values[-0]|$values[00]|$values[0x0]|$values[-0x0]|$values[9223372036854775808]\n";
echo <<<DOC
$values[word]|$values[0]|$values[$index]
DOC;
"#,
        ),
        concat!(
            "alpha|zero|alpha\n",
            "negative-zero|leading-zero|hex-shaped|negative-hex-shaped|overflow\n",
            "alpha|zero|alpha",
        )
    );
}

#[test]
fn heredoc_octal_overflow_diagnostics_keep_compile_order() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
echo <<<DOC
outer=\400
inner=${"\400"}
DOC;
"#,
            "/virtual/heredoc-octal.php",
            "/virtual",
        ),
        concat!(
            "\nWarning: Octal escape sequence overflow \\400 is greater than \\377 in /virtual/heredoc-octal.php on line 3\n",
            "\nWarning: Octal escape sequence overflow \\400 is greater than \\377 in /virtual/heredoc-octal.php on line 4\n",
            "\nDeprecated: Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead in /virtual/heredoc-octal.php on line 3\n",
            "\nWarning: Undefined variable $\0 in /virtual/heredoc-octal.php on line 4\n",
            "outer=\0\ninner=",
        )
    );
}

#[test]
fn standalone_compound_statements_share_the_surrounding_scope() {
    assert_eq!(
        run_php("<?php { $value = 'inside'; } echo $value;"),
        "inside"
    );
}

#[test]
fn simple_property_interpolation_uses_normal_property_reads_and_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
class PropertyInterpolationProbe {
    private $hidden = 'H';
    public $shown = 'S';

    public function render($other) {
        echo "[$this->hidden][$this->shown][$other->shown]|";
        echo "$this->missing|";
        echo "$this->shown-tail";
    }
}
(new PropertyInterpolationProbe())->render(new PropertyInterpolationProbe());
"#,
        ),
        "[H][S][S]|\nWarning: Undefined property: PropertyInterpolationProbe::$missing in PropertyInterpolationProbe::render on line 8\n|S-tail"
    );
}

#[test]
fn nullsafe_string_interpolation_matches_simple_and_braced_expression_boundaries() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class NullsafeInterpolationProbe {
    public $shown = 'S';
    public function show() { return 'M'; }
}
$null = null;
$object = new NullsafeInterpolationProbe();
var_dump("$null?->shown", "$null?->show()", "{$null?->shown}", "{$null?->show()}");
var_dump("$object?->shown", "$object?->show()", "{$object?->shown}", "{$object?->show()}");
"#,
            "/virtual/nullsafe-interpolation.php",
            "/virtual",
        ),
        concat!(
            "string(0) \"\"\n",
            "string(2) \"()\"\n",
            "string(0) \"\"\n",
            "string(0) \"\"\n",
            "\nWarning: Undefined property: NullsafeInterpolationProbe::$show in /virtual/nullsafe-interpolation.php on line 9\n",
            "string(1) \"S\"\n",
            "string(2) \"()\"\n",
            "string(1) \"S\"\n",
            "string(1) \"M\"\n",
        )
    );
}

#[test]
fn test_string_interpolation_no_vars() {
    assert_eq!(run_php("<?php echo \"just a string\";"), "just a string");
}

#[test]
fn test_string_interpolation_only_var() {
    assert_eq!(run_php("<?php $x = 'hello'; echo \"$x\";"), "hello");
}

#[test]
fn test_string_interpolation_with_newline() {
    assert_eq!(run_php("<?php $n = 'Bob'; echo \"Hi $n\\n\";"), "Hi Bob\n");
}

#[test]
fn test_single_quote_no_interpolation() {
    assert_eq!(run_php("<?php $x = 5; echo '$x';"), "$x");
}

#[test]
fn test_interpolation_array_int_index() {
    assert_eq!(
        run_php("<?php $a = ['hello', 'world']; echo \"val: {$a[0]}\";"),
        "val: hello"
    );
}

#[test]
fn test_interpolation_array_string_key() {
    assert_eq!(
        run_php("<?php $m = ['name' => 'PHP']; echo \"lang: {$m['name']}\";"),
        "lang: PHP"
    );
}

#[test]
fn test_practical_string_interpolation_in_loop() {
    assert_eq!(
        run_php(
            "<?php
$names = ['Alice', 'Bob'];
foreach ($names as $name) {
    echo \"Hello $name! \";
}
"
        ),
        "Hello Alice! Hello Bob! "
    );
}

#[test]
fn test_heredoc_interpolation_and_flexible_indentation() {
    assert_eq!(
        run_php(
            "<?php
$name = 'PHP';
echo <<<TEXT
  Hello $name
    indented
  TEXT;
",
        ),
        "Hello PHP\n  indented"
    );
}

#[test]
fn complex_interpolation_calls_methods_in_order_and_propagates_exceptions() {
    assert_eq!(
        run_php(
            r#"<?php
final class InterpolationProbe {
    private $calls = 0;

    private function render($prefix, $value) {
        echo "call{$value}|";
        return $prefix . ++$this->calls;
    }

    private function zero() {
        return 'Z';
    }

    private function fail() {
        throw new RuntimeException('boom');
    }

    public function run() {
        echo "quoted=[{$this->render('Q', 2)}]|";
        echo <<<TEXT
            heredoc=[{$this->render('H', 3)}]|zero=[{$this->zero()}]
            TEXT;
        try {
            echo "|before{$this->fail()}after";
        } catch (RuntimeException $error) {
            echo '|caught:', $error->getMessage();
        }
    }
}

(new InterpolationProbe())->run();
"#,
        ),
        "call2|quoted=[Q1]|call3|heredoc=[H2]|zero=[Z]|caught:boom"
    );
}

#[test]
fn test_nowdoc_is_literal_and_supports_expression_terminator() {
    assert_eq!(
        run_php(
            "<?php
$name = 'PHP';
echo <<<'TEXT'
$name\\n
TEXT;
echo '|';
echo strlen(<<<TEXT
abcd
TEXT);
",
        ),
        "$name\\n|4"
    );
}

// ── COW string aliasing regression tests ──────────────────────────

#[test]
fn test_string_cow_assign_then_mutate() {
    // $b = $a shares Rc. $b .= must COW-detach, not mutate $a.
    assert_eq!(
        run_php(
            "<?php
$a = 'hello';
$b = $a;
$b .= ' world';
echo $a . '|' . $b;
"
        ),
        "hello|hello world"
    );
}

#[test]
fn test_string_cow_function_arg() {
    // Function arg is a clone (Rc bump). .= inside must not affect caller.
    assert_eq!(
        run_php(
            "<?php
function modify($s) { $s .= '!'; return $s; }
$x = 'test';
$y = modify($x);
echo $x . '|' . $y;
"
        ),
        "test|test!"
    );
}

#[test]
fn test_string_cow_multiple_clones() {
    // Multiple clones from same source — each .= independent.
    assert_eq!(
        run_php(
            "<?php
$s = 'base';
$c1 = $s;
$c2 = $s;
$c3 = $s;
$c1 .= '1';
$c2 .= '2';
echo $s . '|' . $c1 . '|' . $c2 . '|' . $c3;
"
        ),
        "base|base1|base2|base"
    );
}

#[test]
fn test_string_cow_sole_owner_inplace() {
    // Sole owner .= should mutate in place (no COW detach needed).
    assert_eq!(
        run_php(
            "<?php
$z = 'only';
$z .= ' me';
echo $z;
"
        ),
        "only me"
    );
}

#[test]
fn test_string_cow_in_array() {
    // String stored in array, copied out, mutated — original in array unchanged.
    assert_eq!(
        run_php(
            "<?php
$arr = ['key' => 'value'];
$copy = $arr['key'];
$copy .= '_modified';
echo $arr['key'] . '|' . $copy;
"
        ),
        "value|value_modified"
    );
}

#[test]
fn test_string_cow_closure_capture() {
    // Closure captures string. .= inside closure must not affect outer.
    assert_eq!(
        run_php(
            "<?php
$s = 'captured';
$fn = function() use ($s) { $s .= '!'; return $s; };
$r = $fn();
echo $s . '|' . $r;
"
        ),
        "captured|captured!"
    );
}

#[test]
fn test_string_cow_loop_append() {
    // Repeated .= on sole-owner string in a loop.
    assert_eq!(
        run_php(
            "<?php
$s = '';
for ($i = 0; $i < 5; $i = $i + 1) {
    $s .= 'x';
}
echo $s;
"
        ),
        "xxxxx"
    );
}

#[test]
fn test_string_self_concat_snapshots_rhs_before_mutation() {
    assert_eq!(
        run_php(
            "<?php
$value = 'ab';
$copy = $value;
for ($i = 0; $i < 22; ++$i) {
    $value .= $value;
}
$number = 12;
$number .= $number;
echo strlen($value), '|', $copy, '|', $number;
"
        ),
        "8388608|ab|1212"
    );
}

#[test]
fn test_string_cow_return_and_second_consumer() {
    // Function returns a string, two callers get it — independent copies.
    assert_eq!(
        run_php(
            "<?php
function make() { return 'base'; }
$a = make();
$b = make();
$a .= '1';
$b .= '2';
echo $a . '|' . $b;
"
        ),
        "base1|base2"
    );
}

#[test]
fn test_string_cow_nested_function_passthrough() {
    // String passed through two function calls, mutated at the end.
    assert_eq!(
        run_php(
            "<?php
function inner($s) { $s .= '!'; return $s; }
function outer($s) { return inner($s); }
$x = 'deep';
$y = outer($x);
echo $x . '|' . $y;
"
        ),
        "deep|deep!"
    );
}

#[test]
fn test_strspn_and_strcspn_with_ranges() {
    assert_eq!(
        run_php(
            "<?php echo strcspn('scheme:/path', ':/?#'), ':', strspn('abc123!', 'abc123'), ':', strcspn('xxabc!', '!', 2, 4);"
        ),
        "6:6:3"
    );
}

#[test]
fn double_quoted_octal_nul_escape_keeps_service_ids_valid() {
    assert_eq!(
        run_php(
            r#"<?php
$id = 'Rphp\SymfonyKernelFixture\HealthController';
echo strlen("\0\r\n'"), ':', strlen($id), ':', strcspn($id, "\0\r\n'"), ':', $id[-1];
"#,
        ),
        "4:42:42:r"
    );
}

#[test]
fn xxh128_hash_matches_php_vectors_and_is_case_insensitive() {
    assert_eq!(
        run_php(
            "<?php echo hash('xxh128', ''), '|', hash('xxh128', 'Symfony'), '|', hash('XXH128', 'framework_17');"
        ),
        "99aa06d3014798d86001c324468d497f|c0e5d7ae7e54d739641100ec43e5d6e6|3eae1805172d81287885e7c2c240684f"
    );
}

#[test]
fn xxh128_hash_binary_output_round_trips_through_base64() {
    assert_eq!(
        run_php("<?php echo base64_encode(hash('xxh128', 'Symfony', true));"),
        "wOXXrn5U1zlkEQDsQ+XW5g=="
    );
}

#[test]
fn crc32_hash_matches_php_byte_order_and_binary_output() {
    assert_eq!(
        run_php(
            "<?php echo hash('crc32', ''), '|', hash('crc32', '123456789'), '|', hash('crc32', 'Symfony'), '|', base64_encode(hash('crc32', '123456789', true));"
        ),
        "00000000|181989fc|313c4a4d|GBmJ/A=="
    );
}

#[test]
fn md5_hash_matches_one_shot_function_and_raw_output() {
    assert_eq!(
        run_php(
            "<?php echo hash('md5', ''), '|', hash('MD5', 'Symfony'), '|', base64_encode(hash('md5', 'Symfony', true)), '|', md5('Symfony');"
        ),
        "d41d8cd98f00b204e9800998ecf8427e|878d66f98b73e26b50c1392e7ee12ad9|h41m+Ytz4mtQwTkufuEq2Q==|878d66f98b73e26b50c1392e7ee12ad9"
    );
}

#[test]
fn scalar_array_serialization_round_trips_php_wire_format() {
    let serialized = concat!(
        "a:3:{",
        "s:4:\"name\";s:7:\"Symfony\";",
        "s:7:\"enabled\";b:1;",
        "s:6:\"nested\";a:3:{i:0;N;i:1;i:-3;i:2;s:1:\"x\";}",
        "}"
    );
    let source = format!(
        "<?php $value = ['name' => 'Symfony', 'enabled' => true, 'nested' => [null, -3, 'x']]; echo serialize($value), '|'; $copy = unserialize('{}', ['allowed_classes' => true]); echo $copy['name'], ':', $copy['enabled'], ':', $copy['nested'][1], ':', $copy['nested'][2];",
        serialized
    );
    assert_eq!(run_php(&source), format!("{serialized}|Symfony:1:-3:x"));
}

#[test]
fn object_serialization_honors_magic_hooks_and_allowed_classes() {
    assert_eq!(
        run_php(
            r#"<?php
class SerializableProbe {
    private string $value;
    public function __construct(string $value) { $this->value = $value; }
    public function __serialize(): array { return ['value' => $this->value]; }
    public function __unserialize(array $data): void { $this->value = $data['value']; }
    public function get(): string { return $this->value; }
}
$serialized = serialize(new SerializableProbe('ok'));
echo $serialized, '|';
echo unserialize($serialized)->get(), '|';
echo json_encode(unserialize($serialized, ['allowed_classes' => false]));
"#,
        ),
        "O:17:\"SerializableProbe\":1:{s:5:\"value\";s:2:\"ok\";}|ok|{\"__PHP_Incomplete_Class_Name\":\"SerializableProbe\",\"value\":\"ok\"}"
    );
}

#[test]
fn legacy_serializable_uses_trait_methods_and_yields_to_modern_hooks() {
    assert_eq!(
        run_php(
            r#"<?php
trait LegacyWire {
    public function serialize() { echo 'legacy>'; return 'payload'; }
    public function unserialize($payload) { echo 'restore:', $payload, '>'; }
}
class TraitPacket implements Serializable { use LegacyWire; }
class InheritedPacket extends TraitPacket {}
interface LegacyContract extends Serializable {}
class IndirectPacket implements LegacyContract { use LegacyWire; }
class ModernPacket implements Serializable {
    use LegacyWire;
    public function __serialize(): array { echo 'modern>'; return ['v' => 3]; }
    public function __unserialize(array $data): void { echo 'hydrate:', $data['v'], '>'; }
}
trait NullWire {
    public function serialize() { return null; }
    public function unserialize($payload) {}
}
class NullPacket implements Serializable { use NullWire; }
trait InvalidWire {
    public function serialize() { return 3; }
    public function unserialize($payload) {}
}
class InvalidPacket implements Serializable { use InvalidWire; }

$legacy = serialize(new TraitPacket());
echo $legacy, '>';
unserialize($legacy);
foreach ([new InheritedPacket(), new IndirectPacket()] as $packet) {
    $wire = serialize($packet);
    echo $wire, '>';
    unserialize($wire);
}
$modern = serialize(new ModernPacket());
echo $modern, '>';
unserialize($modern);
$null = new NullPacket();
echo serialize([$null, $null]), '>';
try { serialize(new InvalidPacket()); }
catch (Exception $error) { echo get_class($error), ':', $error->getMessage(); }
"#,
        ),
        concat!(
            "legacy>C:11:\"TraitPacket\":7:{payload}>restore:payload>",
            "legacy>C:15:\"InheritedPacket\":7:{payload}>restore:payload>",
            "legacy>C:14:\"IndirectPacket\":7:{payload}>restore:payload>",
            "modern>O:12:\"ModernPacket\":1:{s:1:\"v\";i:3;}>",
            "hydrate:3>a:2:{i:0;N;i:1;N;}>",
            "Exception:InvalidPacket::serialize() must return a string or NULL"
        )
    );
}

#[test]
fn object_serialization_preserves_cycles_and_shared_identity() {
    assert_eq!(
        run_php(
            r#"<?php
class CycleProbe { public $self; }
$cycle = new CycleProbe();
$cycle->self = $cycle;
$cycleWire = serialize($cycle);
echo $cycleWire, '|';
$copy = unserialize($cycleWire);
echo $copy === $copy->self ? 'same' : 'different';
echo '|';
$shared = new stdClass();
echo serialize([1, $shared, $shared]);
"#,
        ),
        concat!(
            "O:10:\"CycleProbe\":1:{s:4:\"self\";r:1;}|same|",
            "a:3:{i:0;i:1;i:1;O:8:\"stdClass\":0:{}i:2;r:3;}"
        )
    );
}

#[test]
fn array_serialization_tracks_reference_cycles() {
    assert_eq!(
        run_php(
            r#"<?php
$array = [];
$array[0] =& $array;
var_dump($array);
echo serialize($array);
"#,
        ),
        concat!(
            "array(1) {\n",
            "  [0]=>\n",
            "  *RECURSION*\n",
            "}\n",
            "a:1:{i:0;a:1:{i:0;R:2;}}"
        )
    );
}

#[test]
fn levenshtein_supports_default_and_custom_edit_costs() {
    assert_eq!(
        run_php(
            "<?php echo levenshtein('kitten', 'sitting'), '|', levenshtein('abc', 'axc', 2, 3, 4), '|', levenshtein('', 'abc');"
        ),
        "3|3|3"
    );
}

#[test]
fn explode_supports_positive_zero_and_negative_limits() {
    assert_eq!(
        run_php(
            "<?php echo implode('|', explode(':', 'a:b:c:d', 3)), '#'; echo implode('|', explode(':', 'a:b:c', 0)), '#'; echo implode('|', explode(':', 'a:b:c:d', -2));"
        ),
        "a|b|c:d#a:b:c#a|b"
    );
}

#[test]
fn ucwords_supports_default_and_custom_separators() {
    assert_eq!(
        run_php(
            r#"<?php echo ucwords("hello world\tnext-value"), '|', ucwords('one-two three', '-');"#
        ),
        "Hello World\tNext-value|One-Two three"
    );
}

#[test]
fn test_strpbrk_returns_suffix_or_false() {
    assert_eq!(
        run_php(
            "<?php echo strpbrk('route/{slug}', '?<:{'), '|'; var_dump(strpbrk('route', '?<:'));"
        ),
        "{slug}|bool(false)\n"
    );
}

#[test]
fn test_html_entity_decode_named_numeric_and_utf8() {
    assert_eq!(
        run_php("<?php echo html_entity_decode('a&amp;b&#x21;&#33; ž');"),
        "a&b!! ž"
    );
}
#[test]
fn array_string_conversions_warn_and_internal_settype_preserves_php_mutation_order() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class StringableProbe {
    public function __toString(): string { return 'probe'; }
}

$array = [1];
var_dump((string) $array);
$array = [2];
var_dump(settype($array, 'string'), $array);
var_dump(strval([3]));
$object = new StringableProbe;
var_dump((string) $object);
var_dump(settype($object, 'string'), $object);

$protected = [4];
set_error_handler(function(int $level, string $message): never {
    throw new Exception($message);
});
try {
    settype($protected, 'string');
} catch (Exception $error) {
    echo $error->getMessage(), "\n";
}
restore_error_handler();
var_dump($protected);
"#,
            "test.php",
            "",
        ),
        "\nWarning: Array to string conversion in test.php on line 7\nstring(5) \"Array\"\n\nWarning: Array to string conversion in test.php on line 9\nbool(true)\nstring(5) \"Array\"\n\nWarning: Array to string conversion in test.php on line 10\nstring(5) \"Array\"\nstring(5) \"probe\"\nbool(true)\nstring(5) \"probe\"\nArray to string conversion\nstring(5) \"Array\"\n",
    );
}

#[test]
fn output_string_conversion_uses_tostring_and_throws_before_writing() {
    assert_eq!(
        run_php(
            r#"<?php
class GoodOutput {
    public function __toString(): string { return 'good'; }
}
class InvalidOutput {
    public function __toString() { return []; }
}
$good = new GoodOutput;
echo $good, '|';
print $good;
try { echo new stdClass; } catch (Error $error) { echo "\n", $error->getMessage(); }
try { echo new InvalidOutput; } catch (TypeError $error) { echo "\n", $error->getMessage(); }
set_error_handler(function(int $level, string $message): never {
    throw new Exception($message);
});
try { echo [1]; } catch (Exception $error) { echo "\n", $error->getMessage(); }
"#,
        ),
        "good|good\nObject of class stdClass could not be converted to string\nInvalidOutput::__toString(): Return value must be of type string\nArray to string conversion",
    );
}

#[test]
fn concatenation_converts_arrays_and_objects_in_php_85_operator_order() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) {
    echo "warn:$level:$message\n";
    if (($GLOBALS['mutateWarningRight'] ?? false) === true) {
        $GLOBALS['binaryRight'] = 'mutated-warning';
    }
});

class GoodConcat {
    public function __toString(): string {
        echo "good-call\n";
        return 'good';
    }
}

class InvalidConcat {
    public function __toString() { return []; }
}

class WeakScalarConcat {
    public function __toString() { return 7; }
}

class LeftReentrantConcat {
    public function __toString(): string {
        global $slot;
        $slot = 'mutated-left';
        return 'left-object';
    }
}

class RightReentrantConcat {
    public function __toString(): string {
        global $slot;
        $slot = 'mutated-right';
        return 'right-object';
    }
}

function probeConcat(string $label, Closure $operation): void {
    echo $label, ":\n";
    try {
        var_dump($operation());
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}

$plain = new stdClass;
$good = new GoodConcat;
$closure = function () {};
probeConcat('binary-plain-left-array', fn() => $plain . []);
probeConcat('binary-array-plain-right', fn() => [] . $plain);
probeConcat('binary-closure-left-array', fn() => $closure . []);
probeConcat('binary-good-left-array', fn() => $good . []);
probeConcat('binary-invalid-left', fn() => new InvalidConcat . '-tail');
probeConcat('binary-weak-scalar-left', fn() => new WeakScalarConcat . '-tail');

$binaryRight = 'tail';
$mutateWarningRight = true;
$result = [] . $binaryRight;
$mutateWarningRight = false;
echo "binary-warning-reentrant:$result|after=$binaryRight\n";

$slot = $plain;
try {
    $slot .= [];
} catch (Throwable $error) {
    echo 'assign-plain-left-array:', get_class($error), ':', $error->getMessage(),
        '|same=', $slot === $plain ? 'yes' : 'no', "\n";
}

$slot = [];
try {
    $slot .= $plain;
} catch (Throwable $error) {
    echo 'assign-array-left-plain:', get_class($error), ':', $error->getMessage(),
        '|array=', is_array($slot) ? 'yes' : 'no', "\n";
}

$slot = $good;
$slot .= [];
echo 'assign-good-left-array:', $slot, "\n";

$slot = [];
$slot .= $good;
echo 'assign-array-left-good:', $slot, "\n";

$slot = $plain;
try {
    $result = ($slot .= []);
} catch (Throwable $error) {
    echo 'assign-expression-plain-left-array:', get_class($error), ':', $error->getMessage(),
        '|same=', $slot === $plain ? 'yes' : 'no', "\n";
}

$slot = new LeftReentrantConcat;
$slot .= '-tail';
echo 'assign-left-reentrant:', $slot, "\n";

$slot = 'head-';
$slot .= new RightReentrantConcat;
echo 'assign-right-reentrant:', $slot, "\n";

$slot = new LeftReentrantConcat;
$alias =& $slot;
$slot .= '-tail';
echo 'assign-left-reference-reentrant:', $slot, '|', $alias, "\n";

$slot = 'head-';
$alias =& $slot;
$slot .= new RightReentrantConcat;
echo 'assign-right-reference-reentrant:', $slot, '|', $alias, "\n";
"#,
        ),
        concat!(
            "binary-plain-left-array:\n",
            "warn:2:Array to string conversion\n",
            "Error:Object of class stdClass could not be converted to string\n",
            "binary-array-plain-right:\n",
            "warn:2:Array to string conversion\n",
            "Error:Object of class stdClass could not be converted to string\n",
            "binary-closure-left-array:\n",
            "warn:2:Array to string conversion\n",
            "Error:Object of class Closure could not be converted to string\n",
            "binary-good-left-array:\n",
            "warn:2:Array to string conversion\n",
            "good-call\n",
            "string(9) \"goodArray\"\n",
            "binary-invalid-left:\n",
            "TypeError:InvalidConcat::__toString(): Return value must be of type string, array returned\n",
            "binary-weak-scalar-left:\n",
            "string(6) \"7-tail\"\n",
            "warn:2:Array to string conversion\n",
            "binary-warning-reentrant:Arraymutated-warning|after=mutated-warning\n",
            "assign-plain-left-array:Error:Object of class stdClass could not be converted to string|same=yes\n",
            "warn:2:Array to string conversion\n",
            "assign-array-left-plain:Error:Object of class stdClass could not be converted to string|array=yes\n",
            "good-call\n",
            "warn:2:Array to string conversion\n",
            "assign-good-left-array:goodArray\n",
            "warn:2:Array to string conversion\n",
            "good-call\n",
            "assign-array-left-good:Arraygood\n",
            "assign-expression-plain-left-array:Error:Object of class stdClass could not be converted to string|same=yes\n",
            "assign-left-reentrant:left-object-tail\n",
            "assign-right-reentrant:head-right-object\n",
            "assign-left-reference-reentrant:left-object-tail|left-object-tail\n",
            "assign-right-reference-reentrant:head-right-object|head-right-object\n",
        )
    );
}

#[test]
fn strict_tostring_scalar_return_is_rejected_by_concatenation() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
class StrictScalarConcat {
    public function __toString() { return 7; }
}
class StrictTrueConcat {
    public function __toString() { return true; }
}
try {
    echo new StrictScalarConcat . '-tail';
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
try {
    echo new StrictTrueConcat . '-tail';
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage();
}
"#,
        ),
        concat!(
            "TypeError:StrictScalarConcat::__toString(): Return value must be of type string, int returned\n",
            "TypeError:StrictTrueConcat::__toString(): Return value must be of type string, true returned",
        ),
    );
}

#[test]
fn concat_tostring_callbacks_retain_live_and_throwable_call_sites() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class TracedConcat {
    public function __toString(): string {
        $frame = debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS)[0];
        return $frame['function'] . '@' . $frame['file'] . ':' . $frame['line'];
    }
}
class ThrowingConcat {
    public function __toString(): string {
        throw new Exception('conversion failed');
    }
}
$traced = new TracedConcat;
echo 'explicit=' . $traced, "\n";
echo "interpolated=$traced\n";
$compound = 'compound=';
$compound .= $traced;
echo $compound, "\n";
$value = 'value=';
echo ($value .= $traced), "\n";
$throwing = new ThrowingConcat;
try {
    echo "$throwing";
} catch (Throwable $error) {
    $frame = $error->getTrace()[0];
    echo $error->getFile(), ':', $error->getLine(), '|',
        $frame['class'], '::', $frame['function'], '@',
        $frame['file'], ':', $frame['line'];
}
"#,
            "/virtual/concat-tostring-trace.php",
            "/virtual",
        ),
        concat!(
            "explicit=__toString@/virtual/concat-tostring-trace.php:14\n",
            "interpolated=__toString@/virtual/concat-tostring-trace.php:15\n",
            "compound=__toString@/virtual/concat-tostring-trace.php:17\n",
            "value=__toString@/virtual/concat-tostring-trace.php:20\n",
            "/virtual/concat-tostring-trace.php:10|",
            "ThrowingConcat::__toString@/virtual/concat-tostring-trace.php:23",
        ),
    );
}
