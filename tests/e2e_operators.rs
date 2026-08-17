/// E2E tests: logical operators, ternary, compound assignment, comments.
mod common;
use common::{run_php, run_php_with_source_context};

use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;

// === Logical operators ===

#[test]
fn logical_not_applies_after_instanceof() {
    assert_eq!(
        run_php(
            "<?php interface NegatedType {} class NegatedValue implements NegatedType {} $value = new NegatedValue(); echo (!$value instanceof NegatedType ? 'bad' : 'yes') . ':' . (!$value instanceof Stringable ? 'yes' : 'bad');"
        ),
        "yes:yes"
    );
}

#[test]
fn test_e2e_and_both_true() {
    assert_eq!(
        run_php("<?php if (1 && 1) { echo \"yes\"; } else { echo \"no\"; }"),
        "yes"
    );
}

#[test]
fn test_e2e_and_left_false() {
    assert_eq!(
        run_php("<?php if (0 && 1) { echo \"yes\"; } else { echo \"no\"; }"),
        "no"
    );
}

#[test]
fn test_e2e_and_right_false() {
    assert_eq!(
        run_php("<?php if (1 && 0) { echo \"yes\"; } else { echo \"no\"; }"),
        "no"
    );
}

#[test]
fn test_e2e_or_both_false() {
    assert_eq!(
        run_php("<?php if (0 || 0) { echo \"yes\"; } else { echo \"no\"; }"),
        "no"
    );
}

#[test]
fn test_e2e_or_left_true() {
    assert_eq!(
        run_php("<?php if (1 || 0) { echo \"yes\"; } else { echo \"no\"; }"),
        "yes"
    );
}

#[test]
fn test_e2e_or_right_true() {
    assert_eq!(
        run_php("<?php if (0 || 1) { echo \"yes\"; } else { echo \"no\"; }"),
        "yes"
    );
}

#[test]
fn keyword_logical_operators_use_php_precedence_and_short_circuit_rules() {
    assert_eq!(
        run_php(
            "<?php
function side($value) { echo $value; return $value; }
echo ':';
echo side('L') xor side('') ? 'yes' : 'no';
$or = false or true;
$and = true and false;
var_dump($or, $and, false xor true);
"
        ),
        ":Lbool(false)\nbool(true)\nbool(true)\n"
    );
}

#[test]
fn binary_right_operands_accept_value_producing_assignments() {
    assert_eq!(
        run_php(
            r#"<?php
$add = 2; var_dump(1 + $add = 3, $add);
$concat = 'x'; var_dump('p' . $concat = 'q', $concat);
$multiply = 4; var_dump(8 * $multiply = 5, $multiply);
$shift = 0; var_dump(8 >> $shift = 2, $shift);
$bitwise = 0; var_dump(1 | $bitwise = 2, $bitwise);
$power = 0; var_dump(2 ** $power = 3, $power);
"#
        ),
        "int(4)\nint(3)\nstring(2) \"pq\"\nstring(1) \"q\"\nint(40)\nint(5)\nint(2)\nint(2)\nint(3)\nint(2)\nint(8)\nint(3)\n"
    );
}

#[test]
fn test_e2e_not_true() {
    assert_eq!(
        run_php("<?php if (!0) { echo \"yes\"; } else { echo \"no\"; }"),
        "yes"
    );
}

#[test]
fn test_e2e_not_false() {
    assert_eq!(
        run_php("<?php if (!1) { echo \"yes\"; } else { echo \"no\"; }"),
        "no"
    );
}

#[test]
fn test_e2e_and_short_circuit() {
    assert_eq!(
        run_php(
            "<?php\nfunction side() { echo \"SIDE\"; return 1; }\nif (0 && side()) { echo \"yes\"; }"
        ),
        ""
    );
}

#[test]
fn test_e2e_or_short_circuit() {
    assert_eq!(
        run_php(
            "<?php\nfunction side() { echo \"SIDE\"; return 1; }\nif (1 || side()) { echo \"yes\"; }"
        ),
        "yes"
    );
}

#[test]
fn test_e2e_logical_complex() {
    assert_eq!(
        run_php("<?php if ((1 && 0) || (0 || 1)) { echo \"yes\"; } else { echo \"no\"; }"),
        "yes"
    );
}

// === Ternary operator ===

#[test]
fn test_e2e_ternary_true() {
    assert_eq!(run_php("<?php echo 1 ? \"yes\" : \"no\";"), "yes");
}

#[test]
fn test_e2e_ternary_false() {
    assert_eq!(run_php("<?php echo 0 ? \"yes\" : \"no\";"), "no");
}

#[test]
fn test_e2e_ternary_variable() {
    assert_eq!(
        run_php("<?php $x = 5; echo $x > 3 ? \"big\" : \"small\";"),
        "big"
    );
}

#[test]
fn test_e2e_ternary_nested() {
    assert_eq!(
        run_php("<?php $x = 0; $y = 1; echo $x ? \"a\" : ($y ? \"b\" : \"c\");"),
        "b"
    );
}

#[test]
fn test_e2e_ternary_in_assignment() {
    assert_eq!(
        run_php("<?php $x = 10; $y = $x > 5 ? $x * 2 : $x; echo $y;"),
        "20"
    );
}

#[test]
fn test_e2e_ternary_with_function() {
    assert_eq!(
        run_php(
            "<?php\nfunction double($n) { return $n * 2; }\n$x = 3;\necho $x > 2 ? double($x) : $x;"
        ),
        "6"
    );
}

#[test]
fn test_e2e_nested_ternary_error() {
    let tokens = Lexer::new("<?php echo 1 ? 2 : 3 ? 4 : 5;")
        .tokenize()
        .unwrap();
    let result = Parser::new(tokens).parse();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Unparenthesized"));
}

#[test]
fn test_e2e_parenthesized_ternary_ok() {
    assert_eq!(run_php("<?php echo (1 ? 2 : 0) ? 4 : 5;"), "4");
}

// === Compound assignment ===

#[test]
fn test_e2e_plus_assign() {
    assert_eq!(run_php("<?php $x = 10; $x += 5; echo $x;"), "15");
}

#[test]
fn test_e2e_minus_assign() {
    assert_eq!(run_php("<?php $x = 10; $x -= 3; echo $x;"), "7");
}

#[test]
fn test_e2e_star_assign() {
    assert_eq!(run_php("<?php $x = 4; $x *= 3; echo $x;"), "12");
}

#[test]
fn test_e2e_slash_assign() {
    assert_eq!(run_php("<?php $x = 12; $x /= 4; echo $x;"), "3");
}

#[test]
fn test_e2e_percent_assign() {
    assert_eq!(run_php("<?php $x = 10; $x %= 3; echo $x;"), "1");
}

#[test]
fn test_e2e_dot_assign() {
    assert_eq!(
        run_php("<?php $x = \"hello\"; $x .= \" world\"; echo $x;"),
        "hello world"
    );
}

#[test]
fn test_e2e_compound_assign_mutable_targets_are_evaluated_once() {
    assert_eq!(
        run_php(
            r#"<?php
class Box { public $value = 'a'; }
$box = new Box();
$objects = 0;
$indices = 0;
function objectTarget($box) { global $objects; $objects++; return $box; }
function indexTarget() { global $indices; $indices++; return 'key'; }
objectTarget($box)->value .= 'b';
$values = ['key' => 'c'];
$values[indexTarget()] .= 'd';
echo $box->value, '|', $values['key'], '|', $objects, '|', $indices;
"#
        ),
        "ab|cd|1|1"
    );
}

#[test]
fn test_e2e_compound_assign_in_loop() {
    assert_eq!(
        run_php("<?php $sum = 0; for ($i = 1; $i <= 5; $i++) { $sum += $i; } echo $sum;"),
        "15"
    );
}

#[test]
fn test_e2e_null_coalescing_assign_is_lazy() {
    assert_eq!(
        run_php(
            "<?php function fallback() { echo 'rhs>'; return 9; } $set = 7; $set ??= fallback(); echo $set, '|'; $missing = null; $missing ??= fallback(); echo $missing;"
        ),
        "7|rhs>9"
    );
}

#[test]
fn test_e2e_null_coalescing_assign_array_dimension() {
    assert_eq!(
        run_php(
            "<?php $listeners = []; $listeners[1] ??= 'first'; $listeners[1] ??= 'second'; echo $listeners[1];"
        ),
        "first"
    );
}

#[test]
fn test_e2e_null_coalescing_assign_properties() {
    assert_eq!(
        run_php(
            "<?php class Box { public $value; public static $shared; } $box = new Box(); $box->value ??= 'object'; $box->value ??= 'changed'; Box::$shared ??= 'static'; Box::$shared ??= 'changed'; echo $box->value, '|', Box::$shared;"
        ),
        "object|static"
    );
}

#[test]
fn test_e2e_null_coalescing_silently_initializes_typed_object_property() {
    assert_eq!(
        run_php(
            "<?php class Box { protected string $value; public function get() { $before = $this->value ?? 'unset'; $this->value ??= 'ready'; return $before . ':' . $this->value; } } echo (new Box())->get();"
        ),
        "unset:ready"
    );
}

#[test]
fn test_e2e_null_coalescing_silently_initializes_typed_static_property() {
    assert_eq!(
        run_php(
            r"<?php class Factory { private static \Closure $make; public static function get() { self::$make ??= static fn () => 'ready'; return (self::$make)(); } } echo Factory::get();"
        ),
        "ready"
    );
}

#[test]
fn test_e2e_null_coalescing_silently_initializes_typed_property_array_dimension() {
    assert_eq!(
        run_php(
            "<?php class Cache { private array $values; public function get($key) { return $this->values[$key] ??= strtoupper($key); } } $cache = new Cache(); echo $cache->get('ready'), ':', $cache->get('ready');"
        ),
        "READY:READY"
    );
}

#[test]
fn test_e2e_null_coalescing_assignment_is_value_producing() {
    assert_eq!(
        run_php(
            r#"<?php
class LazyBox { public $value; }
$box = new LazyBox();
function initialize($box) { return $box->value ??= 'ready'; }
$fallback = null;
echo initialize($box), '|';
if (!$fallback ??= '') { echo 'empty'; }
echo '|', $fallback, '|', $box->value;
"#
        ),
        "ready|empty||ready"
    );
}

#[test]
fn test_e2e_null_coalescing_assignment_in_ternary_arms() {
    assert_eq!(
        run_php(
            "<?php $left = null; $right = null; echo true ? $left ??= 3 : 9, '|', false ? 1 : $right ??= 2, '|', $left, '|', $right;"
        ),
        "3|2|3|2"
    );
}

#[test]
fn test_e2e_coalesce_assignment_binds_on_null_coalesce_rhs() {
    assert_eq!(
        run_php(
            r#"<?php
class FactoryHolder {
    private static $factory;

    private static function make() {
        return 'made';
    }

    public static function resolve() {
        return ($missing['service'] ?? self::$factory ??= self::make(...))();
    }
}

echo FactoryHolder::resolve();
"#,
        ),
        "made"
    );
}

#[test]
fn test_e2e_coalesce_assignment_binds_on_comparison_rhs() {
    assert_eq!(
        run_php(
            "<?php $value = null; $result = 0 > $value ??= 1; echo $result ? 'yes' : 'no'; echo '|', $value;"
        ),
        "no|1"
    );
}

#[test]
fn test_e2e_coalesce_assignment_binds_on_concat_rhs() {
    assert_eq!(
        run_php(
            "<?php class Suffix { public $value; } $suffix = new Suffix(); $path = 'prefix-' . $suffix->value ??= 'generated'; echo $path, '|', $suffix->value;"
        ),
        "prefix-generated|generated"
    );
}

#[test]
fn test_e2e_prefix_increment_mutable_member_targets() {
    assert_eq!(
        run_php(
            "<?php class Counts { public static $shared = 100; public $own = 4; } $counts = new Counts(); echo ++Counts::$shared, '|', ++$counts->own;"
        ),
        "101|5"
    );
}

#[test]
fn test_e2e_compound_assignment_expression_on_array_target() {
    assert_eq!(
        run_php(
            "<?php $error = ['type' => 7]; if ($error && $error['type'] &= 3) { echo $error['type']; }"
        ),
        "3"
    );
}

#[test]
fn test_e2e_dynamic_object_property_read_and_coalesce_assignment() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class DynamicProperties {
    public $values = ['ready' => 'yes'];
}
$object = new DynamicProperties();
$property = 'values';
echo $object->{$property}['ready'], '|';
echo $object->$property['missing'] ??= 'created', '|';
$dynamic = 'extra';
$object->{$dynamic} = 'assigned';
echo $object->$dynamic;
"#,
        ),
        "yes|created|assigned"
    );
}

#[test]
fn test_e2e_isset_accepts_dynamic_object_and_static_array_targets() {
    assert_eq!(
        run_php(
            "<?php class IssetTargets { public static array $shared = ['ready' => true]; public $value = 'yes'; } $object = new IssetTargets(); $property = 'value'; echo isset($object->$property) ? 'dynamic:' : 'missing:'; echo isset(IssetTargets::$shared['ready']) ? 'static' : 'missing';"
        ),
        "dynamic:static"
    );
}

#[test]
fn test_e2e_unset_accepts_a_dynamic_object_property() {
    assert_eq!(
        run_php(
            "<?php class DynamicUnset { public $value = 'set'; } $object = new DynamicUnset(); $property = 'value'; unset($object->$property); echo isset($object->$property) ? 'set' : 'unset';"
        ),
        "unset"
    );
}

#[test]
fn test_e2e_post_increment_and_decrement_mutable_targets_return_the_old_value() {
    assert_eq!(
        run_php(
            r#"<?php
class MutableCounter {
    public static int $shared = 4;
    public int $value = 7;
    public static function next(): int { return self::$shared++; }
}
$object = new MutableCounter();
$property = 'value';
$array = [10];
echo MutableCounter::next(), ':', MutableCounter::$shared, '|';
echo $object->$property--, ':', $object->value, '|';
echo $array[0]++, ':', $array[0];
"#,
        ),
        "4:5|7:6|10:11"
    );
}

#[test]
fn test_e2e_array_append_assignments_chain_and_produce_the_assigned_value() {
    assert_eq!(
        run_php(
            "<?php $left = []; $right = []; $result = ($left[] = $right[] = 7); echo $result, ':', $left[0], ':', $right[0], '|'; $self = []; $snapshot = ($self[] = $self); echo count($snapshot), ':', count($self), ':', count($self[0]);"
        ),
        "7:7:7|0:1:0"
    );
}

#[test]
fn empty_array_dimensions_reject_reads_and_unsets_but_allow_append_writes() {
    fn compile_error(source: &str) -> String {
        let tokens = Lexer::new(source).tokenize().expect("source must lex");
        let statements = Parser::new(tokens).parse().expect("source must parse");
        match Compiler::new().compile(&statements) {
            Ok(_) => panic!("empty read or unset dimension must fail during compilation"),
            Err(error) => error.message,
        }
    }

    for source in [
        "<?php\nif (false) { isset($slots[]); }",
        "<?php\n$result = empty($slots['missing'][]);",
        "<?php\n$result = $slots[] ?? 'fallback';",
        "<?php\n$result = $slots[];",
    ] {
        assert_eq!(
            compile_error(source),
            "Cannot use [] for reading on line 2",
            "{source}"
        );
    }
    assert_eq!(
        compile_error("<?php\nunset($slots['missing'][]);"),
        "Cannot use [] for unsetting on line 2"
    );
    assert_eq!(
        compile_error("<?php\nisset($slots\n[]);"),
        "Cannot use [] for reading on line 2"
    );

    assert_eq!(
        run_php(
            "<?php $slots = []; $slots[] = 7; $slots['nested'][] = 9; echo $slots[0], '|', $slots['nested'][0];"
        ),
        "7|9"
    );
}

#[test]
fn test_e2e_cast_wraps_the_value_produced_by_a_following_assignment() {
    assert_eq!(
        run_php(
            "<?php function captureCast($value) { var_dump($value); } $value = 'yes'; captureCast((bool) $value = '0'); echo '|', $value;"
        ),
        "bool(false)\n|0"
    );
}

#[test]
fn test_e2e_property_and_array_assignment_expressions_produce_values() {
    assert_eq!(
        run_php(
            r#"<?php
class AssignmentBox { public $value; }
$box = new AssignmentBox();
$values = [];
if ($box->value = 'property') { echo $box->value; }
echo '|', ($values['key'] = 'array'), '|', $values['key'];
"#
        ),
        "property|array|array"
    );
}

#[test]
fn test_e2e_chained_elvis_is_left_associative_and_lazy() {
    assert_eq!(
        run_php(
            "<?php function mark($v) { echo $v; return $v === 'c' ? 'ok' : ''; } $result = mark('a') ?: mark('b') ?: mark('c') ?: mark('d'); echo ':'.$result;"
        ),
        "abc:ok"
    );
}

#[test]
fn test_e2e_error_control_operator_preserves_value_and_side_effects() {
    assert_eq!(
        run_php("<?php function value() { echo 'side>'; return 42; } echo @value();"),
        "side>42"
    );
}

// === Comments ===

#[test]
fn test_e2e_line_comment_slash() {
    assert_eq!(run_php("<?php // this is a comment\necho 42;"), "42");
}

#[test]
fn test_e2e_line_comment_hash() {
    assert_eq!(run_php("<?php # hash comment\necho 99;"), "99");
}

#[test]
fn test_e2e_block_comment() {
    assert_eq!(run_php("<?php /* block\ncomment */ echo 7;"), "7");
}

#[test]
fn test_e2e_inline_block_comment() {
    assert_eq!(run_php("<?php echo /* between */ 5;"), "5");
}

// === CR regression: break/continue outside loop = compile error ===

#[test]
fn test_e2e_break_outside_loop_error() {
    let tokens = Lexer::new("<?php break;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&stmts);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("break"));
}

#[test]
fn test_e2e_continue_outside_loop_error() {
    let tokens = Lexer::new("<?php continue;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&stmts);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("continue"));
}

// === CR7 regression: nested ternary in then-branch is allowed (PHP 8+) ===

#[test]
fn test_e2e_nested_ternary_in_then_branch_ok() {
    // PHP 8 allows nested ternary in then-branch: 1 ? 2 ? 3 : 4 : 5
    // Parses as: 1 ? (2 ? 3 : 4) : 5 → result is 3
    assert_eq!(run_php("<?php echo 1 ? 2 ? 3 : 4 : 5;"), "3");
}

#[test]
fn test_e2e_nested_ternary_in_then_branch_false() {
    // 0 ? 1 ? 2 : 3 : 4 → else-branch → 4
    assert_eq!(run_php("<?php echo 0 ? 1 ? 2 : 3 : 4;"), "4");
}

// === CR5 regression: unterminated block comment ===

#[test]
fn test_e2e_unterminated_block_comment() {
    let result = Lexer::new("<?php /* unterminated").tokenize();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Unterminated comment"));
}

// === Extra userland args frame integrity ===

#[test]
fn test_e2e_too_many_args_frame_integrity() {
    assert_eq!(
        run_php("<?php\nfunction add($a) { $copy = $a; return $copy; }\necho add(1, 2);"),
        "1"
    );
}

// ========== Strict comparison (===, !==) ==========

#[test]
fn test_identical_int_int() {
    assert_eq!(run_php("<?php echo 1 === 1 ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_identical_int_string_false() {
    assert_eq!(run_php("<?php echo 1 === '1' ? 'yes' : 'no';"), "no");
}

#[test]
fn test_identical_string_string() {
    assert_eq!(run_php("<?php echo 'abc' === 'abc' ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_identical_null_null() {
    assert_eq!(run_php("<?php echo null === null ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_identical_null_false() {
    assert_eq!(run_php("<?php echo null === false ? 'yes' : 'no';"), "no");
}

#[test]
fn test_identical_true_true() {
    assert_eq!(run_php("<?php echo true === true ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_identical_false_false() {
    assert_eq!(run_php("<?php echo false === false ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_identical_true_one() {
    assert_eq!(run_php("<?php echo true === 1 ? 'yes' : 'no';"), "no");
}

#[test]
fn test_not_identical_basic() {
    assert_eq!(run_php("<?php echo 1 !== '1' ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_not_identical_same_type() {
    assert_eq!(run_php("<?php echo 1 !== 1 ? 'yes' : 'no';"), "no");
}

#[test]
fn test_identical_in_if() {
    assert_eq!(
        run_php(
            "<?php
$x = 0;
if ($x === 0) {
    echo 'zero';
} else {
    echo 'other';
}
"
        ),
        "zero"
    );
}

#[test]
fn test_identical_strpos_false_check() {
    assert_eq!(
        run_php(
            "<?php
$pos = 0;
if ($pos === false) {
    echo 'not found';
} else {
    echo 'found at ' . $pos;
}
"
        ),
        "found at 0"
    );
}

#[test]
fn test_identical_arrays_equal() {
    assert_eq!(
        run_php("<?php echo [1, 2, 3] === [1, 2, 3] ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_identical_arrays_different_values() {
    assert_eq!(
        run_php("<?php echo [1, 2] === [1, 3] ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_identical_arrays_different_length() {
    assert_eq!(
        run_php("<?php echo [1, 2] === [1, 2, 3] ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_identical_arrays_different_keys() {
    assert_eq!(
        run_php("<?php echo ['a' => 1] === ['b' => 1] ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_identical_arrays_nested() {
    assert_eq!(
        run_php("<?php echo [[1, 2], [3]] === [[1, 2], [3]] ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_not_identical_arrays() {
    assert_eq!(run_php("<?php echo [1] !== [1] ? 'yes' : 'no';"), "no");
}

#[test]
fn test_practical_identical_vs_equal() {
    assert_eq!(
        run_php(
            "<?php
$results = '';
if (0 === false) { $results .= 'A'; }
if (0 !== false) { $results .= 'B'; }
if ('' === false) { $results .= 'C'; }
if ('' !== false) { $results .= 'D'; }
if (null === false) { $results .= 'E'; }
if (null !== false) { $results .= 'F'; }
echo $results;
"
        ),
        "BDF"
    );
}

// ========== Comparison with concat on right side ==========

#[test]
fn php_85_pipe_preserves_precedence_chaining_and_evaluation_order() {
    assert_eq!(
        run_php(
            r#"<?php
function pipeAdd(int $value): int { echo 'A'; return $value + 1; }
function pipeDouble(int $value): int { echo 'D'; return $value * 2; }
function choosePipe() { echo 'C'; return 'pipeDouble'; }
$result = 5 + 2 |> pipeAdd(...) |> (choosePipe());
echo '|', $result, '|';
var_dump(5 |> pipeDouble(...) == 10);
"#,
        ),
        "ACD|16|Dbool(true)\n"
    );
}

#[test]
fn php_85_pipe_rejects_a_by_reference_parameter() {
    assert_eq!(
        run_php(
            r#"<?php
function pipeMutate(int &$value): int { return ++$value; }
try { 5 |> pipeMutate(...); } catch (Error $error) { echo $error->getMessage(); }
"#,
        ),
        "pipeMutate(): Argument #1 ($value) could not be passed by reference"
    );
}

#[test]
fn php_85_pipe_validates_a_supplied_type_before_missing_arguments() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function pipeNeedsTwo(int $first, int $second): int { return $first + $second; }
try { "wrong" |> "pipeNeedsTwo"; } catch (TypeError $error) { echo $error->getMessage(); }
"#,
            "/fixture/type-order.php",
            "/fixture",
        ),
        "pipeNeedsTwo(): Argument #1 ($first) must be of type int, string given, called in /fixture/type-order.php on line 3"
    );
}

#[test]
fn weak_numeric_string_coercion_precedes_the_missing_argument_error() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function weakNeedsTwo(int $first, string $second): void {}
try { weakNeedsTwo("123"); } catch (ArgumentCountError $error) { echo $error->getMessage(); }
"#,
            "/fixture/weak-call.php",
            "/fixture",
        ),
        "Too few arguments to function weakNeedsTwo(), 1 passed in /fixture/weak-call.php on line 3 and exactly 2 expected"
    );
}

#[test]
fn test_identical_with_concat_rhs() {
    assert_eq!(
        run_php("<?php echo 'xy' === 'x' . 'y' ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_equal_with_concat_rhs() {
    assert_eq!(
        run_php("<?php echo 'ab' == 'a' . 'b' ? 'yes' : 'no';"),
        "yes"
    );
}

// ========== Elvis operator (?:) ==========

#[test]
fn test_elvis_truthy_string() {
    assert_eq!(
        run_php(
            r#"<?php
$name = "PHP";
echo $name ?: "default";
"#
        ),
        "PHP"
    );
}

#[test]
fn test_elvis_falsy_empty_string() {
    assert_eq!(
        run_php(
            r#"<?php
$name = "";
echo $name ?: "default";
"#
        ),
        "default"
    );
}

#[test]
fn test_elvis_falsy_zero() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 0;
echo $x ?: 42;
"#
        ),
        "42"
    );
}

#[test]
fn test_elvis_falsy_null() {
    assert_eq!(
        run_php(
            r#"<?php
$x = null;
echo $x ?: "fallback";
"#
        ),
        "fallback"
    );
}

#[test]
fn test_elvis_truthy_number() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 5;
echo $x ?: 99;
"#
        ),
        "5"
    );
}

#[test]
fn test_elvis_with_function_call() {
    assert_eq!(
        run_php(
            r#"<?php
function getName() { return ""; }
echo getName() ?: "anonymous";
"#
        ),
        "anonymous"
    );
}

#[test]
fn test_elvis_chained_with_parens() {
    assert_eq!(
        run_php(
            r#"<?php
$a = "";
$b = "";
$c = "found";
echo ($a ?: $b) ?: $c;
"#
        ),
        "found"
    );
}

#[test]
fn test_elvis_truthy_array() {
    assert_eq!(
        run_php(
            r#"<?php
$x = "yes";
$y = $x ?: "no";
echo $y;
"#
        ),
        "yes"
    );
}

#[test]
fn test_elvis_in_assignment() {
    assert_eq!(
        run_php(
            r#"<?php
$config = "";
$value = $config ?: "default_value";
echo $value;
"#
        ),
        "default_value"
    );
}

#[test]
fn test_elvis_side_effect_truthy_evaluates_once() {
    // P1 regression: LHS with side effects must be evaluated exactly once
    assert_eq!(
        run_php(
            r#"<?php
function foo() { echo "x"; return 123; }
echo foo() ?: 0;
"#
        ),
        "x123"
    );
}

#[test]
fn test_elvis_side_effect_falsy_evaluates_once() {
    assert_eq!(
        run_php(
            r#"<?php
function bar() { echo "y"; return 0; }
echo bar() ?: 99;
"#
        ),
        "y99"
    );
}

#[test]
fn unary_plus_coerces_numbers_and_preserves_precedence() {
    assert_eq!(
        run_php(
            r#"<?php
$value = "12";
var_dump(+$value);
var_dump(+2.5);
var_dump(+-3);
var_dump(+true);
"#
        ),
        "int(12)\nfloat(2.5)\nint(-3)\nint(1)\n"
    );
}
