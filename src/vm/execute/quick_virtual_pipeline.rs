// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickResolvedVirtualPipeline {
    class_id: u32,
    class_def: *const crate::compiler::compile::ClassDef,
    property_slots: [usize; 8],
    property_arguments: [u8; 8],
    property_count: u8,
    constructor_target: *const FunctionCommon,
    method_receiver: *const Value,
    method_target: *const FunctionCommon,
    method_user: *const UserFunction,
    method_plan: *const ObjectArrayFunctionPlan,
    nested_calls: [ResolvedObjectArrayCall; 8],
    nested_call_count: u8,
    consumer_entries: [u8; 4],
    trailing_entry: Option<u8>,
}

/// Pre-resolve nested calls whose receiver belongs to the invariant service
/// object. ObjectArrayFunctionPlan contains no writes, so these receiver and
/// dispatch guards remain valid for the lifetime of the active quick region.
unsafe fn resolve_quick_object_array_calls(
    eg: &ExecutorGlobals,
    receiver: &Value,
    owner: &UserFunction,
    plan: &ObjectArrayFunctionPlan,
) -> Option<([ResolvedObjectArrayCall; 8], u8)> {
    let mut resolved = [ResolvedObjectArrayCall::EMPTY; 8];
    let mut resolved_count = 0usize;
    let no_arguments = [ObjectLongArgument::None; 8];

    for operation in plan.operations.iter() {
        let ObjectArrayLongOp::Call(call) = operation else {
            continue;
        };
        let call_receiver = match call.receiver {
            ObjectArraySource::Receiver => receiver as *const Value,
            ObjectArraySource::Literal(literal) => {
                owner.op_array.literals.get(literal as usize)? as *const Value
            }
            ObjectArraySource::Property {
                object: ObjectLongObjectSource::Receiver,
                cache_ip,
            } => match object_array_property(
                owner,
                receiver,
                &no_arguments,
                ObjectLongObjectSource::Receiver,
                cache_ip,
            )? {
                ObjectArrayResolved::Borrowed(pointer) => pointer,
                ObjectArrayResolved::Long(_) | ObjectArrayResolved::Virtual(_) => return None,
            },
            ObjectArraySource::Argument(_)
            | ObjectArraySource::LongSlot(_)
            | ObjectArraySource::Property {
                object: ObjectLongObjectSource::Argument(_),
                ..
            } => return None,
        };
        *resolved.get_mut(resolved_count)? =
            resolve_object_array_call(eg, owner, call_receiver, call)?;
        resolved_count += 1;
    }

    Some((resolved, resolved_count as u8))
}

/// Resolve loop-invariant class, signature, method and declared-property
/// guards once when entering a typed region. Constructor values still vary per
/// iteration, but their proven Long/String representations cannot invalidate
/// these dispatch/layout facts while the region is active.
#[cfg(feature = "quick-loops")]
unsafe fn resolve_quick_virtual_object_array_pipeline(
    eg: &ExecutorGlobals,
    caller_op_array: &crate::compiler::OpArray,
    slot_base: *mut Value,
    slots: &[i64; 64],
    string_state: &QuickStringSlotState,
    new_ip: usize,
    constructor_arguments: &[QuickVirtualValueSource; 8],
    argument_count: u8,
    consumers: &[QuickObjectArrayConsumer; 4],
    consumer_count: u8,
    trailing_key_literal: Option<u16>,
) -> Option<QuickResolvedVirtualPipeline> {
    let new_object = caller_op_array.instructions.get(new_ip)?;
    if new_object.opcode != OpCode::NewObj
        || new_object._pad & NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE == 0
        || new_object.op1_type != OpType::Const
        || new_object.extended_value != u32::from(argument_count)
        || argument_count == 0
        || argument_count > 8
    {
        return None;
    }
    let new_cache = caller_op_array.cache.get(new_ip)?;
    if new_cache.class_id == 0 || new_cache.func.is_null() {
        return None;
    }
    let class_def = eg.class_by_id(new_cache.class_id)?;
    let class_name = caller_op_array
        .literals
        .get(new_object.op1 as usize)?
        .as_str()?;
    if !class_def.name.eq_ignore_ascii_case(class_name)
        || class_def
            .methods
            .iter()
            .any(|(name, _, _, _, _)| name.eq_ignore_ascii_case("__destruct"))
    {
        return None;
    }

    let constructor_common = &*new_cache.func;
    if constructor_common.fn_type != FunctionType::User
        || constructor_common.sig.public_arity() != u32::from(argument_count)
        || constructor_common.sig.required_num_args != u32::from(argument_count)
        || constructor_common.sig.ref_args != 0
        || constructor_common.sig.is_variadic
        || !constructor_common.plan.call.is_compact_user_call()
        || constructor_common.plan.ret != ReturnStrategy::Fast
    {
        return None;
    }
    let constructor = &*(new_cache.func as *const UserFunction);
    let constructor_plan = constructor.property_init_plan.as_deref()?;
    if constructor_plan.public_args != argument_count
        || constructor_plan.assignments.len() > 8
    {
        return None;
    }

    let declaring_class = eg.declaring_class_of(new_cache.func);
    for index in 0..argument_count as usize {
        let send = caller_op_array.instructions.get(new_ip + 1 + index)?;
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || send.op2 as u32 != constructor_common.sig.param_cv_index(index as u32)
        {
            return None;
        }
        let hint = constructor_common
            .sig
            .param_type_hints
            .get(index)
            .unwrap_or(&ParamTypeHint::None);
        let valid = match constructor_arguments[index] {
            QuickVirtualValueSource::Long(source) => {
                let value = Value::long(quick_long_operand(slots, source));
                check_type_hint(
                    &value,
                    hint,
                    eg,
                    caller_op_array.strict_types,
                    declaring_class,
                )
            }
            QuickVirtualValueSource::StringLiteral(literal) => {
                let value = caller_op_array.literals.get(literal as usize)?;
                value.value_type() == ValueType::String
                    && !value.is_reference()
                    && check_type_hint(
                        value,
                        hint,
                        eg,
                        caller_op_array.strict_types,
                        declaring_class,
                    )
            }
            QuickVirtualValueSource::StringSlot(slot) => {
                let value = string_state.value(slot);
                value.value_type() == ValueType::String
                    && !value.is_reference()
                    && check_type_hint(
                        value,
                        hint,
                        eg,
                        caller_op_array.strict_types,
                        declaring_class,
                    )
            }
        };
        if !valid {
            return None;
        }
    }

    let constructor_do_ip = new_ip + 1 + argument_count as usize;
    let constructor_do = caller_op_array.instructions.get(constructor_do_ip)?;
    let object_assign = caller_op_array.instructions.get(constructor_do_ip + 1)?;
    if constructor_do.opcode != OpCode::DoFcall
        || object_assign.opcode != OpCode::AssignCv
        || object_assign.op1_type != OpType::Cv
        || object_assign.op2_type != new_object.result_type
        || object_assign.op2 != new_object.result
        || object_assign.result_type != OpType::Unused
    {
        return None;
    }

    let mut property_slots = [usize::MAX; 8];
    let mut property_arguments = [0u8; 8];
    let mut property_count = 0usize;
    for assignment in constructor_plan.assignments.iter().copied() {
        let cache = constructor
            .op_array
            .cache
            .get(assignment.cache_ip as usize)?;
        if cache.class_id != new_cache.class_id || assignment.argument >= argument_count {
            return None;
        }
        let accepts_value = match constructor_arguments[assignment.argument as usize] {
            QuickVirtualValueSource::Long(source) => {
                let value = Value::long(quick_long_operand(slots, source));
                instance_property_cache_accepts_exact_non_generic_write(
                    cache,
                    &value,
                    eg,
                    &class_def.name,
                )
            }
            QuickVirtualValueSource::StringLiteral(literal) => {
                let value = caller_op_array.literals.get(literal as usize)?;
                instance_property_cache_accepts_exact_non_generic_write(
                    cache,
                    value,
                    eg,
                    &class_def.name,
                )
            }
            QuickVirtualValueSource::StringSlot(slot) => {
                instance_property_cache_accepts_exact_non_generic_write(
                    cache,
                    string_state.value(slot),
                    eg,
                    &class_def.name,
                )
            }
        };
        if !accepts_value {
            return None;
        }
        let property_slot = cache.property_slot();
        if let Some(index) = property_slots[..property_count]
            .iter()
            .position(|existing| *existing == property_slot)
        {
            property_arguments[index] = assignment.argument;
        } else {
            *property_slots.get_mut(property_count)? = property_slot;
            property_arguments[property_count] = assignment.argument;
            property_count += 1;
        }
    }

    let object_assign_ip = constructor_do_ip + 1;
    let (method_ip, _) = crate::vm::quick::after_optional_assignment_release(
        caller_op_array,
        object_assign_ip,
        new_object.result_type,
        new_object.result,
    )?;
    let method = caller_op_array.instructions.get(method_ip)?;
    if method.opcode != OpCode::InitMethodCall
        || method._pad & CALL_FLAG_OBJECT_ARRAY_CONSUMERS == 0
        || method.op1_type != OpType::Cv
        || method.extended_value != 1
    {
        return None;
    }
    let method_receiver = &*slot_base.add(method.op1 as usize);
    if method_receiver.value_type() != ValueType::Object || method_receiver.is_reference() {
        return None;
    }
    let receiver_class_id = method_receiver.object_class_id_unchecked();
    let method_cache = caller_op_array.cache.get(method_ip)?;
    if receiver_class_id == 0
        || method_cache.class_id != receiver_class_id
        || method_cache.func.is_null()
        || !method_return_dispatch_contract_matches(method, &*method_cache.func)
    {
        return None;
    }
    let method_common = &*method_cache.func;
    if method_common.fn_type != FunctionType::User
        || method_common.sig.public_arity() != 1
        || method_common.sig.required_num_args != 1
        || method_common.sig.ref_args != 0
        || method_common.sig.is_variadic
        || !method_common.plan.call.is_compact_user_call()
        || method_common.plan.ret != ReturnStrategy::Fast
    {
        return None;
    }
    let method_user = &*(method_cache.func as *const UserFunction);
    let method_plan = method_user.object_array_plan.as_deref()?;
    if method_plan.public_args != 1 {
        return None;
    }

    let send = caller_op_array.instructions.get(method_ip + 1)?;
    let virtual_object = VirtualObject {
        class_id: new_cache.class_id,
        class_def: class_def as *const crate::compiler::compile::ClassDef,
        property_slots,
        property_values: [VirtualPropertyValue::Empty; 8],
        property_count: property_count as u8,
    };
    if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
        || send.op1_type != OpType::Cv
        || send.op1 != object_assign.op1
        || send.op2 as u32 != method_common.sig.param_cv_index(0)
        || !virtual_object_matches_hint(
            &virtual_object,
            method_common
                .sig
                .param_type_hints
                .first()
                .unwrap_or(&ParamTypeHint::None),
            eg,
            eg.declaring_class_of(method_cache.func),
        )
    {
        return None;
    }
    let method_do = caller_op_array.instructions.get(method_ip + 2)?;
    if method_do.opcode != OpCode::DoFcall
        || !matches!(method_do.result_type, OpType::Tmp | OpType::Var)
    {
        return None;
    }
    let (nested_calls, nested_call_count) =
        resolve_quick_object_array_calls(eg, method_receiver, method_user, method_plan)?;
    if consumer_count as usize > consumers.len() {
        return None;
    }
    let mut consumer_entries = [u8::MAX; 4];
    for (index, consumer) in consumers
        .iter()
        .copied()
        .take(consumer_count as usize)
        .enumerate()
    {
        consumer_entries[index] = u8::try_from(object_array_entry_index_for_key(
            caller_op_array,
            consumer.key_literal,
            method_user,
            method_plan,
        )?)
        .ok()?;
    }
    let trailing_entry = match trailing_key_literal {
        Some(key) => Some(
            u8::try_from(object_array_entry_index_for_key(
                caller_op_array,
                key,
                method_user,
                method_plan,
            )?)
            .ok()?,
        ),
        None => None,
    };

    Some(QuickResolvedVirtualPipeline {
        class_id: new_cache.class_id,
        class_def: class_def as *const crate::compiler::compile::ClassDef,
        property_slots,
        property_arguments,
        property_count: property_count as u8,
        constructor_target: new_cache.func,
        method_receiver: method_receiver as *const Value,
        method_target: method_cache.func,
        method_user,
        method_plan,
        nested_calls,
        nested_call_count,
        consumer_entries,
        trailing_entry,
    })
}

/// Evaluate only the per-iteration scalar portion of a pre-resolved virtual
/// pipeline. All writes are delayed until every nested call, key lookup and
/// checked accumulator addition succeeds.
#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn try_execute_resolved_quick_virtual_pipeline(
    eg: &ExecutorGlobals,
    caller_op_array: &crate::compiler::OpArray,
    slots: &mut [i64; 64],
    string_state: &QuickStringSlotState,
    resolved: QuickResolvedVirtualPipeline,
    constructor_arguments: &[QuickVirtualValueSource; 8],
    consumers: &[QuickObjectArrayConsumer; 4],
    consumer_count: u8,
    trailing_result: u16,
) -> Option<ObjectArrayEvaluated> {
    let mut virtual_object = VirtualObject {
        class_id: resolved.class_id,
        class_def: resolved.class_def,
        property_slots: resolved.property_slots,
        property_values: [VirtualPropertyValue::Empty; 8],
        property_count: resolved.property_count,
    };
    for index in 0..resolved.property_count as usize {
        let argument = resolved.property_arguments[index] as usize;
        virtual_object.property_values[index] = match constructor_arguments[argument] {
            QuickVirtualValueSource::Long(source) => {
                VirtualPropertyValue::Long(quick_long_operand(slots, source))
            }
            QuickVirtualValueSource::StringLiteral(literal) => {
                VirtualPropertyValue::Borrowed(
                    caller_op_array.literals.get(literal as usize)? as *const Value,
                )
            }
            QuickVirtualValueSource::StringSlot(slot) => {
                VirtualPropertyValue::Borrowed(string_state.value(slot) as *const Value)
            }
        };
    }

    let mut method_arguments = [ObjectLongArgument::None; 8];
    method_arguments[0] = ObjectLongArgument::Virtual(&virtual_object);
    let evaluated = evaluate_object_array_values(
        eg,
        &*resolved.method_receiver,
        &method_arguments,
        &*resolved.method_user,
        &*resolved.method_plan,
        &resolved.nested_calls[..resolved.nested_call_count as usize],
    )?;

    let mut destinations = [0u16; 4];
    let mut results = [0i64; 4];
    for (index, consumer) in consumers
        .iter()
        .copied()
        .take(consumer_count as usize)
        .enumerate()
    {
        let current = destinations[..index]
            .iter()
            .rposition(|destination| *destination == consumer.accumulator)
            .map(|previous| results[previous])
            .unwrap_or(slots[consumer.accumulator as usize]);
        let entry = *resolved.consumer_entries.get(index)? as usize;
        let value = *evaluated.values.get(entry)?;
        destinations[index] = consumer.accumulator;
        results[index] = current.checked_add(value)?;
    }
    let trailing_value = if let Some(entry) = resolved.trailing_entry {
        Some(*evaluated.values.get(entry as usize)?)
    } else {
        None
    };

    for index in 0..consumer_count as usize {
        slots[destinations[index] as usize] = results[index];
    }
    if let Some(value) = trailing_value {
        slots[trailing_result as usize] = value;
    }
    Some(evaluated)
}
