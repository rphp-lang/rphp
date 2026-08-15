// Kept in the execute module through include! so this structural split does not change visibility or code generation.
const COMPOSED_SCALAR_MAX_CALLS: usize = 8;
const COMPOSED_SCALAR_MAX_OPS: usize = 16;
const QUICK_SCALAR_MAX_RECORDED_CALLS: usize = COMPOSED_SCALAR_MAX_CALLS + 1;

#[inline(always)]
pub(crate) fn record_scalar_call(common: &FunctionCommon) {
    stats::inc_do_fcall_fast();
    stats::inc_return_fast();
    let count = common.call_count.get();
    if count < u32::MAX {
        common.call_count.set(count + 1);
    }
}

#[inline(always)]
fn record_scalar_calls_bulk(common: &FunctionCommon, count: u64) {
    if count == 0 {
        return;
    }
    stats::inc_do_fcall_fast_by(count);
    stats::inc_return_fast_by(count);
    let increment = u32::try_from(count).unwrap_or(u32::MAX);
    common
        .call_count
        .set(common.call_count.get().saturating_add(increment));
}

#[inline(always)]
unsafe fn flush_quick_scalar_calls(
    targets: &[*const FunctionCommon; QUICK_SCALAR_MAX_RECORDED_CALLS],
    target_count: usize,
    success_count: &mut u64,
) {
    if *success_count == 0 {
        return;
    }
    for target in targets.iter().copied().take(target_count) {
        debug_assert!(!target.is_null());
        record_scalar_calls_bulk(&*target, *success_count);
    }
    *success_count = 0;
}

#[inline(always)]
fn resolve_composed_body_source(
    source: ScalarLongSource,
    arguments: &[i64; 8],
    temporaries: &[i64; COMPOSED_SCALAR_MAX_OPS],
) -> i64 {
    match source {
        ScalarLongSource::Input(index) => arguments[index as usize],
        ScalarLongSource::Constant(value) => value,
        ScalarLongSource::Temporary(index) => temporaries[index as usize],
    }
}

#[inline(always)]
fn resolve_composed_string_source(
    source: ScalarStringSource,
    arguments: &[Option<usize>; 8],
    temporaries: &[Option<usize>; COMPOSED_SCALAR_MAX_OPS],
) -> Option<usize> {
    match source {
        ScalarStringSource::Input(index) => arguments[index as usize],
        ScalarStringSource::Temporary(index) => temporaries[index as usize],
    }
}

#[inline(always)]
unsafe fn guarded_scalar_user_target(
    target: *const FunctionCommon,
    argument_count: usize,
) -> Option<*const UserFunction> {
    if target.is_null() {
        return None;
    }
    let common = &*target;
    if common.fn_type != FunctionType::User
        || !common.supports_scalar_long_plan()
        || common.sig.public_arity() != argument_count as u32
    {
        return None;
    }
    Some(target as *const UserFunction)
}

#[inline(always)]
unsafe fn guarded_user_target(
    target: *const FunctionCommon,
    argument_count: usize,
) -> Option<*const UserFunction> {
    if target.is_null() {
        return None;
    }
    let common = &*target;
    if common.fn_type != FunctionType::User || common.sig.public_arity() != argument_count as u32 {
        return None;
    }
    Some(target as *const UserFunction)
}

#[inline(always)]
unsafe fn guarded_cached_user_call_target(
    op_array: &crate::compiler::OpArray,
    guard: ScalarLongCallGuard,
    receiver: Option<&Value>,
    argument_count: usize,
) -> Option<(*const FunctionCommon, *const UserFunction)> {
    let ip = guard.cache_ip();
    let initializer = op_array.instructions.get(ip)?;
    let cache = op_array.cache.get(ip)?;
    let target = match guard {
        ScalarLongCallGuard::FunctionCache { .. } => {
            if initializer.opcode != OpCode::InitFcall {
                return None;
            }
            cache.func
        }
        ScalarLongCallGuard::MethodCache { receiver_slot, .. } => {
            if initializer.opcode != OpCode::InitMethodCall
                || initializer.op1_type != OpType::Cv
                || initializer.op1 != receiver_slot
            {
                return None;
            }
            let receiver = receiver?;
            if receiver.value_type() != ValueType::Object || receiver.is_reference() {
                return None;
            }
            let class_id = receiver.object_class_id_unchecked();
            if class_id == 0 || cache.class_id != class_id {
                return None;
            }
            cache.func
        }
    };
    let user = guarded_user_target(target, argument_count)?;
    Some((target, user))
}

#[inline(always)]
unsafe fn guarded_cached_scalar_call_target(
    op_array: &crate::compiler::OpArray,
    guard: ScalarLongCallGuard,
    receiver: Option<&Value>,
    argument_count: usize,
) -> Option<(*const FunctionCommon, *const UserFunction)> {
    let (target, user) =
        guarded_cached_user_call_target(op_array, guard, receiver, argument_count)?;
    guarded_scalar_user_target(target, argument_count)?;
    Some((target, user))
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[inline(always)]
unsafe fn cached_receiver_generic_method_contract(
    eg: &ExecutorGlobals,
    op_array: &crate::compiler::OpArray,
    cache_ip: usize,
    receiver: &Value,
) -> Option<std::rc::Rc<crate::generics::GenericMethodContract>> {
    let method = op_array
        .instructions
        .get(cache_ip)
        .and_then(|initializer| op_array.literals.get(initializer.op2 as usize))
        .and_then(Value::as_str)?;
    eg.generic_instance_method_contract(receiver, method)
}

#[derive(Clone, Copy)]
enum TypedGenericCallBoundary {
    Long,
    LongDiscarded,
    LongStringToLong { string_arguments: u8 },
    LongToString,
}

/// Validate only the generic part of a frame-free typed method boundary after
/// normal dispatch identity has already been established. Each call site
/// describes its exact Long/String ABI once; native lowering never observes an
/// unproved receiver-specific contract. The existing linked-Long IC proof is
/// reused only for the matching all-Long shape. Mixed and String-return masks
/// remain cold call-site facts and do not grow the IC.
#[inline(always)]
unsafe fn typed_method_generic_contract_matches(
    eg: &ExecutorGlobals,
    op_array: &crate::compiler::OpArray,
    cache_ip: usize,
    receiver: &Value,
    argument_count: usize,
    boundary: TypedGenericCallBoundary,
) -> bool {
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    {
        let Some(cache) = op_array.cache.get(cache_ip) else {
            return false;
        };
        if !cache.method_has_generic_contract()
            || matches!(
                boundary,
                TypedGenericCallBoundary::Long | TypedGenericCallBoundary::LongDiscarded
            )
                && cache.method_has_linked_generic_long_contract()
        {
            return true;
        }
        return cached_receiver_generic_method_contract(eg, op_array, cache_ip, receiver)
            .as_deref()
            .is_some_and(|contract| match boundary {
                TypedGenericCallBoundary::Long => {
                    contract.admits_exact_long_call(argument_count as u32)
                }
                TypedGenericCallBoundary::LongDiscarded => {
                    contract.admits_exact_long_discarded_call(argument_count as u32)
                }
                TypedGenericCallBoundary::LongStringToLong { string_arguments } => contract
                    .admits_exact_long_string_to_long_call(
                        argument_count as u32,
                        string_arguments,
                    ),
                TypedGenericCallBoundary::LongToString => {
                    contract.admits_exact_long_to_string_call(argument_count as u32)
                }
            });
    }

    #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
    {
        let string_arguments = match boundary {
            TypedGenericCallBoundary::LongStringToLong { string_arguments } => string_arguments,
            TypedGenericCallBoundary::Long
            | TypedGenericCallBoundary::LongDiscarded
            | TypedGenericCallBoundary::LongToString => 0,
        };
        let _ = (
            eg,
            op_array,
            cache_ip,
            receiver,
            argument_count,
            string_arguments,
        );
        true
    }
}

/// Guard one frame-free typed method specialization against the same generic
/// boundary as the canonical call path. Bound-erased methods normally carry no
/// receiver-specific contract; concretely linked descendants use the exact
/// proof already interned in the method IC. A reified receiver resolves its
/// class/type tuple once at typed-region entry. The receiver CV cannot be
/// written by an admitted region, so the proof remains valid until its next
/// canonical side exit.
#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn guarded_quick_typed_method_target(
    eg: &ExecutorGlobals,
    op_array: &crate::compiler::OpArray,
    guard: ScalarLongCallGuard,
    receiver: &Value,
    argument_count: usize,
    boundary: TypedGenericCallBoundary,
) -> Option<(*const FunctionCommon, *const UserFunction)> {
    let ScalarLongCallGuard::MethodCache { .. } = guard else {
        return None;
    };
    let (target, user) =
        guarded_cached_user_call_target(op_array, guard, Some(receiver), argument_count)?;
    if !typed_method_generic_contract_matches(
        eg,
        op_array,
        guard.cache_ip(),
        receiver,
        argument_count,
        boundary,
    ) {
        return None;
    }

    Some((target, user))
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn guarded_quick_long_method_target(
    eg: &ExecutorGlobals,
    op_array: &crate::compiler::OpArray,
    guard: ScalarLongCallGuard,
    receiver: &Value,
    argument_count: usize,
) -> Option<(*const FunctionCommon, *const UserFunction)> {
    guarded_quick_typed_method_target(
        eg,
        op_array,
        guard,
        receiver,
        argument_count,
        TypedGenericCallBoundary::Long,
    )
}

/// Resolve and guard one IR `CallScalar` against the canonical inline cache.
/// A successful result has the exact scalar ABI and arity required by the IR;
/// every executor backend shares this identity contract.
unsafe fn guarded_typed_call_target(
    eg: &ExecutorGlobals,
    owner: &UserFunction,
    call: &ScalarLongCall,
    object_arguments: &[*const Value; 8],
    boundary: TypedGenericCallBoundary,
) -> Option<(*const FunctionCommon, *const UserFunction)> {
    let ip = call.guard.cache_ip();
    let initializer = owner.op_array.instructions.get(ip)?;
    let cache = owner.op_array.cache.get(ip)?;
    let receiver = match call.guard {
        ScalarLongCallGuard::FunctionCache { .. } => {
            if initializer.opcode != OpCode::InitFcall {
                return None;
            }
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
            None
        }
        ScalarLongCallGuard::MethodCache { receiver_slot, .. } => {
            if initializer.opcode != OpCode::InitMethodCall
                || initializer.op1_type != OpType::Cv
                || initializer.op1 != receiver_slot
            {
                return None;
            }
            let receiver_index =
                (receiver_slot as u32).checked_sub(owner.common.sig.this_offset)? as usize;
            let receiver = *object_arguments.get(receiver_index)?;
            if receiver.is_null() {
                return None;
            }
            Some(&*receiver)
        }
    };
    let resolved = guarded_cached_user_call_target(
        &owner.op_array,
        call.guard,
        receiver,
        call.arguments.len(),
    )?;
    if let Some(receiver) = receiver {
        if !typed_method_generic_contract_matches(
            eg,
            &owner.op_array,
            ip,
            receiver,
            call.arguments.len(),
            boundary,
        ) {
            return None;
        }
    }
    Some(resolved)
}

unsafe fn guarded_scalar_call_target(
    eg: &ExecutorGlobals,
    owner: &UserFunction,
    call: &ScalarLongCall,
    object_arguments: &[*const Value; 8],
) -> Option<(*const FunctionCommon, *const UserFunction)> {
    let (target, user) = guarded_typed_call_target(
        eg,
        owner,
        call,
        object_arguments,
        TypedGenericCallBoundary::Long,
    )?;
    guarded_scalar_user_target(target, call.arguments.len())?;
    Some((target, user))
}

unsafe fn evaluate_composed_scalar_body_plan(
    eg: &ExecutorGlobals,
    owner: &UserFunction,
    plan: &ComposedScalarLongFunctionPlan,
    arguments: &[i64; 8],
    object_arguments: &[*const Value; 8],
    calls: &mut [*const FunctionCommon; COMPOSED_SCALAR_MAX_CALLS],
    call_count: &mut usize,
    depth: usize,
) -> Option<i64> {
    if depth >= COMPOSED_SCALAR_MAX_CALLS
        || plan.program.operations.len() > COMPOSED_SCALAR_MAX_OPS
        || plan.program.output_count != 1
    {
        return None;
    }
    let mut temporaries = [0i64; COMPOSED_SCALAR_MAX_OPS];
    for (operation_index, operation) in plan.program.operations.iter().enumerate() {
        temporaries[operation_index] = match operation {
            ComposedScalarLongOp::Arithmetic(operation) => {
                let lhs = resolve_composed_body_source(operation.lhs, arguments, &temporaries);
                let rhs = resolve_composed_body_source(operation.rhs, arguments, &temporaries);
                apply_scalar_long_op(operation.kind, lhs, rhs)?
            }
            ComposedScalarLongOp::Call(call) => {
                let sources = &call.arguments;
                if sources.len() > 8 || *call_count >= COMPOSED_SCALAR_MAX_CALLS {
                    return None;
                }
                let (target, target_user) =
                    guarded_scalar_call_target(eg, owner, call, object_arguments)?;
                let target_user = &*target_user;
                let mut target_arguments = [0i64; 8];
                for (index, source) in sources.iter().copied().enumerate() {
                    target_arguments[index] =
                        resolve_composed_body_source(source, arguments, &temporaries);
                }
                let result = if let Some(target_plan) = target_user.scalar_long_plan.as_deref() {
                    evaluate_scalar_long_plan(target_plan, &target_arguments)?
                } else if let Some(target_plan) = target_user.composed_scalar_long_plan.as_deref() {
                    if target_plan.object_argument_mask != 0 {
                        return None;
                    }
                    let no_object_arguments = [std::ptr::null(); 8];
                    evaluate_composed_scalar_body_plan(
                        eg,
                        target_user,
                        target_plan,
                        &target_arguments,
                        &no_object_arguments,
                        calls,
                        call_count,
                        depth + 1,
                    )?
                } else {
                    return None;
                };
                if *call_count >= COMPOSED_SCALAR_MAX_CALLS {
                    return None;
                }
                calls[*call_count] = target;
                *call_count += 1;
                result
            }
        };
    }

    Some(resolve_composed_body_source(
        plan.program.outputs[0],
        arguments,
        &temporaries,
    ))
}

/// Resolve a one-level composed scalar body once at quick-loop entry. Nested
/// composed callees retain the general recursive evaluator, while the common
/// leaf-call shape can avoid repeated cache and function-strategy guards in
/// every loop iteration.
unsafe fn resolve_quick_composed_typed_body(
    eg: &ExecutorGlobals,
    owner: &UserFunction,
    plan: &ComposedTypedLongFunctionPlan,
    object_arguments: &[*const Value; 8],
    targets: &mut [*const FunctionCommon; COMPOSED_SCALAR_MAX_OPS],
    scalar_plans: &mut [*const ScalarLongFunctionPlan; COMPOSED_SCALAR_MAX_OPS],
    string_plans: &mut [*const ScalarStringFunctionPlan; COMPOSED_SCALAR_MAX_OPS],
) -> bool {
    if plan.program.operations.len() > COMPOSED_SCALAR_MAX_OPS || plan.program.output_count != 1 {
        return false;
    }
    let mut call_count = 0usize;
    for (index, operation) in plan.program.operations.iter().enumerate() {
        let (call, returns_string) = match operation {
            ComposedTypedLongOp::Call(call) => (call, false),
            ComposedTypedLongOp::StringCall(call) => (call, true),
            ComposedTypedLongOp::Arithmetic(_)
            | ComposedTypedLongOp::StringConcatLiteral { .. }
            | ComposedTypedLongOp::StringLength(_) => {
                continue;
            }
        };
        call_count += 1;
        if call_count > COMPOSED_SCALAR_MAX_CALLS || call.arguments.len() > 8 {
            return false;
        }
        let boundary = if returns_string {
            TypedGenericCallBoundary::LongToString
        } else {
            TypedGenericCallBoundary::Long
        };
        let Some((target, target_user)) =
            guarded_typed_call_target(eg, owner, call, object_arguments, boundary)
        else {
            return false;
        };
        targets[index] = target;
        if returns_string {
            let Some(target_plan) = (&*target_user).scalar_string_plan.as_deref() else {
                return false;
            };
            if target_plan.public_args as usize != call.arguments.len() {
                return false;
            }
            string_plans[index] = target_plan;
        } else {
            let Some(target_plan) = (&*target_user).scalar_long_plan.as_deref() else {
                return false;
            };
            if target_plan.public_args as usize != call.arguments.len() {
                return false;
            }
            scalar_plans[index] = target_plan;
        }
    }
    true
}

unsafe fn resolve_quick_composed_leaf_body(
    eg: &ExecutorGlobals,
    owner: &UserFunction,
    plan: &ComposedScalarLongFunctionPlan,
    object_arguments: &[*const Value; 8],
    targets: &mut [*const FunctionCommon; COMPOSED_SCALAR_MAX_OPS],
    scalar_plans: &mut [*const ScalarLongFunctionPlan; COMPOSED_SCALAR_MAX_OPS],
) -> bool {
    if plan.program.operations.len() > COMPOSED_SCALAR_MAX_OPS || plan.program.output_count != 1 {
        return false;
    }
    let mut call_count = 0usize;
    for (index, operation) in plan.program.operations.iter().enumerate() {
        let ComposedScalarLongOp::Call(call) = operation else {
            continue;
        };
        call_count += 1;
        if call_count > COMPOSED_SCALAR_MAX_CALLS || call.arguments.len() > 8 {
            return false;
        }
        let Some((target, target_user)) =
            guarded_scalar_call_target(eg, owner, call, object_arguments)
        else {
            return false;
        };
        let Some(target_plan) = (&*target_user).scalar_long_plan.as_deref() else {
            return false;
        };
        if target_plan.public_args as usize != call.arguments.len() {
            return false;
        }
        targets[index] = target;
        scalar_plans[index] = target_plan;
    }
    true
}

#[inline(always)]
unsafe fn evaluate_quick_composed_leaf_body(
    plan: &ComposedScalarLongFunctionPlan,
    arguments: &[i64; 8],
    scalar_plans: &[*const ScalarLongFunctionPlan; COMPOSED_SCALAR_MAX_OPS],
) -> Option<i64> {
    if plan.program.output_count != 1 {
        return None;
    }
    let mut temporaries = [0i64; COMPOSED_SCALAR_MAX_OPS];
    for (operation_index, operation) in plan.program.operations.iter().enumerate() {
        temporaries[operation_index] = match operation {
            ComposedScalarLongOp::Arithmetic(operation) => {
                let lhs = resolve_composed_body_source(operation.lhs, arguments, &temporaries);
                let rhs = resolve_composed_body_source(operation.rhs, arguments, &temporaries);
                apply_scalar_long_op(operation.kind, lhs, rhs)?
            }
            ComposedScalarLongOp::Call(call) => {
                let target_plan = scalar_plans[operation_index];
                if target_plan.is_null() {
                    return None;
                }
                let mut target_arguments = [0i64; 8];
                for (index, source) in call.arguments.iter().copied().enumerate() {
                    target_arguments[index] =
                        resolve_composed_body_source(source, arguments, &temporaries);
                }
                evaluate_scalar_long_plan(&*target_plan, &target_arguments)?
            }
        };
    }
    Some(resolve_composed_body_source(
        plan.program.outputs[0],
        arguments,
        &temporaries,
    ))
}

#[inline(always)]
unsafe fn evaluate_quick_composed_typed_body(
    plan: &ComposedTypedLongFunctionPlan,
    arguments: &[i64; 8],
    string_arguments: &[Option<usize>; 8],
    scalar_plans: &[*const ScalarLongFunctionPlan; COMPOSED_SCALAR_MAX_OPS],
    string_plans: &[*const ScalarStringFunctionPlan; COMPOSED_SCALAR_MAX_OPS],
) -> Option<i64> {
    if plan.program.output_count != 1 {
        return None;
    }
    let mut temporaries = [0i64; COMPOSED_SCALAR_MAX_OPS];
    let mut string_temporaries: [Option<usize>; COMPOSED_SCALAR_MAX_OPS] =
        [None; COMPOSED_SCALAR_MAX_OPS];
    for (operation_index, operation) in plan.program.operations.iter().enumerate() {
        temporaries[operation_index] = match operation {
            ComposedTypedLongOp::Arithmetic(operation) => {
                let lhs = resolve_composed_body_source(operation.lhs, arguments, &temporaries);
                let rhs = resolve_composed_body_source(operation.rhs, arguments, &temporaries);
                apply_scalar_long_op(operation.kind, lhs, rhs)?
            }
            ComposedTypedLongOp::Call(call) => {
                let sources = &call.arguments;
                let target_plan = scalar_plans[operation_index];
                if target_plan.is_null() {
                    return None;
                }
                let mut target_arguments = [0i64; 8];
                for (index, source) in sources.iter().copied().enumerate() {
                    target_arguments[index] =
                        resolve_composed_body_source(source, arguments, &temporaries);
                }
                evaluate_scalar_long_plan(&*target_plan, &target_arguments)?
            }
            ComposedTypedLongOp::StringCall(call) => {
                let target_plan = string_plans[operation_index];
                if target_plan.is_null() {
                    return None;
                }
                let mut target_arguments = [0i64; 8];
                for (index, source) in call.arguments.iter().copied().enumerate() {
                    target_arguments[index] =
                        resolve_composed_body_source(source, arguments, &temporaries);
                }
                string_temporaries[operation_index] =
                    Some(evaluate_scalar_string_plan(&*target_plan, &target_arguments)?.len());
                0
            }
            ComposedTypedLongOp::StringConcatLiteral { value, literal_len } => {
                string_temporaries[operation_index] = Some(
                    resolve_composed_string_source(*value, string_arguments, &string_temporaries)?
                        .checked_add(*literal_len as usize)?,
                );
                0
            }
            ComposedTypedLongOp::StringLength(source) => i64::try_from(
                resolve_composed_string_source(*source, string_arguments, &string_temporaries)?,
            )
            .ok()?,
        };
    }

    Some(resolve_composed_body_source(
        plan.program.outputs[0],
        arguments,
        &temporaries,
    ))
}

#[inline(always)]
unsafe fn caller_long_operand(
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    op_type: OpType,
    operand: u16,
) -> Option<i64> {
    let value = match op_type {
        OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
            &*(*caller).get_op_ptr(operand as u32, op_type, caller_op_array)
        }
        OpType::Unused => return None,
    };
    if value.value_type() != ValueType::Long || value.is_reference() {
        return None;
    }
    Some(value.raw_long())
}

/// Enter a composed body directly from a call-site whose arguments are already
/// scalar operands or one checked arithmetic instruction. This removes the
/// compact pending activation and every Send/DoFcall dispatch for the root.
#[inline(never)]
pub(crate) unsafe fn try_execute_direct_composed_scalar_body_call(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    initializer_ptr: *const Instruction,
    func: *const FunctionCommon,
    owner: &UserFunction,
    plan: &ComposedScalarLongFunctionPlan,
) -> Option<(i64, *const Instruction)> {
    if !composed_scalar_bodies_enabled() {
        return None;
    }
    let common = &*func;
    if common.fn_type != FunctionType::User
        || !common.supports_scalar_long_plan()
        || common.sig.public_arity() != plan.public_args as u32
    {
        return None;
    }

    let mut arguments = [0i64; 8];
    let mut cursor = initializer_ptr.add(1);
    for index in 0..plan.public_args as usize {
        let destination = common.sig.param_cv_index(index as u32) as u16;
        let instruction = &*cursor;
        if matches!(instruction.opcode, OpCode::SendVal | OpCode::SendVarEx) {
            if instruction.op2 != destination {
                return None;
            }
            arguments[index] = caller_long_operand(
                caller,
                caller_op_array,
                instruction.op1_type,
                instruction.op1,
            )?;
            cursor = cursor.add(1);
            continue;
        }

        let kind = match instruction.opcode {
            OpCode::Add | OpCode::Add_TmpTmp | OpCode::Add_CvTmp => ScalarLongOpKind::Add,
            OpCode::Sub | OpCode::Sub_CvConst | OpCode::Sub_TmpTmp => ScalarLongOpKind::Subtract,
            OpCode::Mul => ScalarLongOpKind::Multiply,
            OpCode::Mod | OpCode::Mod_LongLong => ScalarLongOpKind::Modulo,
            OpCode::BitwiseAnd | OpCode::BitwiseAnd_LongLong => ScalarLongOpKind::BitwiseAnd,
            OpCode::BitwiseOr | OpCode::BitwiseOr_LongLong => ScalarLongOpKind::BitwiseOr,
            OpCode::BitwiseXor | OpCode::BitwiseXor_LongLong => ScalarLongOpKind::BitwiseXor,
            _ => return None,
        };
        if !matches!(instruction.result_type, OpType::Tmp | OpType::Var) {
            return None;
        }
        let lhs = caller_long_operand(
            caller,
            caller_op_array,
            instruction.op1_type,
            instruction.op1,
        )?;
        let rhs = caller_long_operand(
            caller,
            caller_op_array,
            instruction.op2_type,
            instruction.op2,
        )?;
        let value = apply_scalar_long_op(kind, lhs, rhs)?;
        cursor = cursor.add(1);
        let send = &*cursor;
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || !matches!(send.op1_type, OpType::Tmp | OpType::Var)
            || send.op1 != instruction.result
            || send.op2 != destination
        {
            return None;
        }
        arguments[index] = value;
        cursor = cursor.add(1);
    }

    let do_fcall = &*cursor;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(
            do_fcall.result_type,
            OpType::Tmp | OpType::Var | OpType::Unused
        )
    {
        return None;
    }
    let mut calls = [std::ptr::null(); COMPOSED_SCALAR_MAX_CALLS];
    let mut call_count = 0usize;
    let result = evaluate_composed_scalar_body_plan(
        eg,
        owner,
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
    record_scalar_call(common);
    Some((result, cursor))
}

#[inline(always)]
unsafe fn composed_scalar_callee(
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    initializer_ptr: *const Instruction,
) -> Option<(*const FunctionCommon, *const ScalarLongFunctionPlan)> {
    let initializer = &*initializer_ptr;
    let ip = initializer_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
    let cache_ip = u32::try_from(ip).ok()?;
    let public_num_args = match initializer.opcode {
        OpCode::InitFcall => initializer.op1 as usize,
        OpCode::InitMethodCall => initializer.extended_value as usize,
        _ => return None,
    };
    let (func, user) = match initializer.opcode {
        OpCode::InitFcall => guarded_cached_scalar_call_target(
            caller_op_array,
            ScalarLongCallGuard::FunctionCache { cache_ip },
            None,
            public_num_args,
        )?,
        OpCode::InitMethodCall if initializer.op1_type == OpType::Cv => {
            let receiver = &*(*caller).get_op_ptr(
                initializer.op1 as u32,
                initializer.op1_type,
                caller_op_array,
            );
            guarded_cached_scalar_call_target(
                caller_op_array,
                ScalarLongCallGuard::MethodCache {
                    cache_ip,
                    receiver_slot: initializer.op1,
                },
                Some(receiver),
                public_num_args,
            )?
        }
        OpCode::InitMethodCall => {
            let cache = caller_op_array.cache.get(ip)?;
            let receiver = match initializer.op1_type {
                OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => &*(*caller).get_op_ptr(
                    initializer.op1 as u32,
                    initializer.op1_type,
                    caller_op_array,
                ),
                OpType::Unused => return None,
            };
            if receiver.value_type() != ValueType::Object {
                return None;
            }
            let class_id = receiver.object_class_id_unchecked();
            if class_id == 0 || cache.func.is_null() || cache.class_id != class_id {
                return None;
            }
            let user = guarded_scalar_user_target(cache.func, public_num_args)?;
            (cache.func, user)
        }
        _ => return None,
    };
    let plan = (&*user).scalar_long_plan.as_deref()?;
    Some((func, plan as *const ScalarLongFunctionPlan))
}

/// Preflight the generic boundaries in an already-recognized scalar call tree
/// without changing the native builder or its stack layout. The root target is
/// guarded by `guarded_quick_scalar_call_target()` before this walk; nested
/// methods consume their own exact erased/reified proof here. Returning the
/// terminal DoFcall also binds the walk to the planner's canonical resume edge.
#[cfg(all(
    feature = "quick-loops",
    any(feature = "php-generics-erased", feature = "php-generics-reified")
))]
#[inline(never)]
unsafe fn guard_quick_scalar_call_tree_generics(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    initializer_ptr: *const Instruction,
    root_proof_checked: bool,
    depth: usize,
) -> Option<*const Instruction> {
    if depth >= COMPOSED_SCALAR_MAX_CALLS {
        return None;
    }
    let initializer = &*initializer_ptr;
    let ip = initializer_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
    let (func, plan) = composed_scalar_callee(caller, caller_op_array, initializer_ptr)?;
    let common = &*func;

    if !root_proof_checked && initializer.opcode == OpCode::InitMethodCall {
        let receiver = match initializer.op1_type {
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => &*(*caller).get_op_ptr(
                initializer.op1 as u32,
                initializer.op1_type,
                caller_op_array,
            ),
            OpType::Unused => return None,
        };
        if !typed_method_generic_contract_matches(
            eg,
            caller_op_array,
            ip,
            receiver,
            (&*plan).public_args as usize,
            TypedGenericCallBoundary::Long,
        ) {
            return None;
        }
    }

    let plan = &*plan;
    let mut cursor = initializer_ptr.add(1);
    for index in 0..plan.public_args as usize {
        let destination = common.sig.param_cv_index(index as u32) as u16;
        let instruction = &*cursor;
        if matches!(instruction.opcode, OpCode::SendVal | OpCode::SendVarEx) {
            if instruction.op2 != destination {
                return None;
            }
            cursor = cursor.add(1);
            continue;
        }

        if matches!(
            instruction.opcode,
            OpCode::Add
                | OpCode::Add_TmpTmp
                | OpCode::Add_CvTmp
                | OpCode::Sub
                | OpCode::Sub_CvConst
                | OpCode::Sub_TmpTmp
                | OpCode::Mul
                | OpCode::Mod
                | OpCode::Mod_LongLong
                | OpCode::BitwiseAnd
                | OpCode::BitwiseAnd_LongLong
                | OpCode::BitwiseOr
                | OpCode::BitwiseOr_LongLong
                | OpCode::BitwiseXor
                | OpCode::BitwiseXor_LongLong
        ) {
            if !matches!(instruction.result_type, OpType::Tmp | OpType::Var) {
                return None;
            }
            let send = &*cursor.add(1);
            if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                || !matches!(send.op1_type, OpType::Tmp | OpType::Var)
                || send.op1 != instruction.result
                || send.op2 != destination
            {
                return None;
            }
            cursor = cursor.add(2);
            continue;
        }

        let nested_do_fcall = guard_quick_scalar_call_tree_generics(
            eg,
            caller,
            caller_op_array,
            cursor,
            false,
            depth + 1,
        )?;
        let nested_result = &*nested_do_fcall;
        let send = &*nested_do_fcall.add(1);
        if !matches!(nested_result.result_type, OpType::Tmp | OpType::Var)
            || !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || !matches!(send.op1_type, OpType::Tmp | OpType::Var)
            || send.op1 != nested_result.result
            || send.op2 != destination
        {
            return None;
        }
        cursor = nested_do_fcall.add(2);
    }

    ((*cursor).opcode == OpCode::DoFcall).then_some(cursor)
}

/// Recursively evaluate a compiler-proven scalar call tree encoded by ordinary
/// Init/Send/DoFcall instructions. Only already-cached direct functions and
/// monomorphic methods participate, so failure is read-only and can restart via
/// the canonical VM protocol.
unsafe fn evaluate_composed_scalar_call(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    initializer_ptr: *const Instruction,
    func: *const FunctionCommon,
    plan: &ScalarLongFunctionPlan,
    calls: &mut [*const FunctionCommon; COMPOSED_SCALAR_MAX_CALLS],
    call_count: &mut usize,
    depth: usize,
) -> Option<(i64, *const Instruction)> {
    if depth >= COMPOSED_SCALAR_MAX_CALLS || *call_count >= COMPOSED_SCALAR_MAX_CALLS {
        return None;
    }
    let common = &*func;
    if common.fn_type != FunctionType::User
        || !common.supports_scalar_long_plan()
        || common.sig.public_arity() != plan.public_args as u32
    {
        return None;
    }

    let mut arguments = [0i64; 8];
    let mut cursor = initializer_ptr.add(1);
    for index in 0..plan.public_args as usize {
        let destination = common.sig.param_cv_index(index as u32) as u16;
        let instruction = &*cursor;
        if matches!(instruction.opcode, OpCode::SendVal | OpCode::SendVarEx) {
            if instruction.op2 != destination {
                return None;
            }
            let value = match instruction.op1_type {
                OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => &*(*caller).get_op_ptr(
                    instruction.op1 as u32,
                    instruction.op1_type,
                    caller_op_array,
                ),
                OpType::Unused => return None,
            };
            if value.value_type() != ValueType::Long || value.is_reference() {
                return None;
            }
            arguments[index] = value.raw_long();
            cursor = cursor.add(1);
            continue;
        }

        let arithmetic_kind = match instruction.opcode {
            OpCode::Add | OpCode::Add_TmpTmp | OpCode::Add_CvTmp => Some(ScalarLongOpKind::Add),
            OpCode::Sub | OpCode::Sub_CvConst | OpCode::Sub_TmpTmp => {
                Some(ScalarLongOpKind::Subtract)
            }
            OpCode::Mul => Some(ScalarLongOpKind::Multiply),
            OpCode::Mod | OpCode::Mod_LongLong => Some(ScalarLongOpKind::Modulo),
            OpCode::BitwiseXor | OpCode::BitwiseXor_LongLong => Some(ScalarLongOpKind::BitwiseXor),
            OpCode::BitwiseAnd | OpCode::BitwiseAnd_LongLong => Some(ScalarLongOpKind::BitwiseAnd),
            OpCode::BitwiseOr | OpCode::BitwiseOr_LongLong => Some(ScalarLongOpKind::BitwiseOr),
            _ => None,
        };
        if let Some(kind) = arithmetic_kind {
            if !matches!(instruction.result_type, OpType::Tmp | OpType::Var) {
                return None;
            }
            let lhs = caller_long_operand(
                caller,
                caller_op_array,
                instruction.op1_type,
                instruction.op1,
            )?;
            let rhs = caller_long_operand(
                caller,
                caller_op_array,
                instruction.op2_type,
                instruction.op2,
            )?;
            let value = apply_scalar_long_op(kind, lhs, rhs)?;
            cursor = cursor.add(1);
            let send = &*cursor;
            if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                || !matches!(send.op1_type, OpType::Tmp | OpType::Var)
                || send.op1 != instruction.result
                || send.op2 != destination
            {
                return None;
            }
            arguments[index] = value;
            cursor = cursor.add(1);
            continue;
        }

        let (nested_func, nested_plan) = composed_scalar_callee(caller, caller_op_array, cursor)?;
        let nested_initializer = &*cursor;
        if nested_initializer.opcode == OpCode::InitMethodCall {
            let nested_ip = cursor.offset_from(caller_op_array.instructions.as_ptr()) as usize;
            let receiver = match nested_initializer.op1_type {
                OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                    &*(*caller).get_op_ptr(
                        nested_initializer.op1 as u32,
                        nested_initializer.op1_type,
                        caller_op_array,
                    )
                }
                OpType::Unused => return None,
            };
            if !typed_method_generic_contract_matches(
                eg,
                caller_op_array,
                nested_ip,
                receiver,
                (&*nested_plan).public_args as usize,
                TypedGenericCallBoundary::Long,
            ) {
                return None;
            }
        }
        let (nested_result, nested_do_fcall) = evaluate_composed_scalar_call(
            eg,
            caller,
            caller_op_array,
            cursor,
            nested_func,
            &*nested_plan,
            calls,
            call_count,
            depth + 1,
        )?;
        let nested_result_instruction = &*nested_do_fcall;
        if !matches!(
            nested_result_instruction.result_type,
            OpType::Tmp | OpType::Var
        ) {
            return None;
        }
        cursor = nested_do_fcall.add(1);
        let send = &*cursor;
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || !matches!(send.op1_type, OpType::Tmp | OpType::Var)
            || send.op1 != nested_result_instruction.result
            || send.op2 != destination
        {
            return None;
        }
        arguments[index] = nested_result;
        cursor = cursor.add(1);
    }

    let do_fcall = &*cursor;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(
            do_fcall.result_type,
            OpType::Tmp | OpType::Var | OpType::Unused
        )
    {
        return None;
    }
    let result = evaluate_scalar_long_plan(plan, &arguments)?;
    calls[*call_count] = func;
    *call_count += 1;
    Some((result, cursor))
}

#[inline(never)]
pub(crate) unsafe fn try_execute_composed_scalar_long_call(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    initializer_ptr: *const Instruction,
    func: *const FunctionCommon,
    plan: &ScalarLongFunctionPlan,
) -> Option<(i64, *const Instruction)> {
    if !composed_scalar_calls_enabled()
        || (*initializer_ptr)._pad & CALL_FLAG_DEFERRED_SCALAR_CANDIDATE == 0
    {
        return None;
    }
    let initializer = &*initializer_ptr;
    if initializer.opcode == OpCode::InitMethodCall {
        let ip = initializer_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
        let receiver = match initializer.op1_type {
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => &*(*caller).get_op_ptr(
                initializer.op1 as u32,
                initializer.op1_type,
                caller_op_array,
            ),
            OpType::Unused => return None,
        };
        if !typed_method_generic_contract_matches(
            eg,
            caller_op_array,
            ip,
            receiver,
            plan.public_args as usize,
            TypedGenericCallBoundary::Long,
        ) {
            return None;
        }
    }
    let mut calls = [std::ptr::null(); COMPOSED_SCALAR_MAX_CALLS];
    let mut call_count = 0usize;
    let evaluated = evaluate_composed_scalar_call(
        eg,
        caller,
        caller_op_array,
        initializer_ptr,
        func,
        plan,
        &mut calls,
        &mut call_count,
        0,
    )?;

    for called in calls.into_iter().take(call_count) {
        record_scalar_call(&*called);
    }
    Some(evaluated)
}

const BINARY_LONG_RECURSION_MAX_DEPTH: usize = 256;

#[derive(Clone, Copy)]
struct BinaryLongActivation {
    argument: i64,
    first_result: i64,
    state: u8,
}

/// Preserve the source recursion's depth-first evaluation order using compact
/// integer activations. The recognized body is pure, so any failed arithmetic
/// guard can safely restart from the root through the canonical PHP executor.
#[inline(never)]
fn execute_binary_long_recursion(
    eg: &ExecutorGlobals,
    plan: &BinaryLongRecursionPlan,
    input: i64,
) -> Result<Option<i64>, VmError> {
    let empty = BinaryLongActivation {
        argument: 0,
        first_result: 0,
        state: 0,
    };
    let mut activations = [empty; BINARY_LONG_RECURSION_MAX_DEPTH];
    let mut depth = 0usize;
    let mut argument = input;
    let mut result = 0i64;
    let mut has_result = false;
    let mut steps = 0u32;

    loop {
        steps = steps.wrapping_add(1);
        if steps & 1023 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            handle_interrupt(eg)?;
        }

        if !has_result {
            let is_base = match plan.condition {
                LongRecursiveCondition::LessThan => argument < plan.threshold,
                LongRecursiveCondition::LessThanOrEqual => argument <= plan.threshold,
            };
            if is_base {
                result = match plan.base {
                    LongRecursiveBase::Argument => argument,
                    LongRecursiveBase::Constant(value) => value,
                };
            } else {
                if depth == BINARY_LONG_RECURSION_MAX_DEPTH {
                    return Ok(None);
                }
                activations[depth] = BinaryLongActivation {
                    argument,
                    first_result: 0,
                    state: 0,
                };
                depth += 1;
                let Some(next) = argument.checked_sub(plan.first_delta) else {
                    return Ok(None);
                };
                argument = next;
                continue;
            }
        }

        if depth == 0 {
            return Ok(Some(result));
        }

        let activation = &mut activations[depth - 1];
        if activation.state == 0 {
            activation.first_result = result;
            activation.state = 1;
            let Some(next) = activation.argument.checked_sub(plan.second_delta) else {
                return Ok(None);
            };
            argument = next;
            has_result = false;
            continue;
        }

        let combined = match plan.combine {
            LongRecursiveCombine::Add => activation.first_result.checked_add(result),
            LongRecursiveCombine::Subtract => activation.first_result.checked_sub(result),
            LongRecursiveCombine::Multiply => activation.first_result.checked_mul(result),
        };
        let Some(combined) = combined else {
            return Ok(None);
        };
        result = combined;
        depth -= 1;
        has_result = true;
    }
}
