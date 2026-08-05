//! x86-64 affine scalar-pair recognition.
//!
//! The target-neutral IR keeps PHP operations and exact side-exit boundaries
//! intact. This module identifies only the unchecked pairs that one x86 SIB
//! address can represent without making an observable intermediate disappear.

use super::{
    NativeStraightLongLoopConfig, NativeStraightLongOperation, QuickLongOperand, ScalarLongOpKind,
    straight_long_operation_input_mask,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct X86StraightAffineFusion {
    pub(super) producer: usize,
    pub(super) consumer: usize,
    pub(super) source: QuickLongOperand,
    pub(super) scale: i64,
    pub(super) bias: i64,
    pub(super) intermediate: u16,
    pub(super) result: u16,
    pub(super) destination: Option<u16>,
}

pub(super) fn x86_straight_affine_fusion(
    config: &NativeStraightLongLoopConfig,
    producer: usize,
) -> Option<X86StraightAffineFusion> {
    let consumer = producer.checked_add(1)?;
    if consumer >= config.operation_count as usize {
        return None;
    }
    let (source, scale, intermediate) = match config.operations[producer] {
        NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(source),
            rhs: QuickLongOperand::Const(scale),
            result,
        }
        | NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Const(scale),
            rhs: QuickLongOperand::Slot(source),
            result,
        } if matches!(scale, 3 | 5 | 9) => (QuickLongOperand::Slot(source), scale, result),
        _ => return None,
    };
    let (bias, result, destination) = match config.operations[consumer] {
        NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(value),
            rhs: QuickLongOperand::Const(bias),
            result,
        }
        | NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Const(bias),
            rhs: QuickLongOperand::Slot(value),
            result,
        } if value == intermediate => (bias, result, None),
        NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(value),
            rhs: QuickLongOperand::Const(bias),
            result,
            destination,
        }
        | NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Const(bias),
            rhs: QuickLongOperand::Slot(value),
            result,
            destination,
        } if value == intermediate => (bias, result, Some(destination)),
        NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(value),
            rhs: QuickLongOperand::Const(bias),
            result,
        } if value == intermediate => (0i64.checked_sub(bias)?, result, None),
        NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(value),
            rhs: QuickLongOperand::Const(bias),
            result,
            destination,
        } if value == intermediate => (0i64.checked_sub(bias)?, result, Some(destination)),
        _ => return None,
    };
    i32::try_from(bias).ok()?;
    let intermediate_mask = 1u64 << intermediate;
    if config.operations[consumer].output_mask() & intermediate_mask == 0
        && config.operations[consumer + 1..config.operation_count as usize]
            .iter()
            .copied()
            .any(|operation| straight_long_operation_input_mask(operation) & intermediate_mask != 0)
    {
        return None;
    }
    Some(X86StraightAffineFusion {
        producer,
        consumer,
        source,
        scale,
        bias,
        intermediate,
        result,
        destination,
    })
}
