//! ARM64 profitability filter for target-neutral multiply/consumer pairs.

use super::super::straight::{
    NativeStraightLongLoopConfig, StraightLongProductCombination,
    straight_long_multiply_consumer_pair,
};
use super::{
    Arm64Assembler, Arm64Register, emit_straight_long_operand_with_resident, long_slot_offset,
};
use crate::vm::quick::QuickLongOperand;

#[derive(Debug, Clone, Copy)]
pub(super) struct Arm64StraightMultiplyAccumulateFusion {
    pub(super) producer: usize,
    pub(super) consumer: usize,
    pub(super) lhs: QuickLongOperand,
    pub(super) rhs: QuickLongOperand,
    pub(super) addend: QuickLongOperand,
    pub(super) subtract_product: bool,
    pub(super) intermediate: u16,
    pub(super) result: u16,
    pub(super) destination: Option<u16>,
}

pub(super) fn arm64_straight_multiply_accumulate_fusion(
    config: &NativeStraightLongLoopConfig,
    producer: usize,
) -> Option<Arm64StraightMultiplyAccumulateFusion> {
    let pair = straight_long_multiply_consumer_pair(config, producer)?;
    if !matches!(pair.lhs, QuickLongOperand::Slot(_))
        || !matches!(pair.rhs, QuickLongOperand::Slot(_))
    {
        return None;
    }
    let (addend, subtract_product) = match pair.combination {
        StraightLongProductCombination::Add(addend @ QuickLongOperand::Slot(_)) => (addend, false),
        StraightLongProductCombination::OperandMinusProduct(addend @ QuickLongOperand::Slot(_)) => {
            (addend, true)
        }
        _ => return None,
    };
    Some(Arm64StraightMultiplyAccumulateFusion {
        producer: pair.producer,
        consumer: pair.consumer,
        lhs: pair.lhs,
        rhs: pair.rhs,
        addend,
        subtract_product,
        intermediate: pair.intermediate,
        result: pair.result,
        destination: pair.destination,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_arm64_straight_multiply_accumulate(
    assembler: &mut Arm64Assembler,
    fusion: Arm64StraightMultiplyAccumulateFusion,
    result: Arm64Register,
    operand_scratch: [Arm64Register; 3],
    induction_slot: u16,
    induction: Arm64Register,
    resident_values: &[(u64, Arm64Register)],
    shadow_store_mask: u64,
) {
    let multiplicand = emit_straight_long_operand_with_resident(
        assembler,
        fusion.lhs,
        operand_scratch[0],
        induction_slot,
        induction,
        resident_values,
    );
    let multiplier = emit_straight_long_operand_with_resident(
        assembler,
        fusion.rhs,
        operand_scratch[1],
        induction_slot,
        induction,
        resident_values,
    );
    let addend = emit_straight_long_operand_with_resident(
        assembler,
        fusion.addend,
        operand_scratch[2],
        induction_slot,
        induction,
        resident_values,
    );
    if fusion.subtract_product {
        assembler.multiply_subtract(result, multiplicand, multiplier, addend);
    } else {
        assembler.multiply_add(result, multiplicand, multiplier, addend);
    }
    if shadow_store_mask & (1u64 << fusion.result) != 0 {
        assembler.store_u64(result, Arm64Register::X0, long_slot_offset(fusion.result));
    }
    if let Some(destination) = fusion.destination
        && destination != fusion.result
        && shadow_store_mask & (1u64 << destination) != 0
    {
        assembler.store_u64(result, Arm64Register::X0, long_slot_offset(destination));
    }
}
