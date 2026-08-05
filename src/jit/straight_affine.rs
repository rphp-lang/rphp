//! Target-neutral discovery of adjacent multiply/combination scalar pairs.
//!
//! Backends keep the profitability decision: ARM64 can consume variable
//! products with MADD/MSUB, while x86-64 filters only SIB-addressing shapes.

use super::{
    NativeStraightLongLoopConfig, NativeStraightLongOperation, QuickLongOperand,
    straight_long_operation_input_mask,
};
use crate::vm::function::ScalarLongOpKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StraightLongProductCombination {
    Add(QuickLongOperand),
    ProductMinus(QuickLongOperand),
    OperandMinusProduct(QuickLongOperand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StraightLongMultiplyConsumerPair {
    pub(crate) producer: usize,
    pub(crate) consumer: usize,
    pub(crate) lhs: QuickLongOperand,
    pub(crate) rhs: QuickLongOperand,
    pub(crate) combination: StraightLongProductCombination,
    pub(crate) intermediate: u16,
    pub(crate) result: u16,
    pub(crate) destination: Option<u16>,
}

pub(crate) fn straight_long_multiply_consumer_pair(
    config: &NativeStraightLongLoopConfig,
    producer: usize,
) -> Option<StraightLongMultiplyConsumerPair> {
    let consumer = producer.checked_add(1)?;
    if consumer >= config.operation_count as usize {
        return None;
    }
    let (lhs, rhs, intermediate) = match config.operations[producer] {
        NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs,
            rhs,
            result,
        } => (lhs, rhs, result),
        _ => return None,
    };
    let (combination, result, destination) = match config.operations[consumer] {
        NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(value),
            rhs,
            result,
        } if value == intermediate => (StraightLongProductCombination::Add(rhs), result, None),
        NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs,
            rhs: QuickLongOperand::Slot(value),
            result,
        } if value == intermediate => (StraightLongProductCombination::Add(lhs), result, None),
        NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(value),
            rhs,
            result,
            destination,
        } if value == intermediate => (
            StraightLongProductCombination::Add(rhs),
            result,
            Some(destination),
        ),
        NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs,
            rhs: QuickLongOperand::Slot(value),
            result,
            destination,
        } if value == intermediate => (
            StraightLongProductCombination::Add(lhs),
            result,
            Some(destination),
        ),
        NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(value),
            rhs,
            result,
        } if value == intermediate => (
            StraightLongProductCombination::ProductMinus(rhs),
            result,
            None,
        ),
        NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(value),
            rhs,
            result,
            destination,
        } if value == intermediate => (
            StraightLongProductCombination::ProductMinus(rhs),
            result,
            Some(destination),
        ),
        NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Subtract,
            lhs,
            rhs: QuickLongOperand::Slot(value),
            result,
        } if value == intermediate => (
            StraightLongProductCombination::OperandMinusProduct(lhs),
            result,
            None,
        ),
        NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Subtract,
            lhs,
            rhs: QuickLongOperand::Slot(value),
            result,
            destination,
        } if value == intermediate => (
            StraightLongProductCombination::OperandMinusProduct(lhs),
            result,
            Some(destination),
        ),
        _ => return None,
    };
    let combination_operand = match combination {
        StraightLongProductCombination::Add(operand)
        | StraightLongProductCombination::ProductMinus(operand)
        | StraightLongProductCombination::OperandMinusProduct(operand) => operand,
    };
    if combination_operand == QuickLongOperand::Slot(intermediate) {
        return None;
    }
    let intermediate_mask = 1u64.checked_shl(u32::from(intermediate))?;
    if config.operations[consumer].output_mask() & intermediate_mask == 0
        && config.operations[consumer + 1..config.operation_count as usize]
            .iter()
            .copied()
            .any(|operation| straight_long_operation_input_mask(operation) & intermediate_mask != 0)
    {
        return None;
    }
    Some(StraightLongMultiplyConsumerPair {
        producer,
        consumer,
        lhs,
        rhs,
        combination,
        intermediate,
        result,
        destination,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::straight::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS;

    fn pair_config(
        combination: NativeStraightLongOperation,
        later: NativeStraightLongOperation,
    ) -> NativeStraightLongLoopConfig {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Slot(1),
            result: 4,
        };
        operations[1] = combination;
        operations[2] = later;
        NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(10),
            operations,
            operation_count: 3,
            post_result: None,
        }
    }

    #[test]
    fn discovers_variable_multiply_add_and_reverse_subtract() {
        let add = pair_config(
            NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(4),
                rhs: QuickLongOperand::Slot(2),
                result: 5,
                destination: 3,
            },
            NativeStraightLongOperation::Move {
                source: QuickLongOperand::Slot(5),
                result: 6,
            },
        );
        let pair = straight_long_multiply_consumer_pair(&add, 0).unwrap();
        assert_eq!(pair.lhs, QuickLongOperand::Slot(0));
        assert_eq!(pair.rhs, QuickLongOperand::Slot(1));
        assert_eq!(
            pair.combination,
            StraightLongProductCombination::Add(QuickLongOperand::Slot(2))
        );
        assert_eq!(pair.destination, Some(3));

        let subtract = pair_config(
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Subtract,
                lhs: QuickLongOperand::Slot(2),
                rhs: QuickLongOperand::Slot(4),
                result: 5,
            },
            NativeStraightLongOperation::Move {
                source: QuickLongOperand::Slot(5),
                result: 6,
            },
        );
        assert_eq!(
            straight_long_multiply_consumer_pair(&subtract, 0)
                .unwrap()
                .combination,
            StraightLongProductCombination::OperandMinusProduct(QuickLongOperand::Slot(2))
        );
    }

    #[test]
    fn rejects_observed_or_self_consumed_intermediate() {
        let observed = pair_config(
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(4),
                rhs: QuickLongOperand::Slot(2),
                result: 5,
            },
            NativeStraightLongOperation::Move {
                source: QuickLongOperand::Slot(4),
                result: 6,
            },
        );
        assert!(straight_long_multiply_consumer_pair(&observed, 0).is_none());

        let doubled = pair_config(
            NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(4),
                rhs: QuickLongOperand::Slot(4),
                result: 5,
            },
            NativeStraightLongOperation::Move {
                source: QuickLongOperand::Slot(5),
                result: 6,
            },
        );
        assert!(straight_long_multiply_consumer_pair(&doubled, 0).is_none());
    }
}
