#![cfg(all(
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]

use rphp::jit::{
    Arm64Assembler, Arm64Register, CompiledAddMultiply, CompiledScalarLongProgram,
    ScalarLongJitError, ScalarLongJitOutcome,
};
use rphp::vm::function::{
    ScalarLongFunctionPlan, ScalarLongOp, ScalarLongOpKind, ScalarLongProgram, ScalarLongSource,
};

fn scalar_plan(
    public_args: u8,
    operations: Vec<ScalarLongOp>,
    output: ScalarLongSource,
) -> ScalarLongFunctionPlan {
    ScalarLongFunctionPlan {
        public_args,
        program: ScalarLongProgram {
            operations: operations.into_boxed_slice(),
            outputs: [output],
            output_count: 1,
        },
        select: None,
    }
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
fn invalid_or_unsupported_ir_is_rejected_before_code_becomes_executable() {
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

    let division = scalar_plan(
        2,
        vec![ScalarLongOp {
            kind: ScalarLongOpKind::IntDivide,
            lhs: ScalarLongSource::Input(0),
            rhs: ScalarLongSource::Input(1),
        }],
        ScalarLongSource::Temporary(0),
    );
    assert!(matches!(
        CompiledScalarLongProgram::compile(&division),
        Err(ScalarLongJitError::UnsupportedOperation(
            ScalarLongOpKind::IntDivide
        ))
    ));
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
                ScalarLongOpKind::IntDivide | ScalarLongOpKind::Modulo => unreachable!(),
            };
            let expected = expected
                .map(ScalarLongJitOutcome::Value)
                .unwrap_or(ScalarLongJitOutcome::SideExit);
            assert_eq!(function.call(&[lhs, rhs]).unwrap(), expected);
        }
    }
}
