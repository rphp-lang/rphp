// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
enum QuickResolvedObjectOp {
    None,
    PropertyRead {
        property: *const Value,
    },
    PropertyMethod {
        receiver: *const Value,
        target: *const FunctionCommon,
        plan: *const LongPropertyMethodPlan,
        property_slots: [usize; 8],
        property_count: u8,
    },
    PropertyGetter {
        receiver: *const Value,
        target: *const FunctionCommon,
        property_slot: usize,
    },
    ScalarMethod {
        target: *const FunctionCommon,
        plan: *const ScalarLongFunctionPlan,
    },
    ObjectLongMethod {
        receiver: *const Value,
        target: *const FunctionCommon,
        user: *const UserFunction,
        plan: *const ObjectLongFunctionPlan,
    },
    ComposedTypedMethod {
        target: *const FunctionCommon,
        plan: *const ComposedTypedLongFunctionPlan,
    },
    ComposedProperty {
        outer_receiver: *const Value,
        outer_target: *const FunctionCommon,
        outer_user: *const UserFunction,
        outer_plan: *const LongPropertyMethodPlan,
        inner_receiver: *const Value,
        inner_target: *const FunctionCommon,
        inner_property_slot: usize,
    },
    VirtualPipeline {
        pipeline: QuickResolvedVirtualPipeline,
    },
}

#[inline]
#[cfg(feature = "quick-loops")]
fn quick_object_property_reads_are_invariant(plan: &QuickLongOpsLoop) -> bool {
    !plan.ops.iter().any(|operation| {
        matches!(
            operation,
            QuickLongOp::PropertyMethodCall { .. }
                | QuickLongOp::PropertyGetterCall { .. }
                | QuickLongOp::ScalarMethodCall { .. }
                | QuickLongOp::ObjectLongMethodCall { .. }
                | QuickLongOp::ComposedPropertyCall { .. }
                | QuickLongOp::VirtualObjectArrayPipeline { .. }
        )
    })
}

/// Materialize property projections that are stable for the whole typed
/// region. Resolution already binds the invariant receiver's property slot;
/// regions containing any object call remain on the per-operation reread path
/// because a method may mutate an aliased receiver property.
#[inline]
#[cfg(feature = "quick-loops")]
unsafe fn prepare_quick_invariant_object_properties(
    plan: &QuickLongOpsLoop,
    resolved: &[QuickResolvedObjectOp],
    slots: &mut [i64; 64],
) -> Option<u64> {
    if !quick_object_property_reads_are_invariant(plan) {
        return Some(0);
    }

    let mut output_mask = 0u64;
    for (index, operation) in plan.ops.iter().copied().enumerate() {
        let (result, string_length) = match operation {
            QuickLongOp::ObjectPropertyLong { result, .. } => (result, false),
            QuickLongOp::ObjectPropertyStringLength { result, .. } => (result, true),
            _ => continue,
        };
        let QuickResolvedObjectOp::PropertyRead { property } = *resolved.get(index)? else {
            return None;
        };
        if property.is_null() || (*property).is_reference() {
            return None;
        }
        slots[result as usize] = if string_length {
            (*property).as_str()?.len() as i64
        } else {
            if (*property).value_type() != ValueType::Long {
                return None;
            }
            (*property).raw_long()
        };
        output_mask |= 1u64 << result;
    }
    Some(output_mask)
}

#[cfg(feature = "quick-loops")]
struct QuickObjectCallRecorder<'a> {
    resolved: &'a [QuickResolvedObjectOp],
    counts: Vec<u64>,
}

#[cfg(feature = "quick-loops")]
impl QuickObjectCallRecorder<'_> {
    #[inline(always)]
    fn record(&mut self, op_index: usize) {
        self.counts[op_index] += 1;
    }

    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn flush(&mut self) {
        for (resolved, count) in self.resolved.iter().zip(self.counts.iter_mut()) {
            if *count == 0 {
                continue;
            }
            unsafe {
                match *resolved {
                    QuickResolvedObjectOp::None | QuickResolvedObjectOp::PropertyRead { .. } => {}
                    QuickResolvedObjectOp::PropertyMethod { target, .. }
                    | QuickResolvedObjectOp::PropertyGetter { target, .. }
                    | QuickResolvedObjectOp::ScalarMethod { target, .. }
                    | QuickResolvedObjectOp::ObjectLongMethod { target, .. }
                    | QuickResolvedObjectOp::ComposedTypedMethod { target, .. } => {
                        record_scalar_calls_bulk(&*target, *count);
                    }
                    QuickResolvedObjectOp::ComposedProperty {
                        outer_target,
                        inner_target,
                        ..
                    } => {
                        record_scalar_calls_bulk(&*inner_target, *count);
                        record_scalar_calls_bulk(&*outer_target, *count);
                    }
                    QuickResolvedObjectOp::VirtualPipeline { pipeline } => {
                        record_scalar_calls_bulk(&*pipeline.constructor_target, *count);
                        record_scalar_calls_bulk(&*pipeline.method_target, *count);
                    }
                }
            }
            *count = 0;
        }
    }
}

#[cfg(feature = "quick-loops")]
impl Drop for QuickObjectCallRecorder<'_> {
    fn drop(&mut self) {
        self.flush();
    }
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn quick_object_method_target(
    op_array: &crate::compiler::OpArray,
    slot_base: *mut Value,
    guard: ScalarLongCallGuard,
    argument_count: usize,
) -> Option<(*const Value, *const FunctionCommon, *const UserFunction)> {
    let ScalarLongCallGuard::MethodCache { receiver_slot, .. } = guard else {
        return None;
    };
    let receiver = slot_base.add(receiver_slot as usize);
    // Dispatch identity (the warmed function/method cache, receiver class and
    // arity) is shared by every frame-free user plan.  Do not require the
    // scalar-only ABI here: callers below validate the concrete property,
    // scalar or mixed object/Long/String plan before executing it.
    let (target, user) = guarded_cached_user_call_target(
        op_array,
        guard,
        Some(&*receiver),
        argument_count,
    )?;
    Some((receiver, target, user))
}

#[cfg(feature = "quick-loops")]
unsafe fn quick_property_getter_slot(
    receiver: *const Value,
    user: *const UserFunction,
) -> Option<usize> {
    let plan = (&*user).property_getter_plan.as_ref()?;
    let class_id = (*receiver).object_class_id_unchecked();
    let cache = (&*user).op_array.cache.get(plan.cache_ip as usize)?;
    if class_id == 0 || cache.class_id != class_id || cache.property_flags() & 1 == 0 {
        return None;
    }
    let slot = cache.property_slot();
    let value = &*(*receiver).object_property_slot_unchecked(slot);
    if value.value_type() != ValueType::Long || value.is_reference() {
        return None;
    }
    Some(slot)
}

#[cfg(feature = "quick-loops")]
unsafe fn quick_object_property_read(
    op_array: &crate::compiler::OpArray,
    receiver: *const Value,
    cache_ip: u32,
    expected_type: ValueType,
) -> Option<*const Value> {
    let cache_ip = usize::try_from(cache_ip).ok()?;
    let instruction = op_array.instructions.get(cache_ip)?;
    let cache = op_array.cache.get(cache_ip)?;
    let receiver_value = &*receiver;
    if receiver_value.value_type() != ValueType::Object || receiver_value.is_reference() {
        return None;
    }

    let property = if cache.is_dynamic_property_read() {
        if receiver_value.object_property_layout_ptr_unchecked()
            != cache.dynamic_property_layout()
        {
            return None;
        }
        let name = op_array
            .literals
            .get(instruction.op2 as usize)?
            .as_str()?;
        let mut property = cache.dynamic_property_position().map_or(
            std::ptr::null(),
            |position| receiver_value.object_dynamic_property_at_unchecked(name, position),
        );
        if property.is_null() {
            property = receiver_value.object_dynamic_property_unchecked(name);
        }
        property
    } else {
        let class_id = receiver_value.object_class_id_unchecked();
        if class_id == 0 || cache.class_id != class_id || cache.property_flags() & 1 == 0 {
            return None;
        }
        receiver_value.object_property_slot_unchecked(cache.property_slot())
    };
    if property.is_null()
        || (*property).value_type() != expected_type
        || (*property).is_reference()
    {
        return None;
    }
    Some(property)
}

#[cfg(feature = "quick-loops")]
fn quick_object_long_arguments_match(
    user: &UserFunction,
    plan: &ObjectLongFunctionPlan,
    arguments: &[QuickObjectLongArgument; 8],
    argument_count: u8,
) -> bool {
    if plan.public_args != argument_count || plan.object_argument_mask != 0 {
        return false;
    }
    for (index, source) in arguments
        .iter()
        .copied()
        .take(argument_count as usize)
        .enumerate()
    {
        let bit = 1u8 << index;
        let is_long = matches!(source, QuickObjectLongArgument::Long(_));
        let is_string = matches!(source, QuickObjectLongArgument::StringSlot(_));
        if (plan.long_argument_mask & bit != 0 && !is_long)
            || (plan.string_argument_mask & bit != 0 && !is_string)
        {
            return false;
        }
        let hint = user
            .common
            .sig
            .param_type_hints
            .get(index)
            .unwrap_or(&ParamTypeHint::None);
        if !matches!(hint, ParamTypeHint::None | ParamTypeHint::Mixed)
            && !matches!((hint, source),
                (ParamTypeHint::Int, QuickObjectLongArgument::Long(_))
                    | (ParamTypeHint::String, QuickObjectLongArgument::StringSlot(_)))
        {
            return false;
        }
    }
    true
}

#[cfg(feature = "quick-loops")]
fn quick_composed_typed_arguments_match(
    user: &UserFunction,
    plan: &ComposedTypedLongFunctionPlan,
    arguments: &[QuickObjectLongArgument; 8],
    argument_count: u8,
) -> bool {
    if plan.public_args != argument_count
        || plan.object_argument_mask != 0
        || plan.program.operations.iter().any(|operation| {
            matches!(
                operation,
                ComposedTypedLongOp::Call(_) | ComposedTypedLongOp::StringCall(_)
            )
        })
    {
        return false;
    }
    for (index, source) in arguments
        .iter()
        .copied()
        .take(argument_count as usize)
        .enumerate()
    {
        let bit = 1u8 << index;
        let matches_plan = match source {
            QuickObjectLongArgument::Long(_) => plan.long_argument_mask & bit != 0,
            QuickObjectLongArgument::StringSlot(_) => plan.string_argument_mask & bit != 0,
        };
        if !matches_plan {
            return false;
        }
        let hint = user
            .common
            .sig
            .param_type_hints
            .get(index)
            .unwrap_or(&ParamTypeHint::None);
        if !matches!(
            (hint, source),
            (ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int,
                QuickObjectLongArgument::Long(_))
                | (ParamTypeHint::String, QuickObjectLongArgument::StringSlot(_))
        ) {
            return false;
        }
    }
    true
}

#[cfg(feature = "quick-loops")]
unsafe fn resolve_quick_object_ops(
    eg: &ExecutorGlobals,
    op_array: &crate::compiler::OpArray,
    slot_base: *mut Value,
    slots: &[i64; 64],
    string_state: &QuickStringSlotState,
    plan: &QuickLongOpsLoop,
) -> Option<Vec<QuickResolvedObjectOp>> {
    let mut resolved = vec![QuickResolvedObjectOp::None; plan.ops.len()];
    for (index, operation) in plan.ops.iter().copied().enumerate() {
        resolved[index] = match operation {
            QuickLongOp::ObjectPropertyLong {
                object,
                cache_ip,
                ..
            } => QuickResolvedObjectOp::PropertyRead {
                property: quick_object_property_read(
                    op_array,
                    slot_base.add(object as usize),
                    cache_ip,
                    ValueType::Long,
                )?,
            },
            QuickLongOp::ObjectPropertyStringLength {
                object,
                cache_ip,
                ..
            } => QuickResolvedObjectOp::PropertyRead {
                property: quick_object_property_read(
                    op_array,
                    slot_base.add(object as usize),
                    cache_ip,
                    ValueType::String,
                )?,
            },
            QuickLongOp::PropertyMethodCall { call } => {
                let (receiver, target, user) = quick_object_method_target(
                    op_array,
                    slot_base,
                    call.guard,
                    call.argument_count as usize,
                )?;
                let property_plan = (&*user).long_property_plan.as_deref()?;
                if property_plan.public_args != call.argument_count
                    || property_plan.properties.len() > 8
                {
                    return None;
                }
                let class_id = (*receiver).object_class_id_unchecked();
                let mut property_slots = [usize::MAX; 8];
                for (index, property) in property_plan.properties.iter().enumerate() {
                    let cache = (&*user).op_array.cache.get(property.cache_ip as usize)?;
                    if cache.class_id != class_id
                        || cache.property_flags() & property.required_flags as u32
                            != property.required_flags as u32
                    {
                        return None;
                    }
                    let slot = cache.property_slot();
                    let value = &*(*receiver).object_property_slot_unchecked(slot);
                    if value.value_type() != ValueType::Long {
                        return None;
                    }
                    property_slots[index] = slot;
                }
                QuickResolvedObjectOp::PropertyMethod {
                    receiver,
                    target,
                    plan: property_plan,
                    property_slots,
                    property_count: property_plan.properties.len() as u8,
                }
            }
            QuickLongOp::PropertyGetterCall { call, .. } => {
                let (receiver, target, user) = quick_object_method_target(
                    op_array,
                    slot_base,
                    call.guard,
                    call.argument_count as usize,
                )?;
                let property_slot = quick_property_getter_slot(receiver, user)?;
                QuickResolvedObjectOp::PropertyGetter {
                    receiver,
                    target,
                    property_slot,
                }
            }
            QuickLongOp::ScalarMethodCall { call, .. } => {
                let (receiver, target, user) = quick_object_method_target(
                    op_array,
                    slot_base,
                    call.guard,
                    call.argument_count as usize,
                )?;
                if let Some(scalar_plan) = (&*user).scalar_long_plan.as_deref()
                    && scalar_plan.public_args == call.argument_count
                {
                    QuickResolvedObjectOp::ScalarMethod {
                        target,
                        plan: scalar_plan,
                    }
                } else {
                    let object_plan = (&*user).object_long_plan.as_deref()?;
                    let arguments = call.arguments.map(QuickObjectLongArgument::Long);
                    if !quick_object_long_arguments_match(
                        &*user,
                        object_plan,
                        &arguments,
                        call.argument_count,
                    ) {
                        return None;
                    }
                    QuickResolvedObjectOp::ObjectLongMethod {
                        receiver,
                        target,
                        user,
                        plan: object_plan,
                    }
                }
            }
            QuickLongOp::ObjectLongMethodCall { call, .. } => {
                let (receiver, target, user) = quick_object_method_target(
                    op_array,
                    slot_base,
                    call.guard,
                    call.argument_count as usize,
                )?;
                if let Some(typed_plan) = (&*user).composed_typed_long_plan.as_deref()
                    && quick_composed_typed_arguments_match(
                        &*user,
                        typed_plan,
                        &call.arguments,
                        call.argument_count,
                    )
                {
                    QuickResolvedObjectOp::ComposedTypedMethod {
                        target,
                        plan: typed_plan,
                    }
                } else {
                    let object_plan = (&*user).object_long_plan.as_deref()?;
                    if !quick_object_long_arguments_match(
                        &*user,
                        object_plan,
                        &call.arguments,
                        call.argument_count,
                    ) {
                        return None;
                    }
                    QuickResolvedObjectOp::ObjectLongMethod {
                        receiver,
                        target,
                        user,
                        plan: object_plan,
                    }
                }
            }
            QuickLongOp::ComposedPropertyCall {
                outer_guard,
                inner_guard,
                ..
            } => {
                let (outer_receiver, outer_target, outer_user) =
                    quick_object_method_target(op_array, slot_base, outer_guard, 1)?;
                let outer_plan = (&*outer_user).long_property_plan.as_deref()?;
                if outer_plan.public_args != 1 {
                    return None;
                }
                let (inner_receiver, inner_target, inner_user) =
                    quick_object_method_target(op_array, slot_base, inner_guard, 0)?;
                let inner_property_slot =
                    quick_property_getter_slot(inner_receiver, inner_user)?;
                QuickResolvedObjectOp::ComposedProperty {
                    outer_receiver,
                    outer_target,
                    outer_user,
                    outer_plan,
                    inner_receiver,
                    inner_target,
                    inner_property_slot,
                }
            }
            QuickLongOp::VirtualObjectArrayPipeline {
                constructor_arguments,
                argument_count,
                consumers,
                consumer_count,
                trailing_key_literal,
                resume_ip,
                ..
            } => QuickResolvedObjectOp::VirtualPipeline {
                pipeline: resolve_quick_virtual_object_array_pipeline(
                    eg,
                    op_array,
                    slot_base,
                    slots,
                    string_state,
                    resume_ip,
                    &constructor_arguments,
                    argument_count,
                    &consumers,
                    consumer_count,
                    trailing_key_literal,
                )?,
            },
            _ => QuickResolvedObjectOp::None,
        };
    }
    Some(resolved)
}
