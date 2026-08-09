use super::*;
use crate::jit::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS;
#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
#[test]
fn resident_scalar_operand_returns_its_register_without_move_or_shadow_load() {
    let mut forwarded = super::super::Arm64Assembler::new();
    let forwarded_register = super::super::emit_straight_long_operand_with_resident(
        &mut forwarded,
        QuickLongOperand::Slot(2),
        super::super::Arm64Register::from_code(6),
        0,
        super::super::Arm64Register::from_code(3),
        &[(1u64 << 2, super::super::Arm64Register::from_code(8))],
    );
    assert_eq!(
        forwarded_register,
        super::super::Arm64Register::from_code(8)
    );
    assert!(forwarded.finish().is_empty());

    let mut already_in_destination = super::super::Arm64Assembler::new();
    let already_resident = super::super::emit_straight_long_operand_with_resident(
        &mut already_in_destination,
        QuickLongOperand::Slot(2),
        super::super::Arm64Register::from_code(8),
        0,
        super::super::Arm64Register::from_code(3),
        &[(1u64 << 2, super::super::Arm64Register::from_code(8))],
    );
    assert_eq!(already_resident, super::super::Arm64Register::from_code(8));
    assert!(already_in_destination.finish().is_empty());

    let mut shadow_load = super::super::Arm64Assembler::new();
    let loaded_register = super::super::emit_straight_long_operand_with_resident(
        &mut shadow_load,
        QuickLongOperand::Slot(2),
        super::super::Arm64Register::from_code(6),
        0,
        super::super::Arm64Register::from_code(3),
        &[(0, super::super::Arm64Register::from_code(8))],
    );
    assert_eq!(loaded_register, super::super::Arm64Register::from_code(6));
    assert_eq!(shadow_load.finish(), 0xf940_0806u32.to_le_bytes());
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
#[test]
fn signed_small_constants_select_exact_add_sub_immediate_forms() {
    use crate::vm::function::ScalarLongOpKind::{Add, Multiply, Subtract};

    assert_eq!(
        super::super::straight_binary_add_sub_immediate(Add, 11),
        Some((true, 11))
    );
    assert_eq!(
        super::super::straight_binary_add_sub_immediate(Add, -11),
        Some((false, 11))
    );
    assert_eq!(
        super::super::straight_binary_add_sub_immediate(Subtract, 11),
        Some((false, 11))
    );
    assert_eq!(
        super::super::straight_binary_add_sub_immediate(Subtract, -11),
        Some((true, 11))
    );
    assert_eq!(
        super::super::straight_binary_add_sub_immediate(Add, 4_095),
        Some((true, 4_095))
    );
    assert_eq!(
        super::super::straight_binary_add_sub_immediate(Add, 4_096),
        None
    );
    assert_eq!(
        super::super::straight_binary_add_sub_immediate(Add, i64::MIN),
        None
    );
    assert_eq!(
        super::super::straight_binary_add_sub_immediate(Multiply, 11),
        None
    );

    assert_eq!(super::super::straight_multiply_shift_add(3), Some(1));
    assert_eq!(super::super::straight_multiply_shift_add(5), Some(2));
    assert_eq!(super::super::straight_multiply_shift_add(9), Some(3));
    assert_eq!(super::super::straight_multiply_shift_add(17), Some(4));
    assert_eq!(super::super::straight_multiply_shift_add(1), None);
    assert_eq!(super::super::straight_multiply_shift_add(7), None);
    assert_eq!(super::super::straight_multiply_shift_add(-3), None);

    assert_eq!(
        super::super::straight_binary_lowering_operands(
            Add,
            QuickLongOperand::Const(11),
            QuickLongOperand::Slot(2),
        ),
        (QuickLongOperand::Slot(2), QuickLongOperand::Const(11))
    );
    assert_eq!(
        super::super::straight_binary_lowering_operands(
            Multiply,
            QuickLongOperand::Const(3),
            QuickLongOperand::Slot(2),
        ),
        (QuickLongOperand::Slot(2), QuickLongOperand::Const(3))
    );
    assert_eq!(
        super::super::straight_binary_lowering_operands(
            Subtract,
            QuickLongOperand::Const(11),
            QuickLongOperand::Slot(2),
        ),
        (QuickLongOperand::Const(11), QuickLongOperand::Slot(2))
    );
}

fn config(operations: &[NativeStraightLongOperation], bound: i64) -> NativeStraightLongLoopConfig {
    let mut body = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    body[..operations.len()].copy_from_slice(operations);
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations: body,
        operation_count: operations.len() as u8,
        post_result: None,
    }
}

#[test]
fn proves_only_guards_whose_expected_edge_covers_the_complete_range() {
    let guarded = |needle| {
        config(
            &[
                NativeStraightLongOperation::BinaryAssign {
                    kind: ScalarLongOpKind::Add,
                    lhs: QuickLongOperand::Slot(1),
                    rhs: QuickLongOperand::Slot(0),
                    result: 2,
                    destination: 1,
                },
                NativeStraightLongOperation::Guard {
                    kind: ScalarLongConditionKind::Equal,
                    lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
                    rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(
                        needle,
                    )),
                    expected: false,
                },
            ],
            100,
        )
    };
    let slots = [0_i64; 64];

    let proof = straight_long_remaining_range_proof(&guarded(-1), &slots)
        .expect("disjoint guard should be valid over the complete range");
    assert_eq!(proof.carried_mask, 1u64 << 1);
    assert!(straight_long_remaining_range_proof(&guarded(100), &slots).is_some());
    assert!(straight_long_remaining_range_proof(&guarded(73), &slots).is_none());
}

#[test]
fn proves_composed_affine_and_modulo_ranges() {
    let config = config(
        &[
            NativeStraightLongOperation::Modulo {
                value: QuickLongOperand::Slot(0),
                divisor: 400,
                result: 2,
            },
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Const(20),
                rhs: QuickLongOperand::Slot(2),
                result: 3,
                destination: 4,
            },
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Multiply,
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(73),
                result: 5,
                destination: 6,
            },
        ],
        10_000_000,
    );
    let slots = [0_i64; 64];
    assert!(straight_long_remaining_range_is_proven(&config, &slots));
}

#[test]
fn proves_forward_branches_and_merges_definitely_written_ranges() {
    let config = config(
        &[
            NativeStraightLongOperation::BranchUnless {
                kind: super::super::ScalarLongConditionKind::LessThan,
                lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
                rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(4)),
                false_target: 3,
            },
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Multiply,
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(3),
                result: 2,
            },
            NativeStraightLongOperation::Jump { target: 4 },
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(7),
                result: 2,
            },
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Multiply,
                lhs: QuickLongOperand::Slot(2),
                rhs: QuickLongOperand::Const(2),
                result: 3,
                destination: 4,
            },
        ],
        10,
    );
    assert!(straight_long_remaining_range_is_proven(
        &config,
        &[0_i64; 64]
    ));
}

#[test]
fn rejects_overflow_division_and_unsupported_loop_carried_values() {
    let multiply = config(
        &[NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(i64::MAX),
            result: 1,
        }],
        3,
    );
    assert!(!straight_long_remaining_range_is_proven(
        &multiply,
        &[0_i64; 64]
    ));

    let divide = config(
        &[NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::IntDivide,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Slot(2),
            result: 1,
        }],
        i64::MIN + 1,
    );
    let mut slots = [0_i64; 64];
    slots[0] = i64::MIN;
    slots[2] = -1;
    assert!(!straight_long_remaining_range_is_proven(&divide, &slots));

    let carried = config(
        &[NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(0),
            result: 2,
            destination: 1,
        }],
        100,
    );
    let carried_proof = straight_long_remaining_range_proof(&carried, &[0_i64; 64])
        .expect("safe direct recurrence should be proven");
    assert_eq!(carried_proof.carried_mask, 1u64 << 1);

    let mut overflowing_slots = [0_i64; 64];
    overflowing_slots[1] = i64::MAX - 10;
    assert!(!straight_long_remaining_range_is_proven(
        &carried,
        &overflowing_slots
    ));

    let unsupported = config(
        &[NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(2),
            result: 2,
            destination: 1,
        }],
        10,
    );
    assert!(!straight_long_remaining_range_is_proven(
        &unsupported,
        &[0_i64; 64]
    ));

    let partially_written = config(
        &[
            NativeStraightLongOperation::BranchUnless {
                kind: super::super::ScalarLongConditionKind::LessThan,
                lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
                rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(50)),
                false_target: 2,
            },
            NativeStraightLongOperation::Move {
                source: QuickLongOperand::Slot(0),
                result: 2,
            },
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(2),
                rhs: QuickLongOperand::Const(1),
                result: 3,
            },
        ],
        100,
    );
    assert!(!straight_long_remaining_range_is_proven(
        &partially_written,
        &[0_i64; 64]
    ));
}

#[test]
fn direct_recurrence_proof_never_accepts_an_overflowing_prefix() {
    for start in [-100_i64, -3, 0, 7] {
        for distance in [1_i64, 2, 17, 101] {
            let bound = start + distance;
            for initial in [i64::MIN + 1_000, -100, 0, 100, i64::MAX - 1_000] {
                for step in [-13_i64, -1, 0, 1, 11] {
                    let config = config(
                        &[
                            NativeStraightLongOperation::BinaryAssign {
                                kind: ScalarLongOpKind::Add,
                                lhs: QuickLongOperand::Slot(1),
                                rhs: QuickLongOperand::Slot(0),
                                result: 2,
                                destination: 1,
                            },
                            NativeStraightLongOperation::BinaryAssign {
                                kind: ScalarLongOpKind::Subtract,
                                lhs: QuickLongOperand::Slot(3),
                                rhs: QuickLongOperand::Slot(5),
                                result: 4,
                                destination: 3,
                            },
                        ],
                        bound,
                    );
                    let mut slots = [0_i64; 64];
                    slots[0] = start;
                    slots[1] = initial;
                    slots[3] = initial;
                    slots[5] = step;

                    let proven = straight_long_remaining_range_proof(&config, &slots);
                    let mut first = initial;
                    let mut second = initial;
                    let mut safe = true;
                    for induction in start..bound {
                        let Some(next_first) = first.checked_add(induction) else {
                            safe = false;
                            break;
                        };
                        let Some(next_second) = second.checked_sub(step) else {
                            safe = false;
                            break;
                        };
                        first = next_first;
                        second = next_second;
                    }
                    assert!(proven.is_none() || safe);
                    if let Some(proof) = proven {
                        assert_eq!(proof.carried_mask, (1u64 << 1) | (1u64 << 3));
                    }
                }
            }
        }
    }
}

#[test]
fn dependent_recurrence_proof_never_accepts_an_overflowing_prefix() {
    for distance in [1_i64, 2, 17, 101] {
        for initial_first in [i64::MIN + 1_000, -100, 0, 100, i64::MAX - 1_000] {
            for initial_second in [i64::MIN + 1_000, -100, 0, 100, i64::MAX - 1_000] {
                for step in [-13_i64, -1, 0, 1, 11] {
                    for reverse_order in [false, true] {
                        let update_first = NativeStraightLongOperation::BinaryAssign {
                            kind: ScalarLongOpKind::Add,
                            lhs: QuickLongOperand::Slot(1),
                            rhs: QuickLongOperand::Slot(5),
                            result: 2,
                            destination: 1,
                        };
                        let update_second = NativeStraightLongOperation::BinaryAssign {
                            kind: ScalarLongOpKind::Add,
                            lhs: QuickLongOperand::Slot(3),
                            rhs: QuickLongOperand::Slot(1),
                            result: 4,
                            destination: 3,
                        };
                        let operations = if reverse_order {
                            [update_second, update_first]
                        } else {
                            [update_first, update_second]
                        };
                        let config = config(&operations, distance);
                        let mut slots = [0_i64; 64];
                        slots[1] = initial_first;
                        slots[3] = initial_second;
                        slots[5] = step;

                        let proven = straight_long_remaining_range_proof(&config, &slots);
                        let mut first = initial_first;
                        let mut second = initial_second;
                        let mut safe = true;
                        for _ in 0..distance {
                            if reverse_order {
                                let Some(next_second) = second.checked_add(first) else {
                                    safe = false;
                                    break;
                                };
                                second = next_second;
                            }
                            let Some(next_first) = first.checked_add(step) else {
                                safe = false;
                                break;
                            };
                            first = next_first;
                            if !reverse_order {
                                let Some(next_second) = second.checked_add(first) else {
                                    safe = false;
                                    break;
                                };
                                second = next_second;
                            }
                        }
                        assert!(proven.is_none() || safe);
                    }
                }
            }
        }
    }
}

#[test]
fn composed_and_acyclic_dependent_recurrences_are_proven() {
    let composed = config(
        &[
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Multiply,
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(3),
                result: 6,
            },
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(6),
                rhs: QuickLongOperand::Slot(5),
                result: 7,
            },
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(1),
                rhs: QuickLongOperand::Slot(7),
                result: 2,
                destination: 1,
            },
        ],
        100,
    );
    let mut slots = [0_i64; 64];
    slots[1] = 10;
    slots[5] = 7;
    let proof = straight_long_remaining_range_proof(&composed, &slots)
        .expect("acyclic scalar delta should be proven");
    assert_eq!(proof.carried_mask, 1u64 << 1);

    let overflowing_delta = config(
        &[
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Multiply,
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(i64::MAX),
                result: 6,
            },
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(1),
                rhs: QuickLongOperand::Slot(6),
                result: 2,
                destination: 1,
            },
        ],
        3,
    );
    assert!(straight_long_remaining_range_proof(&overflowing_delta, &[0_i64; 64]).is_none());

    let dependent = config(
        &[
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(1),
                rhs: QuickLongOperand::Slot(0),
                result: 2,
                destination: 1,
            },
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(3),
                rhs: QuickLongOperand::Slot(1),
                result: 4,
                destination: 3,
            },
        ],
        100,
    );
    let dependent_proof = straight_long_remaining_range_proof(&dependent, &[0_i64; 64])
        .expect("earlier updated recurrence should be available to a later one");
    assert_eq!(dependent_proof.carried_mask, (1u64 << 1) | (1u64 << 3));

    let reverse_dependency = config(&[dependent.operations[1], dependent.operations[0]], 100);
    let reverse_proof = straight_long_remaining_range_proof(&reverse_dependency, &[0_i64; 64])
        .expect("acyclic reverse-order dependency should be proven topologically");
    assert_eq!(reverse_proof.carried_mask, (1u64 << 1) | (1u64 << 3));

    let cyclic = config(
        &[
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(1),
                rhs: QuickLongOperand::Slot(3),
                result: 2,
                destination: 1,
            },
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(3),
                rhs: QuickLongOperand::Slot(1),
                result: 4,
                destination: 3,
            },
        ],
        100,
    );
    assert!(straight_long_remaining_range_proof(&cyclic, &[0_i64; 64]).is_none());
}

#[test]
fn conditional_recurrence_proves_induction_and_carried_guards() {
    let conditional = config(
        &[
            NativeStraightLongOperation::BranchUnless {
                kind: super::super::ScalarLongConditionKind::LessThan,
                lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
                rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(50)),
                false_target: 2,
            },
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(1),
                rhs: QuickLongOperand::Slot(0),
                result: 2,
                destination: 1,
            },
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(3),
                rhs: QuickLongOperand::Const(1),
                result: 4,
                destination: 3,
            },
        ],
        100,
    );
    let proof = straight_long_remaining_range_proof(&conditional, &[0_i64; 64])
        .expect("induction-guarded recurrences should be proven");
    assert_eq!(proof.carried_mask, (1u64 << 1) | (1u64 << 3));

    let carried_guard = config(
        &[
            NativeStraightLongOperation::BranchUnless {
                kind: super::super::ScalarLongConditionKind::LessThan,
                lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(1)),
                rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(50)),
                false_target: 2,
            },
            conditional.operations[1],
            conditional.operations[2],
        ],
        100,
    );
    let carried_guard_proof = straight_long_remaining_range_proof(&carried_guard, &[0_i64; 64])
        .expect("resident carried state should be available to branch conditions");
    assert_eq!(carried_guard_proof.carried_mask, proof.carried_mask);

    let dominated_delta = config(
        &[
            NativeStraightLongOperation::BranchUnless {
                kind: super::super::ScalarLongConditionKind::LessThan,
                lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
                rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(50)),
                false_target: 4,
            },
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Multiply,
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(3),
                result: 6,
            },
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(6),
                rhs: QuickLongOperand::Slot(5),
                result: 7,
            },
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(1),
                rhs: QuickLongOperand::Slot(7),
                result: 2,
                destination: 1,
            },
            conditional.operations[2],
        ],
        100,
    );
    let mut dominated_slots = [0_i64; 64];
    dominated_slots[1] = 10;
    dominated_slots[5] = 7;
    let dominated_proof = straight_long_remaining_range_proof(&dominated_delta, &dominated_slots)
        .expect("branch-dominated scalar delta should be proven");
    assert_eq!(dominated_proof.carried_mask, proof.carried_mask);

    let mut bypassed_delta = dominated_delta;
    bypassed_delta.operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: super::super::ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(50)),
        false_target: 3,
    };
    assert!(straight_long_remaining_range_proof(&bypassed_delta, &dominated_slots).is_none());
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
#[test]
fn proven_structured_program_polls_and_completes_exactly() {
    let config = config(
        &[
            NativeStraightLongOperation::BranchUnless {
                kind: super::super::ScalarLongConditionKind::Equal,
                lhs: NativeStraightLongConditionOperand::BitwiseAnd {
                    lhs: QuickLongOperand::Slot(0),
                    rhs: QuickLongOperand::Const(1),
                },
                rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(0)),
                false_target: 3,
            },
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Multiply,
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(3),
                result: 2,
            },
            NativeStraightLongOperation::Jump { target: 4 },
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(7),
                result: 2,
            },
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Multiply,
                lhs: QuickLongOperand::Slot(2),
                rhs: QuickLongOperand::Const(2),
                result: 3,
                destination: 4,
            },
        ],
        10_000,
    );
    let program =
        super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling(config, 1_024)
            .unwrap();
    let interrupt = AtomicBool::new(false);
    let mut slots = [0_i64; 64];

    let completed = program
        .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 1_024)
        .unwrap();
    assert_eq!(
        completed.outcome,
        super::super::NativeStraightLongLoopOutcome::Completed
    );
    assert_eq!(slots[0], 10_000);
    assert_eq!(slots[2], 10_006);
    assert_eq!(slots[4], 20_012);
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
#[test]
fn proven_straight_program_polls_and_resumes_at_exact_boundaries() {
    let config = config(
        &[
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Multiply,
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(73),
                result: 2,
                destination: 3,
            },
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(5),
                result: 4,
                destination: 5,
            },
        ],
        10_000,
    );
    let program =
        super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling(config, 1_024)
            .unwrap();
    let interrupt = AtomicBool::new(true);
    let mut slots = [0_i64; 64];

    let first = program
        .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 1_024)
        .unwrap();
    assert_eq!(
        first.outcome,
        super::super::NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!(slots[0], 1_024);
    assert_eq!(slots[3], 1_023 * 73);
    assert_eq!(slots[5], 1_028);

    interrupt.store(false, Ordering::Relaxed);
    let completed = program
        .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 2_048)
        .unwrap();
    assert_eq!(
        completed.outcome,
        super::super::NativeStraightLongLoopOutcome::Completed
    );
    assert_eq!(slots[0], 10_000);
    assert_eq!(slots[3], 9_999 * 73);
    assert_eq!(slots[5], 10_004);
}

#[test]
fn interval_transfers_cover_checked_edge_samples() {
    let intervals = [
        LongInterval::exact(i64::MIN),
        LongInterval {
            minimum: i128::from(i64::MIN),
            maximum: i128::from(i64::MIN + 3),
        },
        LongInterval {
            minimum: -100,
            maximum: 100,
        },
        LongInterval {
            minimum: -10,
            maximum: -1,
        },
        LongInterval {
            minimum: 0,
            maximum: 10,
        },
        LongInterval {
            minimum: 1,
            maximum: 10,
        },
        LongInterval {
            minimum: i128::from(i64::MAX - 3),
            maximum: i128::from(i64::MAX),
        },
        LongInterval::FULL,
    ];
    let kinds = [
        ScalarLongOpKind::Add,
        ScalarLongOpKind::Subtract,
        ScalarLongOpKind::Compare,
        ScalarLongOpKind::Multiply,
        ScalarLongOpKind::IntDivide,
        ScalarLongOpKind::Modulo,
        ScalarLongOpKind::BitwiseAnd,
        ScalarLongOpKind::BitwiseOr,
        ScalarLongOpKind::BitwiseXor,
    ];

    for kind in kinds {
        for lhs in intervals {
            for rhs in intervals {
                let Some(result_range) = binary_interval(kind, lhs, rhs) else {
                    continue;
                };
                for left in interval_candidates(lhs).into_iter().flatten() {
                    for right in interval_candidates(rhs).into_iter().flatten() {
                        let result = match kind {
                            ScalarLongOpKind::Add => (left as i64).checked_add(right as i64),
                            ScalarLongOpKind::Subtract => (left as i64).checked_sub(right as i64),
                            ScalarLongOpKind::Compare => {
                                Some(match (left as i64).cmp(&(right as i64)) {
                                    std::cmp::Ordering::Less => -1,
                                    std::cmp::Ordering::Equal => 0,
                                    std::cmp::Ordering::Greater => 1,
                                })
                            }
                            ScalarLongOpKind::Multiply => (left as i64).checked_mul(right as i64),
                            ScalarLongOpKind::IntDivide => (left as i64).checked_div(right as i64),
                            ScalarLongOpKind::Modulo => (left as i64).checked_rem(right as i64),
                            ScalarLongOpKind::BitwiseAnd => Some((left as i64) & (right as i64)),
                            ScalarLongOpKind::BitwiseOr => Some((left as i64) | (right as i64)),
                            ScalarLongOpKind::BitwiseXor => Some((left as i64) ^ (right as i64)),
                        }
                        .expect("accepted interval must exclude checked side exits");
                        assert!(result_range.contains(i128::from(result)));
                    }
                }
            }
        }
    }
}
