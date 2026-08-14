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
        /// Region-local binding indices populated only for the no-JIT typed
        /// executor after native admission has had its opportunity. `u8::MAX`
        /// keeps the canonical per-call property transaction selected.
        shadow_bindings: [u8; 8],
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
    VirtualDeclaredReads {
        values: [i64; 8],
    },
}

#[cfg(feature = "quick-loops")]
const QUICK_PROPERTY_SHADOW_CAPACITY: usize = 8;

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickPropertyShadowBinding {
    property: *mut Value,
    value: i64,
}

#[cfg(feature = "quick-loops")]
impl QuickPropertyShadowBinding {
    const EMPTY: Self = Self {
        property: std::ptr::null_mut(),
        value: 0,
    };
}

/// Fixed request-local shadow state for declared Long properties participating
/// in one closed typed region. Construction and publication stay in the
/// surrounding unsafe executor, while transaction evaluation itself only
/// touches these plain scalar values.
#[cfg(feature = "quick-loops")]
struct QuickPropertyShadowState {
    bindings: [QuickPropertyShadowBinding; QUICK_PROPERTY_SHADOW_CAPACITY],
    count: u8,
    dirty_mask: u8,
}

#[cfg(feature = "quick-loops")]
impl QuickPropertyShadowState {
    fn new() -> Self {
        Self {
            bindings: [QuickPropertyShadowBinding::EMPTY; QUICK_PROPERTY_SHADOW_CAPACITY],
            count: 0,
            dirty_mask: 0,
        }
    }

    fn clear(&mut self) {
        self.count = 0;
        self.dirty_mask = 0;
    }

    fn binding_index(&self, property: *mut Value) -> Option<u8> {
        self.bindings[..self.count as usize]
            .iter()
            .position(|binding| binding.property == property)
            .and_then(|index| u8::try_from(index).ok())
    }

    fn insert(&mut self, property: *mut Value, value: i64) -> Option<u8> {
        if let Some(index) = self.binding_index(property) {
            return Some(index);
        }
        let index = self.count as usize;
        if index == QUICK_PROPERTY_SHADOW_CAPACITY {
            return None;
        }
        self.bindings[index] = QuickPropertyShadowBinding {
            property,
            value,
        };
        self.count += 1;
        Some(index as u8)
    }

    #[inline(always)]
    fn apply(
        &mut self,
        arguments: &[i64; 8],
        plan: &LongPropertyMethodPlan,
        property_count: u8,
        shadow_bindings: &[u8; 8],
    ) -> bool {
        if property_count == 0 || property_count > self.count {
            return false;
        }
        let mut values = [0i64; 8];
        for index in 0..property_count as usize {
            let binding = shadow_bindings[index] as usize;
            let Some(shadow) = self.bindings.get(binding) else {
                return false;
            };
            values[index] = shadow.value;
        }
        let Some(written) = apply_resolved_long_property_plan_values(
            arguments,
            plan,
            &mut values,
            property_count,
        ) else {
            return false;
        };
        for index in 0..property_count as usize {
            if written & (1 << index) == 0 {
                continue;
            }
            let shadow = &mut self.bindings[shadow_bindings[index] as usize];
            shadow.value = values[index];
            self.dirty_mask |= 1 << shadow_bindings[index];
        }
        stats::record_quick_property_shadow_updates(written.count_ones());
        true
    }
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
                | QuickLongOp::VirtualDeclaredObjectReads { .. }
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
                    QuickResolvedObjectOp::None
                    | QuickResolvedObjectOp::PropertyRead { .. }
                    | QuickResolvedObjectOp::VirtualDeclaredReads { .. } => {}
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

#[cfg(all(
    feature = "quick-loops",
    not(any(feature = "php-generics-erased", feature = "php-generics-reified"))
))]
fn resolve_quick_virtual_declared_object_reads(
    eg: &ExecutorGlobals,
    op_array: &crate::compiler::OpArray,
    resume_ip: usize,
    class_literal: u16,
    reads: &[QuickVirtualDeclaredPropertyRead; 8],
    read_count: u8,
) -> Option<[i64; 8]> {
    let new_object = *op_array.instructions.get(resume_ip)?;
    if new_object.opcode != OpCode::NewObj
        || new_object._pad & NEW_FLAG_VIRTUAL_DECLARED_READS == 0
        || new_object.op1_type != OpType::Const
        || new_object.op1 != class_literal
        || new_object.extended_value != 0
        || read_count == 0
        || read_count > 8
    {
        return None;
    }
    let new_cache = op_array.cache.get(resume_ip)?;
    // A null function paired with a class ID is the warmed, stable negative
    // constructor cache. Any constructor body remains canonical.
    if new_cache.class_id == 0 || !new_cache.func.is_null() {
        return None;
    }
    let class_def = eg.class_by_id(new_cache.class_id)?;
    let class_name = op_array.literals.get(class_literal as usize)?.as_str()?;
    if !class_def.name.eq_ignore_ascii_case(class_name)
        || class_def.is_interface
        || class_def.is_abstract
        || class_def.is_enum
        || eg
            .find_method_info(&class_def.name, "__construct")
            .is_some()
        || eg.find_method_info(&class_def.name, "__destruct").is_some()
    {
        return None;
    }

    let mut values = [0i64; 8];
    for (index, read) in reads.iter().copied().enumerate().take(read_count as usize) {
        let fetch_ip = resume_ip + 3 + index;
        let fetch = *op_array.instructions.get(fetch_ip)?;
        if fetch.opcode != OpCode::FetchObjR
            || fetch.op2_type != OpType::Const
            || fetch.op2 != read.property_literal
            || fetch.result != read.result
        {
            return None;
        }
        let cache = op_array.cache.get(fetch_ip)?;
        if cache.class_id != new_cache.class_id || cache.property_flags() & 1 == 0 {
            return None;
        }
        let value = class_def.property_defaults.get(cache.property_slot())?;
        if value.value_type() != ValueType::Long || value.is_reference() {
            return None;
        }
        values[index] = value.as_long()?;
    }
    Some(values)
}

#[cfg(all(
    feature = "quick-loops",
    any(feature = "php-generics-erased", feature = "php-generics-reified")
))]
fn resolve_quick_virtual_declared_object_reads(
    _eg: &ExecutorGlobals,
    _op_array: &crate::compiler::OpArray,
    _resume_ip: usize,
    _class_literal: u16,
    _reads: &[QuickVirtualDeclaredPropertyRead; 8],
    _read_count: u8,
) -> Option<[i64; 8]> {
    None
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
    eg: &ExecutorGlobals,
    op_array: &crate::compiler::OpArray,
    slot_base: *mut Value,
    guard: ScalarLongCallGuard,
    argument_count: usize,
    boundary: TypedGenericCallBoundary,
) -> Option<(*const Value, *const FunctionCommon, *const UserFunction)> {
    let ScalarLongCallGuard::MethodCache { receiver_slot, .. } = guard else {
        return None;
    };
    let receiver = slot_base.add(receiver_slot as usize);
    // Dispatch identity (the warmed function/method cache, receiver class and
    // arity) is shared by every frame-free user plan.  Do not require the
    // scalar-only ABI here: callers below validate the concrete property,
    // scalar or mixed object/Long/String plan before executing it.
    let (target, user) = guarded_quick_typed_method_target(
        eg,
        op_array,
        guard,
        &*receiver,
        argument_count,
        boundary,
    )?;
    Some((receiver, target, user))
}

/// Resolve every declared Long slot once when a closed typed region is
/// admitted. Generic property caches deliberately omit the ordinary
/// write-safe bit so canonical stores repeat their substituted type check.
/// A current exact Long value proves that the immutable erased/reified
/// property contract admits every Long written by this plan; the native loop
/// can then reuse the resolved slot without consulting metadata per call.
#[cfg(feature = "quick-loops")]
unsafe fn quick_long_property_slots(
    eg: &ExecutorGlobals,
    receiver: *const Value,
    user: *const UserFunction,
    plan: &LongPropertyMethodPlan,
) -> Option<([usize; 8], u8)> {
    #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
    let _ = eg;
    if plan.properties.len() > 8 {
        return None;
    }
    let receiver = &*receiver;
    let user = &*user;
    let class_id = receiver.object_class_id_unchecked();
    if class_id == 0 {
        return None;
    }

    let mut property_slots = [usize::MAX; 8];
    for (index, property) in plan.properties.iter().enumerate() {
        let cache_ip = property.cache_ip as usize;
        let cache = user.op_array.cache.get(cache_ip)?;
        if cache.class_id != class_id {
            return None;
        }
        let slot = cache.property_slot();
        let value = &*receiver.object_property_slot_unchecked(slot);
        if value.value_type() != ValueType::Long || value.is_reference() {
            return None;
        }
        if cache.property_flags() & property.required_flags as u32
            != property.required_flags as u32
            && !(property.required_flags == 3
                && instance_property_cache_accepts_long_write(cache))
        {
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            {
                if property.required_flags != 3 {
                    return None;
                }
                let declaration = if cache.property_flags() == 2 {
                    cache
                        .typed_instance_property_definition()?
                        .generic_declaration?
                } else {
                    cache.generic_property_declaration()?
                };
                let instruction = user.op_array.instructions.get(cache_ip)?;
                let name = user
                    .op_array
                    .literals
                    .get(instruction.op2 as usize)?
                    .as_str()?;
                if eg
                    .check_cached_generic_property_value(receiver, name, value, declaration)
                    .is_err()
                {
                    return None;
                }
            }
            #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
            {
                return None;
            }
        }
        property_slots[index] = slot;
    }

    Some((property_slots, plan.properties.len() as u8))
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
            (ParamTypeHint::None | ParamTypeHint::Mixed,
                QuickObjectLongArgument::Long(_) | QuickObjectLongArgument::StringSlot(_))
                | (ParamTypeHint::Int, QuickObjectLongArgument::Long(_))
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
                    eg,
                    op_array,
                    slot_base,
                    call.guard,
                    call.argument_count as usize,
                    TypedGenericCallBoundary::LongDiscarded,
                )?;
                let property_plan = (&*user).long_property_plan.as_deref()?;
                if property_plan.public_args != call.argument_count {
                    return None;
                }
                let (property_slots, property_count) =
                    quick_long_property_slots(eg, receiver, user, property_plan)?;
                QuickResolvedObjectOp::PropertyMethod {
                    receiver,
                    target,
                    plan: property_plan,
                    property_slots,
                    property_count,
                    shadow_bindings: [u8::MAX; 8],
                }
            }
            QuickLongOp::PropertyGetterCall { call, .. } => {
                let (receiver, target, user) = quick_object_method_target(
                    eg,
                    op_array,
                    slot_base,
                    call.guard,
                    call.argument_count as usize,
                    TypedGenericCallBoundary::Long,
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
                    eg,
                    op_array,
                    slot_base,
                    call.guard,
                    call.argument_count as usize,
                    TypedGenericCallBoundary::Long,
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
                let string_arguments = call
                    .arguments
                    .iter()
                    .copied()
                    .take(call.argument_count as usize)
                    .enumerate()
                    .fold(0u8, |mask, (index, argument)| {
                        mask | (u8::from(matches!(argument, QuickObjectLongArgument::StringSlot(_)))
                            << index)
                    });
                let (receiver, target, user) = quick_object_method_target(
                    eg,
                    op_array,
                    slot_base,
                    call.guard,
                    call.argument_count as usize,
                    TypedGenericCallBoundary::LongStringToLong { string_arguments },
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
                    quick_object_method_target(
                        eg,
                        op_array,
                        slot_base,
                        outer_guard,
                        1,
                        TypedGenericCallBoundary::LongDiscarded,
                    )?;
                let outer_plan = (&*outer_user).long_property_plan.as_deref()?;
                if outer_plan.public_args != 1 {
                    return None;
                }
                let (inner_receiver, inner_target, inner_user) =
                    quick_object_method_target(
                        eg,
                        op_array,
                        slot_base,
                        inner_guard,
                        0,
                        TypedGenericCallBoundary::Long,
                    )?;
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
            QuickLongOp::VirtualDeclaredObjectReads {
                class_literal,
                reads,
                read_count,
                resume_ip,
                ..
            } => QuickResolvedObjectOp::VirtualDeclaredReads {
                values: resolve_quick_virtual_declared_object_reads(
                    eg,
                    op_array,
                    resume_ip,
                    class_literal,
                    &reads,
                    read_count,
                )?,
            },
            _ => QuickResolvedObjectOp::None,
        };
    }
    Some(resolved)
}
