use super::{
    NativeStraightLongConditionOperand, NativeStraightLongLoopConfig, NativeStraightLongOperation,
    QuickLongOperand, NATIVE_STRAIGHT_LONG_MAX_OPERATIONS,
};

pub(super) fn straight_long_linear_live_after(
    config: &NativeStraightLongLoopConfig,
) -> [u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS] {
    let mut live_after = [0u64; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut live = 0u64;
    for index in (0..config.operation_count as usize).rev() {
        let operation = config.operations[index];
        live_after[index] = live;
        live = (live & !operation.output_mask()) | operation_input_mask(operation);
    }
    live_after
}

fn operation_input_mask(operation: NativeStraightLongOperation) -> u64 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::function::ScalarLongOpKind;

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
    }

    #[test]
    fn overlapping_value_is_cached_while_shadow_store_remains() {
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

        assert!(words.contains(&0xaa08_03e4)); // MOV x4, x8
        assert!(words.contains(&0xf900_0808)); // STR x8, [x0, #16]
    }
}
