use super::{
    NativeStraightLongConditionOperand, NativeStraightLongLoopConfig, NativeStraightLongOperation,
    QuickLongOperand, NATIVE_STRAIGHT_LONG_MAX_OPERATIONS,
    straight_long_best_invariant_slot_masks, straight_long_operation_input_mask,
};

pub(crate) fn straight_long_linear_live_after(
    config: &NativeStraightLongLoopConfig,
) -> [u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS] {
    let mut live_after = [0u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut live = 0u64;
    for index in (0..config.operation_count as usize).rev() {
        let operation = config.operations[index];
        live_after[index] = live;
        live = (live & !operation.output_mask()) | straight_long_operation_input_mask(operation);
    }
    live_after
}

pub(crate) fn straight_long_linear_shadow_store_mask(
    config: &NativeStraightLongLoopConfig,
    operation_index: usize,
    publication_mask: u64,
    live_after: &[u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
) -> u64 {
    let operation = config.operations[operation_index];
    let survives_immediate_consumer = if operation_index + 1 < config.operation_count as usize {
        let next = config.operations[operation_index + 1];
        live_after[operation_index + 1] & !next.output_mask()
    } else {
        0
    };
    operation.output_mask() & (publication_mask | survives_immediate_consumer)
}

pub(crate) fn straight_long_linear_final_publication_masks(
    config: &NativeStraightLongLoopConfig,
    publication_mask: u64,
) -> [u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS] {
    let mut final_masks = [0u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut later_outputs = 0u64;
    for index in (0..config.operation_count as usize).rev() {
        let output_mask = config.operations[index].output_mask();
        final_masks[index] = output_mask & publication_mask & !later_outputs;
        later_outputs |= output_mask;
    }
    final_masks
}

/// Find the earliest normal-body operation before which a range-proven
/// backend may increment the loop induction value. Every complete forward path
/// must cross the insertion point exactly once, the remaining suffix must be
/// pure scalar code, and that suffix must not observe the old induction value.
///
/// Backends still decide which unchecked entry can use this schedule. Checked
/// and guard-bearing suffixes retain the canonical tail increment so a side
/// exit always publishes the failing iteration.
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64", test))]
pub(crate) fn straight_long_early_induction_increment_operation(
    config: &NativeStraightLongLoopConfig,
) -> Option<usize> {
    if config.post_result.is_some() {
        return None;
    }
    let operation_count = config.operation_count as usize;
    let induction_mask = 1u64 << config.induction_slot;
    (1..operation_count).find(|&candidate| {
        config.operations[candidate..operation_count]
            .iter()
            .copied()
            .all(|operation| {
                matches!(
                    operation,
                    NativeStraightLongOperation::Modulo { .. }
                        | NativeStraightLongOperation::Move { .. }
                        | NativeStraightLongOperation::Binary { .. }
                        | NativeStraightLongOperation::BinaryAssign { .. }
                ) && straight_long_operation_input_mask(operation) & induction_mask == 0
            })
            && straight_long_operation_dominates_exit(config, candidate)
    })
}

#[cfg(any(target_arch = "aarch64", target_arch = "x86_64", test))]
fn straight_long_operation_dominates_exit(
    config: &NativeStraightLongLoopConfig,
    candidate: usize,
) -> bool {
    let operation_count = config.operation_count as usize;
    let mut reachable_without_candidate =
        [false; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1];
    reachable_without_candidate[0] = true;
    for index in 0..operation_count {
        if !reachable_without_candidate[index] || index == candidate {
            continue;
        }
        match config.operations[index] {
            NativeStraightLongOperation::BranchUnless { false_target, .. } => {
                reachable_without_candidate[index + 1] = true;
                reachable_without_candidate[false_target as usize] = true;
            }
            NativeStraightLongOperation::Jump { target } => {
                reachable_without_candidate[target as usize] = true;
            }
            _ => reachable_without_candidate[index + 1] = true,
        }
    }
    !reachable_without_candidate[operation_count]
}

pub(crate) fn straight_long_structured_block_starts(
    config: &NativeStraightLongLoopConfig,
) -> [bool; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1] {
    let mut starts = [false; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1];
    starts[0] = true;
    let operation_count = config.operation_count as usize;
    for (index, operation) in config.operations[..operation_count]
        .iter()
        .copied()
        .enumerate()
    {
        match operation {
            NativeStraightLongOperation::BranchUnless { false_target, .. } => {
                starts[index + 1] = true;
                starts[false_target as usize] = true;
            }
            NativeStraightLongOperation::Jump { target } => {
                starts[index + 1] = true;
                starts[target as usize] = true;
            }
            _ => {}
        }
    }
    starts
}

/// Computes the slots that have definitely been written on every forward path
/// reaching each operation and the loop-body exit. Structured straight loops
/// are validated to contain only forward edges, so a single index-ordered pass
/// reaches the fixed point: the first predecessor initializes a target and
/// later predecessors intersect their facts with it.
pub(crate) fn straight_long_structured_definitely_written(
    config: &NativeStraightLongLoopConfig,
) -> ([u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS], u64) {
    let operation_count = config.operation_count as usize;
    let mut before = [0u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut incoming = [0u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1];
    let mut reachable = [false; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1];
    reachable[0] = true;

    fn merge_forward_fact(
        incoming: &mut [u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1],
        reachable: &mut [bool; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1],
        target: usize,
        fact: u64,
    ) {
        if reachable[target] {
            incoming[target] &= fact;
        } else {
            incoming[target] = fact;
            reachable[target] = true;
        }
    }

    for index in 0..operation_count {
        if !reachable[index] {
            continue;
        }
        before[index] = incoming[index];
        let after = incoming[index] | config.operations[index].output_mask();
        match config.operations[index] {
            NativeStraightLongOperation::BranchUnless { false_target, .. } => {
                merge_forward_fact(&mut incoming, &mut reachable, index + 1, after);
                merge_forward_fact(
                    &mut incoming,
                    &mut reachable,
                    false_target as usize,
                    after,
                );
            }
            NativeStraightLongOperation::Jump { target } => {
                merge_forward_fact(&mut incoming, &mut reachable, target as usize, after);
            }
            _ => {
                merge_forward_fact(&mut incoming, &mut reachable, index + 1, after);
            }
        }
    }

    let exit = reachable[operation_count]
        .then_some(incoming[operation_count])
        .unwrap_or(0);
    (before, exit)
}

/// Conservatively mark operations whose result transitively contributes to a
/// loop-carried slot. Keeping the dependent slot set monotonic admits all
/// branch definitions and intentionally over-approximates reused shadow slots.
pub(crate) fn straight_long_carried_dependency_operations(
    config: &NativeStraightLongLoopConfig,
    carried_mask: u64,
) -> u64 {
    let mut dependent_slots = carried_mask;
    let mut dependent_operations = 0u64;
    loop {
        let before_slots = dependent_slots;
        let before_operations = dependent_operations;
        for (index, operation) in config.operations[..config.operation_count as usize]
            .iter()
            .copied()
            .enumerate()
            .rev()
        {
            if operation.output_mask() & dependent_slots == 0 {
                continue;
            }
            dependent_operations |= 1u64 << index;
            dependent_slots |= straight_long_operation_input_mask(operation);
        }
        if dependent_slots == before_slots && dependent_operations == before_operations {
            return dependent_operations;
        }
    }
}

pub(crate) fn straight_long_structured_local_resident_output_masks(
    config: &NativeStraightLongLoopConfig,
    publication_mask: u64,
    carried_mask: u64,
    block_starts: &[bool; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1],
) -> [u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS] {
    let operation_count = config.operation_count as usize;
    let mut resident_masks = [0u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    for producer in 0..operation_count {
        let mut candidates = config.operations[producer].shadow_output_mask()
            & !publication_mask
            & !carried_mask;
        while candidates != 0 {
            let slot_mask = 1u64 << candidates.trailing_zeros();
            candidates &= candidates - 1;
            let mut local = true;
            for consumer in producer + 1..operation_count {
                if straight_long_operation_input_mask(config.operations[consumer]) & slot_mask != 0
                    && (consumer != producer + 1 || block_starts[consumer])
                {
                    local = false;
                    break;
                }
                if config.operations[consumer].output_mask() & slot_mask != 0 {
                    break;
                }
            }
            if local {
                resident_masks[producer] |= slot_mask;
            }
        }
    }
    resident_masks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::function::ScalarLongOpKind;
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn linear_liveness_tracks_overlapping_values_and_kills_old_versions() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(3),
            result: 2,
        };
        operations[1] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(2),
            rhs: QuickLongOperand::Const(7),
            result: 3,
        };
        operations[2] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(2),
            rhs: QuickLongOperand::Slot(3),
            result: 2,
        };
        operations[3] = NativeStraightLongOperation::Move {
            source: QuickLongOperand::Slot(2),
            result: 4,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10),
            operations,
            operation_count: 4,
            post_result: None,
        };

        let live_after = straight_long_linear_live_after(&config);
        assert_eq!(live_after[0], (1u64 << 2));
        assert_eq!(live_after[1], (1u64 << 2) | (1u64 << 3));
        assert_eq!(live_after[2], 1u64 << 2);
        assert_eq!(live_after[3], 0);
        assert_eq!(
            straight_long_linear_shadow_store_mask(&config, 0, 0, &live_after),
            1u64 << 2
        );
        assert_eq!(
            straight_long_linear_shadow_store_mask(&config, 1, 0, &live_after),
            0
        );
        assert_eq!(
            straight_long_linear_shadow_store_mask(&config, 2, 1u64 << 2, &live_after),
            1u64 << 2
        );
        let final_publications =
            straight_long_linear_final_publication_masks(&config, (1u64 << 2) | (1u64 << 4));
        assert_eq!(final_publications[0], 0);
        assert_eq!(final_publications[2], 1u64 << 2);
        assert_eq!(final_publications[3], 1u64 << 4);
    }

    #[test]
    fn early_induction_increment_requires_a_pure_dominating_suffix() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(90)),
            false_target: 4,
        };
        operations[1] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(3),
            result: 3,
        };
        operations[2] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(3),
            rhs: QuickLongOperand::Const(1),
            result: 4,
            destination: 1,
        };
        operations[3] = NativeStraightLongOperation::Jump { target: 6 };
        operations[4] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(5),
            result: 5,
        };
        operations[5] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(5),
            rhs: QuickLongOperand::Const(2),
            result: 6,
            destination: 1,
        };
        operations[6] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(3),
            result: 7,
            destination: 2,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(100),
            operations,
            operation_count: 7,
            post_result: None,
        };
        assert_eq!(straight_long_early_induction_increment_operation(&config), Some(6));

        let mut materializes_post_increment = config;
        materializes_post_increment.post_result = Some(8);
        assert_eq!(
            straight_long_early_induction_increment_operation(&materializes_post_increment),
            None
        );

        let mut suffix_reads_induction = config;
        suffix_reads_induction.operations[6] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(3),
            result: 7,
            destination: 2,
        };
        assert_eq!(
            straight_long_early_induction_increment_operation(&suffix_reads_induction),
            None
        );

        let mut suffix_has_side_exit = config;
        suffix_has_side_exit.operations[6] = NativeStraightLongOperation::Guard {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(1)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(1_000)),
            expected: true,
        };
        assert_eq!(
            straight_long_early_induction_increment_operation(&suffix_has_side_exit),
            None
        );

        let mut bypasses_join = config;
        bypasses_join.operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(90)),
            false_target: 7,
        };
        assert_eq!(
            straight_long_early_induction_increment_operation(&bypasses_join),
            None
        );
    }

    #[test]
    fn structured_local_residency_stops_at_branch_boundaries() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(50)),
            false_target: 4,
        };
        operations[1] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(3),
            result: 6,
        };
        operations[2] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(6),
            rhs: QuickLongOperand::Const(7),
            result: 7,
        };
        operations[3] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(7),
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
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(100),
            operations,
            operation_count: 5,
            post_result: None,
        };
        let block_starts = straight_long_structured_block_starts(&config);
        assert!(block_starts[0]);
        assert!(block_starts[1]);
        assert!(block_starts[4]);
        assert!(!block_starts[2]);
        assert!(!block_starts[3]);

        let publication_mask = (1u64 << 1) | (1u64 << 3);
        let resident_masks = straight_long_structured_local_resident_output_masks(
            &config,
            publication_mask,
            publication_mask,
            &block_starts,
        );
        assert_eq!(resident_masks[1], 1u64 << 6);
        assert_eq!(resident_masks[2], 1u64 << 7);
        assert_eq!(resident_masks[3], 1u64 << 2);
        assert_eq!(resident_masks[4], 1u64 << 4);

        let mut bypassed = config;
        bypassed.operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(50)),
            false_target: 2,
        };
        let bypassed_starts = straight_long_structured_block_starts(&bypassed);
        let bypassed_masks = straight_long_structured_local_resident_output_masks(
            &bypassed,
            publication_mask,
            publication_mask,
            &bypassed_starts,
        );
        assert_eq!(bypassed_masks[1], 0);
    }

    #[test]
    fn structured_definite_writes_intersect_branch_predecessors() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(50)),
            false_target: 4,
        };
        operations[1] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(3),
            result: 6,
        };
        operations[2] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(6),
            rhs: QuickLongOperand::Const(1),
            result: 7,
            destination: 1,
        };
        operations[3] = NativeStraightLongOperation::Jump { target: 6 };
        operations[4] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(5),
            result: 8,
        };
        operations[5] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(8),
            rhs: QuickLongOperand::Const(2),
            result: 9,
            destination: 1,
        };
        operations[6] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(3),
            result: 10,
            destination: 2,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(100),
            operations,
            operation_count: 7,
            post_result: None,
        };

        let (before, exit) = straight_long_structured_definitely_written(&config);
        assert_eq!(before[1] & (1u64 << 1), 0);
        assert_eq!(before[4] & (1u64 << 1), 0);
        assert_ne!(before[6] & (1u64 << 1), 0);
        assert_ne!(exit & (1u64 << 1), 0);
        assert_ne!(exit & (1u64 << 2), 0);

        let mut partial = config;
        partial.operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(50)),
            false_target: 6,
        };
        let (partial_before, partial_exit) =
            straight_long_structured_definitely_written(&partial);
        assert_eq!(partial_before[6] & (1u64 << 1), 0);
        assert_eq!(partial_exit & (1u64 << 1), 0);
        assert_ne!(partial_exit & (1u64 << 2), 0);
    }

    #[test]
    fn carried_dependency_marks_only_transitive_recurrence_chain() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(3),
            result: 6,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(6),
            result: 7,
            destination: 1,
        };
        operations[2] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(5),
            result: 8,
            destination: 2,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(100),
            operations,
            operation_count: 3,
            post_result: None,
        };

        assert_eq!(
            straight_long_carried_dependency_operations(&config, 1u64 << 1),
            (1u64 << 0) | (1u64 << 1)
        );

        #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
        {
            let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication_and_carried(
                config,
                1_024,
                (1u64 << 1) | (1u64 << 2),
                1u64 << 1,
            )
            .unwrap();
            let words = program
                .code()
                .chunks_exact(4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                .collect::<Vec<_>>();
            assert!(words.contains(&0x9b07_7c68)); // recurrence chain keeps MUL x8, x3, x7
            assert!(words.contains(&0x8b03_0868)); // independent i * 5 uses shifted ADD
        }
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn overlapping_value_is_cached_and_published_from_its_fixed_register() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(3),
            result: 2,
        };
        operations[1] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(2),
            rhs: QuickLongOperand::Const(7),
            result: 3,
        };
        operations[2] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(2),
            rhs: QuickLongOperand::Slot(3),
            result: 4,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10),
            operations,
            operation_count: 3,
            post_result: None,
        };

        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling(
            config, 1_024,
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();

        assert!(words.contains(&0x8b03_0464)); // ADD x4, x3, x3, LSL #1
        assert!(!words.contains(&0xaa08_03e4)); // no MOV x4, x8
        assert!(!words.contains(&0xf900_0808)); // no loop STR x8, [x0, #16]
        assert!(words.contains(&0xf900_0804)); // exit STR x4, [x0, #16]
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn dead_temporary_store_is_omitted_but_visible_destination_is_published() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(5),
            result: 2,
            destination: 3,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10_000),
            operations,
            operation_count: 1,
            post_result: None,
        };

        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication(
            config,
            1_024,
            1u64 << 3,
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert!(!words.contains(&0xf900_0808)); // no STR x8, [x0, #16]
        assert!(words.contains(&0xf900_0c08)); // STR x8, [x0, #24]

        let interrupt = AtomicBool::new(true);
        let mut slots = [0i64; 64];
        slots[2] = 777;
        let interrupted = program
            .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 1_024)
            .unwrap();
        assert_eq!(
            interrupted.outcome,
            super::super::NativeStraightLongLoopOutcome::ChunkExhausted
        );
        assert_eq!(slots[2], 777);
        assert_eq!(slots[3], 1_028);

        interrupt.store(false, Ordering::Relaxed);
        let completed = program
            .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 2_048)
            .unwrap();
        assert_eq!(
            completed.outcome,
            super::super::NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!(slots[2], 777);
        assert_eq!(slots[3], 10_004);
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn immediate_consumer_uses_resident_temporary_without_shadow_store() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(5),
            result: 2,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(2),
            rhs: QuickLongOperand::Const(3),
            result: 3,
            destination: 4,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10),
            operations,
            operation_count: 2,
            post_result: None,
        };
        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication(
            config,
            1_024,
            1u64 << 4,
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();

        assert!(!words.contains(&0xf900_0808)); // no temporary slot 2 store
        assert!(!words.contains(&0xf900_0c08)); // no temporary slot 3 store
        assert!(words.contains(&0xf900_1008)); // visible slot 4 store

        let interrupt = AtomicBool::new(false);
        let mut slots = [0i64; 64];
        slots[2] = 777;
        slots[3] = 888;
        program
            .call_range_proven_polling(&mut slots, 10, interrupt.as_ptr() as *const bool, 10)
            .unwrap();
        assert_eq!(slots[2], 777);
        assert_eq!(slots[3], 888);
        assert_eq!(slots[4], 42);
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn four_visible_results_publish_from_fixed_registers_at_native_exits() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        for (index, (kind, constant)) in [
            (ScalarLongOpKind::Multiply, 3),
            (ScalarLongOpKind::Add, 7),
            (ScalarLongOpKind::Multiply, 5),
            (ScalarLongOpKind::Subtract, 2),
        ]
        .into_iter()
        .enumerate()
        {
            operations[index] = NativeStraightLongOperation::BinaryAssign {
                kind,
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(constant),
                result: 2 + index as u16,
                destination: 10 + index as u16,
            };
        }
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10_000),
            operations,
            operation_count: 4,
            post_result: None,
        };
        let publication_mask = (1u64 << 10) | (1u64 << 11) | (1u64 << 12) | (1u64 << 13);
        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication(
            config,
            1_024,
            publication_mask,
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert!(words.contains(&0x8b03_0464)); // ADD x4, x3, x3, LSL #1
        assert!(words.contains(&0x9100_1c65)); // ADD x5, x3, #7
        assert!(words.contains(&0x8b03_086b)); // ADD x11, x3, x3, LSL #2
        assert!(!words.contains(&0xaa08_03e4)); // no MOV x4, x8
        assert!(!words.contains(&0xaa08_03e5)); // no MOV x5, x8
        assert!(!words.contains(&0xaa08_03eb)); // no MOV x11, x8

        let interrupt = AtomicBool::new(true);
        let mut slots = [0i64; 64];
        let interrupted = program
            .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 1_024)
            .unwrap();
        assert_eq!(
            interrupted.outcome,
            super::super::NativeStraightLongLoopOutcome::ChunkExhausted
        );
        assert_eq!(&slots[10..14], &[3_069, 1_030, 5_115, 1_021]);

        interrupt.store(false, Ordering::Relaxed);
        let completed = program
            .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 2_048)
            .unwrap();
        assert_eq!(
            completed.outcome,
            super::super::NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!(&slots[10..14], &[29_997, 10_006, 49_995, 9_997]);
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn reverse_dependent_values_observe_old_fixed_register_state() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(3),
            rhs: QuickLongOperand::Slot(1),
            result: 4,
            destination: 3,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(0),
            result: 2,
            destination: 1,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10_000),
            operations,
            operation_count: 2,
            post_result: None,
        };
        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication_and_carried(
            config,
            1_024,
            (1u64 << 1) | (1u64 << 3),
            (1u64 << 1) | (1u64 << 3),
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert!(words.contains(&0x8b05_0084)); // ADD x4, x4, x5
        assert!(words.contains(&0x8b03_00a5)); // ADD x5, x5, x3
        assert!(!words.contains(&0xaa08_03e4)); // no MOV x4, x8
        assert!(!words.contains(&0xaa08_03e5)); // no MOV x5, x8

        let interrupt = AtomicBool::new(true);
        let mut slots = [0i64; 64];
        slots[1] = 10;
        slots[3] = -5;
        let interrupted = program
            .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 1_024)
            .unwrap();
        assert_eq!(
            interrupted.outcome,
            super::super::NativeStraightLongLoopOutcome::ChunkExhausted
        );
        assert_eq!(slots[0], 1_024);
        assert_eq!(slots[1], 523_786);
        assert_eq!(slots[3], 178_443_259);

        interrupt.store(false, Ordering::Relaxed);
        let completed = program
            .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 2_048)
            .unwrap();
        assert_eq!(
            completed.outcome,
            super::super::NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!(slots[0], 10_000);
        assert_eq!(slots[1], 49_995_010);
        assert_eq!(slots[3], 166_616_769_995);
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn conditional_recurrence_keeps_old_register_value_on_skipped_path() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(45)),
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
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10_000),
            operations,
            operation_count: 3,
            post_result: None,
        };
        let publication_mask = (1u64 << 1) | (1u64 << 3);
        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication_and_carried(
            config,
            1_024,
            publication_mask,
            publication_mask,
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert!(!words.contains(&0xaa08_03e4)); // carried update writes x4 directly
        assert!(!words.contains(&0xaa08_03e5)); // induction-derived update writes x5 directly
        assert!(!words.contains(&0xf900_0408)); // no loop STR x8, [x0, #8]
        assert!(!words.contains(&0xf900_0c08)); // no loop STR x8, [x0, #24]

        let interrupt = AtomicBool::new(true);
        let mut slots = [0i64; 64];
        slots[1] = 10;
        slots[3] = -5;
        let interrupted = program
            .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 1_024)
            .unwrap();
        assert_eq!(
            interrupted.outcome,
            super::super::NativeStraightLongLoopOutcome::ChunkExhausted
        );
        assert_eq!(slots[0], 1_024);
        assert_eq!(slots[1], 1_235);
        assert_eq!(slots[3], 1_019);

        interrupt.store(false, Ordering::Relaxed);
        let completed = program
            .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 2_048)
            .unwrap();
        assert_eq!(
            completed.outcome,
            super::super::NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!(slots[0], 10_000);
        assert_eq!(slots[1], 1_235);
        assert_eq!(slots[3], 9_995);

        let mut never_config = config;
        never_config.bound = QuickLongOperand::Const(100);
        never_config.operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::Equal,
            lhs: NativeStraightLongConditionOperand::BitwiseAnd {
                lhs: QuickLongOperand::Slot(3),
                rhs: QuickLongOperand::Const(1),
            },
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
            false_target: 2,
        };
        let never_program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication_and_carried(
            never_config,
            1_024,
            publication_mask | (1u64 << 2),
            publication_mask,
        )
        .unwrap();
        let mut never_slots = [0i64; 64];
        never_slots[1] = 10;
        never_slots[2] = 777;
        never_slots[3] = -5;
        let never_completed = never_program
            .call_range_proven_polling(
                &mut never_slots,
                100,
                interrupt.as_ptr() as *const bool,
                100,
            )
            .unwrap();
        assert_eq!(
            never_completed.outcome,
            super::super::NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!(never_slots[1], 10);
        assert_eq!(never_slots[2], 777, "skipped result alias must remain untouched");
        assert_eq!(never_slots[3], 95);
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn structured_local_temporary_chain_stays_in_x8_until_recurrence() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(45)),
            false_target: 4,
        };
        operations[1] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(3),
            result: 6,
        };
        operations[2] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(6),
            rhs: QuickLongOperand::Const(7),
            result: 7,
        };
        operations[3] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(7),
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
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10_000),
            operations,
            operation_count: 5,
            post_result: None,
        };
        let publication_mask = (1u64 << 1) | (1u64 << 3);
        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication_and_carried(
            config,
            1_024,
            publication_mask,
            publication_mask,
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert!(!words.contains(&0xaa08_03e6)); // consumers read resident x8 directly
        assert!(!words.contains(&0xaa08_03e7));
        for omitted_store in [
            0xf900_0808, // STR x8, [x0, #16] result TMP 2
            0xf900_1008, // STR x8, [x0, #32] result TMP 4
            0xf900_1808, // STR x8, [x0, #48] expression TMP 6
            0xf900_1c08, // STR x8, [x0, #56] expression TMP 7
        ] {
            assert!(!words.contains(&omitted_store));
        }

        let interrupt = AtomicBool::new(true);
        let mut slots = [0i64; 64];
        slots[1] = 10;
        slots[2] = 222;
        slots[3] = -5;
        slots[4] = 444;
        slots[6] = 666;
        slots[7] = 777;
        let interrupted = program
            .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 1_024)
            .unwrap();
        assert_eq!(
            interrupted.outcome,
            super::super::NativeStraightLongLoopOutcome::ChunkExhausted
        );
        assert_eq!(slots[0], 1_024);
        assert_eq!(slots[1], 4_035);
        assert_eq!(slots[3], 1_019);
        assert_eq!((slots[2], slots[4], slots[6], slots[7]), (222, 444, 666, 777));

        interrupt.store(false, Ordering::Relaxed);
        let completed = program
            .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 2_048)
            .unwrap();
        assert_eq!(
            completed.outcome,
            super::super::NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!(slots[0], 10_000);
        assert_eq!(slots[1], 4_035);
        assert_eq!(slots[3], 9_995);
        assert_eq!((slots[2], slots[4], slots[6], slots[7]), (222, 444, 666, 777));
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn structured_if_else_publications_merge_in_fixed_registers() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(50)),
            false_target: 4,
        };
        operations[1] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(3),
            result: 6,
        };
        operations[2] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(6),
            rhs: QuickLongOperand::Const(1),
            result: 7,
            destination: 1,
        };
        operations[3] = NativeStraightLongOperation::Jump { target: 6 };
        operations[4] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(5),
            result: 8,
        };
        operations[5] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(8),
            rhs: QuickLongOperand::Const(2),
            result: 9,
            destination: 1,
        };
        operations[6] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(3),
            result: 10,
        };
        operations[7] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(10),
            rhs: QuickLongOperand::Const(11),
            result: 11,
            destination: 2,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10_000),
            operations,
            operation_count: 8,
            post_result: None,
        };
        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication(
            config,
            1_024,
            (1u64 << 1) | (1u64 << 2),
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            words.iter().filter(|&&word| word == 0xaa08_03e4).count(),
            0,
            "both selected definitions must write the x4 merge register directly"
        );
        assert!(!words.contains(&0xaa08_03e5)); // folded writes x5 directly
        assert!(words.contains(&0x9100_0504)); // ADD x4, x8, #1
        assert!(words.contains(&0xd100_0904)); // SUB x4, x8, #2
        assert!(words.contains(&0x9100_2d05)); // ADD x5, x8, #11
        assert!(words.contains(&0x8b03_0468)); // ADD x8, x3, x3, LSL #1: i * 3
        assert!(words.contains(&0x8b03_0868)); // ADD x8, x3, x3, LSL #2: i * 5
        assert!(words.contains(&0x8b04_0488)); // ADD x8, x4, x4, LSL #1: selected * 3
        let induction_increment = 0x9100_0463; // ADD x3, x3, #1
        assert_eq!(
            words
                .iter()
                .filter(|&&word| word == induction_increment)
                .count(),
            1
        );
        let increment_word = words
            .iter()
            .position(|&word| word == induction_increment)
            .unwrap();
        assert_eq!(words[increment_word + 1], 0x8b04_0488);
        assert!(!words.contains(&0xf900_0408)); // no loop STR x8, selected
        assert!(!words.contains(&0xf900_0808)); // no loop STR x8, folded
        assert!(words.contains(&0xf900_0404)); // exit STR x4, selected
        assert!(words.contains(&0xf900_0805)); // exit STR x5, folded

        let interrupt = AtomicBool::new(true);
        let mut slots = [0i64; 64];
        for (slot, sentinel) in (6..=11).zip(606..=611) {
            slots[slot] = sentinel;
        }
        let interrupted = program
            .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 1_024)
            .unwrap();
        assert_eq!(
            interrupted.outcome,
            super::super::NativeStraightLongLoopOutcome::ChunkExhausted
        );
        assert_eq!(slots[0], 1_024);
        assert_eq!((slots[1], slots[2]), (5_113, 15_350));
        assert_eq!(&slots[6..=11], &[606, 607, 608, 609, 610, 611]);

        interrupt.store(false, Ordering::Relaxed);
        let completed = program
            .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 2_048)
            .unwrap();
        assert_eq!(
            completed.outcome,
            super::super::NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!(slots[0], 10_000);
        assert_eq!((slots[1], slots[2]), (49_993, 149_990));
        assert_eq!(&slots[6..=11], &[606, 607, 608, 609, 610, 611]);
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn structured_direct_result_preserves_immediate_temporary_alias() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(100)),
            false_target: 1,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(5),
            result: 2,
            destination: 1,
        };
        operations[2] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(2),
            rhs: QuickLongOperand::Const(3),
            result: 3,
            destination: 4,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10),
            operations,
            operation_count: 3,
            post_result: None,
        };
        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication(
            config,
            1_024,
            (1u64 << 1) | (1u64 << 4),
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert!(words.contains(&0xaa08_03e4)); // preserve result/temp alias in x8
        assert!(!words.contains(&0xaa08_03e5)); // final folded result writes x5 directly
        assert!(!words.contains(&0xf900_0808)); // no temporary slot 2 store

        let interrupt = AtomicBool::new(false);
        let mut slots = [0i64; 64];
        slots[2] = 222;
        slots[3] = 333;
        let completed = program
            .call_range_proven_polling(&mut slots, 10, interrupt.as_ptr() as *const bool, 10)
            .unwrap();
        assert_eq!(
            completed.outcome,
            super::super::NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!((slots[0], slots[1], slots[4]), (10, 14, 42));
        assert_eq!((slots[2], slots[3]), (222, 333));
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn structured_publication_register_overflow_keeps_shadow_fallback() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(100)),
            false_target: 1,
        };
        for (index, constant) in [3, 5, 7, 11].into_iter().enumerate() {
            operations[index + 1] = NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Multiply,
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(constant),
                result: 2 + index as u16,
                destination: 10 + index as u16,
            };
        }
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10),
            operations,
            operation_count: 5,
            post_result: None,
        };
        let publication_mask =
            (1u64 << 10) | (1u64 << 11) | (1u64 << 12) | (1u64 << 13);
        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication(
            config,
            1_024,
            publication_mask,
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert!(!words.contains(&0xaa08_03e4)); // first three write fixed registers directly
        assert!(!words.contains(&0xaa08_03e5));
        assert!(!words.contains(&0xaa08_03eb));
        assert!(words.contains(&0xf900_3408)); // fourth value: STR x8, [x0, #104]

        let interrupt = AtomicBool::new(false);
        let mut slots = [0i64; 64];
        let completed = program
            .call_range_proven_polling(&mut slots, 10, interrupt.as_ptr() as *const bool, 10)
            .unwrap();
        assert_eq!(
            completed.outcome,
            super::super::NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!(&slots[10..14], &[27, 45, 63, 99]);
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn commutative_left_constants_use_immediate_native_lowering() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Const(7),
            rhs: QuickLongOperand::Slot(0),
            result: 2,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Const(3),
            rhs: QuickLongOperand::Slot(2),
            result: 3,
            destination: 1,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(100),
            operations,
            operation_count: 2,
            post_result: None,
        };
        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication(
            config,
            1_024,
            1u64 << 1,
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert!(words.contains(&0x9100_1c68)); // ADD x8, x3, #7
        assert!(words.contains(&0x8b08_0508)); // ADD x8, x8, x8, LSL #1

        let interrupt = AtomicBool::new(false);
        let mut slots = [0i64; 64];
        slots[2] = 222;
        slots[3] = 333;
        let completed = program
            .call_range_proven_polling(&mut slots, 100, interrupt.as_ptr() as *const bool, 100)
            .unwrap();
        assert_eq!(
            completed.outcome,
            super::super::NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!((slots[0], slots[1]), (100, 318));
        assert_eq!((slots[2], slots[3]), (222, 333));
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn structured_invariant_operands_are_loaded_once_before_native_loop() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: super::super::ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
            false_target: 3,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Slot(3),
            result: 6,
            destination: 1,
        };
        operations[2] = NativeStraightLongOperation::Jump { target: 4 };
        operations[3] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(1),
            result: 7,
            destination: 1,
        };
        operations[4] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(4),
            result: 8,
            destination: 2,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(100),
            operations,
            operation_count: 5,
            post_result: None,
        };
        assert_eq!(
            straight_long_best_invariant_slot_masks(&config),
            [1u64 << 3, 1u64 << 4]
        );

        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication(
            config,
            1_024,
            (1u64 << 1) | (1u64 << 2),
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            words.iter().filter(|&&word| word == 0xf940_0c0a).count(),
            1,
            "invariant slot 3 must be loaded into x10 exactly once"
        );
        assert_eq!(
            words.iter().filter(|&&word| word == 0xf940_1009).count(),
            1,
            "invariant slot 4 must be loaded into x9 exactly once"
        );
        assert!(!words.contains(&0xf940_0c06));
        assert!(!words.contains(&0xf940_0c07));
        assert!(!words.contains(&0xf940_1006));
        assert!(!words.contains(&0xf940_1007));

        let interrupt = AtomicBool::new(false);
        let mut slots = [0i64; 64];
        slots[3] = 50;
        slots[4] = 7;
        slots[6] = 666;
        slots[7] = 777;
        slots[8] = 888;
        let completed = program
            .call_range_proven_polling(&mut slots, 100, interrupt.as_ptr() as *const bool, 100)
            .unwrap();
        assert_eq!(
            completed.outcome,
            super::super::NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!(
            (slots[0], slots[1], slots[2], slots[3], slots[4]),
            (100, 98, 105, 50, 7)
        );
        assert_eq!((slots[6], slots[7], slots[8]), (666, 777, 888));
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn modulo_keeps_auxiliary_register_out_of_the_invariant_pool() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::Move {
            source: QuickLongOperand::Slot(3),
            result: 6,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(6),
            rhs: QuickLongOperand::Slot(4),
            result: 7,
            destination: 1,
        };
        operations[2] = NativeStraightLongOperation::Modulo {
            value: QuickLongOperand::Slot(0),
            divisor: 3,
            result: 8,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(4),
            operations,
            operation_count: 3,
            post_result: None,
        };
        assert_eq!(
            straight_long_best_invariant_slot_masks(&config),
            [1u64 << 3, 1u64 << 4]
        );

        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication(
            config,
            1_024,
            1u64 << 1,
        )
        .unwrap();
        let words = program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            words.iter().filter(|&&word| word == 0xf940_0c0a).count(),
            1
        );
        assert!(!words.contains(&0xf940_1009));
        assert!(words.contains(&0xf940_1007));

        let interrupt = AtomicBool::new(false);
        let mut slots = [0i64; 64];
        slots[3] = 10;
        slots[4] = 7;
        let completed = program
            .call_range_proven_polling(&mut slots, 4, interrupt.as_ptr() as *const bool, 4)
            .unwrap();
        assert_eq!(
            completed.outcome,
            super::super::NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!((slots[0], slots[1], slots[3], slots[4]), (4, 17, 10, 7));
    }
}
