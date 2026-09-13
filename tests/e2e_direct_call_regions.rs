mod common;

#[test]
fn scalar_bitmap_reuse_preserves_owners_and_global_array_mirrors() {
    for locals in [0, 80] {
        let declarations = (0..locals)
            .map(|index| format!("$padding{index} = {index};\n"))
            .collect::<String>();
        let source = include_str!("fixtures/scalar_bitmap_mirrors.php")
            .replace("// FRAME_LOCALS", &declarations);
        assert_eq!(common::run_php(&source), "100:5:0:1\n5\n1:1\n11:11\n0\n");
    }
}

#[test]
fn numeric_writeback_quickening_records_only_matching_immutable_adjacency() {
    use rphp::vm::instruction::{
        ARITHMETIC_COMPOUND_ASSIGN, ARITHMETIC_NEXT_PRIMITIVE_ASSIGN, ASSIGN_CV_REBIND,
        Instruction, KnownScalarType, OpType,
    };
    use rphp::vm::opcode::OpCode;
    for invalid in 0..8 {
        let mut code = rphp::compiler::compile::Compiler::new()
            .compile(&[])
            .unwrap()
            .main;
        code.num_cvs = 2;
        code.num_temps = 1;
        let mut add = Instruction::new(OpCode::Add);
        add.op1_type = OpType::Cv;
        add.op2_type = OpType::Const;
        add.result_type = OpType::Tmp;
        add.result = 2;
        add._pad = ARITHMETIC_COMPOUND_ASSIGN;
        add.set_known_result_type(KnownScalarType::Long);
        let mut assign = Instruction::new(OpCode::AssignCv);
        assign.op1_type = OpType::Cv;
        assign.op1 = 1;
        assign.op2_type = OpType::Tmp;
        assign.op2 = 2;
        match invalid {
            1 => assign.op1 = 2,
            2 => assign.op2 = 1,
            3 => assign.op1_type = OpType::Tmp,
            4 => assign.op2_type = OpType::Cv,
            5 => assign.result_type = OpType::Tmp,
            6 => assign._pad = ASSIGN_CV_REBIND,
            7 => add.result_type = OpType::Cv,
            _ => {}
        }
        code.instructions = vec![add, assign];
        for _ in 0..2 {
            code.specialize_opcodes();
            assert_eq!(code.instructions.len(), 2);
            let quickened = &code.instructions[0];
            assert_eq!(
                quickened._pad & ARITHMETIC_NEXT_PRIMITIVE_ASSIGN != 0,
                invalid == 0
            );
            assert_eq!(
                quickened._pad & ARITHMETIC_COMPOUND_ASSIGN,
                ARITHMETIC_COMPOUND_ASSIGN
            );
            assert_eq!(quickened.known_result_type(), KnownScalarType::Long);
            if invalid == 0 {
                assert_eq!(quickened.extended_value, 1);
            }
        }
        code.instructions.pop();
        code.specialize_opcodes();
        assert_eq!(
            code.instructions[0]._pad & ARITHMETIC_NEXT_PRIMITIVE_ASSIGN,
            0
        );
    }
}

#[test]
fn numeric_add_writeback_keeps_temps_aliases_diagnostics_and_evaluation_order() {
    use rphp::vm::{instruction::OpType, opcode::OpCode};
    for locals in [0, 80] {
        let declarations = (0..locals)
            .map(|index| format!("$padding{index} = {index};\n"))
            .collect::<String>();
        let source = include_str!("fixtures/numeric_add_writeback.php")
            .replace("// FRAME_LOCALS", &declarations);
        let statements =
            rphp::parser::Parser::new(rphp::lexer::Lexer::new(&source).tokenize().unwrap())
                .parse()
                .unwrap();
        let compiled = rphp::compiler::compile::Compiler::new()
            .compile(&statements)
            .unwrap();
        let function = &compiled
            .functions
            .iter()
            .find(|(name, _)| name == "numericWriteback")
            .unwrap()
            .1;
        assert_eq!(
            function.op_array.num_cvs + function.op_array.num_temps > 64,
            locals == 80
        );
        assert!(function.op_array.instructions.windows(2).any(|pair| {
            matches!(
                pair[0].opcode,
                OpCode::Add | OpCode::Add_TmpTmp | OpCode::Add_CvTmp
            ) && pair[1].opcode == OpCode::AssignCv
                && pair[1].result_type == OpType::Unused
                && pair[1].op2 == pair[0].result
        }));
        let negative_zero = if cfg!(target_endian = "little") {
            "0000000000000080"
        } else {
            "8000000000000000"
        };
        let expected = format!(
            "integer:21:21\ninteger:2:2\ninteger:97:97\ndouble:35.25:35.25\ninteger:97:97\n6.5:6.5\n11:11\ndouble:0\n{negative_zero}\n{{\"left\":1,\"right\":2}}\n15\nnumeric warning:8\ninvalid:8\nwarnings:1\n"
        );
        assert_eq!(common::run_php(&source), expected);
    }
}

#[test]
fn scalar_reference_and_external_results_preserve_frame_retirement() {
    for locals in [0, 80] {
        let declarations = (0..locals)
            .map(|index| format!("$padding{index} = {index};\n"))
            .collect::<String>();
        let source = include_str!("fixtures/scalar_reference_retirement.php")
            .replace("// FRAME_LOCALS", &declarations);
        let statements =
            rphp::parser::Parser::new(rphp::lexer::Lexer::new(&source).tokenize().unwrap())
                .parse()
                .unwrap();
        let compiled = rphp::compiler::compile::Compiler::new()
            .compile(&statements)
            .unwrap();
        let function = &compiled
            .functions
            .iter()
            .find(|(name, _)| name == "retirementFrame")
            .unwrap()
            .1;
        assert_eq!(
            function.op_array.num_cvs + function.op_array.num_temps > 64,
            locals == 80
        );
        assert_eq!(
            common::run_php(&source),
            "3:1:5\n5\n3:1:6\ndrop:local\n6\n2:caller\ndone\n"
        );
    }
}

#[test]
fn single_argument_leaf_preserves_operand_sources_and_canonical_fallbacks() {
    let expected =
        b"[[10,20,20,33,18,12,9.5,true,true,10,10,10,\"array-error\",\"string-error\"],13,128]\n";
    for disable_jit in [false, true] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_rphp"))
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/scalar_single_argument.php"
            ))
            .env("RPHP_DISABLE_JIT", if disable_jit { "1" } else { "0" })
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(output.stdout, expected);
    }
}

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
