#[test]
fn structured_lowering_executes_both_forward_control_flow_edges() {
    let program = CompiledX86StraightLongLoop::compile(structured_recurrence(4)).unwrap();
    let mut slots = [0_i64; 64];
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[4, 224, 224]);

    assert_eq!(
        program
            .code()
            .windows(5)
            .filter(|window| *window == [0x49, 0x83, 0xfb, 0x02, 0x7d])
            .count(),
        5,
        "each ABI entry should use short JGE for the structured false edge"
    );
    assert_eq!(
        program.code().iter().filter(|byte| **byte == 0xeb).count(),
        5,
        "each ABI entry should use a short unconditional join jump"
    );
}

#[test]
fn structured_lowering_elides_control_flow_to_the_immediate_successor() {
    let mut config = composed_add_recurrence(4);
    config.operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::Equal,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(-1)),
        false_target: 1,
    };
    config.operations[1] = NativeStraightLongOperation::Jump { target: 2 };
    config.operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(0),
        result: 2,
        destination: 1,
    };
    config.operation_count = 3;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[4, 6, 6]);

    let fast_code = &program.code()[..program.checked_entry_offset];
    assert!(
        !fast_code.windows(2).any(|window| window == [0x0f, 0x85]),
        "a predicate whose false edge is fallthrough should not be emitted"
    );
    assert!(
        !fast_code.contains(&0xe9),
        "an unconditional jump to fallthrough should not be emitted"
    );
}

#[test]
fn structured_bitwise_condition_executes_in_private_shadow() {
    let mut config = structured_recurrence(4);
    config.operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::Equal,
        lhs: NativeStraightLongConditionOperand::BitwiseAnd {
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(1),
        },
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(0)),
        false_target: 3,
    };
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[4, 224, 224]);
}

#[test]
fn guard_side_exit_reports_exact_operation_after_completed_iterations() {
    let mut config = structured_recurrence(4);
    config.operations[0] = NativeStraightLongOperation::Guard {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        expected: true,
    };
    config.operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(10),
        result: 2,
        destination: 1,
    };
    config.operations[2] = NativeStraightLongOperation::Jump { target: 5 };
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    let result = program.call(&mut slots).unwrap();
    assert_eq!(
        result,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::OperationSideExit,
            failed_operation: Some(0),
        }
    );
    assert_eq!(&slots[..3], &[2, 20, 20]);
}

#[test]
fn scalar_lowering_executes_divide_modulo_and_bitwise_ops() {
    let mut config = composed_add_recurrence(5);
    config.operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Modulo,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 4,
    };
    config.operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::IntDivide,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(2),
        result: 5,
    };
    config.operations[2] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::BitwiseAnd,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Slot(5),
        result: 6,
    };
    config.operations[3] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::BitwiseOr,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Slot(5),
        result: 7,
    };
    config.operations[4] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::BitwiseXor,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Slot(5),
        result: 8,
    };
    config.operation_count = 5;
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(slots[0], 5);
    assert_eq!(&slots[4..9], &[1, 2, 0, 3, 3]);
}

#[test]
fn checked_division_side_exit_prevents_native_zero_divide() {
    let mut config = composed_add_recurrence(1);
    config.operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::IntDivide,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Slot(7),
        result: 4,
    };
    config.operation_count = 1;
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    slots[4] = 91;
    slots[7] = 0;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(
        result,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::OperationSideExit,
            failed_operation: Some(0),
        }
    );
    assert_eq!(slots[0], 0);
    assert_eq!(slots[4], 91);
}

#[test]
fn checked_operations_share_cold_side_exit_publication() {
    let mut config = composed_add_recurrence(1);
    config.operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::IntDivide,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Slot(7),
        result: 4,
    };
    config.operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(1),
        result: 2,
        destination: 1,
    };
    config.operation_count = 2;
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();

    let mut divide_by_zero = [0_i64; 64];
    divide_by_zero[7] = 0;
    assert_eq!(
        program.call(&mut divide_by_zero).unwrap(),
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::OperationSideExit,
            failed_operation: Some(0),
        }
    );
    let mut sum_overflow = [0_i64; 64];
    sum_overflow[1] = i64::MAX;
    sum_overflow[7] = 1;
    assert_eq!(
        program.call(&mut sum_overflow).unwrap(),
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::OperationSideExit,
            failed_operation: Some(1),
        }
    );

    let checked_code = &program.code()[program.checked_entry_offset..program.chunk_entry_offset];
    for selector in [
        [0xb8, 0x06, 0x00, 0x00, 0x00, 0xe9],
        [0xb8, 0x06, 0x01, 0x00, 0x00, 0xe9],
    ] {
        assert_eq!(
            checked_code
                .windows(selector.len())
                .filter(|window| *window == selector)
                .count(),
            1,
            "each failed operation should select one shared cold epilogue"
        );
    }
}

#[test]
fn standalone_modulo_preserves_signed_remainder_semantics() {
    let mut config = composed_add_recurrence(1);
    config.operations[0] = NativeStraightLongOperation::Modulo {
        value: QuickLongOperand::Slot(6),
        divisor: 2,
        result: 4,
    };
    config.operation_count = 1;
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    slots[6] = -5;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(slots[4], -1);
    assert!(
        program
            .code()
            .windows(4)
            .any(|window| window == [0x48, 0x83, 0xe0, 0x01]),
        "small remainder mask should encode directly in AND"
    );
    let mut divisor_load = vec![0x49, 0xb8];
    divisor_load.extend_from_slice(&2_i64.to_le_bytes());
    assert!(
        !program
            .code()
            .windows(divisor_load.len())
            .any(|window| window == divisor_load),
        "power-of-two divisor should not be materialized before mask lowering"
    );
}

#[test]
fn wide_power_of_two_remainder_materializes_only_the_exact_mask() {
    let divisor = 1_i64 << 40;
    let mask = divisor - 1;
    let mut config = composed_add_recurrence(1);
    config.operations[0] = NativeStraightLongOperation::Modulo {
        value: QuickLongOperand::Slot(6),
        divisor,
        result: 4,
    };
    config.operation_count = 1;
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    slots[6] = -(divisor + 5);
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(slots[4], -5);

    let mut mask_load = vec![0x49, 0xb8];
    mask_load.extend_from_slice(&mask.to_le_bytes());
    assert!(
        program
            .code()
            .windows(mask_load.len())
            .any(|window| window == mask_load),
        "mask outside sign-extended imm32 must retain MOVABS fallback"
    );
    let mut divisor_load = vec![0x49, 0xb8];
    divisor_load.extend_from_slice(&divisor.to_le_bytes());
    assert!(
        !program
            .code()
            .windows(divisor_load.len())
            .any(|window| window == divisor_load),
        "recognized divisor itself is dead even when the mask needs MOVABS"
    );
}
