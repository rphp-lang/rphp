use super::double::X86ScalarDoubleRegisterMap;
use super::{
    CompiledQuickDoubleCallAccumulateLoop, CompiledScalarDoubleProgram,
    NativeDoubleCallAccumulateState, QuickDoubleCallAccumulateJitOutcome,
    SCALAR_DOUBLE_JIT_HOT_THRESHOLD, ScalarDoubleJitDispatch, ScalarDoubleJitOutcome,
    X86_64Assembler, X86_64FloatRegister, X86DoubleInstructionSet,
};
use crate::vm::function::{
    ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleProgram,
    ScalarDoubleSelect, ScalarDoubleSource, ScalarLongConditionKind,
};
use crate::vm::quick::{QuickDoubleArgumentOp, QuickDoubleArgumentProgram, QuickDoubleSource};

fn arithmetic_plan() -> ScalarDoubleFunctionPlan {
    ScalarDoubleFunctionPlan::new(
        3,
        ScalarDoubleProgram {
            operations: vec![
                ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Input(0),
                    rhs: ScalarDoubleSource::Constant(1.5),
                },
                ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Multiply,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Input(1),
                },
                ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Divide,
                    lhs: ScalarDoubleSource::Temporary(1),
                    rhs: ScalarDoubleSource::Input(2),
                },
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(2),
        },
    )
}

fn conditional_plan(kind: ScalarLongConditionKind) -> ScalarDoubleFunctionPlan {
    ScalarDoubleFunctionPlan::new_conditional(
        2,
        ScalarDoubleProgram {
            operations: Vec::new().into_boxed_slice(),
            output: ScalarDoubleSource::Constant(-1.0),
        },
        ScalarDoubleSelect {
            kind,
            lhs: ScalarDoubleSource::Input(0),
            rhs: ScalarDoubleSource::Input(1),
            shared_operation_count: 0,
            when_true_operation_count: 0,
            when_true: ScalarDoubleSource::Constant(1.0),
            when_false: ScalarDoubleSource::Constant(-1.0),
        },
    )
}

fn conditional_arithmetic_plan() -> ScalarDoubleFunctionPlan {
    ScalarDoubleFunctionPlan::new_conditional(
        2,
        ScalarDoubleProgram {
            operations: vec![
                ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Multiply,
                    lhs: ScalarDoubleSource::Input(0),
                    rhs: ScalarDoubleSource::Constant(1.5),
                },
                ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Constant(2.0),
                },
                ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Multiply,
                    lhs: ScalarDoubleSource::Input(0),
                    rhs: ScalarDoubleSource::Constant(0.5),
                },
                ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Subtract,
                    lhs: ScalarDoubleSource::Temporary(2),
                    rhs: ScalarDoubleSource::Constant(1.0),
                },
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(3),
        },
        ScalarDoubleSelect {
            kind: ScalarLongConditionKind::LessThan,
            lhs: ScalarDoubleSource::Input(0),
            rhs: ScalarDoubleSource::Input(1),
            shared_operation_count: 0,
            when_true_operation_count: 2,
            when_true: ScalarDoubleSource::Temporary(1),
            when_false: ScalarDoubleSource::Temporary(3),
        },
    )
}

fn conditional_argument_plan() -> QuickDoubleArgumentProgram {
    QuickDoubleArgumentProgram {
        operations: vec![QuickDoubleArgumentOp {
            kind: ScalarDoubleOpKind::Multiply,
            lhs: QuickDoubleSource::Induction,
            rhs: QuickDoubleSource::Constant(0.5),
        }]
        .into_boxed_slice(),
        outputs: [
            QuickDoubleSource::Temporary(0),
            QuickDoubleSource::Constant(2.5),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
        ],
        output_count: 2,
        input_slots: [u16::MAX; 8],
        input_count: 0,
    }
}

fn selective_division_plan() -> ScalarDoubleFunctionPlan {
    ScalarDoubleFunctionPlan::new_conditional(
        2,
        ScalarDoubleProgram {
            operations: vec![ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Divide,
                lhs: ScalarDoubleSource::Constant(8.0),
                rhs: ScalarDoubleSource::Input(1),
            }]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Constant(3.0),
        },
        ScalarDoubleSelect {
            kind: ScalarLongConditionKind::LessThan,
            lhs: ScalarDoubleSource::Input(0),
            rhs: ScalarDoubleSource::Constant(0.0),
            shared_operation_count: 0,
            when_true_operation_count: 1,
            when_true: ScalarDoubleSource::Temporary(0),
            when_false: ScalarDoubleSource::Constant(3.0),
        },
    )
}

fn two_input_argument_plan() -> QuickDoubleArgumentProgram {
    QuickDoubleArgumentProgram {
        operations: Vec::new().into_boxed_slice(),
        outputs: [
            QuickDoubleSource::Input(0),
            QuickDoubleSource::Input(1),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
        ],
        output_count: 2,
        input_slots: [0, 1, u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX],
        input_count: 2,
    }
}

fn identity_argument_plan() -> QuickDoubleArgumentProgram {
    QuickDoubleArgumentProgram {
        operations: Vec::new().into_boxed_slice(),
        outputs: [
            QuickDoubleSource::Input(0),
            QuickDoubleSource::Input(1),
            QuickDoubleSource::Input(2),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
        ],
        output_count: 3,
        input_slots: [0, 1, 2, u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX],
        input_count: 3,
    }
}

fn zero_divisor_argument_plan() -> QuickDoubleArgumentProgram {
    QuickDoubleArgumentProgram {
        operations: vec![crate::vm::quick::QuickDoubleArgumentOp {
            kind: ScalarDoubleOpKind::Divide,
            lhs: QuickDoubleSource::Constant(1.0),
            rhs: QuickDoubleSource::Constant(-0.0),
        }]
        .into_boxed_slice(),
        outputs: [
            QuickDoubleSource::Temporary(0),
            QuickDoubleSource::Constant(4.0),
            QuickDoubleSource::Constant(2.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
        ],
        output_count: 3,
        input_slots: [u16::MAX; 8],
        input_count: 0,
    }
}

fn dependent_argument_plan() -> QuickDoubleArgumentProgram {
    QuickDoubleArgumentProgram {
        operations: vec![
            QuickDoubleArgumentOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: QuickDoubleSource::Input(0),
                rhs: QuickDoubleSource::Constant(1.0),
            },
            QuickDoubleArgumentOp {
                kind: ScalarDoubleOpKind::Multiply,
                lhs: QuickDoubleSource::Temporary(0),
                rhs: QuickDoubleSource::Induction,
            },
        ]
        .into_boxed_slice(),
        outputs: [
            QuickDoubleSource::Temporary(1),
            QuickDoubleSource::Constant(4.0),
            QuickDoubleSource::Constant(2.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
        ],
        output_count: 3,
        input_slots: [0, u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX],
        input_count: 1,
    }
}

fn rhs_overwrite_argument_plan() -> QuickDoubleArgumentProgram {
    QuickDoubleArgumentProgram {
        operations: vec![QuickDoubleArgumentOp {
            kind: ScalarDoubleOpKind::Add,
            lhs: QuickDoubleSource::Induction,
            rhs: QuickDoubleSource::Constant(0.5),
        }]
        .into_boxed_slice(),
        outputs: [
            QuickDoubleSource::Temporary(0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
            QuickDoubleSource::Constant(0.0),
        ],
        output_count: 1,
        input_slots: [u16::MAX; 8],
        input_count: 0,
    }
}

fn rhs_overwrite_leaf_plan() -> ScalarDoubleFunctionPlan {
    ScalarDoubleFunctionPlan::new(
        1,
        ScalarDoubleProgram {
            operations: vec![ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Subtract,
                lhs: ScalarDoubleSource::Constant(10.0),
                rhs: ScalarDoubleSource::Input(0),
            }]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(0),
        },
    )
}

fn register_to_register_double_move_count(code: &[u8]) -> usize {
    code.windows(4)
        .filter(|bytes| {
            bytes[0] == 0x66
                && bytes[1] == 0x0f
                && bytes[2] == 0x28
                && bytes[3] & 0xc0 == 0xc0
        })
        .count()
}

fn contains_vzeroupper(code: &[u8]) -> bool {
    code.windows(3).any(|bytes| bytes == [0xc5, 0xf8, 0x77])
}

#[test]
fn encoder_produces_exact_scalar_double_bytes() {
    let mut assembler = X86_64Assembler::new();
    assembler.add_double(
        X86_64FloatRegister::from_code(0),
        X86_64FloatRegister::from_code(1),
    );
    assembler.subtract_double(
        X86_64FloatRegister::from_code(2),
        X86_64FloatRegister::from_code(3),
    );
    assembler.multiply_double(
        X86_64FloatRegister::from_code(4),
        X86_64FloatRegister::from_code(5),
    );
    assembler.divide_double(
        X86_64FloatRegister::from_code(6),
        X86_64FloatRegister::from_code(7),
    );
    assert_eq!(
        assembler.finish().as_ref(),
        [
            0xf2, 0x0f, 0x58, 0xc1, 0xf2, 0x0f, 0x5c, 0xd3, 0xf2, 0x0f, 0x59, 0xe5, 0xf2, 0x0f,
            0x5e, 0xf7,
        ]
    );
}

#[test]
fn encoder_uses_a_full_width_double_register_move() {
    let mut assembler = X86_64Assembler::new();
    assembler.move_double(
        X86_64FloatRegister::from_code(2),
        X86_64FloatRegister::from_code(3),
    );
    assert_eq!(assembler.finish().as_ref(), [0x66, 0x0f, 0x28, 0xd3]);
}

#[test]
fn encoder_produces_exact_avx_double_slice_bytes() {
    let mut assembler = X86_64Assembler::new();
    assembler.add_double_avx(
        X86_64FloatRegister::from_code(1),
        X86_64FloatRegister::from_code(2),
        X86_64FloatRegister::from_code(3),
    );
    assembler.subtract_double_avx(
        X86_64FloatRegister::from_code(4),
        X86_64FloatRegister::from_code(5),
        X86_64FloatRegister::from_code(6),
    );
    assembler.multiply_double_avx(
        X86_64FloatRegister::from_code(7),
        X86_64FloatRegister::from_code(8),
        X86_64FloatRegister::from_code(9),
    );
    assembler.divide_double_avx(
        X86_64FloatRegister::from_code(10),
        X86_64FloatRegister::from_code(11),
        X86_64FloatRegister::from_code(2),
    );
    assembler.move_double_avx(
        X86_64FloatRegister::from_code(2),
        X86_64FloatRegister::from_code(3),
    );
    assembler.load_f64_avx(
        X86_64FloatRegister::from_code(1),
        super::X86_64Register::RDI,
        8,
    );
    assembler.store_f64_avx(
        super::X86_64Register::RSI,
        X86_64FloatRegister::from_code(1),
        16,
    );
    assembler.move_gpr_bits_to_double_avx(
        X86_64FloatRegister::from_code(1),
        super::X86_64Register::RAX,
    );
    assembler.move_double_bits_to_gpr_avx(
        super::X86_64Register::RAX,
        X86_64FloatRegister::from_code(1),
    );
    let zero = X86_64FloatRegister::from_code(12);
    assembler.zero_double_register_avx(zero);
    assembler.convert_signed_to_double_avx(
        X86_64FloatRegister::from_code(0),
        zero,
        super::X86_64Register::RCX,
    );
    assembler.vzeroupper();
    assert_eq!(
        assembler.finish().as_ref(),
        [
            0xc5, 0xeb, 0x58, 0xcb, 0xc5, 0xd3, 0x5c, 0xe6, 0xc4, 0xc1, 0x3b, 0x59, 0xf9, 0xc5,
            0x23, 0x5e, 0xd2, 0xc5, 0xf9, 0x28, 0xd3, 0xc5, 0xfb, 0x10, 0x8f, 0x08, 0x00, 0x00,
            0x00, 0xc5, 0xfb, 0x11, 0x8e, 0x10, 0x00, 0x00, 0x00, 0xc4, 0xe1, 0xf9, 0x6e, 0xc8,
            0xc4, 0xe1, 0xf9, 0x7e, 0xc8, 0xc4, 0x41, 0x19, 0x57, 0xe4, 0xc4, 0xe1, 0x9b, 0x2a,
            0xc1, 0xc5, 0xf8, 0x77,
        ]
    );
}

#[test]
fn linear_double_temporaries_reuse_the_dead_lhs_register() {
    let plan = arithmetic_plan();
    let registers = X86ScalarDoubleRegisterMap::new(&plan.program);
    let first = X86_64FloatRegister::from_code(2);
    assert_eq!(registers.temporary(0), first);
    assert_eq!(registers.temporary(1), first);
    assert_eq!(registers.temporary(2), first);
}

#[test]
fn branched_double_temporaries_preserve_live_values() {
    let program = ScalarDoubleProgram {
        operations: vec![
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: ScalarDoubleSource::Input(0),
                rhs: ScalarDoubleSource::Constant(1.0),
            },
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Multiply,
                lhs: ScalarDoubleSource::Temporary(0),
                rhs: ScalarDoubleSource::Constant(2.0),
            },
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Subtract,
                lhs: ScalarDoubleSource::Temporary(0),
                rhs: ScalarDoubleSource::Constant(3.0),
            },
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: ScalarDoubleSource::Temporary(1),
                rhs: ScalarDoubleSource::Temporary(2),
            },
        ]
        .into_boxed_slice(),
        output: ScalarDoubleSource::Temporary(3),
    };
    let registers = X86ScalarDoubleRegisterMap::new(&program);
    assert_eq!(registers.temporary(0), X86_64FloatRegister::from_code(2));
    assert_eq!(registers.temporary(1), X86_64FloatRegister::from_code(3));
    assert_eq!(registers.temporary(2), X86_64FloatRegister::from_code(2));
    assert_eq!(registers.temporary(3), X86_64FloatRegister::from_code(3));

    let plan = ScalarDoubleFunctionPlan::new(1, program);
    let compiled = CompiledScalarDoubleProgram::compile(&plan).unwrap();
    assert_eq!(
        compiled.call(&[5.0]).unwrap(),
        ScalarDoubleJitOutcome::Value(15.0)
    );
}

#[test]
fn linear_double_program_preserves_sse2_copy_and_removes_it_with_avx() {
    let plan = arithmetic_plan();
    let sse2 = CompiledScalarDoubleProgram::compile_with_instruction_set(
        &plan,
        X86DoubleInstructionSet::Sse2,
    )
    .unwrap();
    assert_eq!(register_to_register_double_move_count(sse2.code()), 1);

    let avx = CompiledScalarDoubleProgram::compile_with_instruction_set(
        &plan,
        X86DoubleInstructionSet::Avx,
    )
    .unwrap();
    assert_eq!(register_to_register_double_move_count(avx.code()), 0);
}

#[test]
fn native_double_program_selects_avx_only_when_the_host_supports_it() {
    let program = CompiledScalarDoubleProgram::compile(&arithmetic_plan()).unwrap();
    assert_eq!(
        contains_vzeroupper(program.code()),
        std::is_x86_feature_detected!("avx")
    );
}

#[test]
fn native_double_program_executes_and_division_zero_side_exits_transactionally() {
    let program = CompiledScalarDoubleProgram::compile(&arithmetic_plan()).unwrap();
    assert_eq!(
        program.call(&[2.5, 4.0, 2.0]).unwrap(),
        ScalarDoubleJitOutcome::Value(8.0)
    );
    assert_eq!(
        program.call(&[2.5, 4.0, -0.0]).unwrap(),
        ScalarDoubleJitOutcome::SideExit
    );
    let nan = program.call(&[2.5, 4.0, f64::NAN]).unwrap();
    assert!(matches!(nan, ScalarDoubleJitOutcome::Value(value) if value.is_nan()));
}

#[test]
fn conditional_double_program_preserves_ordered_and_nan_semantics() {
    let cases = [
        (ScalarLongConditionKind::Equal, [2.0, 2.0], 1.0),
        (ScalarLongConditionKind::Equal, [2.0, 3.0], -1.0),
        (ScalarLongConditionKind::Equal, [f64::NAN, 3.0], -1.0),
        (ScalarLongConditionKind::Equal, [0.0, -0.0], 1.0),
        (ScalarLongConditionKind::NotEqual, [2.0, 2.0], -1.0),
        (ScalarLongConditionKind::NotEqual, [2.0, 3.0], 1.0),
        (ScalarLongConditionKind::NotEqual, [f64::NAN, 3.0], 1.0),
        (ScalarLongConditionKind::LessThan, [2.0, 3.0], 1.0),
        (ScalarLongConditionKind::LessThan, [3.0, 2.0], -1.0),
        (ScalarLongConditionKind::LessThan, [f64::NAN, 3.0], -1.0),
        (ScalarLongConditionKind::LessThanOrEqual, [2.0, 2.0], 1.0),
        (ScalarLongConditionKind::LessThanOrEqual, [2.0, 3.0], 1.0),
        (ScalarLongConditionKind::LessThanOrEqual, [3.0, 2.0], -1.0),
        (
            ScalarLongConditionKind::LessThanOrEqual,
            [f64::NAN, 3.0],
            -1.0,
        ),
    ];
    for (kind, inputs, expected) in cases {
        for instruction_set in [X86DoubleInstructionSet::Sse2, X86DoubleInstructionSet::Avx] {
            let program = CompiledScalarDoubleProgram::compile_with_instruction_set(
                &conditional_plan(kind),
                instruction_set,
            )
            .unwrap();
            assert_eq!(
                program.call(&inputs).unwrap(),
                ScalarDoubleJitOutcome::Value(expected)
            );
        }
    }
}

#[test]
fn conditional_double_loop_executes_both_arithmetic_edges() {
    for instruction_set in [X86DoubleInstructionSet::Sse2, X86DoubleInstructionSet::Avx] {
        let program = CompiledQuickDoubleCallAccumulateLoop::compile_with_instruction_set(
            &conditional_argument_plan(),
            &conditional_arithmetic_plan(),
            instruction_set,
        )
        .unwrap();
        let mut state = NativeDoubleCallAccumulateState {
            induction: 0,
            bound: 10,
            accumulator: 0.0,
            last_term: -1.0,
        };
        assert_eq!(
            program.call(&mut state, &[], &false).unwrap(),
            QuickDoubleCallAccumulateJitOutcome::Completed
        );
        assert_eq!(state.induction, 10);
        assert_eq!(state.accumulator, 21.25);
        assert_eq!(state.last_term, 1.25);
    }
}

#[test]
fn conditional_double_loop_side_exits_only_on_the_selected_edge() {
    for instruction_set in [X86DoubleInstructionSet::Sse2, X86DoubleInstructionSet::Avx] {
        let program = CompiledQuickDoubleCallAccumulateLoop::compile_with_instruction_set(
            &two_input_argument_plan(),
            &selective_division_plan(),
            instruction_set,
        )
        .unwrap();
        let mut false_edge = NativeDoubleCallAccumulateState {
            induction: 0,
            bound: 1,
            accumulator: 0.0,
            last_term: -1.0,
        };
        assert_eq!(
            program.call(&mut false_edge, &[1.0, 0.0], &false).unwrap(),
            QuickDoubleCallAccumulateJitOutcome::Completed
        );
        assert_eq!(false_edge.accumulator, 3.0);

        let mut true_edge = NativeDoubleCallAccumulateState {
            induction: 0,
            bound: 1,
            accumulator: 7.0,
            last_term: 2.0,
        };
        assert_eq!(
            program.call(&mut true_edge, &[-1.0, 0.0], &false).unwrap(),
            QuickDoubleCallAccumulateJitOutcome::SideExit
        );
        assert_eq!(
            true_edge,
            NativeDoubleCallAccumulateState {
                induction: 0,
                bound: 1,
                accumulator: 7.0,
                last_term: 2.0,
            }
        );
    }
}

#[test]
fn scalar_double_program_executes_with_forced_sse2_and_avx() {
    let plan = arithmetic_plan();
    for instruction_set in [X86DoubleInstructionSet::Sse2, X86DoubleInstructionSet::Avx] {
        let program =
            CompiledScalarDoubleProgram::compile_with_instruction_set(&plan, instruction_set)
                .unwrap();
        if instruction_set == X86DoubleInstructionSet::Avx && !std::is_x86_feature_detected!("avx")
        {
            continue;
        }
        assert_eq!(
            program.call(&[2.5, 4.0, 2.0]).unwrap(),
            ScalarDoubleJitOutcome::Value(8.0)
        );
        assert_eq!(
            program.call(&[2.5, 4.0, -0.0]).unwrap(),
            ScalarDoubleJitOutcome::SideExit
        );
    }
}

#[test]
fn double_cache_compiles_at_the_shared_leaf_threshold() {
    let plan = arithmetic_plan();
    let arguments = [2.5, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    for _ in 1..SCALAR_DOUBLE_JIT_HOT_THRESHOLD {
        assert_eq!(
            plan.native_jit().dispatch(&plan, &arguments),
            ScalarDoubleJitDispatch::Interpret
        );
    }
    assert_eq!(
        plan.native_jit().dispatch(&plan, &arguments),
        ScalarDoubleJitDispatch::Value(8.0)
    );
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
}

#[test]
fn single_operation_leaf_stays_in_the_rust_adapter() {
    let plan = ScalarDoubleFunctionPlan::new(
        2,
        ScalarDoubleProgram {
            operations: vec![ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: ScalarDoubleSource::Input(0),
                rhs: ScalarDoubleSource::Input(1),
            }]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(0),
        },
    );
    let arguments = [1.5, 2.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    for _ in 0..SCALAR_DOUBLE_JIT_HOT_THRESHOLD * 2 {
        assert_eq!(
            plan.native_jit().dispatch(&plan, &arguments),
            ScalarDoubleJitDispatch::Interpret
        );
    }
    assert!(!plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 0);
}

#[test]
fn composed_double_loop_completes_and_preserves_empty_state() {
    let program = CompiledQuickDoubleCallAccumulateLoop::compile(
        &identity_argument_plan(),
        &arithmetic_plan(),
    )
    .unwrap();
    let mut state = NativeDoubleCallAccumulateState {
        induction: 0,
        bound: 5,
        accumulator: 1.0,
        last_term: -1.0,
    };
    let interrupt = false;
    assert_eq!(
        program.call(&mut state, &[2.5, 4.0, 2.0], &interrupt).unwrap(),
        QuickDoubleCallAccumulateJitOutcome::Completed
    );
    assert_eq!(state.induction, 5);
    assert_eq!(state.accumulator, 41.0);
    assert_eq!(state.last_term, 8.0);

    state.bound = 5;
    state.last_term = 6.0;
    assert_eq!(
        program.call(&mut state, &[2.5, 4.0, 2.0], &interrupt).unwrap(),
        QuickDoubleCallAccumulateJitOutcome::Completed
    );
    assert_eq!(state.last_term, 6.0);
}

#[test]
fn composed_double_loop_executes_with_forced_sse2_and_avx() {
    for instruction_set in [X86DoubleInstructionSet::Sse2, X86DoubleInstructionSet::Avx] {
        let program = CompiledQuickDoubleCallAccumulateLoop::compile_with_instruction_set(
            &identity_argument_plan(),
            &arithmetic_plan(),
            instruction_set,
        )
        .unwrap();
        if instruction_set == X86DoubleInstructionSet::Avx && !std::is_x86_feature_detected!("avx")
        {
            continue;
        }
        let mut state = NativeDoubleCallAccumulateState {
            induction: 0,
            bound: 5,
            accumulator: 1.0,
            last_term: -1.0,
        };
        assert_eq!(
            program.call(&mut state, &[2.5, 4.0, 2.0], &false).unwrap(),
            QuickDoubleCallAccumulateJitOutcome::Completed
        );
        assert_eq!(state.induction, 5);
        assert_eq!(state.accumulator, 41.0);
        assert_eq!(state.last_term, 8.0);
    }
}

#[test]
fn composed_double_loop_polls_and_side_exits_transactionally() {
    let program = CompiledQuickDoubleCallAccumulateLoop::compile(
        &identity_argument_plan(),
        &arithmetic_plan(),
    )
    .unwrap();
    let mut state = NativeDoubleCallAccumulateState {
        induction: 0,
        bound: 2_000,
        accumulator: 0.0,
        last_term: -1.0,
    };
    let interrupt = true;
    assert_eq!(
        program.call(&mut state, &[2.5, 4.0, 2.0], &interrupt).unwrap(),
        QuickDoubleCallAccumulateJitOutcome::Interrupted
    );
    assert_eq!(state.induction, 1_024);
    assert_eq!(state.accumulator, 8_192.0);
    assert_eq!(state.last_term, 8.0);

    let mut side_exit_state = NativeDoubleCallAccumulateState {
        induction: 3,
        bound: 10,
        accumulator: 7.0,
        last_term: 2.0,
    };
    let no_interrupt = false;
    assert_eq!(
        program
            .call(&mut side_exit_state, &[2.5, 4.0, -0.0], &no_interrupt)
            .unwrap(),
        QuickDoubleCallAccumulateJitOutcome::SideExit
    );
    assert_eq!(
        side_exit_state,
        NativeDoubleCallAccumulateState {
            induction: 3,
            bound: 10,
            accumulator: 7.0,
            last_term: 2.0,
        }
    );
}

#[test]
fn composed_double_loop_argument_division_side_exits_before_the_iteration() {
    let program = CompiledQuickDoubleCallAccumulateLoop::compile(
        &zero_divisor_argument_plan(),
        &arithmetic_plan(),
    )
    .unwrap();
    let mut state = NativeDoubleCallAccumulateState {
        induction: 3,
        bound: 10,
        accumulator: 7.0,
        last_term: 2.0,
    };
    assert_eq!(
        program.call(&mut state, &[], &false).unwrap(),
        QuickDoubleCallAccumulateJitOutcome::SideExit
    );
    assert_eq!(
        state,
        NativeDoubleCallAccumulateState {
            induction: 3,
            bound: 10,
            accumulator: 7.0,
            last_term: 2.0,
        }
    );
}

#[test]
fn composed_double_loop_retains_invariant_dependencies_of_dynamic_arguments() {
    let program = CompiledQuickDoubleCallAccumulateLoop::compile(
        &dependent_argument_plan(),
        &arithmetic_plan(),
    )
    .unwrap();
    assert_eq!(program.forwarded_argument_mask(), 1);
    let mut state = NativeDoubleCallAccumulateState {
        induction: 0,
        bound: 5,
        accumulator: 1.0,
        last_term: -1.0,
    };
    assert_eq!(
        program.call(&mut state, &[2.0], &false).unwrap(),
        QuickDoubleCallAccumulateJitOutcome::Completed
    );
    assert_eq!(state.induction, 5);
    assert_eq!(state.accumulator, 76.0);
    assert_eq!(state.last_term, 27.0);
}

#[test]
fn composed_double_loop_buffers_rhs_that_conflicts_with_leaf_destination() {
    let program = CompiledQuickDoubleCallAccumulateLoop::compile(
        &rhs_overwrite_argument_plan(),
        &rhs_overwrite_leaf_plan(),
    )
    .unwrap();
    assert_eq!(program.forwarded_argument_mask(), 0);
    let mut state = NativeDoubleCallAccumulateState {
        induction: 0,
        bound: 3,
        accumulator: 0.0,
        last_term: -1.0,
    };
    assert_eq!(
        program.call(&mut state, &[], &false).unwrap(),
        QuickDoubleCallAccumulateJitOutcome::Completed
    );
    assert_eq!(state.induction, 3);
    assert_eq!(state.accumulator, 25.5);
    assert_eq!(state.last_term, 7.5);
}
