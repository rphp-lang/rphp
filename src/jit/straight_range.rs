use super::{
    NATIVE_STRAIGHT_LONG_MAX_OPERATIONS, NativeStraightLongConditionOperand,
    NativeStraightLongLoopConfig, NativeStraightLongOperation, QuickLongOperand,
    ScalarLongConditionKind, ScalarLongOpKind, straight_long_operation_input_mask,
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
            NativeStraightLongOperation::ArrayLongSet { key, value, .. } => {
                operand_range(
                    key,
                    &state.ranges,
                    config.induction_slot,
                    induction_range,
                    output_mask,
                    state.definitely_written,
                    recurrence.carried_mask,
                )?;
                operand_range(
                    value,
                    &state.ranges,
                    config.induction_slot,
                    induction_range,
                    output_mask,
                    state.definitely_written,
                    recurrence.carried_mask,
                )?;
            }
            NativeStraightLongOperation::Unused
            | NativeStraightLongOperation::StringToken { .. }
            | NativeStraightLongOperation::StringLength { .. }
            | NativeStraightLongOperation::HashLoad { .. }
            | NativeStraightLongOperation::HashStore { .. }
            | NativeStraightLongOperation::IndexedLongLoad { .. } => return None,
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
        | NativeStraightLongOperation::IndexedLongLoad { .. }
        | NativeStraightLongOperation::ArrayLongSet { .. }
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
    let mut reachable_without_definition = [false; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1];
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
            (lhs.minimum == lhs.maximum && rhs.minimum == rhs.maximum)
                .then(|| LongInterval::exact((lhs.minimum as i64) & (rhs.minimum as i64)))
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
        ScalarLongOpKind::Compare => LongInterval::new(-1, 1),
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
        ScalarLongOpKind::BitwiseAnd
        | ScalarLongOpKind::BitwiseOr
        | ScalarLongOpKind::BitwiseXor => {
            if lhs.minimum == lhs.maximum && rhs.minimum == rhs.maximum {
                let lhs = lhs.minimum as i64;
                let rhs = rhs.minimum as i64;
                Some(LongInterval::exact(match kind {
                    ScalarLongOpKind::BitwiseAnd => lhs & rhs,
                    ScalarLongOpKind::BitwiseOr => lhs | rhs,
                    ScalarLongOpKind::BitwiseXor => lhs ^ rhs,
                    _ => unreachable!(),
                }))
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
#[path = "straight_range_tests.rs"]
mod tests;
