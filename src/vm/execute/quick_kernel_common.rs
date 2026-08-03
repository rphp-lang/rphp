// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn quick_long_operand(slots: &[i64; 64], operand: QuickLongOperand) -> i64 {
    match operand {
        QuickLongOperand::Slot(slot) => slots[slot as usize],
        QuickLongOperand::Const(value) => value,
    }
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

