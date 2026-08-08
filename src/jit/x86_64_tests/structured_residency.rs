#[test]
fn range_proven_structured_polling_merges_carried_register_values() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        false_target: 2,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(0),
        result: 2,
        destination: 1,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(3),
        rhs: QuickLongOperand::Const(1),
        result: 4,
        destination: 3,
    };
    let publication_mask = (1u64 << 1) | (1u64 << 3);
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
    slots[1] = 10;
    slots[3] = -5;

    let interrupted = program.call_proven_polling(&mut slots, &true).unwrap();
    assert_eq!(
        interrupted.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!((slots[0], slots[1], slots[3]), (1_024, 31, 1_019));

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[3]), (5_000, 31, 4_995));
}

#[test]
fn range_proven_structured_polling_forwards_branch_local_temporaries() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        false_target: 4,
    };
    operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 5,
    };
    operations[2] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(5),
        rhs: QuickLongOperand::Const(7),
        result: 6,
    };
    operations[3] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(6),
        result: 2,
        destination: 1,
    };
    operations[4] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(3),
        rhs: QuickLongOperand::Const(1),
        result: 4,
        destination: 3,
    };
    let publication_mask = (1u64 << 1) | (1u64 << 3);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 5,
                post_result: None,
            },
            publication_mask,
            publication_mask,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 10;
    slots[3] = -5;
    slots[5] = 77;
    slots[6] = 88;

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[3]), (4, 27, -1));
    assert_eq!((slots[5], slots[6]), (77, 88));
}

#[test]
fn range_proven_structured_polling_defers_visible_phi_publication() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        false_target: 4,
    };
    operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 5,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(5),
        rhs: QuickLongOperand::Const(1),
        result: 2,
        destination: 1,
    };
    operations[3] = NativeStraightLongOperation::Jump { target: 6 };
    operations[4] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(5),
        result: 6,
    };
    operations[5] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(6),
        rhs: QuickLongOperand::Const(2),
        result: 2,
        destination: 1,
    };
    operations[6] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(3),
        result: 7,
    };
    operations[7] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(7),
        rhs: QuickLongOperand::Const(11),
        result: 4,
        destination: 3,
    };
    // Result/destination aliases are defined by the same operation and
    // therefore share one fixed publication register per pair.
    let publication_mask = (1u64 << 1) | (1u64 << 2) | (1u64 << 3) | (1u64 << 4);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 8,
                post_result: None,
            },
            publication_mask,
            0,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(
        (slots[0], slots[1], slots[2], slots[3], slots[4]),
        (4, 13, 13, 50, 50)
    );

    let polling_code = &program.code()[program.polling_entry_offset..];
    for eliminated_copy in [[0x4c, 0x8b, 0xe8], [0x4c, 0x8b, 0xf0]] {
        assert!(
            !polling_code
                .windows(eliminated_copy.len())
                .any(|window| window == eliminated_copy),
            "structured result should be generated directly in its fixed register"
        );
    }
    for eliminated_forward in [[0x49, 0x8b, 0xd5], [0x49, 0x8b, 0xd6]] {
        assert!(
            !polling_code
                .windows(eliminated_forward.len())
                .any(|window| window == eliminated_forward),
            "fully represented fixed result should not be copied to RDX"
        );
    }
    for direct_affine in [
        [0x4f, 0x8d, 0x6c, 0x5b, 0x01],
        [0x4f, 0x8d, 0x6c, 0x9b, 0xfe],
        [0x4f, 0x8d, 0x74, 0x6d, 0x0b],
    ] {
        assert!(
            polling_code
                .windows(direct_affine.len())
                .any(|window| window == direct_affine),
            "expected fused affine arithmetic in its fixed publication register"
        );
    }
    for slot in [1_i32, 2_i32, 3_i32, 4_i32] {
        let mut rax_store = vec![0x48, 0x89, 0x87];
        rax_store.extend_from_slice(&(slot * 8).to_le_bytes());
        assert!(
            !polling_code
                .windows(rax_store.len())
                .any(|window| window == rax_store),
            "visible phi slot {slot} should publish from its fixed register"
        );
    }

    slots[1..=4].copy_from_slice(&[101, 102, 103, 104]);
    let empty = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(empty.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[1..=4], &[101, 102, 103, 104]);
}

#[test]
fn range_proven_direct_result_preserves_old_right_resident() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Slot(1),
        result: 2,
        destination: 1,
    };
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 1,
                post_result: None,
            },
            (1u64 << 1) | (1u64 << 2),
            1u64 << 1,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 1;
    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[2]), (4, 3, 3));

    let polling_code = &program.code()[program.polling_entry_offset..];
    assert!(
        polling_code
            .windows(3)
            .any(|window| window == [0x4d, 0x2b, 0xe8]),
        "subtract should write directly to R13"
    );
    assert!(
        !polling_code
            .windows(3)
            .any(|window| window == [0x4c, 0x8b, 0xe8]),
        "direct subtract should not copy RAX into R13"
    );
    assert!(
        !polling_code
            .windows(3)
            .any(|window| window == [0x49, 0x8b, 0xd5]),
        "dead local result should not be forwarded from R13 to RDX"
    );
}

#[test]
fn range_proven_direct_result_forwards_untracked_immediate_alias() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(0),
        result: 2,
        destination: 1,
    };
    operations[1] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Slot(2),
        result: 3,
    };
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 2,
                post_result: None,
            },
            (1u64 << 1) | (1u64 << 2) | (1u64 << 3),
            1u64 << 1,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 1;
    slots[2] = 99;
    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[2], slots[3]), (4, 7, 7, 7));

    let polling_code = &program.code()[program.polling_entry_offset..];
    assert!(
        polling_code
            .windows(3)
            .any(|window| window == [0x49, 0x8b, 0xd5]),
        "untracked result alias should be forwarded from R13 to RDX"
    );
}

#[test]
fn range_proven_resident_operands_feed_branch_and_rebank_fixed_arithmetic() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(1)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(2)),
        false_target: 2,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(2),
        result: 3,
        destination: 1,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(2),
        rhs: QuickLongOperand::Const(1),
        result: 4,
        destination: 2,
    };
    let publication_mask = (1u64 << 1) | (1u64 << 2);
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
    slots[1] = 1;
    slots[2] = 2;
    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[2]), (4, 7, 6));

    let polling_code = &program.code()[program.polling_entry_offset..];
    let initial_jge = polling_code
        .windows(2)
        .position(|window| window == [0x0f, 0x8d])
        .expect("polling entry should reject an empty range");
    let mut loop_offset = initial_jge + 6;
    while polling_code.get(loop_offset) == Some(&0x90) {
        loop_offset += 1;
    }
    assert_eq!(
        (program.polling_entry_offset + loop_offset) % X86_STRUCTURED_LOOP_ALIGNMENT,
        0,
        "structured polling loop should start on its cache-line boundary"
    );
    assert!(
        polling_code
            .windows(3)
            .any(|window| window == [0x4d, 0x3b, 0xee]),
        "branch should compare R13 and R14 directly"
    );
    assert!(
        polling_code
            .windows(3)
            .any(|window| window == [0x4d, 0x8b, 0xc6]),
        "fixed-to-fixed arithmetic should re-bank R14 through R8"
    );
    assert!(
        polling_code
            .windows(3)
            .any(|window| window == [0x4d, 0x03, 0xe8]),
        "re-banked add should write R13 from R8"
    );
}

#[test]
fn range_proven_resident_rhs_feeds_scratch_result_directly() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Slot(1),
        result: 2,
    };
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 1,
                post_result: None,
            },
            (1u64 << 1) | (1u64 << 2),
            1u64 << 1,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 5;
    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[2]), (4, 5, 8));

    let polling_code = &program.code()[program.polling_entry_offset..];
    assert!(
        polling_code
            .windows(3)
            .any(|window| window == [0x49, 0x03, 0xc5]),
        "scratch result should consume resident R13 directly"
    );
    assert!(
        !polling_code
            .windows(3)
            .any(|window| window == [0x4d, 0x8b, 0xc5]),
        "resident R13 should not be copied into R8 for an RAX result"
    );
}

#[test]
fn range_proven_division_moves_latest_rdx_divisor_before_cqo() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(1),
        result: 2,
    };
    operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::IntDivide,
        lhs: QuickLongOperand::Const(100),
        rhs: QuickLongOperand::Slot(2),
        result: 3,
    };
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 2,
                post_result: None,
            },
            1u64 << 3,
            0,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[3]), (4, 25));

    let polling_code = &program.code()[program.polling_entry_offset..];
    assert!(
        polling_code
            .windows(5)
            .any(|window| window == [0x4c, 0x8b, 0xc2, 0x48, 0x99]),
        "RDX divisor must move to R8 immediately before CQO"
    );
}

#[test]
fn structured_phi_rejects_nonlocal_read_before_merge() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        false_target: 3,
    };
    operations[1] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Const(1),
        result: 1,
    };
    operations[2] = NativeStraightLongOperation::Jump { target: 4 };
    operations[3] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Const(9),
        result: 5,
    };
    operations[4] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Slot(1),
        result: 2,
    };
    operations[5] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Const(2),
        result: 1,
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(4),
        operations,
        operation_count: 6,
        post_result: None,
    };
    let block_starts = straight_long_structured_block_starts(&config);
    let (definitely_written_before, definitely_written_exit) =
        straight_long_structured_definitely_written(&config);
    assert_ne!(definitely_written_exit & (1u64 << 1), 0);
    assert!(!structured_phi_candidate_is_safe(
        &config,
        1u64 << 1,
        &block_starts,
        &definitely_written_before,
    ));
}
