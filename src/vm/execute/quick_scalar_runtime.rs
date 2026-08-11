// Kept in the execute module through include! so this structural split does not change visibility or code generation.

/// Execute the compact typed argument plan emitted once by the quick planner.
/// The original Init/argument/Send region remains the transactional baseline
/// fallback, but successful iterations no longer rescan its bytecode.
#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn resolve_quick_scalar_source<const TEMPORARY_CAPACITY: usize>(
    source: ScalarLongSource,
    slot_base: *mut Value,
    induction_cv: u16,
    accumulator_cv: u16,
    induction: i64,
    accumulator: i64,
    temporaries: &[i64; TEMPORARY_CAPACITY],
) -> Option<i64> {
    match source {
        ScalarLongSource::Input(slot) if slot == induction_cv => Some(induction),
        ScalarLongSource::Input(slot) if slot == accumulator_cv => Some(accumulator),
        ScalarLongSource::Input(slot) => Some((*slot_base.add(slot as usize)).raw_long()),
        ScalarLongSource::Constant(value) => Some(value),
        ScalarLongSource::Temporary(index) => temporaries.get(index as usize).copied(),
    }
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn evaluate_quick_scalar_call_arguments(
    argument_plan: &ScalarLongProgram,
    argument_count: u8,
    long_argument_mask: u8,
    slot_base: *mut Value,
    induction_cv: u16,
    accumulator_cv: u16,
    induction: i64,
    accumulator: i64,
) -> Option<[i64; 8]> {
    if argument_plan.operations.len() > 8
        || argument_plan.output_count != argument_count
    {
        return None;
    }
    let mut temporaries = [0i64; 8];
    for (index, operation) in argument_plan.operations.iter().copied().enumerate() {
        let lhs = resolve_quick_scalar_source(
            operation.lhs,
            slot_base,
            induction_cv,
            accumulator_cv,
            induction,
            accumulator,
            &temporaries,
        )?;
        let rhs = resolve_quick_scalar_source(
            operation.rhs,
            slot_base,
            induction_cv,
            accumulator_cv,
            induction,
            accumulator,
            &temporaries,
        )?;
        temporaries[index] = apply_scalar_long_op(operation.kind, lhs, rhs)?;
    }
    let mut arguments = [0i64; 8];
    for (index, output) in argument_plan.outputs
        .iter()
        .copied()
        .take(argument_plan.output_count as usize)
        .enumerate()
    {
        if long_argument_mask & (1u8 << index) != 0 {
            arguments[index] = resolve_quick_scalar_source(
                output,
                slot_base,
                induction_cv,
                accumulator_cv,
                induction,
                accumulator,
                &temporaries,
            )?;
        }
    }
    Some(arguments)
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn evaluate_quick_fused_scalar_program(
    program: &ScalarLongProgram<ScalarLongOp, 1>,
    slot_base: *mut Value,
    induction_cv: u16,
    accumulator_cv: u16,
    induction: i64,
    accumulator: i64,
) -> Option<i64> {
    debug_assert!(program.operations.len() <= 16);
    debug_assert_eq!(program.output_count, 1);
    let mut temporaries = [0i64; 16];
    macro_rules! evaluate_operation {
        ($index:expr) => {{
            let index = $index;
            let operation = program.operations[index];
            let lhs = resolve_quick_scalar_source(
                operation.lhs,
                slot_base,
                induction_cv,
                accumulator_cv,
                induction,
                accumulator,
                &temporaries,
            )?;
            let rhs = resolve_quick_scalar_source(
                operation.rhs,
                slot_base,
                induction_cv,
                accumulator_cv,
                induction,
                accumulator,
                &temporaries,
            )?;
            temporaries[index] = apply_scalar_long_op(operation.kind, lhs, rhs)?;
        }};
    }

    // Scalar leaves are deliberately small. Removing the inner operation-loop
    // branch for the most common sizes keeps this a generic IR executor while
    // avoiding a second interpreter loop inside every quick-loop iteration.
    match program.operations.len() {
        0 => {}
        1 => evaluate_operation!(0),
        2 => {
            evaluate_operation!(0);
            evaluate_operation!(1);
        }
        3 => {
            evaluate_operation!(0);
            evaluate_operation!(1);
            evaluate_operation!(2);
        }
        4 => {
            evaluate_operation!(0);
            evaluate_operation!(1);
            evaluate_operation!(2);
            evaluate_operation!(3);
        }
        _ => {
            for index in 0..program.operations.len() {
                evaluate_operation!(index);
            }
        }
    }
    resolve_quick_scalar_source(
        program.outputs[0],
        slot_base,
        induction_cv,
        accumulator_cv,
        induction,
        accumulator,
        &temporaries,
    )
}

/// Resolve either direct-function or monomorphic-method dispatch for a quick
/// scalar region. Method identity is valid only while the receiver's current
/// class id matches the canonical method inline cache.
#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn guarded_quick_scalar_call_target(
    eg: &ExecutorGlobals,
    op_array: &crate::compiler::OpArray,
    slot_base: *mut Value,
    guard: ScalarLongCallGuard,
    argument_count: u8,
) -> Option<(*const FunctionCommon, *const UserFunction)> {
    if !direct_user_calls_enabled() {
        return None;
    }
    let receiver = match guard {
        ScalarLongCallGuard::FunctionCache { .. } => {
            let ip = guard.cache_ip();
            let initializer = op_array.instructions.get(ip)?;
            let cache = op_array.cache.get(ip)?;
            if initializer.opcode != OpCode::InitFcall || cache.func.is_null() {
                return None;
            }
            let common = &*cache.func;
            if common.fn_type != FunctionType::User
                || common.sig.public_arity() != argument_count as u32
            {
                return None;
            }
            return Some((cache.func, cache.func as *const UserFunction));
        }
        ScalarLongCallGuard::MethodCache { receiver_slot, .. } => {
            Some(&*slot_base.add(receiver_slot as usize))
        }
    };
    let receiver = receiver?;
    let (target, user) = guarded_quick_long_method_target(
        eg,
        op_array,
        guard,
        receiver,
        argument_count as usize,
    )?;
    guarded_scalar_user_target(target, argument_count as usize)?;
    Some((target, user))
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn prepare_quick_composed_object_arguments(
    eg: &ExecutorGlobals,
    caller_op_array: &crate::compiler::OpArray,
    slot_base: *mut Value,
    target: *const FunctionCommon,
    public_args: u8,
    long_argument_mask: u8,
    object_argument_mask: u8,
    argument_plan: &ScalarLongProgram,
) -> Option<[*const Value; 8]> {
    let argument_count = public_args as usize;
    let expected_mask = if argument_count == 8 {
        u8::MAX
    } else {
        (1u8 << argument_count) - 1
    };
    if argument_plan.output_count != public_args
        || long_argument_mask & object_argument_mask != 0
        || long_argument_mask | object_argument_mask != expected_mask
    {
        return None;
    }

    let common = &*target;
    let callee_class = eg.declaring_class_of(target).map(str::to_string);
    let mut object_arguments = [std::ptr::null(); 8];
    for index in 0..argument_count {
        let output = argument_plan.outputs[index];
        if object_argument_mask & (1u8 << index) != 0 {
            let ScalarLongSource::Input(slot) = output else {
                return None;
            };
            let value = &*slot_base.add(slot as usize);
            let hint = common.sig.param_type_hints.get(index)?;
            if value.is_reference()
                || !check_type_hint(
                    value,
                    hint,
                    eg,
                    caller_op_array.strict_types,
                    callee_class.as_deref(),
                )
            {
                return None;
            }
            object_arguments[index] = value;
        } else if let ScalarLongSource::Input(slot) = output {
            let value = &*slot_base.add(slot as usize);
            if value.value_type() != ValueType::Long || value.is_reference() {
                return None;
            }
        }
    }
    Some(object_arguments)
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn quick_long_argument_outputs_are_valid(
    slot_base: *mut Value,
    argument_plan: &ScalarLongProgram,
    argument_count: u8,
) -> bool {
    if argument_plan.output_count != argument_count {
        return false;
    }
    argument_plan.outputs
        .iter()
        .copied()
        .take(argument_count as usize)
        .all(|output| match output {
            ScalarLongSource::Input(slot) => {
                let value = &*slot_base.add(slot as usize);
                value.value_type() == ValueType::Long && !value.is_reference()
            }
            ScalarLongSource::Constant(_) | ScalarLongSource::Temporary(_) => true,
        })
}
