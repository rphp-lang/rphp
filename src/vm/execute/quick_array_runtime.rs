// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_array_loop_kernel<Fetch, Body>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    kernel: QuickLongArrayLoopKernel,
    mut fetch: Fetch,
    mut execute_body: Body,
) -> Result<QuickLoopOutcome, VmError>
where
    Fetch: FnMut(&[i64; 64]) -> Option<i64>,
    Body: FnMut(&mut [i64; 64], &mut u64, &mut u64) -> Result<(), usize>,
{
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut iterations = 0u64;

    let mut continue_loop =
        slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
    if let Some(slot) = kernel.header_condition_tmp {
        slots[slot as usize] = i64::from(continue_loop);
        dirty_bool_mask |= 1u64 << slot;
    }

    while continue_loop {
        let Some(fetched) = fetch(&slots) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.fetch_resume_ip,
                iterations,
            ));
        };
        slots[kernel.fetch_result as usize] = fetched;
        dirty_long_mask |= 1u64 << kernel.fetch_result;
        if let Some(destination) = kernel.fetch_destination {
            slots[destination as usize] = fetched;
            dirty_long_mask |= 1u64 << destination;
        }

        if let Err(resume_ip) =
            execute_body(&mut slots, &mut dirty_long_mask, &mut dirty_bool_mask)
        {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                resume_ip,
                iterations,
            ));
        }

        let Some(incremented) = slots[kernel.post_value as usize].checked_add(1) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.post_resume_ip,
                iterations,
            ));
        };
        if let Some(result) = kernel.post_result {
            slots[result as usize] = slots[kernel.post_value as usize];
            dirty_long_mask |= 1u64 << result;
        }
        slots[kernel.post_value as usize] = incremented;
        dirty_long_mask |= 1u64 << kernel.post_value;

        continue_loop =
            slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(continue_loop);
            dirty_bool_mask |= 1u64 << slot;
        }
        iterations += 1;

        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            commit_quick_long_ops_slots(
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
            );
            let next_target = if continue_loop {
                kernel.body_target
            } else {
                kernel.exit_target
            };
            let next_ip = plan.target_ip(next_target).unwrap_unchecked();
            (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
            handle_interrupt(eg)?;
        }
    }

    commit_quick_long_ops_slots(
        slot_base,
        &slots,
        dirty_long_mask,
        dirty_bool_mask,
    );
    let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}

#[cfg(feature = "quick-loops")]
// Keep deoptimization, interrupt and slot-publication semantics in one source
// body while allowing both the general and fixed-prefix kernels to inline their
// own hot operations under different function-placement attributes.
macro_rules! run_quick_long_composed_array_loop {
    (
        $eg:ident,
        $frame:ident,
        $op_array:ident,
        $plan:ident,
        $slot_base:ident,
        $slots:ident,
        $kernel:ident,
        prefix = |$prefix_slots:ident, $prefix_dirty_long_mask:ident| $prefix:expr,
        fetch = |$fetch_slots:ident| $fetch:expr,
        body = |$body_slots:ident, $body_dirty_long_mask:ident, $body_dirty_bool_mask:ident| $body:expr $(,)?
    ) => {{
        let mut dirty_long_mask = 0u64;
        let mut dirty_bool_mask = 0u64;
        let mut iterations = 0u64;

        let mut continue_loop = $slots[$kernel.header_lhs as usize]
            < quick_long_operand(&$slots, $kernel.header_rhs);
        if let Some(slot) = $kernel.header_condition_tmp {
            $slots[slot as usize] = i64::from(continue_loop);
            dirty_bool_mask |= 1u64 << slot;
        }

        while continue_loop {
            let prefix_result = {
                let $prefix_slots = &mut $slots;
                let $prefix_dirty_long_mask = &mut dirty_long_mask;
                $prefix
            };
            if let Err(resume_ip) = prefix_result {
                return Ok(deopt_quick_long_kernel(
                    $frame,
                    $op_array,
                    $slot_base,
                    &$slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                    resume_ip,
                    iterations,
                ));
            }
            let fetched = {
                let $fetch_slots = &$slots;
                $fetch
            };
            let Some(fetched) = fetched else {
                return Ok(deopt_quick_long_kernel(
                    $frame,
                    $op_array,
                    $slot_base,
                    &$slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                    $kernel.fetch_resume_ip,
                    iterations,
                ));
            };
            $slots[$kernel.fetch_result as usize] = fetched;
            dirty_long_mask |= 1u64 << $kernel.fetch_result;
            if let Some(destination) = $kernel.fetch_destination {
                $slots[destination as usize] = fetched;
                dirty_long_mask |= 1u64 << destination;
            }

            let body_result = {
                let $body_slots = &mut $slots;
                let $body_dirty_long_mask = &mut dirty_long_mask;
                let $body_dirty_bool_mask = &mut dirty_bool_mask;
                $body
            };
            if let Err(resume_ip) = body_result {
                return Ok(deopt_quick_long_kernel(
                    $frame,
                    $op_array,
                    $slot_base,
                    &$slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                    resume_ip,
                    iterations,
                ));
            }

            let Some(incremented) = $slots[$kernel.post_value as usize].checked_add(1) else {
                return Ok(deopt_quick_long_kernel(
                    $frame,
                    $op_array,
                    $slot_base,
                    &$slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                    $kernel.post_resume_ip,
                    iterations,
                ));
            };
            if let Some(result) = $kernel.post_result {
                $slots[result as usize] = $slots[$kernel.post_value as usize];
                dirty_long_mask |= 1u64 << result;
            }
            $slots[$kernel.post_value as usize] = incremented;
            dirty_long_mask |= 1u64 << $kernel.post_value;

            continue_loop = $slots[$kernel.header_lhs as usize]
                < quick_long_operand(&$slots, $kernel.header_rhs);
            if let Some(slot) = $kernel.header_condition_tmp {
                $slots[slot as usize] = i64::from(continue_loop);
                dirty_bool_mask |= 1u64 << slot;
            }
            iterations += 1;

            if iterations & 31 == 0 && $eg.vm_interrupt.load(Ordering::Relaxed) {
                commit_quick_long_ops_slots(
                    $slot_base,
                    &$slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                let next_target = if continue_loop {
                    $kernel.body_target
                } else {
                    $kernel.exit_target
                };
                let next_ip = $plan.target_ip(next_target).unwrap_unchecked();
                (*$frame).opline = $op_array.instructions.as_ptr().add(next_ip);
                handle_interrupt($eg)?;
            }
        }

        commit_quick_long_ops_slots(
            $slot_base,
            &$slots,
            dirty_long_mask,
            dirty_bool_mask,
        );
        let next_ip = $kernel.exit_target.exit_ip().unwrap_unchecked();
        (*$frame).opline = $op_array.instructions.as_ptr().add(next_ip);
        stats::inc_quick_loop_completed(iterations);
        Ok(QuickLoopOutcome::Completed)
    }};
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_composed_array_loop_kernel<Fetch, Body>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    kernel: QuickLongArrayLoopKernel,
    prefix: &[QuickLongArrayPrefixOp],
    mut fetch: Fetch,
    mut execute_body: Body,
) -> Result<QuickLoopOutcome, VmError>
where
    Fetch: FnMut(&[i64; 64]) -> Option<i64>,
    Body: FnMut(&mut [i64; 64], &mut u64, &mut u64) -> Result<(), usize>,
{
    run_quick_long_composed_array_loop!(
        eg,
        frame,
        op_array,
        plan,
        slot_base,
        slots,
        kernel,
        prefix = |prefix_slots, prefix_dirty_long_mask| execute_quick_long_array_prefix(
            prefix_slots,
            prefix_dirty_long_mask,
            prefix,
        ),
        fetch = |fetch_slots| fetch(fetch_slots),
        body = |body_slots, body_dirty_long_mask, body_dirty_bool_mask| execute_body(
            body_slots,
            body_dirty_long_mask,
            body_dirty_bool_mask,
        ),
    )
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn execute_quick_long_add_assign(
    slots: &mut [i64; 64],
    dirty_long_mask: &mut u64,
    kernel: QuickLongAddAssignKernel,
) -> Result<(), usize> {
    let value = slots[kernel.lhs as usize]
        .checked_add(slots[kernel.rhs as usize])
        .ok_or(kernel.resume_ip)?;
    slots[kernel.result as usize] = value;
    slots[kernel.destination as usize] = value;
    *dirty_long_mask |= (1u64 << kernel.result) | (1u64 << kernel.destination);
    Ok(())
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn execute_quick_long_array_prefix_operation(
    slots: &mut [i64; 64],
    dirty_long_mask: &mut u64,
    operation: QuickLongArrayPrefixOp,
) -> Result<(), usize> {
    let value = apply_scalar_long_op(
        operation.kind,
        quick_long_operand(slots, operation.lhs),
        quick_long_operand(slots, operation.rhs),
    )
    .ok_or(operation.resume_ip)?;
    slots[operation.result as usize] = value;
    *dirty_long_mask |= 1u64 << operation.result;
    if let Some(destination) = operation.destination {
        slots[destination as usize] = value;
        *dirty_long_mask |= 1u64 << destination;
    }
    Ok(())
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn execute_quick_long_array_prefix(
    slots: &mut [i64; 64],
    dirty_long_mask: &mut u64,
    prefix: &[QuickLongArrayPrefixOp],
) -> Result<(), usize> {
    for operation in prefix {
        execute_quick_long_array_prefix_operation(slots, dirty_long_mask, *operation)?;
    }
    Ok(())
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn execute_fixed_quick_long_array_prefix<const PREFIX_LEN: usize>(
    slots: &mut [i64; 64],
    dirty_long_mask: &mut u64,
    prefix: &[QuickLongArrayPrefixOp; PREFIX_LEN],
) -> Result<(), usize> {
    let mut index = 0usize;
    while index < PREFIX_LEN {
        execute_quick_long_array_prefix_operation(
            slots,
            dirty_long_mask,
            *prefix.get(index).unwrap(),
        )?;
        index += 1;
    }
    Ok(())
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn execute_quick_long_add_add_assign(
    slots: &mut [i64; 64],
    dirty_long_mask: &mut u64,
    kernel: QuickLongAddAddAssignKernel,
) -> Result<(), usize> {
    let first = slots[kernel.first_lhs as usize]
        .checked_add(slots[kernel.first_rhs as usize])
        .ok_or(kernel.first_resume_ip)?;
    slots[kernel.first_result as usize] = first;
    *dirty_long_mask |= 1u64 << kernel.first_result;

    let second = slots[kernel.second_lhs as usize]
        .checked_add(slots[kernel.second_rhs as usize])
        .ok_or(kernel.second_resume_ip)?;
    slots[kernel.second_result as usize] = second;
    slots[kernel.destination as usize] = second;
    *dirty_long_mask |= (1u64 << kernel.second_result) | (1u64 << kernel.destination);
    Ok(())
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn execute_quick_long_conditional_add_assign(
    slots: &mut [i64; 64],
    dirty_long_mask: &mut u64,
    dirty_bool_mask: &mut u64,
    kernel: QuickLongConditionalAddAssignKernel,
    condition: bool,
) -> Result<(), usize> {
    if let Some(slot) = kernel.condition_tmp {
        slots[slot as usize] = i64::from(condition);
        *dirty_bool_mask |= 1u64 << slot;
    }
    if condition {
        let value = slots[kernel.lhs as usize]
            .checked_add(slots[kernel.rhs as usize])
            .ok_or(kernel.add_resume_ip)?;
        slots[kernel.result as usize] = value;
        slots[kernel.destination as usize] = value;
        *dirty_long_mask |= (1u64 << kernel.result) | (1u64 << kernel.destination);
    }
    Ok(())
}

#[cold]
#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_composed_indexed_array_one_add_kernel<const PREFIX_LEN: usize>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    array: *const PhpArray,
    index: QuickLongOperand,
    kernel: QuickLongArrayLoopKernel,
    add: QuickLongAddAssignKernel,
    prefix: &[QuickLongArrayPrefixOp; PREFIX_LEN],
) -> Result<QuickLoopOutcome, VmError> {
    let mut next_ordered_position = None;
    let mut ordered_misses = 0u8;
    let mut order_prediction_enabled = true;
    run_quick_long_composed_array_loop!(
        eg,
        frame,
        op_array,
        plan,
        slot_base,
        slots,
        kernel,
        prefix = |prefix_slots, prefix_dirty_long_mask| execute_fixed_quick_long_array_prefix(
            prefix_slots,
            prefix_dirty_long_mask,
            prefix,
        ),
        fetch = |fetch_slots| {
            let key = quick_long_operand(fetch_slots, index);
            let mut predicted_match = false;
            let mut fetched = None;
            if order_prediction_enabled
                && let Some(position) = next_ordered_position
            {
                if let Some(value) = (*array).get_ordered_int_at(position, key) {
                    predicted_match = true;
                    next_ordered_position = position.checked_add(1);
                    ordered_misses = 0;
                    fetched =
                        (value.value_type() == ValueType::Long).then(|| value.raw_long());
                } else {
                    ordered_misses += 1;
                    if ordered_misses == 2 {
                        order_prediction_enabled = false;
                        next_ordered_position = None;
                    }
                }
            }
            if !predicted_match {
                if let Some((position, value)) = (*array).get_indexed_long_with_position(key) {
                    if order_prediction_enabled {
                        next_ordered_position = position.checked_add(1);
                    }
                    fetched = Some(value);
                }
            }
            fetched
        },
        body = |body_slots, body_dirty_long_mask, _body_dirty_bool_mask| {
            execute_quick_long_add_assign(body_slots, body_dirty_long_mask, add)
        },
    )
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_indexed_array_two_adds_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    array: *const PhpArray,
    index: u16,
    kernel: QuickLongArrayLoopKernel,
    first: QuickLongAddAssignKernel,
    second: QuickLongAddAssignKernel,
) -> Result<QuickLoopOutcome, VmError> {
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut iterations = 0u64;

    let mut continue_loop =
        slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
    if let Some(slot) = kernel.header_condition_tmp {
        slots[slot as usize] = i64::from(continue_loop);
        dirty_bool_mask |= 1u64 << slot;
    }

    while continue_loop {
        let fetched = (*array).get_indexed_long(slots[index as usize]);
        let Some(fetched) = fetched else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.fetch_resume_ip,
                iterations,
            ));
        };
        slots[kernel.fetch_result as usize] = fetched;
        dirty_long_mask |= 1u64 << kernel.fetch_result;

        let Some(first_value) = slots[first.lhs as usize]
            .checked_add(slots[first.rhs as usize])
        else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                first.resume_ip,
                iterations,
            ));
        };
        slots[first.result as usize] = first_value;
        slots[first.destination as usize] = first_value;
        dirty_long_mask |= (1u64 << first.result) | (1u64 << first.destination);

        let Some(second_value) = slots[second.lhs as usize]
            .checked_add(slots[second.rhs as usize])
        else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                second.resume_ip,
                iterations,
            ));
        };
        slots[second.result as usize] = second_value;
        slots[second.destination as usize] = second_value;
        dirty_long_mask |= (1u64 << second.result) | (1u64 << second.destination);

        let Some(incremented) = slots[kernel.post_value as usize].checked_add(1) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.post_resume_ip,
                iterations,
            ));
        };
        if let Some(result) = kernel.post_result {
            slots[result as usize] = slots[kernel.post_value as usize];
            dirty_long_mask |= 1u64 << result;
        }
        slots[kernel.post_value as usize] = incremented;
        dirty_long_mask |= 1u64 << kernel.post_value;

        continue_loop =
            slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(continue_loop);
            dirty_bool_mask |= 1u64 << slot;
        }
        iterations += 1;

        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            commit_quick_long_ops_slots(
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
            );
            let next_target = if continue_loop {
                kernel.body_target
            } else {
                kernel.exit_target
            };
            let next_ip = plan.target_ip(next_target).unwrap_unchecked();
            (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
            handle_interrupt(eg)?;
        }
    }

    commit_quick_long_ops_slots(
        slot_base,
        &slots,
        dirty_long_mask,
        dirty_bool_mask,
    );
    let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_array_two_adds_kernel<Fetch>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    kernel: QuickLongArrayLoopKernel,
    first: QuickLongAddAssignKernel,
    second: QuickLongAddAssignKernel,
    mut fetch: Fetch,
) -> Result<QuickLoopOutcome, VmError>
where
    Fetch: FnMut(&[i64; 64]) -> Option<i64>,
{
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut iterations = 0u64;

    let mut continue_loop =
        slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
    if let Some(slot) = kernel.header_condition_tmp {
        slots[slot as usize] = i64::from(continue_loop);
        dirty_bool_mask |= 1u64 << slot;
    }

    while continue_loop {
        let Some(fetched) = fetch(&slots) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.fetch_resume_ip,
                iterations,
            ));
        };
        slots[kernel.fetch_result as usize] = fetched;
        dirty_long_mask |= 1u64 << kernel.fetch_result;
        if let Some(destination) = kernel.fetch_destination {
            slots[destination as usize] = fetched;
            dirty_long_mask |= 1u64 << destination;
        }

        if let Err(resume_ip) =
            execute_quick_long_add_assign(&mut slots, &mut dirty_long_mask, first)
        {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                resume_ip,
                iterations,
            ));
        }
        if let Err(resume_ip) =
            execute_quick_long_add_assign(&mut slots, &mut dirty_long_mask, second)
        {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                resume_ip,
                iterations,
            ));
        }

        let Some(incremented) = slots[kernel.post_value as usize].checked_add(1) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.post_resume_ip,
                iterations,
            ));
        };
        if let Some(result) = kernel.post_result {
            slots[result as usize] = slots[kernel.post_value as usize];
            dirty_long_mask |= 1u64 << result;
        }
        slots[kernel.post_value as usize] = incremented;
        dirty_long_mask |= 1u64 << kernel.post_value;

        continue_loop =
            slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(continue_loop);
            dirty_bool_mask |= 1u64 << slot;
        }
        iterations += 1;

        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            commit_quick_long_ops_slots(
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
            );
            let next_target = if continue_loop {
                kernel.body_target
            } else {
                kernel.exit_target
            };
            let next_ip = plan.target_ip(next_target).unwrap_unchecked();
            (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
            handle_interrupt(eg)?;
        }
    }

    commit_quick_long_ops_slots(
        slot_base,
        &slots,
        dirty_long_mask,
        dirty_bool_mask,
    );
    let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn dispatch_quick_long_array_body_kernel<Fetch>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: [i64; 64],
    kernel: QuickLongArrayLoopKernel,
    body: QuickLongArrayBodyKernel,
    fetch: Fetch,
) -> Result<QuickLoopOutcome, VmError>
where
    Fetch: FnMut(&[i64; 64]) -> Option<i64>,
{
    match body {
        QuickLongArrayBodyKernel::OneAdd { add } => {
            run_quick_long_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                fetch,
                move |slots, dirty_long_mask, _| {
                    execute_quick_long_add_assign(slots, dirty_long_mask, add)
                },
            )
        }
        QuickLongArrayBodyKernel::TwoAdds { first, second } => {
            run_quick_long_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                fetch,
                move |slots, dirty_long_mask, _| {
                    execute_quick_long_add_assign(slots, dirty_long_mask, first)?;
                    execute_quick_long_add_assign(slots, dirty_long_mask, second)
                },
            )
        }
        QuickLongArrayBodyKernel::AddFusedAddAdd {
            first,
            middle,
            last,
        } => run_quick_long_array_loop_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            fetch,
            move |slots, dirty_long_mask, _| {
                execute_quick_long_add_assign(slots, dirty_long_mask, first)?;
                execute_quick_long_add_add_assign(slots, dirty_long_mask, middle)?;
                execute_quick_long_add_assign(slots, dirty_long_mask, last)
            },
        ),
        QuickLongArrayBodyKernel::ConditionalAdd { first, second } => match first.condition {
            QuickLongCondition::Lt { lhs, rhs } => run_quick_long_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                fetch,
                move |slots, dirty_long_mask, dirty_bool_mask| {
                    let condition =
                        slots[lhs as usize] < quick_long_operand(slots, rhs);
                    execute_quick_long_conditional_add_assign(
                        slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                        first,
                        condition,
                    )?;
                    execute_quick_long_add_assign(slots, dirty_long_mask, second)
                },
            ),
            QuickLongCondition::Eq { lhs, rhs } => run_quick_long_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                fetch,
                move |slots, dirty_long_mask, dirty_bool_mask| {
                    let condition =
                        slots[lhs as usize] == quick_long_operand(slots, rhs);
                    execute_quick_long_conditional_add_assign(
                        slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                        first,
                        condition,
                    )?;
                    execute_quick_long_add_assign(slots, dirty_long_mask, second)
                },
            ),
        },
    }
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn dispatch_quick_long_composed_array_body_kernel<Fetch>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: [i64; 64],
    kernel: QuickLongArrayLoopKernel,
    body: QuickLongArrayBodyKernel,
    prefix: &[QuickLongArrayPrefixOp],
    fetch: Fetch,
) -> Result<QuickLoopOutcome, VmError>
where
    Fetch: FnMut(&[i64; 64]) -> Option<i64>,
{
    match body {
        QuickLongArrayBodyKernel::OneAdd { add } => {
            run_quick_long_composed_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                prefix,
                fetch,
                move |slots, dirty_long_mask, _| {
                    execute_quick_long_add_assign(slots, dirty_long_mask, add)
                },
            )
        }
        QuickLongArrayBodyKernel::TwoAdds { first, second } => {
            run_quick_long_composed_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                prefix,
                fetch,
                move |slots, dirty_long_mask, _| {
                    execute_quick_long_add_assign(slots, dirty_long_mask, first)?;
                    execute_quick_long_add_assign(slots, dirty_long_mask, second)
                },
            )
        }
        QuickLongArrayBodyKernel::AddFusedAddAdd {
            first,
            middle,
            last,
        } => run_quick_long_composed_array_loop_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            prefix,
            fetch,
            move |slots, dirty_long_mask, _| {
                execute_quick_long_add_assign(slots, dirty_long_mask, first)?;
                execute_quick_long_add_add_assign(slots, dirty_long_mask, middle)?;
                execute_quick_long_add_assign(slots, dirty_long_mask, last)
            },
        ),
        QuickLongArrayBodyKernel::ConditionalAdd { first, second } => match first.condition {
            QuickLongCondition::Lt { lhs, rhs } => run_quick_long_composed_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                prefix,
                fetch,
                move |slots, dirty_long_mask, dirty_bool_mask| {
                    let condition = slots[lhs as usize] < quick_long_operand(slots, rhs);
                    execute_quick_long_conditional_add_assign(
                        slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                        first,
                        condition,
                    )?;
                    execute_quick_long_add_assign(slots, dirty_long_mask, second)
                },
            ),
            QuickLongCondition::Eq { lhs, rhs } => run_quick_long_composed_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                prefix,
                fetch,
                move |slots, dirty_long_mask, dirty_bool_mask| {
                    let condition = slots[lhs as usize] == quick_long_operand(slots, rhs);
                    execute_quick_long_conditional_add_assign(
                        slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                        first,
                        condition,
                    )?;
                    execute_quick_long_add_assign(slots, dirty_long_mask, second)
                },
            ),
        },
    }
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_exact_int_array_one_add_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    exact_layout: QuickLongExactIntLayout,
    array: *const PhpArray,
    index: QuickLongOperand,
    kernel: QuickLongArrayLoopKernel,
    add: QuickLongAddAssignKernel,
) -> Result<QuickLoopOutcome, VmError> {
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut iterations = 0u64;

    let mut continue_loop =
        slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
    if let Some(slot) = kernel.header_condition_tmp {
        slots[slot as usize] = i64::from(continue_loop);
        dirty_bool_mask |= 1u64 << slot;
    }

    while continue_loop {
        let key = quick_long_operand(&slots, index);
        let Some(fetched) = exact_layout.long_at(array, key) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.fetch_resume_ip,
                iterations,
            ));
        };
        slots[kernel.fetch_result as usize] = fetched;
        dirty_long_mask |= 1u64 << kernel.fetch_result;
        if let Some(destination) = kernel.fetch_destination {
            slots[destination as usize] = fetched;
            dirty_long_mask |= 1u64 << destination;
        }

        if let Err(resume_ip) =
            execute_quick_long_add_assign(&mut slots, &mut dirty_long_mask, add)
        {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                resume_ip,
                iterations,
            ));
        }

        let Some(incremented) = slots[kernel.post_value as usize].checked_add(1) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.post_resume_ip,
                iterations,
            ));
        };
        if let Some(result) = kernel.post_result {
            slots[result as usize] = slots[kernel.post_value as usize];
            dirty_long_mask |= 1u64 << result;
        }
        slots[kernel.post_value as usize] = incremented;
        dirty_long_mask |= 1u64 << kernel.post_value;

        continue_loop =
            slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(continue_loop);
            dirty_bool_mask |= 1u64 << slot;
        }
        iterations += 1;

        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            commit_quick_long_ops_slots(
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
            );
            let next_target = if continue_loop {
                kernel.body_target
            } else {
                kernel.exit_target
            };
            let next_ip = plan.target_ip(next_target).unwrap_unchecked();
            (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
            handle_interrupt(eg)?;
        }
    }

    commit_quick_long_ops_slots(
        slot_base,
        &slots,
        dirty_long_mask,
        dirty_bool_mask,
    );
    let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn dispatch_quick_long_composed_array_loop_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: [i64; 64],
    arrays: &[QuickLongArray; 64],
    int_position_hints: &[Option<QuickLongIntPositionHint>; 64],
    indexed_int_array_mask: u64,
    exact_int_layout: Option<QuickLongExactIntLayout>,
    kernel: QuickLongArrayLoopKernel,
    body: QuickLongArrayBodyKernel,
    prefix: &[QuickLongArrayPrefixOp],
) -> Result<QuickLoopOutcome, VmError> {
    let array = arrays[kernel.array as usize];
    if let (
        Some(exact_layout),
        QuickLongArray::Hash { array },
        QuickArrayIndex::Long(index),
    ) = (exact_int_layout, array, kernel.index)
    {
        return dispatch_quick_long_composed_array_body_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            body,
            prefix,
            move |slots| exact_layout.long_at(array, quick_long_operand(slots, index)),
        );
    }

    if let (
        Some(position_hint),
        QuickLongArray::Hash { array },
        QuickArrayIndex::Long(index),
    ) = (
        int_position_hints[kernel.array as usize],
        array,
        kernel.index,
    ) {
        return dispatch_quick_long_composed_array_body_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            body,
            prefix,
            move |slots| {
                (*array)
                    .get_positioned_int(
                        quick_long_operand(slots, index),
                        position_hint.first_key,
                        position_hint.stride,
                    )
                    .and_then(|value| {
                        (value.value_type() == ValueType::Long).then(|| value.raw_long())
                    })
            },
        );
    }

    if indexed_int_array_mask & (1u64 << kernel.array) != 0 {
        if let (QuickLongArray::Hash { array }, QuickArrayIndex::Long(index)) =
            (array, kernel.index)
        {
            if let QuickLongArrayBodyKernel::OneAdd { add } = body {
                if let Ok(fixed_prefix) = <&[QuickLongArrayPrefixOp; 3]>::try_from(prefix) {
                    return run_quick_long_composed_indexed_array_one_add_kernel::<3>(
                        eg,
                        frame,
                        op_array,
                        plan,
                        slot_base,
                        slots,
                        array,
                        index,
                        kernel,
                        add,
                        fixed_prefix,
                    );
                }
            }
            let mut next_ordered_position = None;
            let mut ordered_misses = 0u8;
            let mut order_prediction_enabled = true;
            return dispatch_quick_long_composed_array_body_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                body,
                prefix,
                move |slots| {
                    let key = quick_long_operand(slots, index);
                    if order_prediction_enabled
                        && let Some(position) = next_ordered_position
                    {
                        if let Some(value) = (*array).get_ordered_int_at(position, key) {
                            next_ordered_position = position.checked_add(1);
                            ordered_misses = 0;
                            return (value.value_type() == ValueType::Long)
                                .then(|| value.raw_long());
                        }
                        ordered_misses += 1;
                        if ordered_misses == 2 {
                            order_prediction_enabled = false;
                            next_ordered_position = None;
                        }
                    }

                    let (position, value) = (*array).get_indexed_long_with_position(key)?;
                    if order_prediction_enabled {
                        next_ordered_position = position.checked_add(1);
                    }
                    Some(value)
                },
            );
        }
    }

    dispatch_quick_long_composed_array_body_kernel(
        eg,
        frame,
        op_array,
        plan,
        slot_base,
        slots,
        kernel,
        body,
        prefix,
        move |slots| array.long_at(kernel.index, slots, op_array),
    )
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn dispatch_quick_long_array_loop_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: [i64; 64],
    arrays: &[QuickLongArray; 64],
    int_position_hints: &[Option<QuickLongIntPositionHint>; 64],
    indexed_int_array_mask: u64,
    kernel: QuickLongArrayLoopKernel,
    body: QuickLongArrayBodyKernel,
) -> Result<QuickLoopOutcome, VmError> {
    let array = arrays[kernel.array as usize];
    let int_position_hint = int_position_hints[kernel.array as usize];
    if let QuickLongArrayBodyKernel::TwoAdds { first, second } = body {
        if let (
            Some(position_hint),
            QuickLongArray::Hash { array },
            QuickArrayIndex::Long(index),
        ) = (int_position_hint, array, kernel.index)
        {
            return run_quick_long_array_two_adds_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                first,
                second,
                move |slots| {
                    (*array)
                        .get_positioned_int(
                            quick_long_operand(slots, index),
                            position_hint.first_key,
                            position_hint.stride,
                        )
                        .and_then(|value| {
                            (value.value_type() == ValueType::Long)
                                .then(|| value.raw_long())
                        })
                },
            );
        }
        if indexed_int_array_mask & (1u64 << kernel.array) != 0 {
            if let (
                QuickLongArray::Hash { array },
                QuickArrayIndex::Long(QuickLongOperand::Slot(index)),
                None,
            ) = (array, kernel.index, kernel.fetch_destination)
            {
                return run_quick_long_indexed_array_two_adds_kernel(
                    eg,
                    frame,
                    op_array,
                    plan,
                    slot_base,
                    slots,
                    array,
                    index,
                    kernel,
                    first,
                    second,
                );
            }
            if let (
                QuickLongArray::Hash { array },
                QuickArrayIndex::Long(index),
            ) = (array, kernel.index)
            {
                return run_quick_long_array_two_adds_kernel(
                    eg,
                    frame,
                    op_array,
                    plan,
                    slot_base,
                    slots,
                    kernel,
                    first,
                    second,
                    move |slots| {
                        (*array).get_indexed_long(quick_long_operand(slots, index))
                    },
                );
            }
        }
        return run_quick_long_array_two_adds_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            first,
            second,
            move |slots| array.long_at(kernel.index, slots, op_array),
        );
    }

    if let (
        Some(position_hint),
        QuickLongArray::Hash { array },
        QuickArrayIndex::Long(index),
    ) = (int_position_hint, array, kernel.index)
    {
        return dispatch_quick_long_array_body_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            body,
            move |slots| {
                (*array)
                    .get_positioned_int(
                        quick_long_operand(slots, index),
                        position_hint.first_key,
                        position_hint.stride,
                    )
                    .and_then(|value| {
                        (value.value_type() == ValueType::Long)
                            .then(|| value.raw_long())
                    })
            },
        );
    }

    if indexed_int_array_mask & (1u64 << kernel.array) != 0 {
        if let (
            QuickLongArray::Hash { array },
            QuickArrayIndex::Long(index),
        ) = (array, kernel.index)
        {
            return dispatch_quick_long_array_body_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                body,
                move |slots| {
                    (*array).get_indexed_long(quick_long_operand(slots, index))
                },
            );
        }
    }

    dispatch_quick_long_array_body_kernel(
        eg,
        frame,
        op_array,
        plan,
        slot_base,
        slots,
        kernel,
        body,
        move |slots| array.long_at(kernel.index, slots, op_array),
    )
}
