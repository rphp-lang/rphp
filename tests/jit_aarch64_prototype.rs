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
    NativeStraightLongLoopConfig, NativeStraightLongLoopOutcome,
    NativeStraightLongOperation, NATIVE_STRAIGHT_LONG_MAX_OPERATIONS,
    QuickLongAccumulateJitError, QuickLongAccumulateJitOutcome,
    SCALAR_LONG_JIT_HOT_THRESHOLD, ScalarLongJitDispatch, ScalarLongJitError,
    ScalarLongJitOutcome,
};
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::execute;
use rphp::vm::function::{
    FunctionCommon, ScalarLongFunctionPlan, ScalarLongOp, ScalarLongOpKind, ScalarLongProgram,
    ScalarLongSource,
};
use rphp::vm::planner::BlockPlan;
use rphp::vm::quick::QuickLongOperand;

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
fn encoder_produces_expected_arm64_instruction_words() {
    let mut assembler = Arm64Assembler::new();
    assembler.add_register(Arm64Register::X0, Arm64Register::X0, Arm64Register::X1);
    assembler.multiply_register(Arm64Register::X0, Arm64Register::X0, Arm64Register::X2);
    assembler.ret();

    assert_eq!(
        assembler.finish(),
        [
            0x00, 0x00, 0x01, 0x8b, // add x0, x0, x1
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
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
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
    assert!(plan.native_jit().native_chunks() > 1);
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
    assert_eq!(
        result.outcome,
        QuickLongAccumulateJitOutcome::ConditionSideExit
    );
    assert!(!result.addition_executed);
    assert_eq!(slots[0], i64::MIN);
    assert_eq!(slots[2], 0);
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
        .expect("MIN modulo -1 should lower to an operation side exit");
    let mut slots = [0_i64; 64];
    slots[0] = i64::MIN;
    slots[1] = i64::MIN + 1;
    let result = program.call(&mut slots, 32).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(result.failed_operation, Some(0));
    assert_eq!(slots[0], i64::MIN);

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
    assert!(plan.native_jit().native_chunks() > 1);
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
    assert!(plan.native_jit().native_chunks() > 1);
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
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
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
    assert!(plan.native_jit().native_chunks() > 1);
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
    assert_eq!(plan.native_jit().side_exits(), 1);
}
