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

    assert_eq!(function.call(&[5]).unwrap(), ScalarLongJitOutcome::Value(6));
    assert_eq!(
        function.call(&[100]).unwrap(),
        ScalarLongJitOutcome::SideExit
    );
}

#[test]
fn constants_subtraction_and_bitwise_ops_share_the_native_lowering() {
    let plan = scalar_plan(
        2,
        vec![
            ScalarLongOp {
                kind: ScalarLongOpKind::Subtract,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(5),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::BitwiseAnd,
                lhs: ScalarLongSource::Temporary(0),
                rhs: ScalarLongSource::Constant(15),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::BitwiseOr,
                lhs: ScalarLongSource::Temporary(1),
                rhs: ScalarLongSource::Constant(16),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::BitwiseXor,
                lhs: ScalarLongSource::Temporary(2),
                rhs: ScalarLongSource::Input(1),
            },
        ],
        ScalarLongSource::Temporary(3),
    );
    let function = CompiledScalarLongProgram::compile(&plan).expect("plan should lower");

    assert_eq!(
        function.call(&[20, 3]).unwrap(),
        ScalarLongJitOutcome::Value(28)
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
        ScalarLongOpKind::BitwiseAnd,
        ScalarLongOpKind::BitwiseOr,
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
                ScalarLongOpKind::Compare => unreachable!("not in native operation matrix"),
                ScalarLongOpKind::Multiply => lhs.checked_mul(rhs),
                ScalarLongOpKind::BitwiseAnd => Some(lhs & rhs),
                ScalarLongOpKind::BitwiseOr => Some(lhs | rhs),
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
