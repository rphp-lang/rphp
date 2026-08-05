// Kept in the execute module through include! so this structural split does not change visibility.

#[inline(always)]
unsafe fn publish_quick_double_call_state(
    induction_ptr: *mut Value,
    accumulator_ptr: *mut Value,
    condition_ptr: Option<*mut Value>,
    term_ptr: *mut Value,
    sum_ptr: *mut Value,
    increment_ptr: Option<*mut Value>,
    induction: i64,
    accumulator: f64,
    condition: bool,
    last_term: f64,
    last_increment: i64,
) {
    Value::write_long(induction_ptr, induction);
    Value::write_double(accumulator_ptr, accumulator);
    if let Some(pointer) = condition_ptr {
        Value::write_bool(pointer, condition);
    }
    Value::write_double(term_ptr, last_term);
    Value::write_double(sum_ptr, accumulator);
    if let Some(pointer) = increment_ptr {
        Value::write_long(pointer, last_increment);
    }
}

#[inline(always)]
fn resolve_quick_double_argument_source(
    source: QuickDoubleSource,
    inputs: &[f64; 8],
    induction: i64,
    temporaries: &[f64; 8],
) -> Option<f64> {
    match source {
        QuickDoubleSource::Input(index) => inputs.get(index as usize).copied(),
        QuickDoubleSource::Induction => Some(induction as f64),
        QuickDoubleSource::Constant(value) => Some(value),
        QuickDoubleSource::Temporary(index) => temporaries.get(index as usize).copied(),
    }
}

#[inline(always)]
fn quick_double_argument_phase_masks(
    program: &QuickDoubleArgumentProgram,
) -> ((u8, u8), (u8, u8)) {
    let mut operation_masks = [0u8; 2];
    let mut output_masks = [0u8; 2];
    for index in 0..program.operations.len() {
        for phase in 0..=1 {
            if program.operation_is_needed_by_output_phase(index, phase != 0) {
                operation_masks[phase] |= 1 << index;
            }
        }
    }
    for (index, output) in program.outputs[..program.output_count as usize]
        .iter()
        .copied()
        .enumerate()
    {
        output_masks[usize::from(program.source_depends_on_induction(output))] |= 1 << index;
    }
    (
        (operation_masks[0], output_masks[0]),
        (operation_masks[1], output_masks[1]),
    )
}

#[inline(always)]
fn evaluate_quick_double_argument_phase(
    program: &QuickDoubleArgumentProgram,
    inputs: &[f64; 8],
    induction: i64,
    operation_mask: u8,
    output_mask: u8,
    arguments: &mut [f64; 8],
) -> bool {
    let mut temporaries = [0.0_f64; 8];
    for (index, operation) in program.operations.iter().copied().enumerate() {
        if operation_mask & (1 << index) == 0 {
            continue;
        }
        let Some(lhs) = resolve_quick_double_argument_source(
            operation.lhs,
            inputs,
            induction,
            &temporaries,
        ) else {
            return false;
        };
        let Some(rhs) = resolve_quick_double_argument_source(
            operation.rhs,
            inputs,
            induction,
            &temporaries,
        ) else {
            return false;
        };
        let Some(result) = apply_scalar_double_op(operation.kind, lhs, rhs) else {
            return false;
        };
        temporaries[index] = result;
    }
    for (index, output) in program.outputs
        [..program.output_count as usize]
        .iter()
        .copied()
        .enumerate()
    {
        if output_mask & (1 << index) == 0 {
            continue;
        }
        let Some(value) =
            resolve_quick_double_argument_source(output, inputs, induction, &temporaries)
        else {
            return false;
        };
        arguments[index] = value;
    }
    true
}

// Root plus at most four recursively composed callees. The independent target
// and operation budgets normally stop useful trees before stack depth matters.
const MAX_COMPOSED_DOUBLE_DEPTH: usize = 4;
const MAX_COMPOSED_DOUBLE_TARGETS: usize = 8;

enum ResolvedDoubleCallee<'a> {
    Flat(&'a ScalarDoubleFunctionPlan),
    Composed {
        public_args: u8,
        program: ScalarDoubleProgram,
    },
}

impl ResolvedDoubleCallee<'_> {
    #[inline(always)]
    fn program(&self) -> ResolvedScalarDoubleProgram<'_> {
        match self {
            Self::Flat(plan) => ResolvedScalarDoubleProgram {
                public_args: plan.public_args,
                program: &plan.program,
            },
            Self::Composed {
                public_args,
                program,
            } => ResolvedScalarDoubleProgram {
                public_args: *public_args,
                program,
            },
        }
    }
}

unsafe fn resolve_composed_double_program_inner(
    eg: &ExecutorGlobals,
    owner: &UserFunction,
    plan: &ComposedScalarDoubleFunctionPlan,
    depth: usize,
    active_targets: &mut [*const FunctionCommon; MAX_COMPOSED_DOUBLE_TARGETS + 1],
    active_target_count: usize,
    targets: &mut [*const FunctionCommon; MAX_COMPOSED_DOUBLE_TARGETS],
    target_count: &mut usize,
) -> Option<ScalarDoubleProgram> {
    if depth > MAX_COMPOSED_DOUBLE_DEPTH || plan.operations.len() > 16 {
        return None;
    }
    let mut callees: [Option<ResolvedDoubleCallee<'_>>; 16] =
        std::array::from_fn(|_| None);

    for (operation_index, operation) in plan.operations.iter().enumerate() {
        let ComposedScalarDoubleOp::Call(call) = operation else {
            continue;
        };
        let ScalarLongCallGuard::FunctionCache { .. } = call.guard else {
            return None;
        };
        let ip = call.guard.cache_ip();
        let initializer = owner.op_array.instructions.get(ip)?;
        if initializer.opcode != OpCode::InitFcall {
            return None;
        }
        let cache = owner.op_array.cache.get(ip)?;
        if cache.func.is_null() {
            let primary = owner
                .op_array
                .literals
                .get(initializer.op2 as usize)?
                .as_str()?;
            let resolved = eg.find_function(primary).or_else(|| {
                if initializer.extended_value == 0 {
                    return None;
                }
                owner
                    .op_array
                    .literals
                    .get(initializer.extended_value as usize)
                    .and_then(Value::as_str)
                    .and_then(|fallback| eg.find_function(fallback))
            })?;
            let cache_mut = &mut *(owner.op_array.cache.as_ptr().add(ip)
                as *mut crate::vm::instruction::InlineCache);
            cache_mut.func = resolved;
        }
        let (target, user) = guarded_cached_user_call_target(
            &owner.op_array,
            call.guard,
            None,
            call.arguments.len(),
        )?;
        let user = &*user;
        if !user.common.supports_scalar_double_plan() {
            return None;
        }
        if *target_count == targets.len()
            || active_targets[..active_target_count].contains(&target)
        {
            return None;
        }
        targets[*target_count] = target;
        *target_count += 1;

        callees[operation_index] = Some(if let Some(leaf) = user.scalar_double_plan.as_deref() {
            if leaf.public_args as usize != call.arguments.len() {
                return None;
            }
            ResolvedDoubleCallee::Flat(leaf)
        } else {
            let composed = user.composed_scalar_double_plan.as_deref()?;
            if composed.public_args as usize != call.arguments.len()
                || depth == MAX_COMPOSED_DOUBLE_DEPTH
                || active_target_count == active_targets.len()
            {
                return None;
            }
            active_targets[active_target_count] = target;
            let program = resolve_composed_double_program_inner(
                eg,
                user,
                composed,
                depth + 1,
                active_targets,
                active_target_count + 1,
                targets,
                target_count,
            )?;
            ResolvedDoubleCallee::Composed {
                public_args: composed.public_args,
                program,
            }
        });
    }

    let resolved_programs: [Option<ResolvedScalarDoubleProgram<'_>>; 16] =
        std::array::from_fn(|index| callees[index].as_ref().map(|callee| callee.program()));
    compose_scalar_double_program(plan, &resolved_programs)
}

unsafe fn resolve_composed_double_program(
    eg: &ExecutorGlobals,
    owner: &UserFunction,
    plan: &ComposedScalarDoubleFunctionPlan,
) -> Option<(ScalarDoubleProgram, [*const FunctionCommon; 8], usize)> {
    let owner_target = &owner.common as *const FunctionCommon;
    let mut active_targets = [std::ptr::null(); MAX_COMPOSED_DOUBLE_TARGETS + 1];
    active_targets[0] = owner_target;
    let mut targets = [std::ptr::null(); MAX_COMPOSED_DOUBLE_TARGETS];
    let mut target_count = 0usize;
    let program = resolve_composed_double_program_inner(
        eg,
        owner,
        plan,
        0,
        &mut active_targets,
        1,
        &mut targets,
        &mut target_count,
    )?;
    Some((program, targets, target_count))
}

#[inline(always)]
unsafe fn record_quick_double_call_targets(
    targets: &[*const FunctionCommon],
    iterations: u64,
) {
    for target in targets.iter().copied() {
        record_scalar_calls_bulk(&*target, iterations);
    }
}

#[inline(never)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
#[allow(clippy::too_many_arguments)]
unsafe fn run_native_quick_double_call_accumulate_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickDoubleCallAccumulateLoop,
    targets: &[*const FunctionCommon],
    call_plan: &ScalarDoubleFunctionPlan,
    inputs: &[f64; 8],
    induction_ptr: *mut Value,
    accumulator_ptr: *mut Value,
    condition_ptr: Option<*mut Value>,
    term_ptr: *mut Value,
    sum_ptr: *mut Value,
    increment_ptr: Option<*mut Value>,
    induction: i64,
    bound: i64,
    accumulator: f64,
    last_term: f64,
    initial_last_increment: i64,
) -> Result<Option<QuickLoopOutcome>, VmError> {
    use crate::jit::{NativeDoubleCallAccumulateState, QuickDoubleCallAccumulateJitOutcome};

    let mut state = NativeDoubleCallAccumulateState {
        induction,
        bound,
        accumulator,
        last_term,
    };
    let mut target_identities = [0usize; 9];
    for (index, target) in targets.iter().copied().enumerate() {
        target_identities[index] = target as usize;
    }
    let mut total_iterations = 0u64;
    loop {
        let before_induction = state.induction;
        let Some(result) = plan.native_jit().dispatch(
            &target_identities[..targets.len()],
            &plan.argument_program,
            call_plan,
            &mut state,
            &inputs[..plan.argument_program.input_count as usize],
            eg.vm_interrupt.as_ptr() as *const bool,
        ) else {
            return Ok(None);
        };
        let iterations = (state.induction as u64).wrapping_sub(before_induction as u64);
        total_iterations = total_iterations.saturating_add(iterations);
        record_quick_double_call_targets(targets, iterations);
        let last_increment = if total_iterations == 0 {
            initial_last_increment
        } else {
            match plan.increment_kind {
                QuickIncrementKind::Pre => state.induction,
                QuickIncrementKind::Post => state.induction.wrapping_sub(1),
            }
        };

        match result {
            Ok(QuickDoubleCallAccumulateJitOutcome::Completed) => {
                publish_quick_double_call_state(
                    induction_ptr,
                    accumulator_ptr,
                    condition_ptr,
                    term_ptr,
                    sum_ptr,
                    increment_ptr,
                    state.induction,
                    state.accumulator,
                    false,
                    state.last_term,
                    last_increment,
                );
                (*frame).opline = op_array.instructions.as_ptr().add(plan.exit_ip);
                stats::inc_quick_loop_completed(total_iterations);
                return Ok(Some(QuickLoopOutcome::Completed));
            }
            Ok(QuickDoubleCallAccumulateJitOutcome::Interrupted) => {
                publish_quick_double_call_state(
                    induction_ptr,
                    accumulator_ptr,
                    condition_ptr,
                    term_ptr,
                    sum_ptr,
                    increment_ptr,
                    state.induction,
                    state.accumulator,
                    true,
                    state.last_term,
                    last_increment,
                );
                (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
                handle_interrupt(eg)?;
            }
            Ok(QuickDoubleCallAccumulateJitOutcome::SideExit) | Err(_) => {
                publish_quick_double_call_state(
                    induction_ptr,
                    accumulator_ptr,
                    condition_ptr,
                    term_ptr,
                    sum_ptr,
                    increment_ptr,
                    state.induction,
                    state.accumulator,
                    true,
                    state.last_term,
                    last_increment,
                );
                (*frame).opline = op_array.instructions.as_ptr().add(plan.guard.cache_ip());
                stats::inc_quick_loop_deoptimized(total_iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
        }
    }
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_double_call_accumulate_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickDoubleCallAccumulateLoop,
) -> Result<QuickLoopOutcome, VmError> {
    if (*frame).num_cvs != op_array.num_cvs || (*frame).num_cvs + (*frame).num_temps > 64 {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let slot_base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    let induction_ptr = slot_base.add(plan.induction_cv as usize);
    let accumulator_ptr = slot_base.add(plan.accumulator_cv as usize);
    let condition_ptr = plan.condition_tmp.map(|slot| slot_base.add(slot as usize));
    let term_ptr = slot_base.add(plan.term_tmp as usize);
    let sum_ptr = slot_base.add(plan.sum_tmp as usize);
    let increment_ptr = plan.increment_tmp.map(|slot| slot_base.add(slot as usize));

    let bound = match plan.bound {
        QuickLongBound::Const(value) => value,
        QuickLongBound::Cv(slot) => {
            let pointer = slot_base.add(slot as usize);
            if quick_loop_slot_has_heap(frame, slot)
                || (*pointer).value_type() != ValueType::Long
                || (*pointer).is_reference()
            {
                stats::inc_quick_loop_guard_failed();
                return Ok(QuickLoopOutcome::GuardFailed);
            }
            (*pointer).raw_long()
        }
    };
    if quick_loop_slot_has_heap(frame, plan.induction_cv)
        || quick_loop_slot_has_heap(frame, plan.accumulator_cv)
        || quick_loop_slot_has_heap(frame, plan.term_tmp)
        || quick_loop_slot_has_heap(frame, plan.sum_tmp)
        || plan
            .condition_tmp
            .is_some_and(|slot| quick_loop_slot_has_heap(frame, slot))
        || plan
            .increment_tmp
            .is_some_and(|slot| quick_loop_slot_has_heap(frame, slot))
        || (*induction_ptr).value_type() != ValueType::Long
        || (*induction_ptr).is_reference()
        || (*accumulator_ptr).value_type() != ValueType::Double
        || (*accumulator_ptr).is_reference()
        || (*term_ptr).value_type() != ValueType::Double
        || (*sum_ptr).value_type() != ValueType::Double
        || plan.condition_tmp.is_some_and(|_| {
            let value = &*condition_ptr.unwrap_unchecked();
            !matches!(value.value_type(), ValueType::True | ValueType::False)
        })
        || plan.increment_tmp.is_some_and(|_| {
            let value = &*increment_ptr.unwrap_unchecked();
            value.value_type() != ValueType::Long
        })
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let Some((target, user)) =
        guarded_quick_scalar_call_target(
            op_array,
            slot_base,
            plan.guard,
            plan.argument_program.output_count,
        )
    else {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    };
    let user = &*user;
    if !user.common.supports_scalar_double_plan() {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }
    let mut call_targets = [std::ptr::null(); 9];
    call_targets[0] = target;
    let mut call_target_count = 1usize;
    let mut composed_call_plan = None;
    if user.scalar_double_plan.is_none() {
        let Some(composed) = user.composed_scalar_double_plan.as_deref() else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        };
        let Some((program, nested_targets, nested_target_count)) =
            resolve_composed_double_program(eg, user, composed)
        else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        };
        call_targets[1..1 + nested_target_count]
            .copy_from_slice(&nested_targets[..nested_target_count]);
        call_target_count += nested_target_count;
        composed_call_plan = Some(ScalarDoubleFunctionPlan::new(
            composed.public_args,
            program,
        ));
    }
    let call_plan = user
        .scalar_double_plan
        .as_deref()
        .or(composed_call_plan.as_ref())
        .unwrap_unchecked();
    if call_plan.public_args != plan.argument_program.output_count
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let mut inputs = [0.0_f64; 8];
    for (index, input) in inputs
        .iter_mut()
        .enumerate()
        .take(plan.argument_program.input_count as usize)
    {
        let slot = plan.argument_program.input_slots[index];
        let value = &*slot_base.add(slot as usize);
        if quick_loop_slot_has_heap(frame, slot)
            || value.value_type() != ValueType::Double
            || value.is_reference()
        {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        }
        *input = value.raw_double();
    }

    let mut induction = (*induction_ptr).raw_long();
    let mut accumulator = (*accumulator_ptr).raw_double();
    let mut last_term = (*term_ptr).raw_double();
    let mut last_increment = increment_ptr
        .map(|pointer| (*pointer).raw_long())
        .unwrap_or(induction);

    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    if let Some(outcome) = run_native_quick_double_call_accumulate_loop(
        eg,
        frame,
        op_array,
        plan,
        &call_targets[..call_target_count],
        call_plan,
        &inputs,
        induction_ptr,
        accumulator_ptr,
        condition_ptr,
        term_ptr,
        sum_ptr,
        increment_ptr,
        induction,
        bound,
        accumulator,
        last_term,
        last_increment,
    )? {
        return Ok(outcome);
    }

    let (invariant_argument_masks, dynamic_argument_masks) =
        quick_double_argument_phase_masks(&plan.argument_program);
    let mut arguments = [0.0_f64; 8];
    let mut iterations = 0u64;

    if induction < bound
        && !evaluate_quick_double_argument_phase(
            &plan.argument_program,
            &inputs,
            induction,
            invariant_argument_masks.0,
            invariant_argument_masks.1,
            &mut arguments,
        )
    {
        publish_quick_double_call_state(
            induction_ptr,
            accumulator_ptr,
            condition_ptr,
            term_ptr,
            sum_ptr,
            increment_ptr,
            induction,
            accumulator,
            true,
            last_term,
            last_increment,
        );
        (*frame).opline = op_array.instructions.as_ptr().add(plan.guard.cache_ip());
        record_quick_double_call_targets(
            &call_targets[..call_target_count],
            iterations,
        );
        stats::inc_quick_loop_deoptimized(iterations);
        return Ok(QuickLoopOutcome::Deoptimized);
    }

    loop {
        if induction >= bound {
            publish_quick_double_call_state(
                induction_ptr,
                accumulator_ptr,
                condition_ptr,
                term_ptr,
                sum_ptr,
                increment_ptr,
                induction,
                accumulator,
                false,
                last_term,
                last_increment,
            );
            (*frame).opline = op_array.instructions.as_ptr().add(plan.exit_ip);
            record_quick_double_call_targets(
                &call_targets[..call_target_count],
                iterations,
            );
            stats::inc_quick_loop_completed(iterations);
            return Ok(QuickLoopOutcome::Completed);
        }

        if (dynamic_argument_masks.0 != 0 || dynamic_argument_masks.1 != 0)
            && !evaluate_quick_double_argument_phase(
                &plan.argument_program,
                &inputs,
                induction,
                dynamic_argument_masks.0,
                dynamic_argument_masks.1,
                &mut arguments,
            )
        {
            publish_quick_double_call_state(
                induction_ptr,
                accumulator_ptr,
                condition_ptr,
                term_ptr,
                sum_ptr,
                increment_ptr,
                induction,
                accumulator,
                true,
                last_term,
                last_increment,
            );
            (*frame).opline = op_array.instructions.as_ptr().add(plan.guard.cache_ip());
            record_quick_double_call_targets(
                &call_targets[..call_target_count],
                iterations,
            );
            stats::inc_quick_loop_deoptimized(iterations);
            return Ok(QuickLoopOutcome::Deoptimized);
        }
        let Some(term) = evaluate_scalar_double_plan_rust(call_plan, &arguments) else {
            publish_quick_double_call_state(
                induction_ptr,
                accumulator_ptr,
                condition_ptr,
                term_ptr,
                sum_ptr,
                increment_ptr,
                induction,
                accumulator,
                true,
                last_term,
                last_increment,
            );
            (*frame).opline = op_array.instructions.as_ptr().add(plan.guard.cache_ip());
            record_quick_double_call_targets(
                &call_targets[..call_target_count],
                iterations,
            );
            stats::inc_quick_loop_deoptimized(iterations);
            return Ok(QuickLoopOutcome::Deoptimized);
        };
        let next_accumulator = accumulator + term;
        let Some(next_induction) = induction.checked_add(1) else {
            last_term = term;
            accumulator = next_accumulator;
            record_quick_double_call_targets(
                &call_targets[..call_target_count],
                iterations.saturating_add(1),
            );
            publish_quick_double_call_state(
                induction_ptr,
                accumulator_ptr,
                condition_ptr,
                term_ptr,
                sum_ptr,
                increment_ptr,
                induction,
                accumulator,
                true,
                last_term,
                last_increment,
            );
            (*frame).opline = op_array.instructions.as_ptr().add(plan.increment_ip);
            stats::inc_quick_loop_deoptimized(iterations);
            return Ok(QuickLoopOutcome::Deoptimized);
        };
        last_term = term;
        last_increment = match plan.increment_kind {
            QuickIncrementKind::Pre => next_induction,
            QuickIncrementKind::Post => induction,
        };
        induction = next_induction;
        accumulator = next_accumulator;
        iterations += 1;

        if iterations & 7 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            publish_quick_double_call_state(
                induction_ptr,
                accumulator_ptr,
                condition_ptr,
                term_ptr,
                sum_ptr,
                increment_ptr,
                induction,
                accumulator,
                true,
                last_term,
                last_increment,
            );
            (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
            record_quick_double_call_targets(
                &call_targets[..call_target_count],
                iterations,
            );
            iterations = 0;
            handle_interrupt(eg)?;
        }
    }
}
