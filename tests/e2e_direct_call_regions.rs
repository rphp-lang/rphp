mod common;

#[test]
fn published_property_initializers_keep_finalized_signature_guards() {
    use rphp::{compiler::compile::Compiler, lexer::Lexer, parser::Parser};
    let source = r#"<?php
class PublishedInitializers {
    public $value;
    public function zero() {}
    public function one($value) { $this->value = $value; }
    public function eight($a, $b, $c, $d, $e, $f, $g, $h) { $this->value = $h; }
    public function optional($value = null) { $this->value = $value; }
    public function reference(&$value) { $this->value = $value; }
    public function variadic(...$values) { $this->value = $values; }
    public function nine($a, $b, $c, $d, $e, $f, $g, $h, $i) { $this->value = $i; }
    #[Deprecated]
    public function diagnostic($value) { $this->value = $value; }
}
"#;
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let mut admitted = Vec::new();
    let mut rejected = Vec::new();
    for class in &compilation.class_defs {
        for (name, _, _, _, function) in &class.methods {
            let Some(plan) = &function.property_init_plan else {
                rejected.push(name.as_str());
                continue;
            };
            let common = &function.common;
            assert_eq!(u32::from(plan.public_args), common.sig.public_arity());
            assert!(common.plan.call.is_compact_user_call());
            assert_eq!(common.plan.ret, rphp::vm::function::ReturnStrategy::Fast);
            assert_eq!(common.sig.ref_args, 0);
            assert!(!common.sig.is_variadic);
            assert!(plan.public_args <= 8);
            assert!(
                plan.assignments
                    .iter()
                    .all(|assignment| assignment.argument < plan.public_args)
            );
            admitted.push(plan.public_args);
        }
    }
    admitted.sort();
    assert_eq!(admitted, [0, 1, 8]);
    // Declared properties also publish their two canonical accessors.
    assert_eq!(
        rejected,
        [
            "optional",
            "reference",
            "variadic",
            "nine",
            "diagnostic",
            "$value::get",
            "$value::set"
        ]
    );
}

#[test]
fn published_scalar_plans_keep_their_own_finalized_signature() {
    use rphp::{compiler::compile::Compiler, lexer::Lexer, parser::Parser};
    let source = r#"<?php
function zero() { return 7; }
function one($value) { return $value + 1; }
function eight($a, $b, $c, $d, $e, $f, $g, $h) { return $a + $h; }
function branch(int $value): int { if ($value < 0) { return $value - 1; } return $value + 1; }
class FinalizedCalls {
    public function one($value) { return $value + 2; }
    public static function fixed($value) { return $value + 3; }
}
"#;
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let mut checked = 0;
    for function in compilation
        .functions
        .iter()
        .map(|(_, function)| function)
        .chain(
            compilation
                .class_defs
                .iter()
                .flat_map(|class| class.methods.iter().map(|(_, _, _, _, method)| method)),
        )
    {
        let plan = function
            .scalar_long_plan
            .as_ref()
            .expect("pure fixed signature");
        assert_eq!(
            u32::from(plan.public_args),
            function.common.sig.public_arity()
        );
        assert!(plan.public_args <= 8);
        assert!(function.common.supports_scalar_long_plan());
        checked += 1;
    }
    assert_eq!(checked, 6);
}

#[test]
fn property_closure_call_preserves_empty_and_captured_environments() {
    let output = common::run_php(
        r#"<?php
final class Transform {
    public $callback;
    public function __construct($callback) { $this->callback = $callback; }
    public function apply($value) {
        $callback = $this->callback;
        return $callback($value);
    }
}
function runTransform($transform, $iterations) {
    $result = 0;
    for ($index = 0; $index < $iterations; $index++) {
        $result = $transform->apply($result);
    }
    return $result;
}
$empty = new Transform(function ($value) { return $value + 1; });
$offset = 3;
$captured = new Transform(function ($value) use ($offset) { return $value + $offset; });
echo runTransform($empty, 1000), ':', runTransform($captured, 1000);
"#,
    );
    assert_eq!(output, "1000:3000");
}

#[test]
fn property_closure_call_falls_back_for_references_and_string_callables() {
    let output = common::run_php(
        r#"<?php
final class Transform {
    public $callback;
    public function __construct($callback) { $this->callback = $callback; }
    public function apply($value) {
        $callback = $this->callback;
        return $callback($value);
    }
}
function runTransform($transform, $iterations) {
    $result = 0;
    for ($index = 0; $index < $iterations; $index++) {
        $result = $transform->apply($result);
    }
    return $result;
}
function incrementValue($value) { return $value + 1; }
$callback = function ($value) { return $value + 2; };
$referenced = new Transform(null);
$referenced->callback =& $callback;
$named = new Transform('incrementValue');
echo runTransform($referenced, 1000), ':', runTransform($named, 1000);
"#,
    );
    assert_eq!(output, "2000:1000");
}

#[test]
fn captured_argument_closure_preserves_live_alias_and_reference_captures() {
    let output = common::run_php(
        r#"<?php
function invokeCaptured(Closure $callback, int $value): int {
    return $callback($value);
}
function runLiveAlias(Closure $callback): string {
    $sum = 0;
    for ($index = 0; $index < 1000; $index++) {
        $copy = $callback;
        $sum += invokeCaptured($copy, $index & 7);
    }
    return $sum . ':' . $copy(1);
}
$offset = 0;
$reference = function ($value) use (&$offset) {
    $offset++;
    return $value + $offset;
};
echo runLiveAlias($reference), ':', $offset;
"#,
    );
    assert_eq!(output, "504000:1002:1001");
}

#[test]
fn by_reference_closure_return_through_wrapper_reads_the_php_value() {
    let output = common::run_php(
        r#"<?php
function &invokeReference(Closure $callback, int $value): int {
    return $callback($value);
}
$seed = 5;
$callback = static function &(int $ignored) use ($seed): int {
    return $seed;
};
echo invokeReference($callback, 1);
"#,
    );
    assert_eq!(output, "5");
}
