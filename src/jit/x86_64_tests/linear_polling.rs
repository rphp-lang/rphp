#[test]
fn range_proven_loop_executes_and_publishes_exact_slots() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(100, false)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 5;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(result.failed_operation, None);
    assert_eq!(slots[0], 100);
    assert_eq!(slots[1], 4_955);
    assert_eq!(slots[2], 4_955);

    assert!(
        program
            .code()
            .windows(6)
            .any(|bytes| bytes == [0x0f, 0x8d, 0x10, 0, 0, 0])
    );
    assert!(
        program
            .code()
            .windows(6)
            .any(|bytes| bytes == [0x0f, 0x8c, 0xf0, 0xff, 0xff, 0xff])
    );
    assert!(
        program
            .code()
            .windows(7)
            .any(|bytes| bytes == [0x48, 0x89, 0x97, 0x10, 0, 0, 0])
    );
}

#[test]
fn dynamic_bound_is_loaded_from_shadow_on_every_native_entry() {
    let mut config = additive_recurrence(0, false);
    config.bound = QuickLongOperand::Slot(3);
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();

    let mut first = [0_i64; 64];
    first[1] = 10;
    first[3] = 4;
    program.call(&mut first).unwrap();
    assert_eq!(&first[..4], &[4, 16, 16, 4]);

    let mut second = [0_i64; 64];
    second[1] = 1;
    second[3] = 6;
    program.call(&mut second).unwrap();
    assert_eq!(&second[..4], &[6, 16, 16, 6]);
}

#[test]
fn linear_lowering_executes_composed_operations_and_post_result() {
    let program = CompiledX86StraightLongLoop::compile(composed_add_recurrence(4)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 10;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..6], &[4, 20, 20, 0, 4, 3]);
}

#[test]
fn linear_lowering_supports_subtract_and_multiply() {
    let mut config = composed_add_recurrence(3);
    config.operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(2),
        result: 2,
        destination: 1,
    };
    config.operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(3),
        result: 4,
    };
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 2;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..5], &[3, 16, 16, 0, 13]);
}

#[test]
fn linear_checked_exit_reports_exact_failed_operation() {
    let mut config = composed_add_recurrence(1);
    config.operations[0] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Const(2),
        result: 4,
    };
    config.operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(4),
        result: 2,
        destination: 1,
    };
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = i64::MAX;
    slots[2] = 77;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(
        result,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::OperationSideExit,
            failed_operation: Some(1),
        }
    );
    assert_eq!(&slots[..5], &[0, i64::MAX, 77, 0, 2]);
}

#[test]
fn linear_polling_entry_preserves_composed_state_at_safepoint() {
    let program = CompiledX86StraightLongLoop::compile(composed_add_recurrence(5_000)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 10;
    let interrupt = true;
    let result = program.call_proven_polling(&mut slots, &interrupt).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!(slots[0], 1_024);
    assert_eq!(slots[1], 524_810);
    assert_eq!(slots[2], 524_810);
    assert_eq!(slots[4], 1_024);
    assert_eq!(slots[5], 1_023);
}

#[test]
fn range_proven_polling_schedules_induction_before_a_common_scalar_suffix() {
    let config = structured_recurrence(5_000);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            config,
            config.body_output_mask(),
            1u64 << 1,
        )
        .unwrap();
    let mut slots = [0_i64; 64];

    let interrupted = program.call_proven_polling(&mut slots, &true).unwrap();
    assert_eq!(
        interrupted.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!(&slots[..3], &[1_024, 103_244, 103_244]);

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[5_000, 504_820, 504_820]);

    let polling_code = &program.code()[program.polling_entry_offset..];
    let induction_increment = [0x49, 0x83, 0xc3, 0x01];
    let common_suffix_add = [0x49, 0x83, 0xc5, 0x01];
    assert_eq!(
        polling_code
            .windows(induction_increment.len())
            .filter(|window| *window == induction_increment)
            .count(),
        1
    );
    let increment_offset = polling_code
        .windows(induction_increment.len())
        .position(|window| window == induction_increment)
        .unwrap();
    assert_eq!(
        &polling_code[increment_offset + induction_increment.len()
            ..increment_offset + induction_increment.len() + common_suffix_add.len()],
        &common_suffix_add
    );
}

#[test]
fn range_proven_polling_fuses_immediate_affine_scalar_pair() {
    let config = structured_affine_expression(5_000);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            config,
            (1u64 << 1) | (1u64 << 2),
            0,
        )
        .unwrap();
    let mut slots = [0_i64; 64];

    let interrupted = program.call_proven_polling(&mut slots, &true).unwrap();
    assert_eq!(
        interrupted.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!(&slots[..3], &[1_024, 5_113, 15_350]);

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[5_000, 24_993, 74_990]);

    let polling_code = &program.code()[program.polling_entry_offset..];
    let scheduled_affine = [
        0x49, 0x83, 0xc3, 0x01, // ADD R11, 1
        0x4f, 0x8d, 0x74, 0x6d, 0x0b, // LEA R14, [R13 + R13*2 + 11]
    ];
    assert_eq!(
        polling_code
            .windows(scheduled_affine.len())
            .filter(|window| *window == scheduled_affine)
            .count(),
        1
    );
}

#[test]
fn range_proven_polling_preserves_published_affine_intermediate() {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 1,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(11),
        result: 2,
        destination: 2,
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(5_000),
        operations,
        operation_count: 2,
        post_result: None,
    };
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            config,
            (1u64 << 1) | (1u64 << 2),
            0,
        )
        .unwrap();
    let mut slots = [0_i64; 64];

    let interrupted = program.call_proven_polling(&mut slots, &true).unwrap();
    assert_eq!(
        interrupted.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!(&slots[..3], &[1_024, 3_069, 3_080]);

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[5_000, 14_997, 15_008]);

    let polling_code = &program.code()[program.polling_entry_offset..];
    assert!(polling_code.windows(2).all(|window| window != [0x4f, 0x8d]));
}

#[test]
fn range_proven_polling_does_not_fuse_across_scheduled_induction_increment() {
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            scheduled_increment_between_affine_pair(5_000),
            (1u64 << 1) | (1u64 << 5),
            0,
        )
        .unwrap();
    let mut slots = [0_i64; 64];

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[5]), (5_000, 15_008, 15_008));
    let polling_code = &program.code()[program.polling_entry_offset..];
    assert!(polling_code.windows(2).all(|window| window != [0x4f, 0x8d]));
}

#[test]
fn range_proven_polling_keeps_three_recurrences_resident_and_publishes_them() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(0),
        result: 4,
        destination: 1,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(2),
        rhs: QuickLongOperand::Const(2),
        result: 5,
        destination: 2,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::BitwiseXor,
        lhs: QuickLongOperand::Slot(3),
        rhs: QuickLongOperand::Slot(0),
        result: 6,
        destination: 3,
    };
    let publication_mask = (1u64 << 1) | (1u64 << 2) | (1u64 << 3);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(5_000),
                operations,
                operation_count: 3,
                post_result: None,
            },
            publication_mask,
            publication_mask,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 5;
    slots[2] = 7;
    slots[3] = 9;

    let interrupted = program.call_proven_polling(&mut slots, &true).unwrap();
    assert_eq!(
        interrupted.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    let mut expected_xor = 9;
    for value in 0..1_024 {
        expected_xor ^= value;
    }
    assert_eq!(&slots[..4], &[1_024, 523_781, 2_055, expected_xor]);
    assert_eq!(&slots[4..7], &[0, 0, 0]);

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    for value in 1_024..5_000 {
        expected_xor ^= value;
    }
    assert_eq!(slots[0], 5_000);
    assert_eq!(slots[1], 12_497_505);
    assert_eq!(slots[2], 10_007);
    assert_eq!(slots[3], expected_xor);
    assert_eq!(&slots[4..7], &[0, 0, 0]);
}

#[test]
fn constant_bound_frees_rcx_for_a_fourth_resident_value() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    for (index, slot) in [1u16, 2, 3].into_iter().enumerate() {
        operations[index] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(slot),
            rhs: QuickLongOperand::Slot(7),
            result: slot + 3,
            destination: slot,
        };
    }
    let publication_mask = (1u64 << 1) | (1u64 << 2) | (1u64 << 3);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 3,
                post_result: None,
            },
            publication_mask,
            publication_mask,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1..=3].copy_from_slice(&[1, 2, 3]);
    slots[7] = 5;

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..4], &[4, 21, 22, 23]);

    let polling_code = &program.code()[program.polling_entry_offset..];
    let slot_7_rcx_load = [0x48, 0x8b, 0x8f, 0x38, 0x00, 0x00, 0x00];
    assert_eq!(
        polling_code
            .windows(slot_7_rcx_load.len())
            .filter(|window| *window == slot_7_rcx_load)
            .count(),
        1,
        "the freed bound register should cache invariant slot 7 once"
    );
    assert_eq!(
        polling_code
            .windows(4)
            .filter(|window| *window == [0x49, 0x83, 0xfb, 0x04])
            .count(),
        2,
        "entry and backedge should compare induction against the embedded bound"
    );
    for direct_add in [[0x4c, 0x03, 0xe9], [0x4c, 0x03, 0xf1], [0x4c, 0x03, 0xf9]] {
        assert!(
            polling_code
                .windows(direct_add.len())
                .any(|window| window == direct_add),
            "each carried recurrence should consume invariant RCX directly"
        );
    }
}

#[test]
fn wide_constant_bound_keeps_the_dedicated_bound_register() {
    assert_eq!(
        x86_embedded_loop_bound(QuickLongOperand::Const(i64::from(i32::MAX) + 1)),
        None
    );
    assert_eq!(
        x86_embedded_loop_bound(QuickLongOperand::Const(i64::from(i32::MIN) - 1)),
        None
    );
    assert_eq!(
        x86_embedded_loop_bound(QuickLongOperand::Const(i64::from(i32::MAX))),
        Some(i64::from(i32::MAX))
    );
    assert_eq!(x86_embedded_loop_bound(QuickLongOperand::Slot(1)), None);
}
