//! x86-64 affine scalar-pair recognition.
//!
//! The target-neutral IR keeps PHP operations and exact side-exit boundaries
//! intact. This module identifies only the unchecked pairs that one x86 SIB
//! address can represent without making an observable intermediate disappear.

use super::super::straight::{
    NativeStraightLongLoopConfig, StraightLongProductCombination,
    straight_long_multiply_consumer_pair,
};
use crate::vm::quick::QuickLongOperand;

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
    let pair = straight_long_multiply_consumer_pair(config, producer)?;
    let (source, scale) = match (pair.lhs, pair.rhs) {
        (QuickLongOperand::Slot(source), QuickLongOperand::Const(scale))
        | (QuickLongOperand::Const(scale), QuickLongOperand::Slot(source))
            if matches!(scale, 3 | 5 | 9) =>
        {
            (QuickLongOperand::Slot(source), scale)
        }
        _ => return None,
    };
    let bias = match pair.combination {
        StraightLongProductCombination::Add(QuickLongOperand::Const(bias)) => bias,
        StraightLongProductCombination::ProductMinus(QuickLongOperand::Const(bias)) => {
            0i64.checked_sub(bias)?
        }
        _ => return None,
    };
    i32::try_from(bias).ok()?;
    Some(X86StraightAffineFusion {
        producer: pair.producer,
        consumer: pair.consumer,
        source,
        scale,
        bias,
        intermediate: pair.intermediate,
        result: pair.result,
        destination: pair.destination,
    })
}
