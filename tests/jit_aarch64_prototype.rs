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
    CompiledScalarLongProgram, NativeLongAccumulateState, QuickLongAccumulateJitError,
    QuickLongAccumulateJitOutcome, SCALAR_LONG_JIT_HOT_THRESHOLD, ScalarLongJitDispatch,
    ScalarLongJitError, ScalarLongJitOutcome,
};
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::execute;
use rphp::vm::function::{
    FunctionCommon, ScalarLongFunctionPlan, ScalarLongOp, ScalarLongOpKind, ScalarLongProgram,
    ScalarLongSource,
};
use rphp::vm::planner::BlockPlan;

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
