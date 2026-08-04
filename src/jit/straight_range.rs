use super::{
    NativeStraightLongConditionOperand, NativeStraightLongLoopConfig, NativeStraightLongOperation,
    QuickLongOperand, ScalarLongConditionKind, ScalarLongOpKind,
    NATIVE_STRAIGHT_LONG_MAX_OPERATIONS,
    straight_long_operation_input_mask,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StraightLongRangeProof {
    pub(crate) carried_mask: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LinearRecurrenceProof {
    operation_ranges: [Option<LongInterval>; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    carried_mask: u64,
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
/// remaining induction range. Linear additive and subtractive loop-carried
/// values may consume acyclic scalar expressions and other proven recurrences.
/// Dependencies are solved topologically; cycles retain the checked path.
pub(crate) fn straight_long_remaining_range_proof(
    config: &NativeStraightLongLoopConfig,
    slots: &[i64; 64],
) -> Option<StraightLongRangeProof> {
    let induction = slots[config.induction_slot as usize];
    let bound = operand_value(slots, config.bound);
    if induction >= bound {
        return None;
    }
    let Some(last_induction) = bound.checked_sub(1) else {
        return None;
    };
    let induction_range = LongInterval {
        minimum: i128::from(induction),
        maximum: i128::from(last_induction),
    };
    let output_mask = config.body_output_mask();
    let operation_count = config.operation_count as usize;
    if operation_count == 0 || operation_count > NATIVE_STRAIGHT_LONG_MAX_OPERATIONS {
        return None;
    }
    let iterations = (bound as u64).wrapping_sub(induction as u64);
    let recurrence =
        linear_recurrence_proof(config, slots, induction_range, iterations, output_mask)?;
    let mut states = [None; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1];
    let mut initial_state = StraightRangeState::initial(slots);
    for slot in 0..64 {
        if recurrence.carried_mask & (1u64 << slot) == 0 {
            continue;
        }
        let operation_index = recurrence_operation_for_slot(config, slot as u16)?;
        initial_state.ranges[slot] = recurrence.operation_ranges[operation_index]?;
    }
    states[0] = Some(initial_state);

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
                    return None;
                }
                let Some(value) = operand_range(
                    value,
                    &state.ranges,
                    config.induction_slot,
                    induction_range,
                    output_mask,
                    state.definitely_written,
                    recurrence.carried_mask,
                ) else {
                    return None;
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
                    recurrence.carried_mask,
                ) else {
                    return None;
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
                let result_range = if let Some(range) = recurrence.operation_ranges[operation_index]
                {
                    range
                } else {
                    let (lhs, rhs) = binary_operand_ranges(
                        lhs,
                        rhs,
                        &state.ranges,
                        config,
                        induction_range,
                        output_mask,
                        state.definitely_written,
                        recurrence.carried_mask,
                    )?;
                    binary_interval(kind, lhs, rhs)?
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
                let result_range = if let Some(range) = recurrence.operation_ranges[operation_index]
                {
                    range
                } else {
                    let (lhs, rhs) = binary_operand_ranges(
                        lhs,
                        rhs,
                        &state.ranges,
                        config,
                        induction_range,
                        output_mask,
                        state.definitely_written,
                        recurrence.carried_mask,
                    )?;
                    binary_interval(kind, lhs, rhs)?
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
                    recurrence.carried_mask,
                ) || !condition_operand_is_available(
                    rhs,
                    &state,
                    config.induction_slot,
                    induction_range,
                    output_mask,
                    recurrence.carried_mask,
                ) {
                    return None;
                }
                let false_target = false_target as usize;
                if false_target <= operation_index || false_target > operation_count {
                    return None;
                }
                merge_range_state(&mut states[operation_index + 1], state);
                merge_range_state(&mut states[false_target], state);
                continue;
            }
            NativeStraightLongOperation::Jump { target } => {
                let target = target as usize;
                if target <= operation_index || target > operation_count {
                    return None;
                }
                merge_range_state(&mut states[target], state);
                continue;
            }
            NativeStraightLongOperation::Guard {
                kind,
                lhs,
                rhs,
                expected,
            } => {
                let lhs = condition_operand_range(
                    lhs,
                    &state,
                    config.induction_slot,
                    induction_range,
                    output_mask,
                    recurrence.carried_mask,
                )?;
                let rhs = condition_operand_range(
                    rhs,
                    &state,
                    config.induction_slot,
                    induction_range,
                    output_mask,
                    recurrence.carried_mask,
                )?;
                if interval_condition(kind, lhs, rhs)? != expected {
                    return None;
                }
            }
            NativeStraightLongOperation::Unused
            | NativeStraightLongOperation::StringToken { .. }
            | NativeStraightLongOperation::StringLength { .. }
            | NativeStraightLongOperation::HashLoad { .. }
            | NativeStraightLongOperation::HashStore { .. } => return None,
        }
        merge_range_state(&mut states[operation_index + 1], state);
    }
    states[operation_count]?;
    Some(StraightLongRangeProof {
        carried_mask: recurrence.carried_mask,
    })
}

#[cfg(test)]
pub(super) fn straight_long_remaining_range_is_proven(
    config: &NativeStraightLongLoopConfig,
    slots: &[i64; 64],
) -> bool {
    straight_long_remaining_range_proof(config, slots).is_some()
}

fn linear_recurrence_proof(
    config: &NativeStraightLongLoopConfig,
    slots: &[i64; 64],
    induction_range: LongInterval,
    iterations: u64,
    output_mask: u64,
) -> Option<LinearRecurrenceProof> {
    let mut written_mask = 0u64;
    let mut carried_mask = 0u64;
    let mut has_control_flow = false;
    for operation in config
        .operations
        .iter()
        .copied()
        .take(config.operation_count as usize)
    {
        carried_mask |= straight_long_operation_input_mask(operation) & output_mask & !written_mask;
        written_mask |= operation.output_mask();
        has_control_flow |= matches!(
            operation,
            NativeStraightLongOperation::BranchUnless { .. }
                | NativeStraightLongOperation::Jump { .. }
        );
    }
    carried_mask &= !(1u64 << config.induction_slot);
    if carried_mask == 0 {
        return Some(LinearRecurrenceProof {
            operation_ranges: [None; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
            carried_mask: 0,
        });
    }
    if carried_mask.count_ones() > 3 {
        return None;
    }
    let mut carried_slots_by_operation = [None; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut recurrence_kinds = [None; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut recurrence_deltas = [None; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut remaining = carried_mask;
    while remaining != 0 {
        let slot = remaining.trailing_zeros() as u16;
        remaining &= remaining - 1;
        let operation_index = recurrence_operation_for_slot(config, slot)?;
        if carried_slots_by_operation[operation_index]
            .replace(slot)
            .is_some()
            || (config.operations[operation_index].output_mask() & carried_mask).count_ones() != 1
        {
            return None;
        }
        let (kind, delta) = recurrence_update(config.operations[operation_index], slot)?;
        recurrence_kinds[operation_index] = Some(kind);
        recurrence_deltas[operation_index] = Some(delta);
    }

    let mut operation_ranges = [None; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut carried_ranges = [None; 64];
    let recurrence_count = carried_mask.count_ones() as usize;
    let mut proven_count = 0usize;
    while proven_count < recurrence_count {
        let before_count = proven_count;
        for (operation_index, carried_slot) in carried_slots_by_operation
            .iter()
            .copied()
            .take(config.operation_count as usize)
            .enumerate()
        {
            let Some(slot) = carried_slot else {
                continue;
            };
            if operation_ranges[operation_index].is_some() {
                continue;
            }
            let Some(delta) = recurrence_expression_operand_range(
                config,
                slots,
                recurrence_deltas[operation_index]?,
                operation_index,
                induction_range,
                output_mask,
                carried_mask,
                &carried_ranges,
                has_control_flow,
            ) else {
                continue;
            };
            let contribution = match recurrence_kinds[operation_index]? {
                ScalarLongOpKind::Add => delta,
                ScalarLongOpKind::Subtract => LongInterval {
                    minimum: -delta.maximum,
                    maximum: -delta.minimum,
                },
                _ => unreachable!("linear recurrence admits only add or subtract"),
            };
            let count = i128::from(iterations);
            let minimum_delta = if contribution.minimum < 0 {
                contribution.minimum.checked_mul(count)?
            } else {
                0
            };
            let maximum_delta = if contribution.maximum > 0 {
                contribution.maximum.checked_mul(count)?
            } else {
                0
            };
            let initial = i128::from(slots[slot as usize]);
            let range = LongInterval::new(
                initial.checked_add(minimum_delta)?,
                initial.checked_add(maximum_delta)?,
            )?;
            operation_ranges[operation_index] = Some(range);
            carried_ranges[slot as usize] = Some(range);
            proven_count += 1;
        }
        if proven_count == before_count {
            return None;
        }
    }

    Some(LinearRecurrenceProof {
        operation_ranges,
        carried_mask,
    })
}

fn recurrence_update(
    operation: NativeStraightLongOperation,
    slot: u16,
) -> Option<(ScalarLongOpKind, QuickLongOperand)> {
    match operation {
        NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(lhs),
            rhs,
            destination,
            ..
        } if lhs == slot && destination == slot => Some((ScalarLongOpKind::Add, rhs)),
        NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs,
            rhs: QuickLongOperand::Slot(rhs),
            destination,
            ..
        } if rhs == slot && destination == slot => Some((ScalarLongOpKind::Add, lhs)),
        NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(lhs),
            rhs,
            destination,
            ..
        } if lhs == slot && destination == slot => Some((ScalarLongOpKind::Subtract, rhs)),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn recurrence_expression_operand_range(
    config: &NativeStraightLongLoopConfig,
    slots: &[i64; 64],
    operand: QuickLongOperand,
    before_operation: usize,
    induction_range: LongInterval,
    output_mask: u64,
    carried_mask: u64,
    carried_ranges: &[Option<LongInterval>; 64],
    require_dominating_definitions: bool,
) -> Option<LongInterval> {
    let slot = match operand {
        QuickLongOperand::Const(value) => return Some(LongInterval::exact(value)),
        QuickLongOperand::Slot(slot) if slot == config.induction_slot => {
            return Some(induction_range);
        }
        QuickLongOperand::Slot(slot) => slot,
    };
    let slot_mask = 1u64 << slot;
    if carried_mask & slot_mask != 0 {
        return carried_ranges[slot as usize];
    }
    let definition = config.operations[..before_operation]
        .iter()
        .copied()
        .enumerate()
        .rev()
        .find(|(_, operation)| operation.output_mask() & slot_mask != 0);
    let Some((definition_index, operation)) = definition else {
        return (output_mask & slot_mask == 0).then(|| LongInterval::exact(slots[slot as usize]));
    };
    if require_dominating_definitions
        && !straight_operation_dominates(config, definition_index, before_operation)
    {
        return None;
    }
    let operand_range = |operand| {
        recurrence_expression_operand_range(
            config,
            slots,
            operand,
            definition_index,
            induction_range,
            output_mask,
            carried_mask,
            carried_ranges,
            require_dominating_definitions,
        )
    };
    match operation {
        NativeStraightLongOperation::Modulo { value, divisor, .. } => {
            if divisor == 0 {
                None
            } else {
                Some(modulo_interval(
                    operand_range(value)?,
                    LongInterval::exact(divisor),
                ))
            }
        }
        NativeStraightLongOperation::Move { source, .. } => operand_range(source),
        NativeStraightLongOperation::Binary { kind, lhs, rhs, .. }
        | NativeStraightLongOperation::BinaryAssign { kind, lhs, rhs, .. } => {
            binary_interval(kind, operand_range(lhs)?, operand_range(rhs)?)
        }
        NativeStraightLongOperation::Unused
        | NativeStraightLongOperation::StringToken { .. }
        | NativeStraightLongOperation::StringLength { .. }
        | NativeStraightLongOperation::HashLoad { .. }
        | NativeStraightLongOperation::HashStore { .. }
        | NativeStraightLongOperation::Guard { .. }
        | NativeStraightLongOperation::BranchUnless { .. }
        | NativeStraightLongOperation::Jump { .. } => None,
    }
}

fn straight_operation_dominates(
    config: &NativeStraightLongLoopConfig,
    definition_index: usize,
    use_index: usize,
) -> bool {
    if definition_index >= use_index || use_index > config.operation_count as usize {
        return false;
    }
    let mut reachable_without_definition =
        [false; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1];
    reachable_without_definition[0] = true;
    for index in 0..use_index {
        if !reachable_without_definition[index] || index == definition_index {
            continue;
        }
        match config.operations[index] {
            NativeStraightLongOperation::BranchUnless { false_target, .. } => {
                reachable_without_definition[index + 1] = true;
                let false_target = false_target as usize;
                if false_target <= use_index {
                    reachable_without_definition[false_target] = true;
                }
            }
            NativeStraightLongOperation::Jump { target } => {
                let target = target as usize;
                if target <= use_index {
                    reachable_without_definition[target] = true;
                }
            }
            _ => reachable_without_definition[index + 1] = true,
        }
    }
    !reachable_without_definition[use_index]
}

fn recurrence_operation_for_slot(
    config: &NativeStraightLongLoopConfig,
    slot: u16,
) -> Option<usize> {
    let slot_mask = 1u64 << slot;
    let mut operation_index = None;
    for (index, operation) in config
        .operations
        .iter()
        .copied()
        .take(config.operation_count as usize)
        .enumerate()
    {
        if operation.output_mask() & slot_mask == 0 {
            continue;
        }
        if operation_index.replace(index).is_some() {
            return None;
        }
    }
    operation_index
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
    carried_mask: u64,
) -> bool {
    let available = |operand| {
        operand_range(
            operand,
            &state.ranges,
            induction_slot,
            induction_range,
            output_mask,
            state.definitely_written,
            carried_mask,
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

fn condition_operand_range(
    operand: NativeStraightLongConditionOperand,
    state: &StraightRangeState,
    induction_slot: u16,
    induction_range: LongInterval,
    output_mask: u64,
    carried_mask: u64,
) -> Option<LongInterval> {
    let range = |operand| {
        operand_range(
            operand,
            &state.ranges,
            induction_slot,
            induction_range,
            output_mask,
            state.definitely_written,
            carried_mask,
        )
    };
    match operand {
        NativeStraightLongConditionOperand::Source(source) => range(source),
        NativeStraightLongConditionOperand::BitwiseAnd { lhs, rhs } => {
            let lhs = range(lhs)?;
            let rhs = range(rhs)?;
            (lhs.minimum == lhs.maximum && rhs.minimum == rhs.maximum).then(|| {
                LongInterval::exact((lhs.minimum as i64) & (rhs.minimum as i64))
            })
        }
    }
}

fn interval_condition(
    kind: ScalarLongConditionKind,
    lhs: LongInterval,
    rhs: LongInterval,
) -> Option<bool> {
    match kind {
        ScalarLongConditionKind::Equal | ScalarLongConditionKind::NotEqual => {
            let equal = if lhs.maximum < rhs.minimum || rhs.maximum < lhs.minimum {
                false
            } else if lhs.minimum == lhs.maximum && lhs == rhs {
                true
            } else {
                return None;
            };
            Some(if kind == ScalarLongConditionKind::Equal {
                equal
            } else {
                !equal
            })
        }
        ScalarLongConditionKind::LessThan => {
            if lhs.maximum < rhs.minimum {
                Some(true)
            } else if lhs.minimum >= rhs.maximum {
                Some(false)
            } else {
                None
            }
        }
        ScalarLongConditionKind::LessThanOrEqual => {
            if lhs.maximum <= rhs.minimum {
                Some(true)
            } else if lhs.minimum > rhs.maximum {
                Some(false)
            } else {
                None
            }
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
    carried_mask: u64,
) -> Option<LongInterval> {
    match operand {
        QuickLongOperand::Const(value) => Some(LongInterval::exact(value)),
        QuickLongOperand::Slot(slot) if slot == induction_slot => Some(induction_range),
        QuickLongOperand::Slot(slot)
            if output_mask & (1u64 << slot) != 0
                && written_mask & (1u64 << slot) == 0
                && carried_mask & (1u64 << slot) == 0 =>
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
    carried_mask: u64,
) -> Option<(LongInterval, LongInterval)> {
    Some((
        operand_range(
            lhs,
            ranges,
            config.induction_slot,
            induction_range,
            output_mask,
            written_mask,
            carried_mask,
        )?,
        operand_range(
            rhs,
            ranges,
            config.induction_slot,
            induction_range,
            output_mask,
            written_mask,
            carried_mask,
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
        assert_eq!(forwarded_register, super::super::Arm64Register::from_code(8));
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

        assert_eq!(super::super::straight_binary_add_sub_immediate(Add, 11), Some((true, 11)));
        assert_eq!(super::super::straight_binary_add_sub_immediate(Add, -11), Some((false, 11)));
        assert_eq!(super::super::straight_binary_add_sub_immediate(Subtract, 11), Some((false, 11)));
        assert_eq!(super::super::straight_binary_add_sub_immediate(Subtract, -11), Some((true, 11)));
        assert_eq!(super::super::straight_binary_add_sub_immediate(Add, 4_095), Some((true, 4_095)));
        assert_eq!(super::super::straight_binary_add_sub_immediate(Add, 4_096), None);
        assert_eq!(super::super::straight_binary_add_sub_immediate(Add, i64::MIN), None);
        assert_eq!(super::super::straight_binary_add_sub_immediate(Multiply, 11), None);

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
                        lhs: NativeStraightLongConditionOperand::Source(
                            QuickLongOperand::Slot(0),
                        ),
                        rhs: NativeStraightLongConditionOperand::Source(
                            QuickLongOperand::Const(needle),
                        ),
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
        let carried_guard_proof =
            straight_long_remaining_range_proof(&carried_guard, &[0_i64; 64])
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
        let dominated_proof =
            straight_long_remaining_range_proof(&dominated_delta, &dominated_slots)
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
