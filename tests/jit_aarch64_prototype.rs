#![cfg(all(
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]

mod common;

use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::jit::{
    Arm64Assembler, Arm64Register, CompiledAddMultiply, CompiledQuickLongAccumulateLoop,
    CompiledQuickLongConditionalAccumulateLoop, CompiledScalarLongProgram,
    CompiledQuickLongStraightLoop, NativeConditionalLongLoopCondition,
    NativeConditionalLongLoopConfig, NativeLongAccumulateState,
    NativeStraightLongConditionOperand, NativeStraightLongLoopConfig,
    NativeStraightLongLoopOutcome, NativeStraightLongOperation,
    NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES, NATIVE_STRAIGHT_LONG_MAX_OPERATIONS,
    QuickLongAccumulateJitError, QuickLongAccumulateJitOutcome,
    SCALAR_DOUBLE_JIT_HOT_THRESHOLD, SCALAR_LONG_JIT_HOT_THRESHOLD, ScalarLongJitDispatch, ScalarLongJitError,
    ScalarLongJitOutcome,
};
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::execute;
use rphp::vm::function::{
    FunctionCommon, ScalarLongConditionKind, ScalarLongConditionOperand, ScalarLongFunctionPlan,
    ScalarLongOp, ScalarLongOpKind, ScalarLongProgram, ScalarLongSelect, ScalarLongSource,
};
use rphp::vm::planner::BlockPlan;
use rphp::vm::quick::{QuickLongOp, QuickLongOperand};

fn scalar_plan(
    public_args: u8,
    operations: Vec<ScalarLongOp>,
    output: ScalarLongSource,
) -> ScalarLongFunctionPlan {
    ScalarLongFunctionPlan::new(
        public_args,
        ScalarLongProgram {
            operations: operations.into_boxed_slice(),
            outputs: [output],
            output_count: 1,
        },
        None,
    )
}

#[test]
fn real_php_exact_float_calls_enter_double_jit_and_long_inputs_fallback() {
    let call_count = usize::from(SCALAR_DOUBLE_JIT_HOT_THRESHOLD) + 8;
    let mut source = String::from(
        "<?php function blend(float $a, float $b, float $c): float { return (($a + 1.5) * $b) / $c; } $total = 0.0;",
    );
    for _ in 0..call_count {
        source.push_str("$total = $total + blend(2.5, 4.0, 2.0);");
    }
    source.push_str("echo $total . ':' . blend(2, 4, 2);");

    let tokens = Lexer::new(&source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "576:7"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("blend"))
        .map(|(_, function)| function)
        .expect("compiled blend function");
    let plan = function
        .scalar_double_plan
        .as_deref()
        .expect("Double scalar plan");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 9);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_typed_double_call_accumulation_enters_one_native_region() {
    let source = "<?php function calculateFloat(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } $scale = 2.0; $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += calculateFloat(1.5, 2.5, $scale); } echo $i . ':' . $total;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:600000"
    );

    let loop_plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a Double call/accumulate loop");
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);
    assert_eq!(loop_plan.native_jit().side_exits(), 0);

    let leaf = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("calculateFloat"))
        .and_then(|(_, function)| function.scalar_double_plan.as_deref())
        .expect("Double leaf plan");
    assert!(!leaf.native_jit().is_compiled());
}

#[test]
fn typed_double_argument_expressions_enter_one_native_region() {
    let source = "<?php function calculateFloat(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } $scale = 2.0; $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += calculateFloat($i * 0.5, $scale + 1.0, 2.0); } echo $i . ':' . $total;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:5000350000"
    );

    let loop_plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should compose Double argument expressions");
    assert_eq!(loop_plan.argument_program.operations.len(), 2);
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);
    assert_eq!(loop_plan.native_jit().side_exits(), 0);

    let leaf = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("calculateFloat"))
        .and_then(|(_, function)| function.scalar_double_plan.as_deref())
        .expect("Double leaf plan");
    assert!(!leaf.native_jit().is_compiled());
}

#[test]
fn monomorphic_float_method_uses_class_cache_and_double_jit() {
    let call_count = usize::from(SCALAR_DOUBLE_JIT_HOT_THRESHOLD) + 8;
    let source = format!(
        "<?php class FloatModel {{ public function blend(float $a, float $b, float $c): float {{ return (($a + 1.5) * $b) / $c; }} }} $model = new FloatModel(); $total = 0.0; for ($i = 0; $i < {call_count}; $i++) {{ $total += $model->blend(2.5, 4.0, 2.0); }} echo $total;"
    );
    let tokens = Lexer::new(&source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "576"
    );

    let class = globals
        .class_table
        .values()
        .find(|class| class.name.eq_ignore_ascii_case("FloatModel"))
        .expect("registered FloatModel");
    let method = class
        .methods
        .iter()
        .find(|(name, ..)| name.eq_ignore_ascii_case("blend"))
        .map(|(_, _, _, _, method)| method)
        .expect("compiled blend method");
    let plan = method
        .scalar_double_plan
        .as_deref()
        .expect("Double scalar method plan");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 8);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn double_jit_zero_divisor_replays_canonical_php_error() {
    let call_count = usize::from(SCALAR_DOUBLE_JIT_HOT_THRESHOLD) + 8;
    let mut source = String::from(
        "<?php function divideFloat(float $value, float $divisor): float { return ($value + 1.0) / $divisor; }",
    );
    for _ in 0..call_count {
        source.push_str("divideFloat(7.0, 2.0);");
    }
    source.push_str("divideFloat(7.0, 0.0);");

    let tokens = Lexer::new(&source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, _output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    let error = execute::execute(&mut globals, &main).unwrap_err();
    assert!(matches!(
        error,
        rphp::vm::execute::VmError::Fatal(message) if message == "Division by zero"
    ));

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("divideFloat"))
        .map(|(_, function)| function)
        .expect("compiled divideFloat function");
    let plan = function
        .scalar_double_plan
        .as_deref()
        .expect("Double scalar plan");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 10);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

fn contains_signed_divide(code: &[u8]) -> bool {
    code.chunks_exact(4).any(|bytes| {
        let word = u32::from_le_bytes(bytes.try_into().unwrap());
        word & 0xffe0_fc00 == 0x9ac0_0c00
    })
}

fn assert_amortized_safepoint_chunks(chunks: u64) {
    assert!(
        (90..=100).contains(&chunks),
        "100,000 native iterations should use roughly 1,024-iteration safepoint chunks, got {chunks}"
    );
}

fn conditional_scalar_plan(
    public_args: u8,
    operations: Vec<ScalarLongOp>,
    select: ScalarLongSelect,
) -> ScalarLongFunctionPlan {
    ScalarLongFunctionPlan::new(
        public_args,
        ScalarLongProgram {
            operations: operations.into_boxed_slice(),
            outputs: [select.when_true],
            output_count: 1,
        },
        Some(select),
    )
}

#[test]
fn encoder_produces_expected_arm64_instruction_words() {
    let mut assembler = Arm64Assembler::new();
    assembler.add_register(Arm64Register::X0, Arm64Register::X0, Arm64Register::X1);
    assembler.add_immediate(Arm64Register::X0, Arm64Register::X0, 1);
    assembler.multiply_register(Arm64Register::X0, Arm64Register::X0, Arm64Register::X2);
    assembler.ret();

    assert_eq!(
        assembler.finish(),
        [
            0x00, 0x00, 0x01, 0x8b, // add x0, x0, x1
            0x00, 0x04, 0x00, 0x91, // add x0, x0, #1
            0x00, 0x7c, 0x02, 0x9b, // mul x0, x0, x2
            0xc0, 0x03, 0x5f, 0xd6, // ret
        ]
    );
}

#[test]
fn generated_code_executes_through_the_arm64_abi() {
    let function = CompiledAddMultiply::compile().expect("JIT code should be executable");

    assert_eq!(function.call(7, 5, 3), 36);
    assert_eq!(function.call(-9, 4, 8), -40);
    assert_eq!(function.call(0, 123, -7), -861);
}

#[test]
fn scalar_long_ir_is_lowered_and_executed_as_native_code() {
    let plan = scalar_plan(
        3,
        vec![
            ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Input(1),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Temporary(0),
                rhs: ScalarLongSource::Input(2),
            },
        ],
        ScalarLongSource::Temporary(1),
    );
    let function = CompiledScalarLongProgram::compile(&plan).expect("plan should lower");

    assert_eq!(
        function.call(&[7, 5, 3]).unwrap(),
        ScalarLongJitOutcome::Value(36)
    );
    assert_eq!(
        function.call(&[-9, 4, 8]).unwrap(),
        ScalarLongJitOutcome::Value(-40)
    );
}

#[test]
fn conditional_scalar_ir_lowers_all_comparison_kinds() {
    let cases = [
        (ScalarLongConditionKind::Equal, [4, 4], 11),
        (ScalarLongConditionKind::Equal, [4, 5], 22),
        (ScalarLongConditionKind::NotEqual, [4, 5], 11),
        (ScalarLongConditionKind::NotEqual, [4, 4], 22),
        (ScalarLongConditionKind::LessThan, [4, 5], 11),
        (ScalarLongConditionKind::LessThan, [5, 4], 22),
        (ScalarLongConditionKind::LessThanOrEqual, [4, 4], 11),
        (ScalarLongConditionKind::LessThanOrEqual, [5, 4], 22),
    ];

    for (kind, inputs, expected) in cases {
        let plan = conditional_scalar_plan(
            2,
            vec![],
            ScalarLongSelect {
                kind,
                lhs: ScalarLongConditionOperand::Source(ScalarLongSource::Input(0)),
                rhs: ScalarLongConditionOperand::Source(ScalarLongSource::Input(1)),
                shared_operation_count: 0,
                when_true_operation_count: 0,
                when_true: ScalarLongSource::Constant(11),
                when_false: ScalarLongSource::Constant(22),
            },
        );
        let function = CompiledScalarLongProgram::compile(&plan).expect("select should lower");
        assert_eq!(
            function.call(&inputs).unwrap(),
            ScalarLongJitOutcome::Value(expected),
            "{kind:?} with {inputs:?}"
        );
    }
}

#[test]
fn conditional_scalar_ir_executes_only_the_selected_bitmask_arm() {
    let plan = conditional_scalar_plan(
        1,
        vec![
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(3),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(5),
            },
        ],
        ScalarLongSelect {
            kind: ScalarLongConditionKind::Equal,
            lhs: ScalarLongConditionOperand::BitwiseAnd {
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(1),
            },
            rhs: ScalarLongConditionOperand::Source(ScalarLongSource::Constant(0)),
            shared_operation_count: 0,
            when_true_operation_count: 1,
            when_true: ScalarLongSource::Temporary(0),
            when_false: ScalarLongSource::Temporary(1),
        },
    );
    let function = CompiledScalarLongProgram::compile(&plan).expect("select should lower");

    assert_eq!(
        function.call(&[4]).unwrap(),
        ScalarLongJitOutcome::Value(12)
    );
    assert_eq!(
        function.call(&[5]).unwrap(),
        ScalarLongJitOutcome::Value(25)
    );
}

#[test]
fn conditional_scalar_ir_skips_inactive_overflow_and_exits_on_selected_overflow() {
    let plan = conditional_scalar_plan(
        1,
        vec![
            ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(1),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(100_000_000_000_000_000),
            },
        ],
        ScalarLongSelect {
            kind: ScalarLongConditionKind::LessThan,
            lhs: ScalarLongConditionOperand::Source(ScalarLongSource::Input(0)),
            rhs: ScalarLongConditionOperand::Source(ScalarLongSource::Constant(100)),
            shared_operation_count: 0,
            when_true_operation_count: 1,
            when_true: ScalarLongSource::Temporary(0),
            when_false: ScalarLongSource::Temporary(1),
        },
    );
    let function = CompiledScalarLongProgram::compile(&plan).expect("select should lower");

    assert_eq!(
        function.call(&[5]).unwrap(),
        ScalarLongJitOutcome::Value(6)
    );
    assert_eq!(
        function.call(&[100]).unwrap(),
        ScalarLongJitOutcome::SideExit
    );
}

#[test]
fn constants_subtraction_and_xor_share_the_native_lowering() {
    let plan = scalar_plan(
        2,
        vec![
            ScalarLongOp {
                kind: ScalarLongOpKind::Subtract,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(5),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::BitwiseXor,
                lhs: ScalarLongSource::Temporary(0),
                rhs: ScalarLongSource::Input(1),
            },
        ],
        ScalarLongSource::Temporary(1),
    );
    let function = CompiledScalarLongProgram::compile(&plan).expect("plan should lower");

    assert_eq!(
        function.call(&[20, 3]).unwrap(),
        ScalarLongJitOutcome::Value(12)
    );
    assert!(function.code().len() >= 4);
}

#[test]
fn checked_arithmetic_side_exits_before_publishing_an_overflowed_result() {
    let cases = [
        (ScalarLongOpKind::Add, i64::MAX, 1),
        (ScalarLongOpKind::Subtract, i64::MIN, 1),
        (ScalarLongOpKind::Multiply, i64::MAX, 2),
        (ScalarLongOpKind::Multiply, i64::MIN, -1),
    ];

    for (kind, lhs, rhs) in cases {
        let plan = scalar_plan(
            2,
            vec![ScalarLongOp {
                kind,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Input(1),
            }],
            ScalarLongSource::Temporary(0),
        );
        let function = CompiledScalarLongProgram::compile(&plan).expect("plan should lower");
        assert_eq!(
            function.call(&[lhs, rhs]).unwrap(),
            ScalarLongJitOutcome::SideExit,
            "{kind:?} should side-exit"
        );
    }
}

#[test]
fn invalid_ir_is_rejected_before_code_becomes_executable() {
    let forward_temporary = scalar_plan(
        1,
        vec![ScalarLongOp {
            kind: ScalarLongOpKind::Add,
            lhs: ScalarLongSource::Temporary(0),
            rhs: ScalarLongSource::Input(0),
        }],
        ScalarLongSource::Temporary(0),
    );
    assert!(matches!(
        CompiledScalarLongProgram::compile(&forward_temporary),
        Err(ScalarLongJitError::InvalidProgram(_))
    ));

    let false_edge_uses_true_temporary = conditional_scalar_plan(
        1,
        vec![
            ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(1),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Temporary(0),
                rhs: ScalarLongSource::Constant(2),
            },
        ],
        ScalarLongSelect {
            kind: ScalarLongConditionKind::Equal,
            lhs: ScalarLongConditionOperand::Source(ScalarLongSource::Input(0)),
            rhs: ScalarLongConditionOperand::Source(ScalarLongSource::Constant(0)),
            shared_operation_count: 0,
            when_true_operation_count: 1,
            when_true: ScalarLongSource::Temporary(0),
            when_false: ScalarLongSource::Temporary(1),
        },
    );
    assert!(matches!(
        CompiledScalarLongProgram::compile(&false_edge_uses_true_temporary),
        Err(ScalarLongJitError::InvalidProgram(_))
    ));
}

#[test]
fn division_and_modulo_match_checked_scalar_semantics() {
    for kind in [ScalarLongOpKind::IntDivide, ScalarLongOpKind::Modulo] {
        let plan = scalar_plan(
            2,
            vec![ScalarLongOp {
                kind,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Input(1),
            }],
            ScalarLongSource::Temporary(0),
        );
        let function = CompiledScalarLongProgram::compile(&plan).expect("operation should lower");

        for (lhs, rhs) in [
            (17_i64, 5_i64),
            (-17_i64, 5_i64),
            (17_i64, -5_i64),
            (-17_i64, -5_i64),
        ] {
            let expected = match kind {
                ScalarLongOpKind::IntDivide => lhs.checked_div(rhs),
                ScalarLongOpKind::Modulo => lhs.checked_rem(rhs),
                _ => unreachable!(),
            }
            .map(ScalarLongJitOutcome::Value)
            .unwrap_or(ScalarLongJitOutcome::SideExit);
            assert_eq!(function.call(&[lhs, rhs]).unwrap(), expected);
        }

        assert_eq!(
            function.call(&[123, 0]).unwrap(),
            ScalarLongJitOutcome::SideExit
        );
        assert_eq!(
            function.call(&[i64::MIN, -1]).unwrap(),
            ScalarLongJitOutcome::SideExit
        );
    }
}

#[test]
fn constant_power_of_two_modulo_uses_signed_divide_free_lowering() {
    let values = [
        i64::MIN,
        i64::MIN + 1,
        -17,
        -8,
        -3,
        -1,
        0,
        1,
        3,
        8,
        17,
        i64::MAX,
    ];

    for divisor in [1, -1, 2, -2, 8, -8, i64::MIN] {
        let plan = scalar_plan(
            1,
            vec![ScalarLongOp {
                kind: ScalarLongOpKind::Modulo,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(divisor),
            }],
            ScalarLongSource::Temporary(0),
        );
        let function = CompiledScalarLongProgram::compile(&plan)
            .expect("constant power-of-two modulo should lower");
        assert!(
            !contains_signed_divide(function.code()),
            "constant modulo by {divisor} must not contain SDIV"
        );

        for value in values {
            let expected = value.checked_rem(divisor).unwrap_or(0);
            assert_eq!(
                function.call(&[value]).unwrap(),
                ScalarLongJitOutcome::Value(expected),
                "incorrect {value} % {divisor}"
            );
        }
    }
}

#[test]
fn native_scalar_abi_rejects_the_wrong_input_count() {
    let plan = scalar_plan(1, Vec::new(), ScalarLongSource::Input(0));
    let function = CompiledScalarLongProgram::compile(&plan).expect("plan should lower");

    assert!(matches!(
        function.call(&[]),
        Err(ScalarLongJitError::InputCount {
            expected: 1,
            actual: 0
        })
    ));
}

#[test]
fn immediate_materialization_preserves_all_64_bits() {
    let constants = [
        0,
        1,
        -1,
        i64::MIN,
        i64::MAX,
        0x1234_5678_9abc_def0_u64 as i64,
        0x0001_0000_0000_0000,
    ];

    for value in constants {
        let plan = scalar_plan(0, Vec::new(), ScalarLongSource::Constant(value));
        let function = CompiledScalarLongProgram::compile(&plan).expect("constant should lower");
        assert_eq!(
            function.call(&[]).unwrap(),
            ScalarLongJitOutcome::Value(value)
        );
    }
}

#[test]
fn native_checked_arithmetic_matches_rust_over_many_inputs() {
    let operations = [
        ScalarLongOpKind::Add,
        ScalarLongOpKind::Subtract,
        ScalarLongOpKind::Multiply,
        ScalarLongOpKind::BitwiseXor,
        ScalarLongOpKind::IntDivide,
        ScalarLongOpKind::Modulo,
    ];
    let mut state = 0x6a09_e667_f3bc_c909_u64;

    for kind in operations {
        let plan = scalar_plan(
            2,
            vec![ScalarLongOp {
                kind,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Input(1),
            }],
            ScalarLongSource::Temporary(0),
        );
        let function = CompiledScalarLongProgram::compile(&plan).expect("operation should lower");

        for _ in 0..10_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let lhs = state as i64;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let rhs = state as i64;

            let expected = match kind {
                ScalarLongOpKind::Add => lhs.checked_add(rhs),
                ScalarLongOpKind::Subtract => lhs.checked_sub(rhs),
                ScalarLongOpKind::Multiply => lhs.checked_mul(rhs),
                ScalarLongOpKind::BitwiseXor => Some(lhs ^ rhs),
                ScalarLongOpKind::IntDivide => lhs.checked_div(rhs),
                ScalarLongOpKind::Modulo => lhs.checked_rem(rhs),
            };
            let expected = expected
                .map(ScalarLongJitOutcome::Value)
                .unwrap_or(ScalarLongJitOutcome::SideExit);
            assert_eq!(function.call(&[lhs, rhs]).unwrap(), expected);
        }
    }
}

#[test]
fn plan_cache_compiles_only_after_hotness_and_tracks_native_side_exits() {
    let plan = scalar_plan(
        2,
        vec![
            ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Input(1),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Temporary(0),
                rhs: ScalarLongSource::Constant(3),
            },
        ],
        ScalarLongSource::Temporary(1),
    );
    let mut arguments = [0_i64; 8];
    arguments[0] = 7;
    arguments[1] = 5;

    for _ in 1..SCALAR_LONG_JIT_HOT_THRESHOLD {
        assert_eq!(
            plan.native_jit().dispatch(&plan, &arguments),
            ScalarLongJitDispatch::Interpret
        );
    }
    assert!(!plan.native_jit().is_compiled());
    assert_eq!(
        plan.native_jit().dispatch(&plan, &arguments),
        ScalarLongJitDispatch::Value(36)
    );
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);

    arguments[0] = i64::MAX;
    arguments[1] = 1;
    assert_eq!(
        plan.native_jit().dispatch(&plan, &arguments),
        ScalarLongJitDispatch::SideExit
    );
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn conditional_plan_cache_compiles_after_hotness() {
    let plan = conditional_scalar_plan(
        1,
        vec![
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(3),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(5),
            },
        ],
        ScalarLongSelect {
            kind: ScalarLongConditionKind::Equal,
            lhs: ScalarLongConditionOperand::BitwiseAnd {
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(1),
            },
            rhs: ScalarLongConditionOperand::Source(ScalarLongSource::Constant(0)),
            shared_operation_count: 0,
            when_true_operation_count: 1,
            when_true: ScalarLongSource::Temporary(0),
            when_false: ScalarLongSource::Temporary(1),
        },
    );
    let mut arguments = [0_i64; 8];
    arguments[0] = 4;

    for _ in 1..SCALAR_LONG_JIT_HOT_THRESHOLD {
        assert_eq!(
            plan.native_jit().dispatch(&plan, &arguments),
            ScalarLongJitDispatch::Interpret
        );
    }
    assert_eq!(
        plan.native_jit().dispatch(&plan, &arguments),
        ScalarLongJitDispatch::Value(12)
    );
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
}

#[test]
fn real_php_calls_enter_cached_native_plan_and_fallback_on_overflow() {
    let call_count = usize::from(SCALAR_LONG_JIT_HOT_THRESHOLD) + 8;
    let mut source = String::from(
        "<?php function calc(int $a, int $b): int { return ($a + $b) * 3; } $total = 0;",
    );
    for _ in 0..call_count {
        source.push_str("$total = $total + calc(1, 2);");
    }
    source.push_str(
        "echo $total; try { calc(PHP_INT_MAX, 1); } catch (TypeError $error) { echo ':caught'; }",
    );

    let tokens = Lexer::new(&source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "648:caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("calc"))
        .map(|(_, function)| function)
        .expect("compiled calc function");
    let plan = function.scalar_long_plan.as_deref().expect("scalar plan");
    assert!(plan.native_jit().is_compiled());
    assert!(plan.native_jit().native_entries() >= 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn real_php_conditional_calls_enter_the_standalone_native_plan() {
    let call_count = usize::from(SCALAR_LONG_JIT_HOT_THRESHOLD) + 8;
    let mut source = String::from(
        "<?php function route(int $value): int { if (($value & 1) == 0) { return ($value * 3) + 1; } return ($value * 5) - 2; } $total = 0;",
    );
    for index in 0..call_count {
        source.push_str(if index & 1 == 0 {
            "$total = $total + route(4);"
        } else {
            "$total = $total + route(5);"
        });
    }
    source.push_str("echo $total;");

    let tokens = Lexer::new(&source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "1296"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("route"))
        .map(|(_, function)| function)
        .expect("compiled route function");
    let plan = function.scalar_long_plan.as_deref().expect("scalar plan");
    assert!(plan.select.is_some());
    assert!(plan.native_jit().is_compiled());
    assert!(plan.native_jit().native_entries() >= 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn cold_strict_branch_is_guarded_inside_the_native_call_region() {
    let source = "<?php function routeStandalone(int $value): int { if (($value & 1) == 0) { return ($value * 3) + 1; } return ($value * 5) - 2; } $total = 0; for ($i = 0; $i < 100; $i++) { $total += routeStandalone($i); if ($i === -1) { echo 'never'; } } echo $total;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "19800"
    );
    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("cold strict branch should retain the scalar-call accumulate region");
    assert!(plan.tail_guard.is_some());
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn taken_trace_guard_resumes_the_canonical_cold_block_before_increment() {
    let source = "<?php function routeGuarded(int $value): int { return ($value * 2) + 1; } $needle = 73; $sum = 0; for ($i = 0; $i < 100; $i++) { $sum += routeGuarded($i); if ($i === $needle) { echo 'hit:' . $i . '|'; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "hit:73|100:10000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("dynamic strict branch should use a guarded call region");
    assert!(plan.native_jit().is_call_compiled());
    assert!(plan.native_jit().native_entries() >= 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn cold_simple_accumulate_guard_stays_inside_the_native_region() {
    let source = "<?php $needle = -1; $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $i; if ($i === $needle) { echo 'never'; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:4999950000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("cold branch should retain the simple accumulate region");
    assert!(plan.tail_guard.is_some());
    assert!(plan.native_jit().is_straight_compiled());
    assert!(!plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 98);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn guarded_invariant_term_is_composed_into_one_native_call() {
    let source = "<?php $offset = 7; $needle = -1; $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $i + $offset; if ($i === $needle) { echo 'never'; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:5000650000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("guarded invariant term should retain the accumulate region");
    assert!(plan.tail_guard.is_some());
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn taken_simple_accumulate_guard_replays_the_cold_block() {
    let source = "<?php $needle = 73; $sum = 0; for ($i = 0; $i < 100; $i++) { $sum += $i; if ($i === $needle) { echo 'hit:' . $i . '|'; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "hit:73|100:4950"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("taken branch should retain the guarded accumulate region");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn native_accumulate_loop_preserves_chunk_and_overflow_boundaries() {
    let program =
        CompiledQuickLongAccumulateLoop::compile().expect("loop should lower to ARM64");
    let mut state = NativeLongAccumulateState {
        induction: 0,
        bound: 100,
        accumulator: 0,
    };

    assert_eq!(
        program.call(&mut state, 32).unwrap(),
        QuickLongAccumulateJitOutcome::ChunkExhausted
    );
    assert_eq!(state.induction, 32);
    assert_eq!(state.accumulator, 496);

    assert_eq!(
        program.call(&mut state, 64).unwrap(),
        QuickLongAccumulateJitOutcome::ChunkExhausted
    );
    assert_eq!(state.induction, 96);
    assert_eq!(state.accumulator, 4_560);

    assert_eq!(
        program.call(&mut state, 32).unwrap(),
        QuickLongAccumulateJitOutcome::Completed
    );
    assert_eq!(state.induction, 100);
    assert_eq!(state.accumulator, 4_950);

    let mut overflow = NativeLongAccumulateState {
        induction: 1,
        bound: 2,
        accumulator: i64::MAX,
    };
    assert_eq!(
        program.call(&mut overflow, 32).unwrap(),
        QuickLongAccumulateJitOutcome::SumOverflow
    );
    assert_eq!(
        overflow,
        NativeLongAccumulateState {
            induction: 1,
            bound: 2,
            accumulator: i64::MAX,
        },
        "overflow must not publish the wrapped ADD result"
    );
    assert!(matches!(
        program.call(&mut state, 0),
        Err(QuickLongAccumulateJitError::ZeroIterationBudget)
    ));
    assert!(!program.code().is_empty());

    let plus_one = CompiledQuickLongAccumulateLoop::compile_with_addend(1)
        .expect("constant term should lower to ARM64");
    let mut plus_one_state = NativeLongAccumulateState {
        induction: 0,
        bound: 10,
        accumulator: 0,
    };
    assert_eq!(
        plus_one.call(&mut plus_one_state, 32).unwrap(),
        QuickLongAccumulateJitOutcome::Completed
    );
    assert_eq!(plus_one_state.induction, 10);
    assert_eq!(plus_one_state.accumulator, 55);

    let plus_two = CompiledQuickLongAccumulateLoop::compile_with_addend(2)
        .expect("overflowing constant term should still lower transactionally");
    let mut term_overflow = NativeLongAccumulateState {
        induction: i64::MAX - 1,
        bound: i64::MAX,
        accumulator: 17,
    };
    assert_eq!(
        plus_two.call(&mut term_overflow, 32).unwrap(),
        QuickLongAccumulateJitOutcome::TermOverflow
    );
    assert_eq!(
        term_overflow,
        NativeLongAccumulateState {
            induction: i64::MAX - 1,
            bound: i64::MAX,
            accumulator: 17,
        },
        "term overflow must preserve the exact term instruction resume state"
    );
}

#[test]
fn real_php_accumulate_loop_enters_native_region() {
    let source = "<?php $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $i; } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:4999950000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select an accumulate quick loop");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn negative_accumulate_loop_uses_range_proven_native_chunks() {
    let source = "<?php $sum = 0; for ($i = -1000; $i < 1000; $i++) { $sum += $i; } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "1000:-1000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a negative accumulate quick loop");
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_guarded_scalar_method_enters_native_accumulate_region() {
    let source = "<?php class ScalarKernel { public function transform(int $value, int $scale): int { return ($value * $scale) + 7; } } $kernel = new ScalarKernel(); $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $kernel->transform($i, 73); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:364997050000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a scalar-method accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_finite_string_method_and_hash_update_enter_one_native_region() {
    let source = "<?php class MixedNativeModel { public function score(int $value, string $key): int { return $value + strlen($key); } } $model = new MixedNativeModel(); $values = ['left' => 0, 'right' => 0]; $key = 'left'; $needle = -1; for ($i = 0; $i < 100000; $i++) { if (($i % 2) == 0) { $key = 'right'; } else { $key = 'left'; } $score = $model->score($i, $key); $values[$key] = $values[$key] + $score; if ($i === $needle) { echo 'never'; } } echo $values['left'] . ':' . $values['right'] . ':' . $i;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "2500200000:2500200000:100000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a mixed typed loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn native_mixed_hash_region_replays_taken_cold_edge_after_prior_store() {
    let source = "<?php class MixedColdModel { public function score(int $value, string $key): int { return $value + strlen($key); } } $model = new MixedColdModel(); $values = ['left' => 0, 'right' => 0]; $key = 'left'; $needle = 73; for ($i = 0; $i < 1000; $i++) { if (($i % 2) == 0) { $key = 'right'; } else { $key = 'left'; } $score = $model->score($i, $key); $values[$key] = $values[$key] + $score; if ($i === $needle) { echo 'hit:' . $i . '|'; } } echo $values['left'] . ':' . $values['right'] . ':' . $i;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "hit:73|252000:252000:1000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should retain the mixed cold-edge region");
    assert!(plan.native_jit().native_entries() >= 2);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn real_php_routing_holdout_enters_multi_method_native_region() {
    let source = include_str!("../benches/holdout_routing_pipeline.php")
        .replace("$start = microtime(true);", "")
        .replace("$elapsed = microtime(true) - $start;", "$elapsed = 0;");
    let tokens = Lexer::new(&source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }
    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "290394364,154183816,54660174,384960,192495,64134,108411|0"
    );
    let plan = functions
        .iter()
        .find_map(|(_, function)| {
            function.op_array.block_plans.iter().find_map(|plan| match plan {
                BlockPlan::QuickLongOps(plan) => Some(plan),
                _ => None,
            })
        })
        .expect("compiler should select the routing holdout as one typed loop");
    assert_eq!(plan.ops.len(), 28);
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn application_order_corpus_enters_virtual_pipeline_native_region() {
    for (function_name, original) in [
        (
            "runQuotePipeline",
            include_str!("../benches/corpus_order_pipeline.php"),
        ),
        (
            "runTypedQuotePipeline",
            include_str!("../benches/corpus_typed_order_pipeline.php"),
        ),
    ] {
        let source = original
            .replace("$start = microtime(true);", "")
            .replace("$elapsed = microtime(true) - $start;", "$elapsed = 0;");
        let tokens = Lexer::new(&source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let compilation = Compiler::new().compile(&statements).unwrap();
        let main = make_user_function(compilation.main);
        let functions = compilation.functions;
        let class_defs = compilation.class_defs;
        let (mut globals, output) = common::make_eg_with_capture();
        for (name, function) in &functions {
            globals
                .register_function(name, &function.common as *const FunctionCommon)
                .unwrap();
        }
        for class_def in class_defs {
            globals.register_class(class_def).unwrap();
        }
        execute::execute(&mut globals, &main).unwrap();
        drop(globals);
        assert_eq!(
            String::from_utf8(output.lock().unwrap().clone()).unwrap(),
            "9895778000,1327440292,11223218292,210000|0"
        );

        let function = functions
            .iter()
            .find_map(|(name, function)| (name == function_name).then_some(function))
            .expect("corpus function should be compiled");
        let plan = function
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickLongOps(plan)
                    if plan
                        .ops
                        .iter()
                        .any(|operation| matches!(operation, QuickLongOp::VirtualObjectArrayPipeline { .. })) =>
                {
                    Some(plan)
                }
                _ => None,
            })
            .expect("compiler should select the virtual object-array pipeline");
        assert!(plan.native_jit().is_straight_compiled());
        assert_eq!(plan.native_jit().native_entries(), 1);
        assert!(plan.native_jit().native_chunks() > 1);
        assert_eq!(plan.native_jit().side_exits(), 0);
    }
}

#[test]
fn application_ledger_corpus_enters_property_native_region() {
    for (function_name, original) in [
        (
            "runLedgerPipeline",
            include_str!("../benches/corpus_ledger_pipeline.php"),
        ),
        (
            "runTypedLedgerPipeline",
            include_str!("../benches/corpus_typed_ledger_pipeline.php"),
        ),
    ] {
        let source = original
            .replace("$start = microtime(true);", "")
            .replace("$elapsed = microtime(true) - $start;", "$elapsed = 0;");
        let tokens = Lexer::new(&source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let compilation = Compiler::new().compile(&statements).unwrap();
        let main = make_user_function(compilation.main);
        let functions = compilation.functions;
        let class_defs = compilation.class_defs;
        let (mut globals, output) = common::make_eg_with_capture();
        for (name, function) in &functions {
            globals
                .register_function(name, &function.common as *const FunctionCommon)
                .unwrap();
        }
        for class_def in class_defs {
            globals.register_class(class_def).unwrap();
        }
        execute::execute(&mut globals, &main).unwrap();
        drop(globals);
        assert_eq!(
            String::from_utf8(output.lock().unwrap().clone()).unwrap(),
            "500000,7981250000,280500000,182500|0"
        );

        let function = functions
            .iter()
            .find_map(|(name, function)| (name == function_name).then_some(function))
            .expect("corpus function should be compiled");
        let plan = function
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickLongOps(plan)
                    if plan
                        .ops
                        .iter()
                        .any(|operation| matches!(operation, QuickLongOp::PropertyMethodCall { .. })) =>
                {
                    Some(plan)
                }
                _ => None,
            })
            .expect("compiler should select the stateful property pipeline");
        assert!(plan.native_jit().is_straight_compiled());
        assert_eq!(plan.native_jit().native_entries(), 1);
        assert!(plan.native_jit().native_chunks() > 1);
        assert_eq!(plan.native_jit().side_exits(), 0);
    }
}

#[test]
fn native_property_method_replays_overflow_transaction_exactly_once() {
    let source = "<?php class NativePropertyLedger { public $count = 0; public $total = 9223372036854775707; public function record($value) { $this->count = $this->count + 1; $this->total = $this->total + $value; } } $ledger = new NativePropertyLedger(); for ($i = 0; $i < 1000; $i++) { $ledger->record(1); } echo $ledger->count . ':' . $i;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "1000:1000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the property method loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn native_property_method_rebinds_cached_program_to_each_activation() {
    let source = "<?php class NativeReboundLedger { public $total = 0; public function record($value) { $this->total = $this->total + $value; } } function runNativeReboundLedger($iterations) { $ledger = new NativeReboundLedger(); for ($i = 0; $i < $iterations; $i++) { $ledger->record($i); } return $ledger->total; } echo runNativeReboundLedger(1000) . ':' . runNativeReboundLedger(2000);";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "499500:1999000"
    );

    let plan = functions
        .iter()
        .find_map(|(_, function)| {
            function.op_array.block_plans.iter().find_map(|plan| match plan {
                BlockPlan::QuickLongOps(plan) => Some(plan),
                _ => None,
            })
        })
        .expect("compiler should select the rebound property loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 2);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_scalar_function_enters_native_accumulate_region() {
    let source = "<?php function calculateNative(int $value): int { return ($value * 2) + 1; } $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += calculateNative($i); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:10000000000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a scalar-function accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn nested_scalar_functions_enter_one_native_accumulate_region() {
    let source = "<?php function addNative(int $left, int $right): int { return $left + $right; } function mulNative(int $left, int $right): int { return $left * $right; } $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += addNative($i + 1, mulNative($i, 2)); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:14999950000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a nested scalar-function accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn mixed_function_method_tree_enters_one_native_accumulate_region() {
    let source = "<?php class NativeMultiplier { public function mul(int $left, int $right): int { return $left * $right; } } function addNative(int $left, int $right): int { return $left + $right; } $math = new NativeMultiplier(); $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += addNative($i, $math->mul($i, 2)); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:14999850000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a mixed scalar-call accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn native_scalar_function_overflow_resumes_canonical_root_call() {
    let source = "<?php function overflowNative(int $value): int { return ($value * 100000000000000000) % 7; } function runFunctionOverflow(): int { $sum = 0; for ($i = 0; $i < 100; $i++) { $sum += overflowNative($i); } return $sum; } runFunctionOverflow();";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    let error = execute::execute(&mut globals, &main).unwrap_err();
    drop(globals);
    assert!(matches!(
        error,
        execute::VmError::Fatal(message)
            if message == "Unsupported operand types for %"
    ));
    assert!(output.lock().unwrap().is_empty());

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runFunctionOverflow"))
        .map(|(_, function)| function)
        .expect("compiled runFunctionOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runFunctionOverflow should use a scalar-function accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn real_php_nested_scalar_methods_enter_one_native_accumulate_region() {
    let source = "<?php class MathTree { public function add($left, $right) { return $left + $right; } public function mul($left, $right) { return $left * $right; } } $math = new MathTree(); $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $math->add($i, $math->mul($i, 2)); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:14999850000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a nested scalar-method accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn nested_scalar_methods_lower_checked_caller_argument_expressions() {
    let source = "<?php class ExpressionTree { public function add($left, $right) { return $left + $right; } public function mul($left, $right) { return $left * $right; } } $math = new ExpressionTree(); $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $math->add($i + 1, $math->mul($i, 2)); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:14999950000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a scalar argument-expression loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn conditional_scalar_method_enters_native_accumulate_region() {
    let source = "<?php class ConditionalKernel { public function route(int $value): int { if (($value & 1) == 0) { return ($value * 3) + 1; } return ($value * 5) - 2; } } $kernel = new ConditionalKernel(); $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $kernel->route($i); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:19999800000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a conditional scalar-method loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn nested_conditional_scalar_method_flattens_with_outer_method() {
    let source = "<?php class ConditionalTree { public function add(int $left, int $right): int { return $left + $right; } public function route(int $value): int { if (($value & 1) == 0) { return $value * 2; } return $value + 3; } } $tree = new ConditionalTree(); $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $tree->add($i, $tree->route($i)); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:12500000000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a nested conditional scalar loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn conditional_scalar_method_skips_overflow_in_inactive_arm() {
    let source = "<?php class InactiveOverflowKernel { public function choose(int $value): int { if ($value < 100) { return $value + 1; } return ($value * 100000000000000000) % 7; } } $kernel = new InactiveOverflowKernel(); $sum = 0; for ($i = 0; $i < 80; $i++) { $sum += $kernel->choose($i); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "80:3240"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the inactive-overflow scalar loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn conditional_scalar_method_selected_overflow_replays_canonical_call() {
    let source = "<?php class SelectedOverflowKernel { public function choose(int $value): int { if ($value < 90) { return $value + 1; } return ($value * 100000000000000000) % 7; } } function runSelectedOverflow(): int { $kernel = new SelectedOverflowKernel(); $sum = 0; for ($i = 0; $i < 100; $i++) { $sum += $kernel->choose($i); } return $sum; } runSelectedOverflow();";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    let error = execute::execute(&mut globals, &main).unwrap_err();
    drop(globals);
    assert!(matches!(
        error,
        execute::VmError::Fatal(message)
            if message == "Unsupported operand types for %"
    ));
    assert!(output.lock().unwrap().is_empty());

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runSelectedOverflow"))
        .map(|(_, function)| function)
        .expect("compiled runSelectedOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runSelectedOverflow should use a conditional scalar loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn conditional_scalar_method_rejects_polymorphic_target() {
    let source = "<?php class FirstConditional { public function route(int $value): int { if (($value & 1) == 0) { return $value + 1; } return $value + 2; } } class SecondConditional { public function route(int $value): int { if (($value & 1) == 0) { return $value + 3; } return $value + 4; } } function runConditional($kernel): int { $sum = 0; for ($i = 0; $i < 1000; $i++) { $sum += $kernel->route($i); } return $sum; } echo runConditional(new FirstConditional()) . ':' . runConditional(new SecondConditional());";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "501000:503000"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runConditional"))
        .map(|(_, function)| function)
        .expect("compiled runConditional function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runConditional should use a conditional scalar-method loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
}

#[test]
fn nested_scalar_method_guard_rejects_changed_inner_target() {
    let source = "<?php class OuterMath { public function add($left, $right) { return $left + $right; } } class DoubleMath { public function mul($left, $right) { return $left * $right; } } class TripleMath { public function mul($left, $right) { return $left * ($right + 1); } } function runTree($outer, $inner): int { $sum = 0; for ($i = 0; $i < 1000; $i++) { $sum += $outer->add($i, $inner->mul($i, 2)); } return $sum; } $outer = new OuterMath(); echo runTree($outer, new DoubleMath()) . ':' . runTree($outer, new TripleMath());";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "1498500:1998000"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runTree"))
        .map(|(_, function)| function)
        .expect("compiled runTree function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runTree should use a nested scalar-method accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
}

#[test]
fn nested_scalar_method_overflow_replays_the_root_call_tree() {
    let source = "<?php class OuterOverflow { public function add($left, $right) { return $left + $right; } } class InnerOverflow { public function transform($value) { return ($value * 100000000000000000) % 7; } } function runNestedOverflow(): int { $outer = new OuterOverflow(); $inner = new InnerOverflow(); $sum = 0; for ($i = 0; $i < 100; $i++) { $sum += $outer->add($i, $inner->transform($i)); } return $sum; } runNestedOverflow();";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    let error = execute::execute(&mut globals, &main).unwrap_err();
    drop(globals);
    assert!(matches!(
        error,
        execute::VmError::Fatal(message)
            if message == "Unsupported operand types for %"
    ));
    assert!(output.lock().unwrap().is_empty());

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runNestedOverflow"))
        .map(|(_, function)| function)
        .expect("compiled runNestedOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runNestedOverflow should use a nested scalar-method loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn native_scalar_method_guard_rejects_polymorphic_target() {
    let source = "<?php class FirstKernel { public function transform(int $value): int { return $value + 1; } } class SecondKernel { public function transform(int $value): int { return $value + 2; } } function runKernel($kernel): int { $sum = 0; for ($i = 0; $i < 1000; $i++) { $sum += $kernel->transform($i); } return $sum; } echo runKernel(new FirstKernel()) . ':' . runKernel(new SecondKernel());";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "500500:501500"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runKernel"))
        .map(|(_, function)| function)
        .expect("compiled runKernel function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runKernel should use a scalar-method accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
}

#[test]
fn native_scalar_method_overflow_resumes_canonical_call() {
    let source = "<?php class OverflowKernel { public function transform(int $value): int { return ($value * 100000000000000000) % 7; } } function runOverflow(): int { $kernel = new OverflowKernel(); $sum = 0; for ($i = 0; $i < 100; $i++) { $sum += $kernel->transform($i); } return $sum; } try { runOverflow(); } catch (TypeError $error) { echo 'caught'; }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    let error = execute::execute(&mut globals, &main).unwrap_err();
    drop(globals);
    assert!(matches!(
        error,
        execute::VmError::Fatal(message)
            if message == "Unsupported operand types for %"
    ));
    assert!(output.lock().unwrap().is_empty());

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runOverflow"))
        .map(|(_, function)| function)
        .expect("compiled runOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runOverflow should use a scalar-method accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn native_scalar_method_sum_overflow_resumes_canonical_add() {
    let source = "<?php class SumKernel { public function transform(int $value): int { return $value + 1; } } function runSumOverflow(): int { $kernel = new SumKernel(); $sum = PHP_INT_MAX - 100000; for ($i = 0; $i < 1000; $i++) { $sum += $kernel->transform($i); } return $sum; } try { runSumOverflow(); } catch (TypeError $error) { echo 'caught'; }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runSumOverflow"))
        .map(|(_, function)| function)
        .expect("compiled runSumOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runSumOverflow should use a scalar-method accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn real_php_constant_term_loop_enters_specialized_native_region() {
    let source = "<?php $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $i + 1; } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:5000050000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a constant-term accumulate loop");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn native_loop_sum_overflow_resumes_canonical_php_instruction() {
    let source = "<?php function overflow(): int { $sum = PHP_INT_MAX - 1000; for ($i = 0; $i < 60; $i++) { $sum += $i; } return $sum; } try { overflow(); } catch (TypeError $error) { echo 'caught'; }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "caught"
    );

    let overflow = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("overflow"))
        .map(|(_, function)| function)
        .expect("compiled overflow function");
    let plan = overflow
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("overflow function should have an accumulate plan");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 2);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn native_constant_term_overflow_resumes_canonical_term_instruction() {
    let source = "<?php function plusTwo(int $start, int $bound): int { $sum = 0; for ($i = $start; $i < $bound; $i++) { $sum += $i + 2; } return $sum; } plusTwo(0, 100); try { plusTwo(PHP_INT_MAX - 2, PHP_INT_MAX); } catch (TypeError $error) { echo 'caught'; }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "caught"
    );

    let plus_two = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("plusTwo"))
        .map(|(_, function)| function)
        .expect("compiled plusTwo function");
    let plan = plus_two
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("plusTwo should have a constant-term accumulate plan");
    assert!(plan.native_jit().is_compiled());
    assert!(plan.native_jit().native_entries() >= 2);
    assert_eq!(plan.native_jit().native_calls(), 2);
    assert!(plan.native_jit().range_proven_chunks() >= 1);
    assert!(
        plan.native_jit().range_proven_chunks()
            < plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().range_proof_evaluations(), 3);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn general_conditional_loop_ir_runs_as_a_native_chunked_region() {
    let config = NativeConditionalLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        condition: NativeConditionalLongLoopCondition::LessThan {
            rhs: QuickLongOperand::Slot(2),
        },
        accumulator_slot: 3,
    };
    let program = CompiledQuickLongConditionalAccumulateLoop::compile(config)
        .expect("conditional Long loop should lower");
    let mut slots = [0_i64; 64];
    slots[0] = 0;
    slots[1] = 100;
    slots[2] = 50;
    slots[3] = 0;

    assert_eq!(
        program.call(&mut slots, 32).unwrap().outcome,
        QuickLongAccumulateJitOutcome::ChunkExhausted
    );
    assert_eq!(slots[0], 32);
    assert_eq!(slots[3], 496);
    assert_eq!(
        program.call(&mut slots, 64).unwrap().outcome,
        QuickLongAccumulateJitOutcome::ChunkExhausted
    );
    assert_eq!(slots[0], 96);
    assert_eq!(slots[3], 1_225);
    assert_eq!(
        program.call(&mut slots, 32).unwrap().outcome,
        QuickLongAccumulateJitOutcome::Completed
    );
    assert_eq!(slots[0], 100);
    assert_eq!(slots[3], 1_225);
    assert_eq!(program.config(), config);
    assert!(!program.code().is_empty());

    slots[0] = 1;
    slots[1] = 2;
    slots[2] = 2;
    slots[3] = i64::MAX;
    assert_eq!(
        program.call(&mut slots, 32).unwrap().outcome,
        QuickLongAccumulateJitOutcome::SumOverflow
    );
    assert_eq!(slots[0], 1);
    assert_eq!(slots[3], i64::MAX);

    let aliased = NativeConditionalLongLoopConfig {
        accumulator_slot: 0,
        ..config
    };
    assert!(matches!(
        CompiledQuickLongConditionalAccumulateLoop::compile(aliased),
        Err(QuickLongAccumulateJitError::InvalidProgram(_))
    ));
}

#[test]
fn general_conditional_loop_ir_lowers_modulo_equality_and_precise_guards() {
    let modulo_even = NativeConditionalLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        condition: NativeConditionalLongLoopCondition::ModuloEqual {
            divisor: 2,
            rhs: QuickLongOperand::Const(0),
        },
        accumulator_slot: 2,
    };
    let program = CompiledQuickLongConditionalAccumulateLoop::compile(modulo_even)
        .expect("modulo equality loop should lower");
    assert!(
        !contains_signed_divide(program.code()),
        "power-of-two zero-remainder predicate must not contain SDIV"
    );
    let mut slots = [0_i64; 64];
    slots[1] = 10;
    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(result.outcome, QuickLongAccumulateJitOutcome::Completed);
    assert!(result.addition_executed);
    assert_eq!(slots[0], 10);
    assert_eq!(slots[2], 20);

    let never_matches = NativeConditionalLongLoopConfig {
        condition: NativeConditionalLongLoopCondition::ModuloEqual {
            divisor: 2,
            rhs: QuickLongOperand::Const(2),
        },
        ..modulo_even
    };
    let program = CompiledQuickLongConditionalAccumulateLoop::compile(never_matches)
        .expect("never-matching modulo loop should lower");
    let mut slots = [0_i64; 64];
    slots[1] = 10;
    slots[2] = 7;
    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(result.outcome, QuickLongAccumulateJitOutcome::Completed);
    assert!(!result.addition_executed);
    assert_eq!(slots[0], 10);
    assert_eq!(slots[2], 7);

    let negative_remainder = NativeConditionalLongLoopConfig {
        condition: NativeConditionalLongLoopCondition::ModuloEqual {
            divisor: 2,
            rhs: QuickLongOperand::Const(-1),
        },
        ..modulo_even
    };
    let program = CompiledQuickLongConditionalAccumulateLoop::compile(negative_remainder)
        .expect("negative signed remainder should lower without division");
    assert!(!contains_signed_divide(program.code()));
    let mut slots = [0_i64; 64];
    slots[0] = -5;
    slots[1] = 0;
    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(result.outcome, QuickLongAccumulateJitOutcome::Completed);
    assert_eq!(slots[0], 0);
    assert_eq!(slots[2], -9);

    let zero_divisor = NativeConditionalLongLoopConfig {
        condition: NativeConditionalLongLoopCondition::ModuloEqual {
            divisor: 0,
            rhs: QuickLongOperand::Const(0),
        },
        ..modulo_even
    };
    let program = CompiledQuickLongConditionalAccumulateLoop::compile(zero_divisor)
        .expect("zero divisor should compile to a guarded side exit");
    let mut slots = [0_i64; 64];
    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(result.outcome, QuickLongAccumulateJitOutcome::Completed);
    assert!(!result.addition_executed);

    slots[1] = 1;
    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(
        result.outcome,
        QuickLongAccumulateJitOutcome::ConditionSideExit
    );
    assert!(!result.addition_executed);
    assert_eq!(slots[0], 0);
    assert_eq!(slots[2], 0);

    let min_over_minus_one = NativeConditionalLongLoopConfig {
        condition: NativeConditionalLongLoopCondition::ModuloEqual {
            divisor: -1,
            rhs: QuickLongOperand::Const(0),
        },
        ..modulo_even
    };
    let program = CompiledQuickLongConditionalAccumulateLoop::compile(min_over_minus_one)
        .expect("MIN modulo -1 should compile to a guarded side exit");
    let mut slots = [0_i64; 64];
    slots[1] = 100;
    assert_eq!(
        program.call(&mut slots, 32).unwrap().outcome,
        QuickLongAccumulateJitOutcome::ChunkExhausted
    );
    slots[0] = i64::MIN;
    slots[1] = i64::MIN + 1;
    slots[2] = 0;
    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(result.outcome, QuickLongAccumulateJitOutcome::Completed);
    assert!(result.addition_executed);
    assert_eq!(slots[0], i64::MIN + 1);
    assert_eq!(slots[2], i64::MIN);
}

#[test]
fn straight_long_loop_lowers_linear_modulo_and_binary_assign_body() {
    let mut operations =
        [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Modulo {
        value: QuickLongOperand::Slot(0),
        divisor: 400,
        result: 2,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Const(20),
        rhs: QuickLongOperand::Slot(2),
        result: 3,
        destination: 4,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(73),
        result: 5,
        destination: 6,
    };
    operations[3] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(0),
        result: 7,
        destination: 8,
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 4,
        post_result: Some(9),
    };
    let program = CompiledQuickLongStraightLoop::compile(config)
        .expect("straight Long loop should lower");
    let mut slots = [0_i64; 64];
    slots[1] = 100;

    assert_eq!(
        program.call(&mut slots, 32).unwrap().outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!(slots[0], 32);
    assert_eq!(slots[2], 31);
    assert_eq!(slots[4], 51);
    assert_eq!(slots[6], 2_263);
    assert_eq!(slots[8], 69);
    assert_eq!(slots[9], 31);

    assert_eq!(
        program.call(&mut slots, 128).unwrap().outcome,
        NativeStraightLongLoopOutcome::Completed
    );
    assert_eq!(slots[0], 100);
    assert_eq!(slots[2], 99);
    assert_eq!(slots[4], 119);
    assert_eq!(slots[6], 7_227);
    assert_eq!(slots[8], 1);
    assert_eq!(slots[9], 99);
    assert_eq!(program.config(), config);
    assert!(!program.code().is_empty());
}

#[test]
fn straight_long_loop_lowers_non_materialized_binary_chain() {
    let mut operations =
        [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(73),
        result: 2,
    };
    operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(2),
        rhs: QuickLongOperand::Const(20),
        result: 3,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(3),
        rhs: QuickLongOperand::Const(7),
        result: 4,
        destination: 5,
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 3,
        post_result: None,
    };
    let program = CompiledQuickLongStraightLoop::compile(config)
        .expect("non-materialized binary chain should lower");
    let mut slots = [0_i64; 64];
    slots[1] = 10;

    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(slots[0], 10);
    assert_eq!(slots[2], 657);
    assert_eq!(slots[3], 677);
    assert_eq!(slots[4], 670);
    assert_eq!(slots[5], 670);
    assert_eq!(config.output_mask_before(0), 0);
    assert_eq!(config.output_mask_before(1), 1u64 << 2);
    assert_eq!(
        config.output_mask_before(2),
        (1u64 << 2) | (1u64 << 3)
    );

    let mut overflow_operations = operations;
    overflow_operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(1),
        result: 2,
    };
    overflow_operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(6),
        rhs: QuickLongOperand::Const(1),
        result: 3,
    };
    let overflow_program = CompiledQuickLongStraightLoop::compile(
        NativeStraightLongLoopConfig {
            operations: overflow_operations,
            ..config
        },
    )
    .expect("checked intermediate binary operation should lower");
    slots = [0_i64; 64];
    slots[1] = 1;
    slots[6] = i64::MAX;
    let result = overflow_program.call(&mut slots, 32).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(result.failed_operation, Some(1));
    assert_eq!(slots[0], 0);
    assert_eq!(slots[2], 1);
    assert_eq!(slots[3], 0);
    assert_eq!(slots[5], 0);
}

#[test]
fn straight_long_loop_lowers_division_modulo_xor_and_move() {
    let mut operations =
        [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::IntDivide,
        lhs: QuickLongOperand::Const(17),
        rhs: QuickLongOperand::Const(5),
        result: 2,
    };
    operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Modulo,
        lhs: QuickLongOperand::Const(17),
        rhs: QuickLongOperand::Const(5),
        result: 3,
    };
    operations[2] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::BitwiseXor,
        lhs: QuickLongOperand::Slot(2),
        rhs: QuickLongOperand::Slot(3),
        result: 4,
    };
    operations[3] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Slot(4),
        result: 5,
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 4,
        post_result: None,
    };
    let program = CompiledQuickLongStraightLoop::compile(config)
        .expect("division, modulo, xor, and move should lower");
    let mut slots = [0_i64; 64];
    slots[1] = 1;

    assert_eq!(
        program.call(&mut slots, 32).unwrap().outcome,
        NativeStraightLongLoopOutcome::Completed
    );
    assert_eq!(slots[2], 3);
    assert_eq!(slots[3], 2);
    assert_eq!(slots[4], 1);
    assert_eq!(slots[5], 1);

    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::IntDivide,
        lhs: QuickLongOperand::Const(17),
        rhs: QuickLongOperand::Const(0),
        result: 2,
    };
    let guarded = CompiledQuickLongStraightLoop::compile(
        NativeStraightLongLoopConfig {
            operations,
            ..config
        },
    )
    .expect("division by zero should lower to a precise side exit");
    slots = [0_i64; 64];
    slots[1] = 1;
    let result = guarded.call(&mut slots, 32).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(result.failed_operation, Some(0));
    assert_eq!(slots[0], 0);
    assert_eq!(slots[2], 0);
}

#[test]
fn straight_binary_constant_power_of_two_modulo_avoids_signed_divide() {
    let mut operations =
        [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Modulo,
        lhs: QuickLongOperand::Const(-17),
        rhs: QuickLongOperand::Const(8),
        result: 2,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Modulo,
        lhs: QuickLongOperand::Const(i64::MIN),
        rhs: QuickLongOperand::Const(-1),
        result: 3,
        destination: 4,
    };
    let program = CompiledQuickLongStraightLoop::compile(NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 2,
        post_result: None,
    })
    .expect("constant power-of-two operations should lower");
    assert!(!contains_signed_divide(program.code()));

    let mut slots = [0_i64; 64];
    slots[1] = 1;
    assert_eq!(
        program.call(&mut slots, 32).unwrap().outcome,
        NativeStraightLongLoopOutcome::Completed
    );
    assert_eq!(slots[2], -1);
    assert_eq!(slots[3], 0);
    assert_eq!(slots[4], 0);
}

#[test]
fn straight_long_loop_executes_structured_scalar_conditions() {
    let cases = [
        (ScalarLongConditionKind::Equal, 1, 77),
        (ScalarLongConditionKind::NotEqual, 1, 55),
        (ScalarLongConditionKind::LessThan, 2, 66),
        (ScalarLongConditionKind::LessThanOrEqual, 2, 55),
    ];
    for (kind, rhs, expected) in cases {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(rhs)),
            false_target: 3,
        };
        operations[1] = NativeStraightLongOperation::Move {
            source: QuickLongOperand::Const(11),
            result: 2,
        };
        operations[2] = NativeStraightLongOperation::Jump { target: 4 };
        operations[3] = NativeStraightLongOperation::Move {
            source: QuickLongOperand::Const(22),
            result: 2,
        };
        operations[4] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(3),
            rhs: QuickLongOperand::Slot(2),
            result: 4,
            destination: 3,
        };
        let program = CompiledQuickLongStraightLoop::compile(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Slot(1),
                operations,
                operation_count: 5,
                post_result: None,
            },
        )
        .expect("structured scalar condition should lower");
        let mut slots = [0_i64; 64];
        slots[1] = 4;
        assert_eq!(
            program.call(&mut slots, 32).unwrap().outcome,
            NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!(slots[3], expected, "condition {kind:?}");
    }

    let mut operations =
        [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::Equal,
        lhs: NativeStraightLongConditionOperand::BitwiseAnd {
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(1),
        },
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(0)),
        false_target: 3,
    };
    operations[1] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Const(11),
        result: 2,
    };
    operations[2] = NativeStraightLongOperation::Jump { target: 4 };
    operations[3] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Const(22),
        result: 2,
    };
    operations[4] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(3),
        rhs: QuickLongOperand::Slot(2),
        result: 4,
        destination: 3,
    };
    let program = CompiledQuickLongStraightLoop::compile(NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 5,
        post_result: None,
    })
    .expect("masked scalar condition should lower");
    let mut slots = [0_i64; 64];
    slots[1] = 4;
    assert_eq!(
        program.call(&mut slots, 32).unwrap().outcome,
        NativeStraightLongLoopOutcome::Completed
    );
    assert_eq!(slots[3], 66);

    operations =
        [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Jump { target: 0 };
    assert!(matches!(
        CompiledQuickLongStraightLoop::compile(NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Slot(1),
            operations,
            operation_count: 1,
            post_result: None,
        }),
        Err(QuickLongAccumulateJitError::InvalidProgram(_))
    ));
}

#[test]
fn straight_long_guard_side_exits_after_prior_outputs_and_before_increment() {
    let mut operations =
        [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(3),
        rhs: QuickLongOperand::Slot(0),
        result: 2,
        destination: 3,
    };
    operations[1] = NativeStraightLongOperation::Guard {
        kind: ScalarLongConditionKind::Equal,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        expected: false,
    };
    let program = CompiledQuickLongStraightLoop::compile(NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 2,
        post_result: Some(4),
    })
    .expect("trace guard should lower through the general straight IR");
    let mut slots = [0_i64; 64];
    slots[1] = 10;

    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(result.failed_operation, Some(1));
    assert_eq!(slots[0], 2, "guard failure must precede increment");
    assert_eq!(slots[2], 3, "prior result remains in shadow state");
    assert_eq!(slots[3], 3, "prior assignment remains in shadow state");
    assert_eq!(slots[4], 1, "last completed post-increment remains published");
}

#[test]
fn finite_string_hash_operations_use_runtime_context_without_embedded_pointers() {
    let mut operations =
        [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::StringToken {
        token: 1,
        result: 2,
    };
    operations[1] = NativeStraightLongOperation::StringLength {
        source: 2,
        lengths: [4, 5, 0, 0],
        token_count: 2,
        result: 3,
    };
    operations[2] = NativeStraightLongOperation::HashLoad {
        key: 2,
        entry_base: 0,
        token_count: 2,
        result: 4,
        destination: None,
    };
    operations[3] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Slot(3),
        result: 5,
    };
    operations[4] = NativeStraightLongOperation::HashStore {
        key: 2,
        entry_base: 0,
        token_count: 2,
        source: QuickLongOperand::Slot(5),
    };
    let program = CompiledQuickLongStraightLoop::compile(NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 5,
        post_result: None,
    })
    .expect("finite String and contextual hash operations should lower");

    let mut left = 7i64;
    let mut right = 10i64;
    let mut entries = [std::ptr::null_mut(); NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    entries[0] = &mut left;
    entries[1] = &mut right;
    let mut slots = [0i64; 64];
    slots[1] = 1;
    let outcome = program.call_with_context(&mut slots, 8, &entries).unwrap();

    assert_eq!(outcome.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(slots[0], 1);
    assert_eq!(slots[2], 1);
    assert_eq!(slots[3], 5);
    assert_eq!(slots[4], 10);
    assert_eq!(slots[5], 15);
    assert_eq!(left, 7);
    assert_eq!(right, 15);

    let missing_entries =
        [std::ptr::null_mut(); NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    assert!(matches!(
        program.call_with_context(&mut slots, 8, &missing_entries),
        Err(QuickLongAccumulateJitError::InvalidProgram(_))
    ));
}

#[test]
fn straight_long_loop_reports_exact_failed_operation_transactionally() {
    let mut operations =
        [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(1),
        result: 2,
        destination: 3,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Const(1),
        result: 5,
        destination: 4,
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 2,
        post_result: None,
    };
    let program = CompiledQuickLongStraightLoop::compile(config)
        .expect("checked straight Long loop should lower");
    let mut slots = [0_i64; 64];
    slots[1] = 10;
    slots[4] = i64::MAX;

    let outcome = program.call(&mut slots, 32).unwrap();
    assert_eq!(
        outcome.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(outcome.failed_operation, Some(1));
    assert_eq!(slots[0], 0);
    assert_eq!(slots[2], 1);
    assert_eq!(slots[3], 1);
    assert_eq!(slots[4], i64::MAX);
    assert_eq!(slots[5], 0);

    let invalid_bound_alias = NativeStraightLongLoopConfig {
        bound: QuickLongOperand::Slot(3),
        ..config
    };
    assert!(matches!(
        CompiledQuickLongStraightLoop::compile(invalid_bound_alias),
        Err(QuickLongAccumulateJitError::InvalidProgram(_))
    ));

    let mut guarded_operations =
        [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    guarded_operations[0] = NativeStraightLongOperation::Modulo {
        value: QuickLongOperand::Slot(0),
        divisor: 0,
        result: 2,
    };
    let guarded_config = NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations: guarded_operations,
        operation_count: 1,
        post_result: None,
    };
    let program = CompiledQuickLongStraightLoop::compile(guarded_config)
        .expect("zero divisor should lower to an operation side exit");
    let mut slots = [0_i64; 64];
    assert_eq!(
        program.call(&mut slots, 32).unwrap().outcome,
        NativeStraightLongLoopOutcome::Completed
    );
    slots[1] = 1;
    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(result.failed_operation, Some(0));
    assert_eq!(slots[0], 0);
    assert_eq!(slots[2], 0);

    guarded_operations[0] = NativeStraightLongOperation::Modulo {
        value: QuickLongOperand::Slot(0),
        divisor: -1,
        result: 2,
    };
    let min_modulo_config = NativeStraightLongLoopConfig {
        operations: guarded_operations,
        ..guarded_config
    };
    let program = CompiledQuickLongStraightLoop::compile(min_modulo_config)
        .expect("MIN modulo -1 should lower without division");
    assert!(!contains_signed_divide(program.code()));
    let mut slots = [0_i64; 64];
    slots[0] = i64::MIN;
    slots[1] = i64::MIN + 1;
    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(result.failed_operation, None);
    assert_eq!(slots[0], i64::MIN + 1);
    assert_eq!(slots[2], 0);

    guarded_operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(2),
        rhs: QuickLongOperand::Const(2),
        result: 3,
        destination: 2,
    };
    let multiply_overflow_config = NativeStraightLongLoopConfig {
        operations: guarded_operations,
        ..guarded_config
    };
    let program = CompiledQuickLongStraightLoop::compile(multiply_overflow_config)
        .expect("checked multiply should lower");
    let mut slots = [0_i64; 64];
    slots[1] = 1;
    slots[2] = i64::MAX;
    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(result.failed_operation, Some(0));
    assert_eq!(slots[2], i64::MAX);
    assert_eq!(slots[3], 0);

    guarded_operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(2),
        rhs: QuickLongOperand::Const(1),
        result: 3,
        destination: 2,
    };
    let subtract_overflow_config = NativeStraightLongLoopConfig {
        operations: guarded_operations,
        ..guarded_config
    };
    let program = CompiledQuickLongStraightLoop::compile(subtract_overflow_config)
        .expect("checked subtraction should lower");
    let mut slots = [0_i64; 64];
    slots[1] = 1;
    slots[2] = i64::MIN;
    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(result.failed_operation, Some(0));
    assert_eq!(slots[2], i64::MIN);
    assert_eq!(slots[3], 0);
}

#[test]
fn real_php_branch_loop_enters_general_native_ir_region() {
    let source = "<?php $sum = 0; $bound = 100000; $cutoff = 50000; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $sum += $i; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:1249975000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the general Long loop IR");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_modulo_branch_loop_enters_general_native_ir_region() {
    let source = "<?php $sum = 0; $bound = 100000; $expected = 0; for ($i = 0; $i < $bound; $i++) { if (($i % 3) == $expected) { $sum += $i; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:1666683333"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the modulo Long loop IR");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_modulo_min_over_minus_one_preserves_canonical_semantics() {
    let source = "<?php function moduloLoop(int $start, int $bound): int { $sum = 0; for ($i = $start; $i < $bound; $i++) { if (($i % -1) == 0) { $sum += $i; } } return $sum; } moduloLoop(0, 100); echo moduloLoop(PHP_INT_MIN, PHP_INT_MIN + 1);";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        i64::MIN.to_string()
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("moduloLoop"))
        .map(|(_, function)| function)
        .expect("compiled moduloLoop function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("moduloLoop should use general Long loop IR");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 2);
    // The VM executes the first iteration canonically before entering the hot
    // backedge region, so MIN % -1 is already resolved when native code sees
    // MIN + 1. The direct ABI test above covers the native guard itself.
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_straight_binary_body_enters_general_native_ir_region() {
    let source = "<?php $bound = 100000; $last = 0; $product = 0; $remaining = 0; for ($i = 0; $i < $bound; $i++) { $last = 20 + ($i % 400); $product = $i * 73; $remaining = $bound - $i; } echo $i . ':' . $last . ':' . $product . ':' . $remaining;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:419:7299927:1"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the straight Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn general_native_trace_guard_resumes_taken_cold_edge_transactionally() {
    let source = "<?php $needle = 74; $sum = 0; $count = 0; for ($i = 0; $i < 100; $i++) { $sum = $sum + $i; $count = $count + 1; if ($count === $needle) { echo 'hit:' . $i . ':' . $count . '|'; } } echo $sum . ':' . $count . ':' . $i;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "hit:73:74|4950:100:100"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("strict cold edge should retain the general Long loop IR");
    assert!(plan
        .ops
        .iter()
        .any(|operation| matches!(operation, QuickLongOp::TraceGuard { .. })));
    assert!(plan.native_jit().is_straight_compiled());
    assert!(plan.native_jit().native_entries() >= 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn real_php_scalar_expression_chains_enter_general_native_ir_region() {
    let source = "<?php $bound = 100000; $left = 2; $right = 3; $literal = 0; $cv = 0; for ($i = 0; $i < $bound; $i++) { $literal = (($i * 73) + 20) - 7; $cv = $i + $left + $right; } echo $i . ':' . $literal . ':' . $cv;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:7299940:100004"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the scalar-expression Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_overlapping_scalar_lifetimes_enter_range_proven_native_region() {
    let source = "<?php $bound = 100000; $a = 0; $b = 0; $c = 0; $d = 0; for ($i = 0; $i < $bound; $i++) { $a = $i * 3; $b = $a + 7; $c = $a + $b; $d = $a + $b + $c; } echo $i . ':' . $a . ':' . $b . ':' . $c . ':' . $d;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:299997:300004:600001:1200002"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select overlapping scalar lifetimes");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_independent_recurrences_stay_in_range_proven_native_region() {
    let source = "<?php $bound = 100000; $sum = 10; $count = -5; $step = 2; for ($i = 0; $i < $bound; $i++) { $sum = $sum + $i; $count = $count + $step; } echo $i . ':' . $sum . ':' . $count;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:4999950010:199995"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the independent recurrence Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_carried_condition_recurrences_share_one_native_region() {
    let source = "<?php $bound = 100000; $cutoff = 49995; $sum = 10; $count = -5; for ($i = 0; $i < $bound; $i++) { if ($count < $cutoff) { $sum = $sum + $i; } $count = $count + 1; } echo $i . ':' . $sum . ':' . $count;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:1249975010:99995"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the structured recurrence Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_conditional_composed_recurrence_delta_is_range_proven() {
    let source = "<?php $bound = 100000; $cutoff = 49995; $offset = 7; $sum = 10; $count = -5; for ($i = 0; $i < $bound; $i++) { if ($count < $cutoff) { $sum = $sum + (($i * 3) + $offset); } $count = $count + 1; } echo $i . ':' . $sum . ':' . $count;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:3750275010:99995"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the conditional composed recurrence IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_forward_dependent_recurrences_stay_in_one_native_region() {
    let source = "<?php $bound = 100000; $a = 3; $b = -7; for ($i = 0; $i < $bound; $i++) { $a = $a + 1; $b = $b + $a; } echo $i . ':' . $a . ':' . $b;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:100003:5000349993"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the dependent recurrence Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_reverse_order_dependency_preserves_old_value_semantics() {
    let source = "<?php $bound = 100000; $a = 3; $b = -7; for ($i = 0; $i < $bound; $i++) { $b = $b + $a; $a = $a + 1; } echo $i . ':' . $a . ':' . $b;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:100003:5000249993"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the reverse dependency Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_composed_recurrence_delta_stays_in_range_proven_native_region() {
    let source = "<?php $bound = 100000; $sum = 10; $offset = 7; for ($i = 0; $i < $bound; $i++) { $sum = $sum + (($i * 3) + $offset); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:15000550010"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the composed recurrence Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_composed_recurrence_overflow_uses_precise_checked_side_exit() {
    let source = "<?php function composedDeltaOverflow(): int { $sum = 0; $factor = 92233720368547758; for ($i = 0; $i < 200; $i++) { $sum = $sum + (($i * $factor) - ($i * $factor)); } return $sum; } try { composedDeltaOverflow(); } catch (TypeError $error) { echo 'caught'; }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("composedDeltaOverflow"))
        .map(|(_, function)| function)
        .expect("compiled composedDeltaOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("overflowing composed recurrence should retain the Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn real_php_forward_scalar_branches_use_range_proven_native_region() {
    let source = "<?php $bound = 100000; $cutoff = 50000; $selected = 0; $folded = 0; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $selected = ($i * 3) + 1; } else { $selected = ($i * 5) - 2; } $folded = ($selected * 3) + 11; } echo $i . ':' . $selected . ':' . $folded;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:499993:1499990"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the forward-branch Long loop IR");
    assert!(
        plan.native_jit().is_straight_compiled(),
        "forward branch did not select straight native IR: {:#?}",
        plan.ops
    );
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_reversed_commutative_constants_use_range_proven_native_region() {
    let source = "<?php $bound = 100000; $cutoff = 50000; $selected = 0; $folded = 0; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $selected = 1 + (3 * $i); } else { $selected = (5 * $i) - 2; } $folded = 11 + (3 * $selected); } echo $i . ':' . $selected . ':' . $folded;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:499993:1499990"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the reversed-commutative Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_runtime_invariant_arguments_share_native_registers() {
    let source = "<?php function runTwoInvariantLoop(int $bound, int $cutoff, int $offset): int { $selected = 0; $folded = 0; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $selected = ($i * 3) + $offset; } else { $selected = ($i * 5) - $offset; } $folded = ($selected * 3) + $offset; } return $selected + $folded; } echo runTwoInvariantLoop(100000, 50000, 7);";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "1999959"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runTwoInvariantLoop"))
        .map(|(_, function)| function)
        .expect("compiled two-invariant function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the two-invariant forward-branch IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_partially_written_branch_keeps_checked_native_chunks() {
    let source = "<?php $bound = 100000; $cutoff = 50000; $selected = 7; $folded = 0; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $selected = $i * 3; } $folded = $selected + 1; } echo $i . ':' . $selected . ':' . $folded;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:149997:149998"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should retain the partially-written branch loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_amortized_safepoint_chunks(plan.native_jit().native_calls());
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_straight_binary_overflow_resumes_exact_canonical_operation() {
    let source = "<?php function binaryOverflow(): int { $value = PHP_INT_MAX - 40; $prefix = 0; for ($i = 0; $i < 100; $i++) { $prefix = $i + 1; $value = $value + 1; } return $prefix + $value; } try { binaryOverflow(); } catch (TypeError $error) { echo 'caught'; }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("binaryOverflow"))
        .map(|(_, function)| function)
        .expect("compiled binaryOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("binaryOverflow should use general Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn general_native_ir_handles_never_taken_add_and_exact_chunk_completion() {
    let source = "<?php $sum = 7; $bound = 65; $cutoff = 0; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $sum += $i; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "65:7"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the general Long loop IR");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn general_native_ir_sum_overflow_resumes_canonical_add() {
    let source = "<?php function conditionalOverflow(int $bound, int $cutoff): int { $sum = PHP_INT_MAX - 1000; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $sum += $i; } } return $sum; } try { conditionalOverflow(60, 60); } catch (TypeError $error) { echo 'caught'; }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("conditionalOverflow"))
        .map(|(_, function)| function)
        .expect("compiled conditionalOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("conditionalOverflow should use general Long loop IR");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn structured_recurrence_overflow_uses_checked_fallback() {
    let source = "<?php function structuredOverflow(int $bound, int $cutoff): int { $sum = PHP_INT_MAX - 3000; $count = 0; for ($i = 0; $i < $bound; $i++) { if ($count < $cutoff) { $sum = $sum + (($i * 3) + 7); } $count = $count + 1; } return $sum; } try { structuredOverflow(60, 60); } catch (TypeError $error) { echo 'caught'; }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("structuredOverflow"))
        .map(|(_, function)| function)
        .expect("compiled structuredOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("structuredOverflow should use general Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn general_native_modulo_ir_sum_overflow_resumes_canonical_add() {
    let source = "<?php function moduloOverflow(int $bound): int { $sum = PHP_INT_MAX - 2000; for ($i = 0; $i < $bound; $i++) { if (($i % 2) == 0) { $sum += $i; } } return $sum; } try { moduloOverflow(100); } catch (TypeError $error) { echo 'caught'; }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("moduloOverflow"))
        .map(|(_, function)| function)
        .expect("compiled moduloOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("moduloOverflow should use general Long loop IR");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_eq!(plan.native_jit().side_exits(), 1);
}
