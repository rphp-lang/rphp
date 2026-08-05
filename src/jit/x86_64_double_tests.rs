use super::{
    CompiledQuickDoubleCallAccumulateLoop, CompiledScalarDoubleProgram,
    NativeDoubleCallAccumulateState, QuickDoubleCallAccumulateJitOutcome,
    SCALAR_DOUBLE_JIT_HOT_THRESHOLD, ScalarDoubleJitDispatch, ScalarDoubleJitOutcome,
    X86_64Assembler, X86_64FloatRegister,
};
use crate::vm::function::{
    ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleProgram,
    ScalarDoubleSource,
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
