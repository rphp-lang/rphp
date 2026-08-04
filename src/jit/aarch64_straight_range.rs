use super::{
    NativeStraightLongConditionOperand, NativeStraightLongLoopConfig, NativeStraightLongOperation,
    QuickLongOperand, ScalarLongOpKind, NATIVE_STRAIGHT_LONG_MAX_OPERATIONS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LongInterval {
    minimum: i128,
    maximum: i128,
}

impl LongInterval {
    const FULL: Self = Self {
        minimum: i64::MIN as i128,
        maximum: i64::MAX as i128,
    };

    fn exact(value: i64) -> Self {
        Self {
            minimum: i128::from(value),
            maximum: i128::from(value),
        }
    }

    fn new(minimum: i128, maximum: i128) -> Option<Self> {
        (minimum >= i128::from(i64::MIN) && maximum <= i128::from(i64::MAX))
            .then_some(Self { minimum, maximum })
    }

    fn contains(self, value: i128) -> bool {
        self.minimum <= value && value <= self.maximum
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StraightRangeState {
    ranges: [LongInterval; 64],
    definitely_written: u64,
}

impl StraightRangeState {
    fn initial(slots: &[i64; 64]) -> Self {
        Self {
            ranges: slots.map(LongInterval::exact),
            definitely_written: 0,
        }
    }

    fn merge(&mut self, incoming: Self) {
        self.definitely_written &= incoming.definitely_written;
        for (current, incoming) in self.ranges.iter_mut().zip(incoming.ranges) {
            current.minimum = current.minimum.min(incoming.minimum);
            current.maximum = current.maximum.max(incoming.maximum);
        }
    }
}

/// Conservatively proves every checked arithmetic result over the complete
/// remaining induction range. The first domain is deliberately straight and
/// side-effect free. Reading a body output before it is overwritten means the
/// value is loop-carried, which requires a recurrence proof and is rejected.
pub(super) fn straight_long_remaining_range_is_proven(
    config: &NativeStraightLongLoopConfig,
    slots: &[i64; 64],
) -> bool {
    let induction = slots[config.induction_slot as usize];
    let bound = operand_value(slots, config.bound);
    if induction >= bound {
        return false;
    }
    let Some(last_induction) = bound.checked_sub(1) else {
        return false;
    };
    let induction_range = LongInterval {
        minimum: i128::from(induction),
        maximum: i128::from(last_induction),
    };
    let output_mask = config.body_output_mask();
    let operation_count = config.operation_count as usize;
    if operation_count == 0 || operation_count > NATIVE_STRAIGHT_LONG_MAX_OPERATIONS {
        return false;
    }
    let mut states = [None; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1];
    states[0] = Some(StraightRangeState::initial(slots));

    for operation_index in 0..operation_count {
        let Some(mut state) = states[operation_index] else {
            continue;
        };
        let operation = config.operations[operation_index];
        match operation {
            NativeStraightLongOperation::Modulo {
                value,
                divisor,
                result,
            } => {
                if divisor == 0 {
                    return false;
                }
                let Some(value) = operand_range(
                    value,
                    &state.ranges,
                    config.induction_slot,
                    induction_range,
                    output_mask,
                    state.definitely_written,
                ) else {
                    return false;
                };
                state.ranges[result as usize] =
                    modulo_interval(value, LongInterval::exact(divisor));
                state.definitely_written |= 1u64 << result;
            }
            NativeStraightLongOperation::Move { source, result } => {
                let Some(value) = operand_range(
                    source,
                    &state.ranges,
                    config.induction_slot,
                    induction_range,
                    output_mask,
                    state.definitely_written,
                ) else {
                    return false;
                };
                state.ranges[result as usize] = value;
                state.definitely_written |= 1u64 << result;
            }
            NativeStraightLongOperation::Binary {
                kind,
                lhs,
                rhs,
                result,
            } => {
                let Some((lhs, rhs)) = binary_operand_ranges(
                    lhs,
                    rhs,
                    &state.ranges,
                    config,
                    induction_range,
                    output_mask,
                    state.definitely_written,
                ) else {
                    return false;
                };
                let Some(result_range) = binary_interval(kind, lhs, rhs) else {
                    return false;
                };
                state.ranges[result as usize] = result_range;
                state.definitely_written |= 1u64 << result;
            }
            NativeStraightLongOperation::BinaryAssign {
                kind,
                lhs,
                rhs,
                result,
                destination,
            } => {
                let Some((lhs, rhs)) = binary_operand_ranges(
                    lhs,
                    rhs,
                    &state.ranges,
                    config,
                    induction_range,
                    output_mask,
                    state.definitely_written,
                ) else {
                    return false;
                };
                let Some(result_range) = binary_interval(kind, lhs, rhs) else {
                    return false;
                };
                state.ranges[result as usize] = result_range;
                state.ranges[destination as usize] = result_range;
                state.definitely_written |= (1u64 << result) | (1u64 << destination);
            }
            NativeStraightLongOperation::BranchUnless {
                lhs,
                rhs,
                false_target,
                ..
            } => {
                if !condition_operand_is_available(
                    lhs,
                    &state,
                    config.induction_slot,
                    induction_range,
                    output_mask,
                ) || !condition_operand_is_available(
                    rhs,
                    &state,
                    config.induction_slot,
                    induction_range,
                    output_mask,
                ) {
                    return false;
                }
                let false_target = false_target as usize;
                if false_target <= operation_index || false_target > operation_count {
                    return false;
                }
                merge_range_state(&mut states[operation_index + 1], state);
                merge_range_state(&mut states[false_target], state);
                continue;
            }
            NativeStraightLongOperation::Jump { target } => {
                let target = target as usize;
                if target <= operation_index || target > operation_count {
                    return false;
                }
                merge_range_state(&mut states[target], state);
                continue;
            }
            NativeStraightLongOperation::Unused
            | NativeStraightLongOperation::StringToken { .. }
            | NativeStraightLongOperation::StringLength { .. }
            | NativeStraightLongOperation::HashLoad { .. }
            | NativeStraightLongOperation::HashStore { .. }
            | NativeStraightLongOperation::Guard { .. } => return false,
        }
        merge_range_state(&mut states[operation_index + 1], state);
    }
    states[operation_count].is_some()
}

fn merge_range_state(target: &mut Option<StraightRangeState>, incoming: StraightRangeState) {
    match target {
        Some(current) => current.merge(incoming),
        None => *target = Some(incoming),
    }
}

fn condition_operand_is_available(
    operand: NativeStraightLongConditionOperand,
    state: &StraightRangeState,
    induction_slot: u16,
    induction_range: LongInterval,
    output_mask: u64,
) -> bool {
    let available = |operand| {
        operand_range(
            operand,
            &state.ranges,
            induction_slot,
            induction_range,
            output_mask,
            state.definitely_written,
        )
        .is_some()
    };
    match operand {
        NativeStraightLongConditionOperand::Source(source) => available(source),
        NativeStraightLongConditionOperand::BitwiseAnd { lhs, rhs } => {
            available(lhs) && available(rhs)
        }
    }
}

fn operand_value(slots: &[i64; 64], operand: QuickLongOperand) -> i64 {
    match operand {
        QuickLongOperand::Slot(slot) => slots[slot as usize],
        QuickLongOperand::Const(value) => value,
    }
}

fn operand_range(
    operand: QuickLongOperand,
    ranges: &[LongInterval; 64],
    induction_slot: u16,
    induction_range: LongInterval,
    output_mask: u64,
    written_mask: u64,
) -> Option<LongInterval> {
    match operand {
        QuickLongOperand::Const(value) => Some(LongInterval::exact(value)),
        QuickLongOperand::Slot(slot) if slot == induction_slot => Some(induction_range),
        QuickLongOperand::Slot(slot)
            if output_mask & (1u64 << slot) != 0 && written_mask & (1u64 << slot) == 0 =>
        {
            None
        }
        QuickLongOperand::Slot(slot) => Some(ranges[slot as usize]),
    }
}

#[allow(clippy::too_many_arguments)]
fn binary_operand_ranges(
    lhs: QuickLongOperand,
    rhs: QuickLongOperand,
    ranges: &[LongInterval; 64],
    config: &NativeStraightLongLoopConfig,
    induction_range: LongInterval,
    output_mask: u64,
    written_mask: u64,
) -> Option<(LongInterval, LongInterval)> {
    Some((
        operand_range(
            lhs,
            ranges,
            config.induction_slot,
            induction_range,
            output_mask,
            written_mask,
        )?,
        operand_range(
            rhs,
            ranges,
            config.induction_slot,
            induction_range,
            output_mask,
            written_mask,
        )?,
    ))
}

fn binary_interval(
    kind: ScalarLongOpKind,
    lhs: LongInterval,
    rhs: LongInterval,
) -> Option<LongInterval> {
    match kind {
        ScalarLongOpKind::Add => LongInterval::new(
            lhs.minimum.checked_add(rhs.minimum)?,
            lhs.maximum.checked_add(rhs.maximum)?,
        ),
        ScalarLongOpKind::Subtract => LongInterval::new(
            lhs.minimum.checked_sub(rhs.maximum)?,
            lhs.maximum.checked_sub(rhs.minimum)?,
        ),
        ScalarLongOpKind::Multiply => {
            let products = [
                lhs.minimum.checked_mul(rhs.minimum)?,
                lhs.minimum.checked_mul(rhs.maximum)?,
                lhs.maximum.checked_mul(rhs.minimum)?,
                lhs.maximum.checked_mul(rhs.maximum)?,
            ];
            LongInterval::new(*products.iter().min()?, *products.iter().max()?)
        }
        ScalarLongOpKind::IntDivide => divide_interval(lhs, rhs),
        ScalarLongOpKind::Modulo => {
            if rhs.contains(0) || (lhs.contains(i128::from(i64::MIN)) && rhs.contains(-1)) {
                None
            } else {
                Some(modulo_interval(lhs, rhs))
            }
        }
        ScalarLongOpKind::BitwiseXor => {
            if lhs.minimum == lhs.maximum && rhs.minimum == rhs.maximum {
                Some(LongInterval::exact(
                    (lhs.minimum as i64) ^ (rhs.minimum as i64),
                ))
            } else {
                Some(LongInterval::FULL)
            }
        }
    }
}

fn divide_interval(lhs: LongInterval, rhs: LongInterval) -> Option<LongInterval> {
    if rhs.contains(0) || (lhs.contains(i128::from(i64::MIN)) && rhs.contains(-1)) {
        return None;
    }
    let lhs_candidates = interval_candidates(lhs);
    let rhs_candidates = interval_candidates(rhs);
    let mut minimum = i128::MAX;
    let mut maximum = i128::MIN;
    for numerator in lhs_candidates.into_iter().flatten() {
        for denominator in rhs_candidates.into_iter().flatten() {
            if denominator == 0 {
                continue;
            }
            let value = numerator / denominator;
            minimum = minimum.min(value);
            maximum = maximum.max(value);
        }
    }
    LongInterval::new(minimum, maximum)
}

fn interval_candidates(interval: LongInterval) -> [Option<i128>; 5] {
    [
        Some(interval.minimum),
        Some(interval.maximum),
        interval.contains(-1).then_some(-1),
        interval.contains(0).then_some(0),
        interval.contains(1).then_some(1),
    ]
}

fn modulo_interval(lhs: LongInterval, rhs: LongInterval) -> LongInterval {
    let divisor_magnitude = rhs.minimum.abs().max(rhs.maximum.abs());
    let limit = (divisor_magnitude - 1).min(i128::from(i64::MAX));
    if lhs.minimum >= 0 {
        LongInterval {
            minimum: 0,
            maximum: limit.min(lhs.maximum),
        }
    } else if lhs.maximum <= 0 {
        LongInterval {
            minimum: (-limit).max(lhs.minimum),
            maximum: 0,
        }
    } else {
        LongInterval {
            minimum: -limit,
            maximum: limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn config(
        operations: &[NativeStraightLongOperation],
        bound: i64,
    ) -> NativeStraightLongLoopConfig {
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
    fn rejects_overflow_division_guards_and_loop_carried_values() {
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
        assert!(!straight_long_remaining_range_is_proven(
            &carried,
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
        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling(
            config, 1_024,
        )
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
        let program = super::super::CompiledQuickLongStraightLoop::compile_range_proven_polling(
            config, 1_024,
        )
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
            ScalarLongOpKind::Multiply,
            ScalarLongOpKind::IntDivide,
            ScalarLongOpKind::Modulo,
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
                                ScalarLongOpKind::Subtract => {
                                    (left as i64).checked_sub(right as i64)
                                }
                                ScalarLongOpKind::Multiply => {
                                    (left as i64).checked_mul(right as i64)
                                }
                                ScalarLongOpKind::IntDivide => {
                                    (left as i64).checked_div(right as i64)
                                }
                                ScalarLongOpKind::Modulo => (left as i64).checked_rem(right as i64),
                                ScalarLongOpKind::BitwiseXor => {
                                    Some((left as i64) ^ (right as i64))
                                }
                            }
                            .expect("accepted interval must exclude checked side exits");
                            assert!(result_range.contains(i128::from(result)));
                        }
                    }
                }
            }
        }
    }
}
