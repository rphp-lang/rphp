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
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
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
    let program =
        CompiledQuickLongStraightLoop::compile(config).expect("straight Long loop should lower");
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
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
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
    assert_eq!(config.output_mask_before(2), (1u64 << 2) | (1u64 << 3));

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
    let overflow_program = CompiledQuickLongStraightLoop::compile(NativeStraightLongLoopConfig {
        operations: overflow_operations,
        ..config
    })
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
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
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
    let guarded = CompiledQuickLongStraightLoop::compile(NativeStraightLongLoopConfig {
        operations,
        ..config
    })
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
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
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
        let program = CompiledQuickLongStraightLoop::compile(NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Slot(1),
            operations,
            operation_count: 5,
            post_result: None,
        })
        .expect("structured scalar condition should lower");
        let mut slots = [0_i64; 64];
        slots[1] = 4;
        assert_eq!(
            program.call(&mut slots, 32).unwrap().outcome,
            NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!(slots[3], expected, "condition {kind:?}");
    }

    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
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

    operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
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
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
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
    assert_eq!(
        slots[4], 1,
        "last completed post-increment remains published"
    );
}

#[test]
fn finite_string_hash_operations_use_runtime_context_without_embedded_pointers() {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
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

    let missing_entries = [std::ptr::null_mut(); NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    assert!(matches!(
        program.call_with_context(&mut slots, 8, &missing_entries),
        Err(QuickLongAccumulateJitError::InvalidProgram(_))
    ));
}

#[test]
fn invalid_finite_string_token_side_exits_at_read_only_hash_load() {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::HashLoad {
        key: 2,
        entry_base: 0,
        token_count: 2,
        result: 3,
        destination: Some(4),
    };
    let program = CompiledQuickLongStraightLoop::compile(NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 1,
        post_result: None,
    })
    .expect("guarded read-only hash load should lower");

    let mut left = 7i64;
    let mut right = 20i64;
    let mut entries = [std::ptr::null_mut(); NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    entries[0] = &mut left;
    entries[1] = &mut right;
    let mut slots = [0i64; 64];
    slots[1] = 1;
    slots[2] = 3;
    slots[3] = -1;
    slots[4] = -2;
    let outcome = program.call_with_context(&mut slots, 8, &entries).unwrap();

    assert_eq!(
        outcome.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(outcome.failed_operation, Some(0));
    assert_eq!(slots[0], 0);
    assert_eq!(slots[3], -1);
    assert_eq!(slots[4], -2);
    assert_eq!(left, 7);
    assert_eq!(right, 20);
}

#[test]
fn straight_long_loop_reports_exact_failed_operation_transactionally() {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
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
