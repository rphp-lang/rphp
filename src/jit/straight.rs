//! Target-neutral native straight-loop IR.
//!
//! Backends consume this guarded, fixed-capacity representation after the PHP
//! planner has selected a closed scalar or mixed loop region. Physical
//! registers, instruction encodings, executable memory and calling conventions
//! deliberately do not appear here.

// The first parity checkpoint deliberately compiles target-neutral analyses
// before a non-ARM backend consumes them. Keep that temporary state warning-free
// without weakening dead-code diagnostics in the rest of the crate.
#![cfg_attr(
    not(all(target_arch = "aarch64", target_os = "macos")),
    allow(dead_code)
)]

use crate::vm::function::{ScalarLongConditionKind, ScalarLongOpKind};
use crate::vm::quick::QuickLongOperand;

#[cfg(all(test, target_arch = "aarch64", target_os = "macos"))]
use super::aarch64::{
    Arm64Assembler, Arm64Register, CompiledQuickLongStraightLoop,
    emit_straight_long_operand_with_resident, straight_binary_add_sub_immediate,
    straight_binary_lowering_operands, straight_multiply_shift_add,
};

#[path = "straight_liveness.rs"]
mod liveness;
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
pub(crate) use liveness::straight_long_early_induction_increment_operation;
pub(crate) use liveness::{
    straight_long_carried_dependency_operations, straight_long_linear_final_publication_masks,
    straight_long_linear_live_after, straight_long_linear_shadow_store_mask,
    straight_long_structured_block_starts, straight_long_structured_definitely_written,
    straight_long_structured_local_resident_output_masks,
};

#[path = "straight_affine.rs"]
mod affine;
pub(crate) use affine::{
    StraightLongMultiplyConsumerPair, StraightLongProductCombination,
    straight_long_multiply_consumer_pair,
};

#[path = "straight_range.rs"]
mod range;
pub(crate) use range::{StraightLongRangeProof, straight_long_remaining_range_proof};

/// Upper bound for one closed native scalar/mixed region. The byte-sized
/// branch ABI still leaves ample headroom; 48 admits application-shaped
/// regions with multiple inlined typed calls without making the shadow slot
/// namespace or compile-time validation dynamic.
pub const NATIVE_STRAIGHT_LONG_MAX_OPERATIONS: usize = 48;

/// Runtime entry pointers address the payload word of already validated Long
/// hash values. They are activation state and are never embedded in code.
pub const NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES: usize = 16;

/// Maximum number of guarded scalar callees composed into one native region.
/// This is part of the shared region ABI rather than an architecture detail.
pub const NATIVE_QUICK_LONG_MAX_CALL_TARGETS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeStraightLongConditionOperand {
    Source(QuickLongOperand),
    BitwiseAnd {
        lhs: QuickLongOperand,
        rhs: QuickLongOperand,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeStraightLongOperation {
    Unused,
    Modulo {
        value: QuickLongOperand,
        divisor: i64,
        result: u16,
    },
    Move {
        source: QuickLongOperand,
        result: u16,
    },
    /// Store a finite-state String token in the private shadow. The VM maps it
    /// back to the guarded immutable String value when native execution exits.
    StringToken {
        token: u8,
        result: u16,
    },
    /// Resolve the byte length of a finite-state String without dereferencing
    /// Rust's heap representation from generated code.
    StringLength {
        source: u16,
        lengths: [i64; 4],
        token_count: u8,
        result: u16,
    },
    /// Load an existing, entry-guarded Long hash value selected by a String
    /// token. Entry pointers are supplied through the per-dispatch context.
    HashLoad {
        key: u16,
        entry_base: u8,
        token_count: u8,
        result: u16,
        destination: Option<u16>,
    },
    /// Store a Long payload through the same prevalidated contextual entry
    /// table. Structural array writes remain outside this native operation.
    HashStore {
        key: u16,
        entry_base: u8,
        token_count: u8,
        source: QuickLongOperand,
    },
    Binary {
        kind: ScalarLongOpKind,
        lhs: QuickLongOperand,
        rhs: QuickLongOperand,
        result: u16,
    },
    BinaryAssign {
        kind: ScalarLongOpKind,
        lhs: QuickLongOperand,
        rhs: QuickLongOperand,
        result: u16,
        destination: u16,
    },
    /// Exit before an uncommon control-flow edge while leaving all prior
    /// operation outputs committed in the native shadow state.
    Guard {
        kind: ScalarLongConditionKind,
        lhs: NativeStraightLongConditionOperand,
        rhs: NativeStraightLongConditionOperand,
        expected: bool,
    },
    BranchUnless {
        kind: ScalarLongConditionKind,
        lhs: NativeStraightLongConditionOperand,
        rhs: NativeStraightLongConditionOperand,
        false_target: u8,
    },
    Jump {
        target: u8,
    },
}

impl NativeStraightLongOperation {
    pub fn output_mask(self) -> u64 {
        match self {
            Self::Unused => 0,
            Self::Modulo { result, .. } => 1u64 << result,
            Self::Move { result, .. } => 1u64 << result,
            Self::StringToken { .. } => 0,
            Self::StringLength { result, .. } => 1u64 << result,
            Self::HashLoad {
                result,
                destination,
                ..
            } => (1u64 << result) | destination.map_or(0, |slot| 1u64 << slot),
            Self::HashStore { .. } => 0,
            Self::Binary { result, .. } => 1u64 << result,
            Self::BinaryAssign {
                result,
                destination,
                ..
            } => (1u64 << result) | (1u64 << destination),
            Self::Guard { .. } | Self::BranchUnless { .. } | Self::Jump { .. } => 0,
        }
    }

    pub fn shadow_output_mask(self) -> u64 {
        match self {
            Self::StringToken { result, .. } => 1u64 << result,
            _ => self.output_mask(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeStraightLongLoopConfig {
    pub induction_slot: u16,
    pub bound: QuickLongOperand,
    pub operations: [NativeStraightLongOperation; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    pub operation_count: u8,
    pub post_result: Option<u16>,
}

impl NativeStraightLongLoopConfig {
    pub fn body_output_mask(&self) -> u64 {
        self.operations
            .iter()
            .copied()
            .take(self.operation_count as usize)
            .fold(0, |mask, operation| mask | operation.output_mask())
    }

    pub fn output_mask_before(&self, operation_index: u8) -> u64 {
        self.operations
            .iter()
            .copied()
            .take(operation_index as usize)
            .fold(0, |mask, operation| mask | operation.output_mask())
    }
}

/// Return every shadow slot read by one target-neutral native operation.
pub(crate) fn straight_long_operation_input_mask(operation: NativeStraightLongOperation) -> u64 {
    match operation {
        NativeStraightLongOperation::Unused
        | NativeStraightLongOperation::StringToken { .. }
        | NativeStraightLongOperation::Jump { .. } => 0,
        NativeStraightLongOperation::Modulo { value, .. } => operand_mask(value),
        NativeStraightLongOperation::Move { source, .. } => operand_mask(source),
        NativeStraightLongOperation::StringLength { source, .. }
        | NativeStraightLongOperation::HashLoad { key: source, .. } => 1u64 << source,
        NativeStraightLongOperation::HashStore { key, source, .. } => {
            (1u64 << key) | operand_mask(source)
        }
        NativeStraightLongOperation::Binary { lhs, rhs, .. }
        | NativeStraightLongOperation::BinaryAssign { lhs, rhs, .. } => {
            operand_mask(lhs) | operand_mask(rhs)
        }
        NativeStraightLongOperation::Guard { lhs, rhs, .. }
        | NativeStraightLongOperation::BranchUnless { lhs, rhs, .. } => {
            condition_operand_mask(lhs) | condition_operand_mask(rhs)
        }
    }
}

/// Pick the two most frequently read slots that the native body never writes.
/// Ties are deterministic and favor the lower shadow-slot index. Backends may
/// conservatively use fewer candidates when their physical registers clash
/// with instruction-specific scratch requirements.
pub(crate) fn straight_long_best_invariant_slot_masks(
    config: &NativeStraightLongLoopConfig,
) -> [u64; 2] {
    let excluded = config.operations[..config.operation_count as usize]
        .iter()
        .copied()
        .fold(1u64 << config.induction_slot, |mask, operation| {
            mask | operation.shadow_output_mask()
        });
    let mut uses = [0u8; 64];
    for operation in config.operations[..config.operation_count as usize]
        .iter()
        .copied()
    {
        let mut inputs = straight_long_operation_input_mask(operation) & !excluded;
        while inputs != 0 {
            let slot = inputs.trailing_zeros() as usize;
            inputs &= inputs - 1;
            uses[slot] = uses[slot].saturating_add(1);
        }
    }
    let mut best_slots = [usize::MAX; 2];
    for slot in 0..uses.len() {
        if uses[slot] == 0 {
            continue;
        }
        let insertion = if best_slots[0] == usize::MAX || uses[slot] > uses[best_slots[0]] {
            0
        } else if best_slots[1] == usize::MAX || uses[slot] > uses[best_slots[1]] {
            1
        } else {
            continue;
        };
        if insertion == 0 {
            best_slots[1] = best_slots[0];
        }
        best_slots[insertion] = slot;
    }
    best_slots.map(|slot| if slot == usize::MAX { 0 } else { 1u64 << slot })
}

fn condition_operand_mask(operand: NativeStraightLongConditionOperand) -> u64 {
    match operand {
        NativeStraightLongConditionOperand::Source(source) => operand_mask(source),
        NativeStraightLongConditionOperand::BitwiseAnd { lhs, rhs } => {
            operand_mask(lhs) | operand_mask(rhs)
        }
    }
}

fn operand_mask(operand: QuickLongOperand) -> u64 {
    match operand {
        QuickLongOperand::Slot(slot) => 1u64 << slot,
        QuickLongOperand::Const(_) => 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeStraightLongLoopOutcome {
    Completed,
    ChunkExhausted,
    OperationSideExit,
    IncrementOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeStraightLongLoopResult {
    pub outcome: NativeStraightLongLoopOutcome,
    pub failed_operation: Option<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_masks_are_target_independent() {
        let operation = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(7),
            result: 4,
            destination: 2,
        };
        assert_eq!(operation.output_mask(), (1u64 << 4) | (1u64 << 2));
        assert_eq!(operation.shadow_output_mask(), operation.output_mask());

        let token = NativeStraightLongOperation::StringToken {
            token: 1,
            result: 6,
        };
        assert_eq!(token.output_mask(), 0);
        assert_eq!(token.shadow_output_mask(), 1u64 << 6);
    }

    #[test]
    fn invariant_ranking_is_target_independent() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
            false_target: 2,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(4),
            result: 6,
            destination: 1,
        };
        operations[2] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(3),
            rhs: QuickLongOperand::Slot(4),
            result: 7,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(100),
            operations,
            operation_count: 3,
            post_result: None,
        };

        assert_eq!(
            straight_long_operation_input_mask(operations[0]),
            (1u64 << 0) | (1u64 << 3)
        );
        assert_eq!(
            straight_long_best_invariant_slot_masks(&config),
            [1u64 << 3, 1u64 << 4]
        );
    }

    #[test]
    fn finite_string_token_outputs_are_not_ranked_as_invariants() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::StringToken {
            token: 1,
            result: 3,
        };
        operations[1] = NativeStraightLongOperation::StringLength {
            source: 3,
            lengths: [4, 5, 0, 0],
            token_count: 2,
            result: 4,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10),
            operations,
            operation_count: 2,
            post_result: None,
        };

        assert_eq!(straight_long_best_invariant_slot_masks(&config), [0, 0]);
    }
}
