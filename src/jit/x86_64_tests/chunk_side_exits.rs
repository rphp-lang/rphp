#[test]
fn modulo_conditional_accumulate_matches_quick_ops_shape() {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Modulo {
        value: QuickLongOperand::Slot(2),
        divisor: 2,
        result: 4,
    };
    operations[1] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::Equal,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(4)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(0)),
        false_target: 3,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(2),
        result: 6,
        destination: 1,
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: 2,
        bound: QuickLongOperand::Slot(0),
        operations,
        operation_count: 3,
        post_result: None,
    };
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    slots[0] = 100_000;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(slots[2], 100_000);
    assert_eq!(slots[1], 2_499_950_000);
    assert!(
        program.code().windows(4).any(|window| {
            matches!(window[0], 0x48 | 0x49)
                && window[1] == 0x83
                && window[2] & 0xf8 == 0xf8
                && window[3] == 0
        }),
        "comparison against zero should use CMP r64, imm8"
    );
}

#[test]
fn chunk_entry_publishes_exact_safepoint_and_resumes_to_completion() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(10, false)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 5;

    let first = program.call_chunk(&mut slots, 3).unwrap();
    assert_eq!(
        first,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::ChunkExhausted,
            failed_operation: None,
        }
    );
    assert_eq!(&slots[..3], &[3, 8, 8]);

    let second = program.call_chunk(&mut slots, 7).unwrap();
    assert_eq!(
        second,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::Completed,
            failed_operation: None,
        }
    );
    assert_eq!(&slots[..3], &[10, 50, 50]);

    let mut exact = [0_i64; 64];
    exact[1] = 5;
    let exact_result = program.call_chunk(&mut exact, 10).unwrap();
    assert_eq!(
        exact_result.outcome,
        NativeStraightLongLoopOutcome::Completed
    );
    assert_eq!(&exact[..3], &[10, 50, 50]);
}

#[test]
fn chunk_entry_rejects_zero_budget_and_retains_checked_side_exit() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(2, false)).unwrap();
    let mut slots = [0_i64; 64];
    assert!(matches!(
        program.call_chunk(&mut slots, 0),
        Err(X86StraightLongLoopError::ZeroIterationBudget)
    ));

    slots[0] = 1;
    slots[1] = i64::MAX;
    slots[2] = 77;
    let side_exit = program.call_chunk(&mut slots, 1).unwrap();
    assert_eq!(
        side_exit,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::OperationSideExit,
            failed_operation: Some(0),
        }
    );
    assert_eq!(&slots[..3], &[1, i64::MAX, 77]);
}

#[test]
fn polling_entry_stays_native_until_interrupt_or_completion() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(5_000, false)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 5;
    let interrupt = true;
    let interrupted = program.call_proven_polling(&mut slots, &interrupt).unwrap();
    assert_eq!(
        interrupted,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::ChunkExhausted,
            failed_operation: None,
        }
    );
    assert_eq!(&slots[..3], &[1_024, 523_781, 523_781]);

    let interrupt = false;
    let completed = program.call_proven_polling(&mut slots, &interrupt).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[5_000, 12_497_505, 12_497_505]);
}

#[test]
fn polling_entry_gives_completion_priority_over_pending_interrupt() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(100, false)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 5;
    let interrupt = true;
    let result = program.call_proven_polling(&mut slots, &interrupt).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[100, 4_955, 4_955]);
}

#[test]
fn checked_side_exit_preserves_state_before_first_failed_operation() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(2, false)).unwrap();
    let mut slots = [0_i64; 64];
    slots[0] = 1;
    slots[1] = i64::MAX;
    slots[2] = 77;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(result.failed_operation, Some(0));
    assert_eq!(&slots[..3], &[1, i64::MAX, 77]);
    assert!(
        !program.code()[..program.checked_entry_offset]
            .windows(2)
            .any(|bytes| bytes == [0x0f, 0x80])
    );
    assert!(
        program.code()[program.checked_entry_offset..]
            .windows(2)
            .any(|bytes| bytes == [0x0f, 0x80])
    );
}

#[test]
fn checked_side_exit_publishes_last_successful_iteration() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(2, false)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = i64::MAX;
    slots[2] = 77;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(result.failed_operation, Some(0));
    assert_eq!(&slots[..3], &[1, i64::MAX, i64::MAX]);
}

#[test]
fn reversed_addition_and_empty_range_preserve_semantics() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(4, true)).unwrap();
    let mut slots = [0_i64; 64];
    slots[0] = -2;
    slots[1] = 10;
    program.call(&mut slots).unwrap();
    assert_eq!(&slots[..3], &[4, 13, 13]);

    let mut empty = [0_i64; 64];
    empty[0] = 4;
    empty[1] = 9;
    empty[2] = 81;
    program.call(&mut empty).unwrap();
    assert_eq!(&empty[..3], &[4, 9, 81]);
}
