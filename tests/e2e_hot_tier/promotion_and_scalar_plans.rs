// ══════════════════════════════════════════════════════════════════════
// 1. Promotion: scalar recursion enters hot executor
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_scalar_recursion_fib() {
    // fib(25) generates ~150K recursive calls → well past threshold.
    // Hot executor handles the entire recursive tree after promotion.
    assert_eq!(
        run_php(
            "<?php
function fib($n) {
    if ($n <= 1) return $n;
    return fib($n - 1) + fib($n - 2);
}
echo fib(25);
"
        ),
        "75025"
    );
}

#[test]
fn test_hot_scalar_recursion_factorial() {
    // Tail-ish recursion with multiplication.
    assert_eq!(
        run_php(
            "<?php
function fact($n) {
    if ($n <= 1) return 1;
    return $n * fact($n - 1);
}
echo fact(10);
"
        ),
        "3628800"
    );
}

#[test]
fn test_binary_long_recursion_plan_is_compiled_for_function_and_method() {
    let source = "<?php
function fib($n) {
    if ($n <= 1) return $n;
    return fib($n - 1) + fib($n - 2);
}
class Calculator {
    public function fib($n) {
        if ($n <= 1) return $n;
        return $this->fib($n - 1) + $this->fib($n - 2);
    }
}
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let function = result
        .functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("fib"))
        .map(|(_, function)| function)
        .unwrap();
    let method = &result.class_defs[0].methods[0].4;

    assert!(function.binary_long_recursion_plan.is_some());
    assert!(method.binary_long_recursion_plan.is_some());
}

#[test]
fn test_binary_long_recursion_plan_preserves_function_and_method_results() {
    assert_eq!(
        run_php(
            "<?php
function fib($n) {
    if ($n <= 1) return $n;
    return fib($n - 1) + fib($n - 2);
}
class Calculator {
    public function fib($n) {
        if ($n <= 1) return $n;
        return $this->fib($n - 1) + $this->fib($n - 2);
    }
}
$calculator = new Calculator();
echo fib(20) . '|' . $calculator->fib(20);
"
        ),
        "6765|6765"
    );
}

#[test]
fn test_binary_long_recursion_method_respects_inheritance_dispatch() {
    assert_eq!(
        run_php(
            "<?php
class Calculator {
    public function fib($n) {
        if ($n <= 1) return $n;
        return $this->fib($n - 1) + $this->fib($n - 2);
    }
}
class InheritedCalculator extends Calculator {}
class OverrideCalculator extends Calculator {
    public function fib($n) { return 99; }
}
$inherited = new InheritedCalculator();
$override = new OverrideCalculator();
echo $inherited->fib(20) . '|' . $override->fib(20);
"
        ),
        "6765|99"
    );
}

#[test]
fn test_binary_long_recursion_plan_supports_strict_base_comparison() {
    assert_eq!(
        run_php(
            "<?php
function fib($n) {
    if ($n < 2) return $n;
    return fib($n - 1) + fib($n - 2);
}
echo fib(20);
"
        ),
        "6765"
    );
}

#[test]
fn test_binary_long_recursion_plan_rejects_calls_to_another_function() {
    let source = "<?php
function leaf($n) { return $n; }
function not_recursive($n) {
    if ($n <= 1) return $n;
    return leaf($n - 1) + leaf($n - 2);
}
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let function = result
        .functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("not_recursive"))
        .map(|(_, function)| function)
        .unwrap();

    assert!(function.binary_long_recursion_plan.is_none());
    assert_eq!(run_php(source), "");
}

#[test]
fn test_binary_long_recursion_plan_falls_back_for_double_input() {
    assert_eq!(
        run_php(
            "<?php
function fib($n) {
    if ($n <= 1) return $n;
    return fib($n - 1) + fib($n - 2);
}
echo fib(10.0);
"
        ),
        "55"
    );
}

#[test]
fn test_binary_long_recursion_plan_falls_back_on_result_overflow() {
    assert_eq!(
        run_php(
            "<?php
function grow($n) {
    if ($n <= 1) return 2;
    return grow($n - 1) * grow($n - 2);
}
echo gettype(grow(10));
"
        ),
        "double"
    );
}

#[test]
fn test_binary_long_recursion_plan_falls_back_past_compact_depth() {
    assert_eq!(
        run_php(
            "<?php
function linearish($n) {
    if ($n <= 0) return 1;
    return linearish($n - 1) + linearish($n - 1000);
}
echo linearish(257);
"
        ),
        "258"
    );
}

#[test]
fn test_hot_many_iterations_same_function() {
    // Non-recursive but called many times → crosses threshold via loop.
    assert_eq!(
        run_php(
            "<?php
function add($a, $b) { return $a + $b; }
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum = add($sum, $i);
}
echo $sum;
"
        ),
        "4950"
    );
}

#[test]
fn test_direct_scalar_call_chain_gets_fast_scalar_plan() {
    let source = "<?php
function leaf($x) { return $x + 1; }
function chain($x) { return leaf($x); }
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let chain = result
        .functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("chain"))
        .map(|(_, function)| function)
        .unwrap();

    assert!(!chain.op_array.may_access_globals);
    assert_eq!(chain.common.plan.call, CallStrategy::FastScalar);
}

#[test]
fn test_straight_line_integer_function_gets_scalar_long_plan() {
    let source = "<?php function calc($a, $b) { return ($a + 1) * ($b - 2); }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let calc = result
        .functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("calc"))
        .map(|(_, function)| function)
        .unwrap();

    let plan = calc.scalar_long_plan.as_deref().expect("scalar long plan");
    assert_eq!(plan.public_args, 2);
    assert_eq!(plan.program.operations.len(), 3);
}

#[test]
fn test_spaceship_function_gets_target_neutral_scalar_long_plan() {
    let source = "<?php function compare($left, $right) { return $left <=> $right; }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let compare = result
        .functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("compare"))
        .map(|(_, function)| function)
        .unwrap();

    let plan = compare
        .scalar_long_plan
        .as_deref()
        .expect("spaceship scalar long plan");
    assert_eq!(plan.public_args, 2);
    assert_eq!(plan.program.operations.len(), 1);
    assert_eq!(plan.program.operations[0].kind, ScalarLongOpKind::Compare);
}

#[test]
fn test_pure_call_chain_gets_composed_scalar_body_plan() {
    let source = "<?php
function add1($x) { return $x + 1; }
function double($x) { return $x + $x; }
function combine($a, $b) { return add1($a) + double($b); }
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let combine = result
        .functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("combine"))
        .map(|(_, function)| function)
        .unwrap();

    assert!(combine.scalar_long_plan.is_none());
    let plan = combine
        .composed_scalar_long_plan
        .as_deref()
        .expect("composed scalar body plan");
    assert_eq!(plan.public_args, 2);
    assert_eq!(plan.program.operations.len(), 3);
    assert_eq!(plan.program.output_count, 1);
    assert_eq!(
        plan.program
            .operations
            .iter()
            .filter(|operation| matches!(operation, ComposedScalarLongOp::Call(_)))
            .count(),
        2
    );
    assert!(
        plan.program
            .operations
            .iter()
            .all(|operation| match operation {
                ComposedScalarLongOp::Arithmetic(_) => true,
                ComposedScalarLongOp::Call(call) => {
                    matches!(call.guard, ScalarLongCallGuard::FunctionCache { .. })
                }
            })
    );
}

#[test]
fn test_hot_direct_scalar_leaf_falls_back_on_overflow() {
    assert_eq!(
        run_php(
            "<?php
function leaf($value) { return $value + 1; }
function chain($value) { return leaf($value); }
$sum = 0;
for ($i = 0; $i < 100; $i++) { $sum += chain($i); }
echo $sum . ':' . gettype(chain(9223372036854775807));
"
        ),
        "5050:double"
    );
}

#[test]
fn test_hot_deferred_scalar_call_with_nested_argument() {
    assert_eq!(
        run_php(
            "<?php
function add($a, $b) { return $a + $b; }
function twice($value) { return $value * 2; }
function chain($value) { return add($value, twice($value)); }
$sum = 0;
for ($i = 0; $i < 100; $i++) { $sum += chain($i); }
echo $sum;
"
        ),
        "14850"
    );
}

#[test]
fn test_transitive_global_chain_stays_conservative() {
    let source = "<?php
function reader() { global $value; return $value; }
function chain() { return reader(); }
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let chain = result
        .functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("chain"))
        .map(|(_, function)| function)
        .unwrap();

    assert!(chain.op_array.may_access_globals);
    assert_ne!(chain.common.plan.call, CallStrategy::FastScalar);
}

#[test]
fn test_leaf_scalar_method_gets_fast_scalar_plan() {
    let source = "<?php
class Math {
    public function add($a, $b) { return $a + $b; }
}
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let method = &result.class_defs[0].methods[0].4;

    assert_eq!(method.common.sig.this_offset, 1);
    assert_eq!(method.common.sig.public_arity(), 2);
    assert_eq!(method.common.plan.call, CallStrategy::FastScalar);
    assert!(method.common.plan.borrow_this);
}

#[test]
fn test_method_with_dynamic_dispatch_stays_conservative() {
    let source = "<?php
class Wrapper {
    public function run($other, $value) { return $other->apply($value); }
}
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let method = &result.class_defs[0].methods[0].4;

    assert!(method.op_array.may_access_globals);
    assert_ne!(method.common.plan.call, CallStrategy::FastScalar);
}

#[test]
fn test_method_returning_this_keeps_owned_receiver() {
    let source = "<?php
class Identity {
    public function me() { return $this; }
}
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let method = &result.class_defs[0].methods[0].4;

    assert_eq!(method.common.plan.call, CallStrategy::FastScalar);
    assert!(!method.common.plan.borrow_this);
    assert_eq!(
        run_php(
            "<?php
class Identity {
    public function me() { return $this; }
}
$object = new Identity();
echo $object->me() === $object ? 'same' : 'different';
"
        ),
        "same"
    );
}
