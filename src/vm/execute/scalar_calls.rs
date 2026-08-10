// Kept in the execute module through include! so this structural split does not change visibility or code generation.
#[inline(always)]
fn apply_scalar_long_op(kind: ScalarLongOpKind, lhs: i64, rhs: i64) -> Option<i64> {
    match kind {
        ScalarLongOpKind::Add => lhs.checked_add(rhs),
        ScalarLongOpKind::Subtract => lhs.checked_sub(rhs),
        ScalarLongOpKind::Compare => Some(match lhs.cmp(&rhs) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }),
        ScalarLongOpKind::Multiply => lhs.checked_mul(rhs),
        ScalarLongOpKind::IntDivide => lhs.checked_div(rhs),
        ScalarLongOpKind::Modulo => lhs.checked_rem(rhs),
        ScalarLongOpKind::BitwiseAnd => Some(lhs & rhs),
        ScalarLongOpKind::BitwiseOr => Some(lhs | rhs),
        ScalarLongOpKind::BitwiseXor => Some(lhs ^ rhs),
    }
}

#[inline(always)]
fn apply_scalar_long_condition(kind: ScalarLongConditionKind, lhs: i64, rhs: i64) -> bool {
    match kind {
        ScalarLongConditionKind::Equal => lhs == rhs,
        ScalarLongConditionKind::NotEqual => lhs != rhs,
        ScalarLongConditionKind::LessThan => lhs < rhs,
        ScalarLongConditionKind::LessThanOrEqual => lhs <= rhs,
    }
}

#[inline(always)]
fn resolve_scalar_function_source(
    source: ScalarLongSource,
    arguments: &[i64; 8],
    temporaries: &[i64; 8],
) -> Option<i64> {
    match source {
        ScalarLongSource::Input(index) => arguments.get(index as usize).copied(),
        ScalarLongSource::Constant(value) => Some(value),
        ScalarLongSource::Temporary(index) => temporaries.get(index as usize).copied(),
    }
}

#[inline(always)]
unsafe fn evaluate_scalar_long_operation(
    operations: &[ScalarLongOp],
    arguments: &[i64; 8],
    temporaries: &mut [i64; 8],
    index: usize,
) -> Option<()> {
    let operation = *operations.get_unchecked(index);
    let lhs = resolve_scalar_function_source(operation.lhs, arguments, temporaries)?;
    let rhs = resolve_scalar_function_source(operation.rhs, arguments, temporaries)?;
    *temporaries.get_unchecked_mut(index) = apply_scalar_long_op(operation.kind, lhs, rhs)?;
    Some(())
}

/// Execute one validated scalar-plan range without an interpreter backedge
/// for the common tiny bodies. The compiler caps the complete program at
/// eight operations; the explicit range check keeps malformed public plans on
/// the ordinary safe failure path before the unchecked per-operation access.
#[inline(always)]
fn evaluate_scalar_long_operation_range(
    operations: &[ScalarLongOp],
    arguments: &[i64; 8],
    temporaries: &mut [i64; 8],
    start: usize,
    end: usize,
) -> Option<()> {
    let count = end.checked_sub(start)?;
    if end > operations.len() || end > temporaries.len() {
        return None;
    }
    macro_rules! evaluate {
        ($offset:expr) => {
            unsafe {
                evaluate_scalar_long_operation(
                    operations,
                    arguments,
                    temporaries,
                    start + $offset,
                )?;
            }
        };
    }
    match count {
        0 => {}
        1 => evaluate!(0),
        2 => {
            evaluate!(0);
            evaluate!(1);
        }
        3 => {
            evaluate!(0);
            evaluate!(1);
            evaluate!(2);
        }
        4 => {
            evaluate!(0);
            evaluate!(1);
            evaluate!(2);
            evaluate!(3);
        }
        _ => {
            for index in start..end {
                unsafe {
                    evaluate_scalar_long_operation(operations, arguments, temporaries, index)?;
                }
            }
        }
    }
    Some(())
}

#[cfg(test)]
mod scalar_long_operation_range_tests {
    use super::*;

    #[test]
    fn tiny_ranges_preserve_temporary_dependencies() {
        let operations = [
            ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(2),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Temporary(0),
                rhs: ScalarLongSource::Constant(3),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::Subtract,
                lhs: ScalarLongSource::Temporary(1),
                rhs: ScalarLongSource::Constant(4),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::BitwiseXor,
                lhs: ScalarLongSource::Temporary(2),
                rhs: ScalarLongSource::Constant(1),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Temporary(3),
                rhs: ScalarLongSource::Input(1),
            },
        ];
        let mut arguments = [0; 8];
        arguments[0] = 5;
        arguments[1] = 6;
        let mut temporaries = [0; 8];

        assert_eq!(
            evaluate_scalar_long_operation_range(
                &operations,
                &arguments,
                &mut temporaries,
                0,
                operations.len(),
            ),
            Some(())
        );
        assert_eq!(&temporaries[..5], &[7, 21, 17, 16, 22]);
    }

    #[test]
    fn malformed_or_failing_ranges_return_none_before_unchecked_access() {
        let operations = [ScalarLongOp {
            kind: ScalarLongOpKind::IntDivide,
            lhs: ScalarLongSource::Input(0),
            rhs: ScalarLongSource::Constant(0),
        }];
        let arguments = [1; 8];
        let mut temporaries = [0; 8];

        assert_eq!(
            evaluate_scalar_long_operation_range(&operations, &arguments, &mut temporaries, 1, 0,),
            None
        );
        assert_eq!(
            evaluate_scalar_long_operation_range(&operations, &arguments, &mut temporaries, 0, 9,),
            None
        );
        assert_eq!(
            evaluate_scalar_long_operation_range(&operations, &arguments, &mut temporaries, 0, 1,),
            None
        );
    }
}

#[inline(always)]
fn evaluate_scalar_long_plan(plan: &ScalarLongFunctionPlan, arguments: &[i64; 8]) -> Option<i64> {
    if plan.program.operations.len() > 8 || plan.program.output_count != 1 {
        return None;
    }
    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    match plan.native_jit().dispatch(plan, arguments) {
        ScalarLongJitDispatch::Interpret => {}
        ScalarLongJitDispatch::Value(value) => {
            #[cfg(feature = "vm-stats")]
            stats::inc_jit_native_execution(stats::JitRegionKind::ScalarLongFunction);
            return Some(value);
        }
        ScalarLongJitDispatch::SideExit => {
            #[cfg(feature = "vm-stats")]
            {
                stats::inc_jit_native_execution(stats::JitRegionKind::ScalarLongFunction);
                stats::inc_jit_native_side_exit(stats::JitRegionKind::ScalarLongFunction);
            }
            return None;
        }
    }
    let mut temporaries = [0i64; 8];
    let operations = plan.program.operations.as_ref();
    let output = if let Some(select) = plan.select {
        let shared_end = select.shared_operation_count as usize;
        let true_end = shared_end.checked_add(select.when_true_operation_count as usize)?;
        if true_end > plan.program.operations.len() {
            return None;
        }
        evaluate_scalar_long_operation_range(
            operations,
            arguments,
            &mut temporaries,
            0,
            shared_end,
        )?;
        let resolve_condition_operand = |operand| match operand {
            ScalarLongConditionOperand::Source(source) => {
                resolve_scalar_function_source(source, arguments, &temporaries)
            }
            ScalarLongConditionOperand::BitwiseAnd { lhs, rhs } => Some(
                resolve_scalar_function_source(lhs, arguments, &temporaries)?
                    & resolve_scalar_function_source(rhs, arguments, &temporaries)?,
            ),
        };
        let lhs = resolve_condition_operand(select.lhs)?;
        let rhs = resolve_condition_operand(select.rhs)?;
        let condition = match select.kind {
            ScalarLongConditionKind::Equal => lhs == rhs,
            ScalarLongConditionKind::NotEqual => lhs != rhs,
            ScalarLongConditionKind::LessThan => lhs < rhs,
            ScalarLongConditionKind::LessThanOrEqual => lhs <= rhs,
        };
        if condition {
            evaluate_scalar_long_operation_range(
                operations,
                arguments,
                &mut temporaries,
                shared_end,
                true_end,
            )?;
            select.when_true
        } else {
            evaluate_scalar_long_operation_range(
                operations,
                arguments,
                &mut temporaries,
                true_end,
                operations.len(),
            )?;
            select.when_false
        }
    } else {
        evaluate_scalar_long_operation_range(
            operations,
            arguments,
            &mut temporaries,
            0,
            operations.len(),
        )?;
        plan.program.outputs[0]
    };
    resolve_scalar_function_source(output, arguments, &temporaries)
}

#[inline(always)]
fn resolve_scalar_double_source(
    source: ScalarDoubleSource,
    arguments: &[f64; 8],
    temporaries: &[f64; 8],
    selection: Option<f64>,
) -> Option<f64> {
    match source {
        ScalarDoubleSource::Input(index) => arguments.get(index as usize).copied(),
        ScalarDoubleSource::Constant(value) => Some(value),
        ScalarDoubleSource::Temporary(index) => temporaries.get(index as usize).copied(),
        ScalarDoubleSource::Selection => selection,
    }
}

#[inline(always)]
fn apply_scalar_double_op(kind: ScalarDoubleOpKind, lhs: f64, rhs: f64) -> Option<f64> {
    match kind {
        ScalarDoubleOpKind::Add => Some(lhs + rhs),
        ScalarDoubleOpKind::Subtract => Some(lhs - rhs),
        ScalarDoubleOpKind::Multiply => Some(lhs * rhs),
        ScalarDoubleOpKind::Divide if rhs == 0.0 => None,
        ScalarDoubleOpKind::Divide => Some(lhs / rhs),
    }
}

#[inline(always)]
fn evaluate_scalar_double_plan(
    plan: &ScalarDoubleFunctionPlan,
    arguments: &[f64; 8],
) -> Option<f64> {
    if plan.program.operations.len() > 8 {
        return None;
    }
    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    match plan.native_jit().dispatch(plan, arguments) {
        ScalarDoubleJitDispatch::Interpret => {}
        ScalarDoubleJitDispatch::Value(value) => {
            #[cfg(feature = "vm-stats")]
            stats::inc_jit_native_execution(stats::JitRegionKind::ScalarDoubleFunction);
            return Some(value);
        }
        ScalarDoubleJitDispatch::SideExit => {
            #[cfg(feature = "vm-stats")]
            {
                stats::inc_jit_native_execution(stats::JitRegionKind::ScalarDoubleFunction);
                stats::inc_jit_native_side_exit(stats::JitRegionKind::ScalarDoubleFunction);
            }
            return None;
        }
    }
    evaluate_scalar_double_plan_rust(plan, arguments)
}

#[inline(always)]
fn evaluate_scalar_double_plan_rust(
    plan: &ScalarDoubleFunctionPlan,
    arguments: &[f64; 8],
) -> Option<f64> {
    if plan.program.operations.len() > 8 {
        return None;
    }
    let mut temporaries = [0.0_f64; 8];
    let evaluate_operations =
        |start: usize, end: usize, temporaries: &mut [f64; 8], selection: Option<f64>| {
            for index in start..end {
                let operation = plan.program.operations[index];
                let lhs =
                    resolve_scalar_double_source(operation.lhs, arguments, temporaries, selection)?;
                let rhs =
                    resolve_scalar_double_source(operation.rhs, arguments, temporaries, selection)?;
                temporaries[index] = apply_scalar_double_op(operation.kind, lhs, rhs)?;
            }
            Some(())
        };
    if let Some(select) = plan.select {
        let (shared_end, true_end, false_end) =
            select.operation_ranges(plan.program.operations.len())?;
        evaluate_operations(0, shared_end, &mut temporaries, None)?;
        let lhs = resolve_scalar_double_source(select.lhs, arguments, &temporaries, None)?;
        let rhs = resolve_scalar_double_source(select.rhs, arguments, &temporaries, None)?;
        let selected_source = if apply_scalar_double_condition(select.kind, lhs, rhs) {
            evaluate_operations(shared_end, true_end, &mut temporaries, None)?;
            select.when_true
        } else {
            evaluate_operations(true_end, false_end, &mut temporaries, None)?;
            select.when_false
        };
        if !select.merge_result {
            if false_end != plan.program.operations.len() {
                return None;
            }
            return resolve_scalar_double_source(selected_source, arguments, &temporaries, None);
        }
        let selection =
            resolve_scalar_double_source(selected_source, arguments, &temporaries, None)?;
        evaluate_operations(
            false_end,
            plan.program.operations.len(),
            &mut temporaries,
            Some(selection),
        )?;
        resolve_scalar_double_source(
            plan.program.output,
            arguments,
            &temporaries,
            Some(selection),
        )
    } else {
        evaluate_operations(0, plan.program.operations.len(), &mut temporaries, None)?;
        resolve_scalar_double_source(plan.program.output, arguments, &temporaries, None)
    }
}

#[inline(always)]
fn apply_scalar_double_condition(kind: ScalarLongConditionKind, lhs: f64, rhs: f64) -> bool {
    match kind {
        ScalarLongConditionKind::Equal => lhs == rhs,
        ScalarLongConditionKind::NotEqual => lhs != rhs,
        ScalarLongConditionKind::LessThan => lhs < rhs,
        ScalarLongConditionKind::LessThanOrEqual => lhs <= rhs,
    }
}

#[inline(always)]
fn evaluate_scalar_string_plan<'a>(
    plan: &'a ScalarStringFunctionPlan,
    arguments: &[i64; 8],
) -> Option<&'a str> {
    if plan.operations.len() > 8 {
        return None;
    }
    let mut temporaries = [0i64; 8];
    for (index, operation) in plan.operations.iter().copied().enumerate() {
        let lhs = resolve_scalar_function_source(operation.lhs, arguments, &temporaries)?;
        let rhs = resolve_scalar_function_source(operation.rhs, arguments, &temporaries)?;
        temporaries[index] = apply_scalar_long_op(operation.kind, lhs, rhs)?;
    }
    let Some(select) = plan.select else {
        return Some(&plan.when_true);
    };
    let resolve_condition_operand = |operand| match operand {
        ScalarLongConditionOperand::Source(source) => {
            resolve_scalar_function_source(source, arguments, &temporaries)
        }
        ScalarLongConditionOperand::BitwiseAnd { lhs, rhs } => Some(
            resolve_scalar_function_source(lhs, arguments, &temporaries)?
                & resolve_scalar_function_source(rhs, arguments, &temporaries)?,
        ),
    };
    let lhs = resolve_condition_operand(select.lhs)?;
    let rhs = resolve_condition_operand(select.rhs)?;
    let condition = match select.kind {
        ScalarLongConditionKind::Equal => lhs == rhs,
        ScalarLongConditionKind::NotEqual => lhs != rhs,
        ScalarLongConditionKind::LessThan => lhs < rhs,
        ScalarLongConditionKind::LessThanOrEqual => lhs <= rhs,
    };
    Some(if condition {
        &plan.when_true
    } else {
        &plan.when_false
    })
}

#[inline(always)]
pub(crate) fn should_defer_scalar_call(
    initializer: &Instruction,
    scalar_plan_eligible: bool,
) -> bool {
    if !deferred_scalar_calls_enabled()
        || initializer._pad & CALL_FLAG_DEFERRED_SCALAR_CANDIDATE == 0
    {
        return false;
    }
    scalar_plan_eligible
}

/// Evaluate a scalar plan from values already captured in a compact pending
/// activation. This is the non-contiguous counterpart of the direct Send scan:
/// argument expressions have run exactly once, but no body frame exists yet.
#[inline(always)]
pub(crate) unsafe fn try_execute_deferred_scalar_long_call(
    eg: &ExecutorGlobals,
    call: *mut ExecuteData,
) -> Option<i64> {
    let common = &*(*call).func;
    if !(*call).deferred_scalar_call
        || common.fn_type != FunctionType::User
        || !common.supports_scalar_long_plan()
        || (*call).num_args != common.sig.public_arity()
        || (*call).named_args_used
    {
        return None;
    }
    let user = &*((*call).func as *const UserFunction);
    let public_args = user
        .scalar_long_plan
        .as_deref()
        .map(|plan| plan.public_args)
        .or_else(|| {
            user.composed_scalar_long_plan
                .as_deref()
                .map(|plan| plan.public_args)
        })?;
    if public_args as u32 != common.sig.public_arity() {
        return None;
    }

    let mut arguments = [0i64; 8];
    for (index, argument) in arguments.iter_mut().enumerate().take(public_args as usize) {
        let cv_index = common.sig.param_cv_index(index as u32);
        let value = (*call).cv(cv_index);
        if value.value_type() != ValueType::Long || value.is_reference() {
            return None;
        }
        *argument = value.raw_long();
    }

    if let Some(plan) = user.scalar_long_plan.as_deref() {
        if plan.select.is_none()
            && plan.program.operations.len() == 1
            && plan.program.output_count == 1
            && plan.program.outputs[0] == ScalarLongSource::Temporary(0)
        {
            let operation = plan.program.operations[0];
            let operand = |source| match source {
                ScalarLongSource::Input(index) => Some(arguments[index as usize]),
                ScalarLongSource::Constant(value) => Some(value),
                ScalarLongSource::Temporary(_) => None,
            };
            let lhs = operand(operation.lhs)?;
            let rhs = operand(operation.rhs)?;
            return apply_scalar_long_op(operation.kind, lhs, rhs);
        }
        return evaluate_scalar_long_plan(plan, &arguments);
    }

    if !composed_scalar_bodies_enabled() {
        return None;
    }
    let plan = user.composed_scalar_long_plan.as_deref()?;
    let mut calls = [std::ptr::null(); COMPOSED_SCALAR_MAX_CALLS];
    let mut call_count = 0usize;
    let result = evaluate_composed_scalar_body_plan(
        eg,
        user,
        plan,
        &arguments,
        &[std::ptr::null(); 8],
        &mut calls,
        &mut call_count,
        0,
    )?;
    for called in calls.into_iter().take(call_count) {
        record_scalar_call(&*called);
    }
    Some(result)
}

/// Execute an already-captured positional activation through the exact
/// Double leaf ABI. Raw Long arguments intentionally fail this guard so the
/// canonical frame performs declared-float coercion and diagnostics.
#[inline(always)]
pub(crate) unsafe fn try_execute_deferred_scalar_double_call(
    call: *mut ExecuteData,
) -> Option<f64> {
    let common = &*(*call).func;
    if !(*call).deferred_scalar_call
        || common.fn_type != FunctionType::User
        || !common.supports_scalar_double_plan()
        || (*call).num_args != common.sig.public_arity()
        || (*call).named_args_used
    {
        return None;
    }
    let user = &*((*call).func as *const UserFunction);
    let plan = user.scalar_double_plan.as_deref()?;
    if plan.public_args as u32 != common.sig.public_arity() {
        return None;
    }

    let mut arguments = [0.0_f64; 8];
    for (index, argument) in arguments
        .iter_mut()
        .enumerate()
        .take(plan.public_args as usize)
    {
        let cv_index = common.sig.param_cv_index(index as u32);
        let value = (*call).cv(cv_index);
        if value.value_type() != ValueType::Double || value.is_reference() {
            return None;
        }
        *argument = value.raw_double();
    }
    evaluate_scalar_double_plan(plan, &arguments)
}

/// Consume a deferred method activation after all argument expressions have
/// executed, without expanding it into the callee's complete CV/TMP frame.
#[inline(never)]
unsafe fn try_execute_deferred_object_long_call(
    eg: &ExecutorGlobals,
    call: *mut ExecuteData,
) -> Option<i64> {
    let common = &*(*call).func;
    if !(*call).deferred_scalar_call
        || common.fn_type != FunctionType::User
        || !common.plan.call.is_compact_user_call()
        || common.plan.ret != ReturnStrategy::Fast
        || (*call).num_args != common.sig.public_arity()
        || (*call).named_args_used
    {
        return None;
    }
    let callee = &*((*call).func as *const UserFunction);
    let plan = callee.object_long_plan.as_deref()?;
    if plan.public_args as u32 != common.sig.public_arity() {
        return None;
    }

    let receiver = (*call).cv(0);
    if receiver.value_type() != ValueType::Object || receiver.is_reference() {
        return None;
    }
    let caller = (*call).prev_execute_data;
    if caller.is_null() {
        return None;
    }
    let caller_op_array = (*caller).op_array();
    let declaring_class = eg.declaring_class_of(&callee.common as *const FunctionCommon);
    let mut slots = [const { std::mem::MaybeUninit::<i64>::uninit() }; 64];
    let mut initialized = 0u64;
    let mut object_arguments = [ObjectLongArgument::None; 8];
    let mut string_arguments = [std::ptr::null(); 8];

    for index in 0..plan.public_args as usize {
        let value = (*call).cv(common.sig.param_cv_index(index as u32));
        if value.is_reference() {
            return None;
        }
        let hint = common
            .sig
            .param_type_hints
            .get(index)
            .unwrap_or(&ParamTypeHint::None);
        if !check_type_hint(
            value,
            hint,
            eg,
            caller_op_array.strict_types,
            declaring_class,
        ) {
            return None;
        }

        let bit = 1u8 << index;
        if plan.long_argument_mask & bit != 0 {
            if value.value_type() != ValueType::Long {
                return None;
            }
            let slot = common.sig.param_cv_index(index as u32) as usize;
            slots[slot].write(value.raw_long());
            initialized |= 1u64 << slot;
        }
        if plan.object_argument_mask & bit != 0 {
            if value.value_type() != ValueType::Object {
                return None;
            }
            object_arguments[index] = ObjectLongArgument::Borrowed(value as *const Value);
        }
        if plan.string_argument_mask & bit != 0 {
            if value.value_type() != ValueType::String {
                return None;
            }
            string_arguments[index] = value as *const Value;
        }
    }

    evaluate_object_long_plan(
        receiver,
        &object_arguments,
        &string_arguments,
        &mut slots,
        initialized,
        callee,
        plan,
    )
}

/// Execute a deferred compiler-proven property mutator from arguments already
/// captured in its compact activation.  As with the contiguous variant, all
/// type/cache/arithmetic guards complete before the first property write.
#[inline(always)]
unsafe fn try_execute_deferred_long_property_method(call: *mut ExecuteData) -> bool {
    let common = &*(*call).func;
    if !(*call).deferred_scalar_call
        || common.fn_type != FunctionType::User
        || !common.supports_scalar_long_plan()
        || (*call).num_args != common.sig.public_arity()
        || (*call).named_args_used
    {
        return false;
    }
    let user = &*((*call).func as *const UserFunction);
    let Some(plan) = user.long_property_plan.as_deref() else {
        return false;
    };
    if plan.public_args as u32 != common.sig.public_arity() {
        return false;
    }

    let receiver = (*call).cv(0);
    if receiver.value_type() != ValueType::Object || receiver.is_reference() {
        return false;
    }
    let mut arguments = [0i64; 8];
    for (index, argument) in arguments
        .iter_mut()
        .enumerate()
        .take(plan.public_args as usize)
    {
        let value = (*call).cv(common.sig.param_cv_index(index as u32));
        if value.value_type() != ValueType::Long || value.is_reference() {
            return false;
        }
        *argument = value.raw_long();
    }

    try_execute_long_property_plan(receiver, &arguments, plan, user)
}

/// Expand an argument-only activation into the canonical function ABI after a
/// scalar type/arithmetic guard fails. Values are moved, not re-evaluated.
#[inline(never)]
pub(crate) unsafe fn materialize_deferred_scalar_call(
    eg: &mut ExecutorGlobals,
    compact: *mut ExecuteData,
) -> *mut ExecuteData {
    debug_assert!((*compact).deferred_scalar_call);
    let storage_num_args = (*compact).num_cvs;
    let full = eg.vm_stack.push_call_frame(
        (*compact).func,
        storage_num_args,
        (*compact).num_args,
        (*compact).prev_execute_data,
        (*compact).call,
    );
    for index in 0..storage_num_args {
        Value::raw_copy((*compact).slot_ptr(index), (*full).slot_ptr(index));
    }
    (*full).has_heap_slots = (*compact).has_heap_slots;
    (*full).named_args_used = (*compact).named_args_used;
    (*full).heap_bitmap = (*compact).heap_bitmap;

    // Ownership moved to the ordinary frame. The compact storage is now just
    // raw bump memory and must not release any captured heap value.
    (*compact).has_heap_slots = false;
    (*compact).heap_bitmap = 0;
    eg.pending_call_stack.pop_call_frame(compact);
    full
}

/// Finish a deferred activation outside the main dispatcher body. A null return
/// means the scalar call completed; a non-null return is the materialized frame
/// that must continue through the canonical DoFcall path.
#[inline(never)]
pub(crate) unsafe fn resolve_deferred_scalar_call(
    eg: &mut ExecutorGlobals,
    caller: *mut ExecuteData,
    compact: *mut ExecuteData,
    do_fcall: &Instruction,
    do_fcall_ptr: *const Instruction,
) -> *mut ExecuteData {
    if do_fcall.result_type == OpType::Unused && try_execute_deferred_long_property_method(compact)
    {
        let common = &*(*compact).func;
        record_scalar_call(common);
        (*caller).opline = do_fcall_ptr.add(1);
        if (*compact).has_heap_slots {
            cleanup_frame_slots(compact);
        }
        eg.pending_call_stack.pop_call_frame(compact);
        return std::ptr::null_mut();
    }

    let accepts_scalar_result = matches!(
        do_fcall.result_type,
        OpType::Tmp | OpType::Var | OpType::Unused
    );
    let evaluated_long = if accepts_scalar_result {
        try_execute_deferred_object_long_call(eg, compact)
            .or_else(|| try_execute_deferred_scalar_long_call(eg, compact))
    } else {
        None
    };
    let common = &*(*compact).func;
    if let Some(result) = evaluated_long {
        record_scalar_call(common);
        complete_direct_scalar_long_call(caller, do_fcall_ptr, result);
    } else if accepts_scalar_result
        && let Some(result) = try_execute_deferred_scalar_double_call(compact)
    {
        record_scalar_call(common);
        complete_direct_scalar_double_call(caller, do_fcall_ptr, result);
    } else {
        return materialize_deferred_scalar_call(eg, compact);
    }
    if (*compact).has_heap_slots {
        cleanup_frame_slots(compact);
    }
    eg.pending_call_stack.pop_call_frame(compact);
    std::ptr::null_mut()
}

/// Compact hot-executor specialization for the overwhelmingly common leaf
/// shape `return arg OP arg_or_const`. Keeping this separate from the general
/// planner avoids an out-of-line Rust call per PHP leaf invocation without
/// inlining the larger multi-step evaluator into the baseline dispatcher.
#[inline(always)]
pub(crate) unsafe fn try_execute_direct_single_scalar_long_op(
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    sends: *const Instruction,
    common: &FunctionCommon,
    plan: &ScalarLongFunctionPlan,
) -> Option<(i64, *const Instruction)> {
    if !common.supports_scalar_long_plan()
        || common.sig.public_arity() != plan.public_args as u32
        || plan.select.is_some()
        || plan.program.operations.len() != 1
        || plan.program.output_count != 1
        || plan.program.outputs[0] != ScalarLongSource::Temporary(0)
    {
        return None;
    }

    let mut arguments = [0i64; 8];
    for index in 0..plan.public_args as usize {
        let send = &*sends.add(index);
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || send.op2 as u32 != common.sig.param_cv_index(index as u32)
        {
            return None;
        }
        let value = match send.op1_type {
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                &*(*caller).get_op_ptr(send.op1 as u32, send.op1_type, caller_op_array)
            }
            OpType::Unused => return None,
        };
        if value.value_type() != ValueType::Long {
            return None;
        }
        arguments[index] = value.raw_long();
    }

    let operation = plan.program.operations[0];
    let operand = |source| match source {
        ScalarLongSource::Input(index) => Some(arguments[index as usize]),
        ScalarLongSource::Constant(value) => Some(value),
        ScalarLongSource::Temporary(_) => None,
    };
    let lhs = operand(operation.lhs)?;
    let rhs = operand(operation.rhs)?;
    let result = apply_scalar_long_op(operation.kind, lhs, rhs)?;
    let do_fcall_ptr = sends.add(plan.public_args as usize);
    let do_fcall = &*do_fcall_ptr;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(
            do_fcall.result_type,
            OpType::Tmp | OpType::Var | OpType::Unused
        )
    {
        return None;
    }
    Some((result, do_fcall_ptr))
}

/// Borrow a contiguous positional Send sequence and evaluate a pure scalar
/// callee before any ExecuteData frame is allocated. Argument expressions that
/// need their own opcodes simply fail this shape guard and retain the ordinary
/// call protocol.
#[inline(never)]
pub(crate) unsafe fn try_execute_direct_scalar_long_call(
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    sends: *const Instruction,
    common: &FunctionCommon,
    plan: &ScalarLongFunctionPlan,
) -> Option<(i64, *const Instruction)> {
    if !common.supports_scalar_long_plan() || common.sig.public_arity() != plan.public_args as u32 {
        return None;
    }

    let mut arguments = [0i64; 8];
    for (index, argument) in arguments
        .iter_mut()
        .enumerate()
        .take(plan.public_args as usize)
    {
        let send = &*sends.add(index);
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || send.op2 as u32 != common.sig.param_cv_index(index as u32)
        {
            return None;
        }
        let value = match send.op1_type {
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                &*(*caller).get_op_ptr(send.op1 as u32, send.op1_type, caller_op_array)
            }
            OpType::Unused => return None,
        };
        if value.value_type() != ValueType::Long {
            return None;
        }
        *argument = value.raw_long();
    }

    let do_fcall_ptr = sends.add(plan.public_args as usize);
    let do_fcall = &*do_fcall_ptr;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(
            do_fcall.result_type,
            OpType::Tmp | OpType::Var | OpType::Unused
        )
    {
        return None;
    }
    let result = evaluate_scalar_long_plan(plan, &arguments)?;
    Some((result, do_fcall_ptr))
}

/// Borrow a contiguous positional Send sequence and enter the exact-Double
/// leaf ABI without allocating an ExecuteData frame. Long values deliberately
/// fail this guard so weak float coercion remains on the canonical PHP path.
#[inline(never)]
pub(crate) unsafe fn try_execute_direct_scalar_double_call(
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    sends: *const Instruction,
    common: &FunctionCommon,
    plan: &ScalarDoubleFunctionPlan,
) -> Option<(f64, *const Instruction)> {
    if !common.supports_scalar_double_plan() || common.sig.public_arity() != plan.public_args as u32
    {
        return None;
    }

    let mut arguments = [0.0_f64; 8];
    for (index, argument) in arguments
        .iter_mut()
        .enumerate()
        .take(plan.public_args as usize)
    {
        let send = &*sends.add(index);
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || send.op2 as u32 != common.sig.param_cv_index(index as u32)
        {
            return None;
        }
        let value = match send.op1_type {
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                &*(*caller).get_op_ptr(send.op1 as u32, send.op1_type, caller_op_array)
            }
            OpType::Unused => return None,
        };
        if value.value_type() != ValueType::Double || value.is_reference() {
            return None;
        }
        *argument = value.raw_double();
    }

    let do_fcall_ptr = sends.add(plan.public_args as usize);
    let do_fcall = &*do_fcall_ptr;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(
            do_fcall.result_type,
            OpType::Tmp | OpType::Var | OpType::Unused
        )
    {
        return None;
    }
    let result = evaluate_scalar_double_plan(plan, &arguments)?;
    Some((result, do_fcall_ptr))
}

/// Enter a straight-line composed Double body from an ordinary contiguous
/// call site. Nested targets are guarded and flattened exactly as they are for
/// a quick loop; a failed guard leaves every original Send/DoFcall instruction
/// untouched for canonical execution.
#[inline(never)]
pub(crate) unsafe fn try_execute_direct_composed_scalar_double_call(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    sends: *const Instruction,
    common: &FunctionCommon,
    owner: &UserFunction,
    owner_receiver: Option<&Value>,
    plan: &ComposedScalarDoubleFunctionPlan,
) -> Option<(f64, *const Instruction)> {
    if !common.supports_scalar_double_plan() || common.sig.public_arity() != plan.public_args as u32
    {
        return None;
    }

    let mut arguments = [0.0_f64; 8];
    for (index, argument) in arguments
        .iter_mut()
        .enumerate()
        .take(plan.public_args as usize)
    {
        let send = &*sends.add(index);
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || send.op2 as u32 != common.sig.param_cv_index(index as u32)
        {
            return None;
        }
        let value = match send.op1_type {
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                &*(*caller).get_op_ptr(send.op1 as u32, send.op1_type, caller_op_array)
            }
            OpType::Unused => return None,
        };
        if value.value_type() != ValueType::Double || value.is_reference() {
            return None;
        }
        *argument = value.raw_double();
    }

    let do_fcall_ptr = sends.add(plan.public_args as usize);
    let do_fcall = &*do_fcall_ptr;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(
            do_fcall.result_type,
            OpType::Tmp | OpType::Var | OpType::Unused
        )
    {
        return None;
    }

    let (flattened, nested_targets, nested_target_count) =
        resolve_composed_double_program(eg, owner, owner_receiver, plan)?;
    let result = evaluate_scalar_double_plan_rust(&flattened, &arguments)?;
    for target in nested_targets.into_iter().take(nested_target_count) {
        record_scalar_call(&*target);
    }
    record_scalar_call(common);
    Some((result, do_fcall_ptr))
}
