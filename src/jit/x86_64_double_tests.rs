use super::{
    CompiledScalarDoubleProgram, SCALAR_DOUBLE_JIT_HOT_THRESHOLD, ScalarDoubleJitDispatch,
    ScalarDoubleJitOutcome, X86_64Assembler, X86_64FloatRegister,
};
use crate::vm::function::{
    ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleProgram,
    ScalarDoubleSource,
};

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
