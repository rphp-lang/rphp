// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn quick_long_operand(slots: &[i64; 64], operand: QuickLongOperand) -> i64 {
    match operand {
        QuickLongOperand::Slot(slot) => slots[slot as usize],
        QuickLongOperand::Const(value) => value,
    }
}

const QUICK_UNIT_LOOP_CHUNK: i64 = 32;

/// Prove one complete interrupt-sized unit-induction interval. Every slot
/// capable of changing the loop bound is rejected, leaving unusual aliasing
/// layouts on the exact one-step implementation.
#[inline(always)]
#[cfg(feature = "quick-loops")]
fn quick_unit_loop_chunk(
    slots: &[i64; 64],
    header_lhs: u16,
    header_rhs: QuickLongOperand,
    condition_tmp: Option<u16>,
    post_value: u16,
    post_result: Option<u16>,
) -> Option<(i64, i64, i64)> {
    if header_lhs != post_value
        || condition_tmp == Some(post_value)
        || post_result == Some(post_value)
        || condition_tmp.is_some() && condition_tmp == post_result
    {
        return None;
    }
    if let QuickLongOperand::Slot(bound_slot) = header_rhs
        && (bound_slot == post_value
            || condition_tmp == Some(bound_slot)
            || post_result == Some(bound_slot))
    {
        return None;
    }
    let current = slots[post_value as usize];
    let bound = quick_long_operand(slots, header_rhs);
    let end = current.checked_add(QUICK_UNIT_LOOP_CHUNK)?;
    (end <= bound).then_some((current, end, bound))
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn quick_typed_method_arguments(
    slots: &[i64; 64],
    call: &QuickTypedMethodCall,
) -> [i64; 8] {
    let mut arguments = [0i64; 8];
    for (index, source) in call
        .arguments
        .iter()
        .copied()
        .take(call.argument_count as usize)
        .enumerate()
    {
        arguments[index] = quick_long_operand(slots, source);
    }
    arguments
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn evaluate_quick_object_long_method(
    receiver: *const Value,
    user: *const UserFunction,
    plan: *const ObjectLongFunctionPlan,
    arguments: &[QuickObjectLongArgument; 8],
    argument_count: u8,
    long_slots: &[i64; 64],
    string_state: &QuickStringSlotState,
) -> Option<i64> {
    let mut slots = [const { std::mem::MaybeUninit::<i64>::uninit() }; 64];
    let mut initialized = 0u64;
    let object_arguments = [ObjectLongArgument::None; 8];
    let mut string_arguments = [std::ptr::null(); 8];

    for (index, source) in arguments
        .iter()
        .copied()
        .take(argument_count as usize)
        .enumerate()
    {
        match source {
            QuickObjectLongArgument::Long(source) => {
                let slot = (*user).common.sig.param_cv_index(index as u32) as usize;
                slots[slot].write(quick_long_operand(long_slots, source));
                initialized |= 1u64 << slot;
            }
            QuickObjectLongArgument::StringSlot(slot) => {
                string_arguments[index] = string_state.value(slot) as *const Value;
            }
        }
    }

    evaluate_object_long_plan(
        &*receiver,
        &object_arguments,
        &string_arguments,
        &mut slots,
        initialized,
        &*user,
        &*plan,
    )
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn evaluate_quick_composed_typed_method(
    plan: *const ComposedTypedLongFunctionPlan,
    arguments: &[QuickObjectLongArgument; 8],
    argument_count: u8,
    long_slots: &[i64; 64],
    string_state: &QuickStringSlotState,
) -> Option<i64> {
    let mut scalar_arguments = [0i64; 8];
    for (index, source) in arguments
        .iter()
        .copied()
        .take(argument_count as usize)
        .enumerate()
    {
        match source {
            QuickObjectLongArgument::Long(source) => {
                scalar_arguments[index] = quick_long_operand(long_slots, source);
            }
            QuickObjectLongArgument::StringSlot(slot) => {
                let value = string_state.value(slot);
                if value.is_reference() {
                    return None;
                }
                scalar_arguments[index] = i64::try_from(value.as_str()?.len()).ok()?;
            }
        }
    }

    let plan = &*plan;
    if plan.program.output_count != 1 {
        return None;
    }
    let mut temporaries = [0i64; COMPOSED_SCALAR_MAX_OPS];
    for (operation_index, operation) in plan.program.operations.iter().enumerate() {
        temporaries[operation_index] = match operation {
            ComposedTypedLongOp::Arithmetic(operation) => {
                let lhs = resolve_composed_body_source(
                    operation.lhs,
                    &scalar_arguments,
                    &temporaries,
                );
                let rhs = resolve_composed_body_source(
                    operation.rhs,
                    &scalar_arguments,
                    &temporaries,
                );
                apply_scalar_long_op(operation.kind, lhs, rhs)?
            }
            ComposedTypedLongOp::StringConcatLiteral { value, literal_len } => {
                resolve_quick_direct_string_source(
                    *value,
                    &scalar_arguments,
                    &temporaries,
                )?
                .checked_add(*literal_len as i64)?
            }
            ComposedTypedLongOp::StringLength(source) => {
                resolve_quick_direct_string_source(
                    *source,
                    &scalar_arguments,
                    &temporaries,
                )?
            }
            ComposedTypedLongOp::Call(_) | ComposedTypedLongOp::StringCall(_) => return None,
        };
    }
    Some(resolve_composed_body_source(
        plan.program.outputs[0],
        &scalar_arguments,
        &temporaries,
    ))
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn resolve_quick_direct_string_source(
    source: ScalarStringSource,
    arguments: &[i64; 8],
    temporaries: &[i64; COMPOSED_SCALAR_MAX_OPS],
) -> Option<i64> {
    match source {
        ScalarStringSource::Input(index) => arguments.get(index as usize).copied(),
        ScalarStringSource::Temporary(index) => temporaries.get(index as usize).copied(),
    }
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn deopt_quick_long_kernel(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    slot_base: *mut Value,
    slots: &[i64; 64],
    dirty_long_mask: u64,
    dirty_bool_mask: u64,
    resume_ip: usize,
    iterations: u64,
) -> QuickLoopOutcome {
    commit_quick_long_ops_slots(
        slot_base,
        slots,
        dirty_long_mask,
        dirty_bool_mask,
    );
    (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
    stats::inc_quick_loop_deoptimized(iterations);
    QuickLoopOutcome::Deoptimized
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn deopt_quick_typed_method_call(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    slot_base: *mut Value,
    slots: &[i64; 64],
    dirty_long_mask: u64,
    dirty_bool_mask: u64,
    string_state: &mut QuickStringSlotState,
    call: QuickTypedMethodCall,
    iterations: u64,
) -> QuickLoopOutcome {
    string_state.commit();
    deopt_quick_long_kernel(
        frame,
        op_array,
        slot_base,
        slots,
        dirty_long_mask,
        dirty_bool_mask,
        call.resume_ip,
        iterations,
    )
}

#[cfg(all(test, feature = "quick-loops"))]
mod quick_unit_loop_chunk_tests {
    use super::{QuickLongOperand, quick_unit_loop_chunk};

    #[test]
    fn proves_one_complete_invariant_unit_interval() {
        let mut slots = [0i64; 64];
        slots[0] = 5;
        slots[1] = 37;
        assert_eq!(
            quick_unit_loop_chunk(&slots, 0, QuickLongOperand::Const(37), Some(2), 0, Some(3)),
            Some((5, 37, 37))
        );
        assert_eq!(
            quick_unit_loop_chunk(&slots, 0, QuickLongOperand::Slot(1), Some(2), 0, Some(3)),
            Some((5, 37, 37))
        );
    }

    #[test]
    fn rejects_short_overflowing_and_mutable_bound_intervals() {
        let mut slots = [0i64; 64];
        slots[0] = 5;
        slots[1] = 36;
        assert!(
            quick_unit_loop_chunk(&slots, 0, QuickLongOperand::Slot(1), Some(2), 0, Some(3))
                .is_none()
        );
        slots[0] = i64::MAX - 31;
        assert!(
            quick_unit_loop_chunk(
                &slots,
                0,
                QuickLongOperand::Const(i64::MAX),
                Some(2),
                0,
                Some(3),
            )
            .is_none()
        );
        slots[0] = 0;
        slots[3] = 100;
        assert!(
            quick_unit_loop_chunk(&slots, 0, QuickLongOperand::Slot(3), Some(2), 0, Some(3))
                .is_none()
        );
    }
}
