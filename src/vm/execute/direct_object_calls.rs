// Kept in the execute module through include! so this structural split does not change visibility or code generation.
#[inline(always)]
fn resolve_object_long_source(
    source: ObjectLongSource,
    slots: &[std::mem::MaybeUninit<i64>; 64],
    initialized: u64,
) -> Option<i64> {
    match source {
        ObjectLongSource::Slot(slot) => {
            let bit = 1u64.checked_shl(slot as u32)?;
            if initialized & bit == 0 {
                return None;
            }
            Some(unsafe { slots.get(slot as usize)?.assume_init() })
        }
        ObjectLongSource::Constant(value) => Some(value),
    }
}

#[derive(Clone, Copy)]
enum VirtualPropertyValue {
    Empty,
    Long(i64),
    Borrowed(*const Value),
}

struct VirtualObject {
    class_id: u32,
    class_def: *const crate::compiler::compile::ClassDef,
    property_slots: [usize; 8],
    property_values: [VirtualPropertyValue; 8],
    property_count: u8,
}

impl VirtualObject {
    #[inline(always)]
    unsafe fn class_name(&self) -> Option<&str> {
        (!self.class_def.is_null()).then(|| (*self.class_def).name.as_str())
    }

    #[inline(always)]
    unsafe fn property(&self, slot: usize) -> Option<VirtualPropertyValue> {
        for index in 0..self.property_count as usize {
            if self.property_slots[index] == slot {
                return Some(self.property_values[index]);
            }
        }
        let class_def = self.class_def.as_ref()?;
        class_def
            .property_defaults
            .get(slot)
            .map(|value| VirtualPropertyValue::Borrowed(value as *const Value))
    }
}

#[derive(Clone, Copy)]
enum ObjectLongArgument {
    None,
    Borrowed(*const Value),
    Virtual(*const VirtualObject),
}

#[inline(always)]
unsafe fn virtual_object_matches_hint(
    object: &VirtualObject,
    hint: &ParamTypeHint,
    eg: &ExecutorGlobals,
    callee_class: Option<&str>,
) -> bool {
    match hint {
        ParamTypeHint::None | ParamTypeHint::Mixed => true,
        ParamTypeHint::ClassName(class_name) => {
            let resolved = match class_name.as_str() {
                "self" | "static" => callee_class.unwrap_or(class_name),
                "parent" => callee_class
                    .and_then(|declaring| eg.class_table.get(declaring))
                    .and_then(|class| class.parent.as_deref())
                    .unwrap_or(class_name),
                _ => class_name,
            };
            object
                .class_name()
                .is_some_and(|actual| eg.class_is_a(actual, resolved))
        }
        _ => false,
    }
}

/// Execute a compiler-proven read-only object/Long method body against the
/// callee's warmed property caches. Every failure is side-effect free, so the
/// caller can allocate the ordinary frame and replay canonical PHP bytecode.
#[inline(never)]
unsafe fn evaluate_object_long_plan(
    receiver: &Value,
    object_arguments: &[ObjectLongArgument; 8],
    string_arguments: &[*const Value; 8],
    slots: &mut [std::mem::MaybeUninit<i64>; 64],
    mut initialized: u64,
    callee: &UserFunction,
    plan: &ObjectLongFunctionPlan,
) -> Option<i64> {
    if plan.slot_count as usize > slots.len()
        || plan.operations.len() > 64
        || plan.operations.is_empty()
    {
        return None;
    }

    if let Some(select) = plan.string_intdiv_select.as_deref() {
        let pointer = *string_arguments.get(select.string_argument as usize)?;
        if pointer.is_null() {
            return None;
        }
        let value = (&*pointer).as_str()?;
        let mut arm = select.default_arm;
        for case in select.cases.iter().copied() {
            let literal = callee
                .op_array
                .literals
                .get(case.literal as usize)?
                .as_str()?;
            if value == literal {
                arm = case.arm;
                break;
            }
        }
        let input = resolve_object_long_source(select.input, slots, initialized)?;
        return input.checked_mul(arm.multiplier)?.checked_div(arm.divisor);
    }

    if let Some(select) = plan.modulo_any_select.as_deref() {
        for term in select.terms.iter().copied() {
            let input = resolve_object_long_source(term.input, slots, initialized)?;
            if input.checked_rem(term.divisor)? == term.expected {
                return Some(select.when_match);
            }
        }
        return Some(select.when_miss);
    }

    if let Some(score) = plan.weighted_string_score.as_deref() {
        let weighted = resolve_object_long_source(score.weighted_input, slots, initialized)?;
        let additive = resolve_object_long_source(score.additive_input, slots, initialized)?;
        let pointer = *string_arguments.get(score.string_argument as usize)?;
        if pointer.is_null() {
            return None;
        }
        let string = (&*pointer).as_str()?;
        let mut value = weighted
            .checked_mul(score.multiplier)?
            .checked_add(additive)?
            .checked_div(score.divisor)?
            .checked_add(i64::try_from(string.len()).ok()?)?;
        for adjustment in score.string_adjustments.iter().copied() {
            let literal = callee
                .op_array
                .literals
                .get(adjustment.literal as usize)?
                .as_str()?;
            if string == literal {
                value = value.checked_add(adjustment.addend)?;
                break;
            }
        }
        for adjustment in score.conditional_adjustments.iter().copied() {
            let lhs = resolve_object_long_source(adjustment.lhs, slots, initialized)?;
            let rhs = resolve_object_long_source(adjustment.rhs, slots, initialized)?;
            if apply_scalar_long_condition(adjustment.kind, lhs, rhs) {
                value = value.checked_add(adjustment.addend)?;
            }
        }
        return Some(value);
    }

    let operations = &plan.operations;
    let mut ip = 0usize;
    while ip < operations.len() {
        match operations[ip] {
            ObjectLongOp::Noop => {}
            ObjectLongOp::Assign {
                destination,
                source,
            } => {
                slots[destination as usize].write(resolve_object_long_source(
                    source,
                    slots,
                    initialized,
                )?);
                initialized |= 1u64 << destination;
            }
            ObjectLongOp::FetchProperty {
                object,
                cache_ip,
                destination,
            } => {
                let cache = callee.op_array.cache.get(cache_ip as usize)?;
                let property = match object {
                    ObjectLongObjectSource::Receiver => {
                        if receiver.value_type() != ValueType::Object || receiver.is_reference() {
                            return None;
                        }
                        let class_id = receiver.object_class_id_unchecked();
                        if class_id == 0
                            || cache.class_id != class_id
                            || cache.property_flags() & 1 == 0
                        {
                            return None;
                        }
                        VirtualPropertyValue::Borrowed(
                            receiver.object_property_slot_unchecked(cache.property_slot()),
                        )
                    }
                    ObjectLongObjectSource::Argument(argument) => {
                        match *object_arguments.get(argument as usize)? {
                            ObjectLongArgument::Borrowed(pointer) => {
                                if pointer.is_null()
                                    || (*pointer).value_type() != ValueType::Object
                                    || (*pointer).is_reference()
                                {
                                    return None;
                                }
                                let class_id = (*pointer).object_class_id_unchecked();
                                if class_id == 0
                                    || cache.class_id != class_id
                                    || cache.property_flags() & 1 == 0
                                {
                                    return None;
                                }
                                VirtualPropertyValue::Borrowed(
                                    (*pointer)
                                        .object_property_slot_unchecked(cache.property_slot()),
                                )
                            }
                            ObjectLongArgument::Virtual(pointer) => {
                                if pointer.is_null()
                                    || cache.class_id != (*pointer).class_id
                                    || cache.property_flags() & 1 == 0
                                {
                                    return None;
                                }
                                (*pointer).property(cache.property_slot())?
                            }
                            ObjectLongArgument::None => return None,
                        }
                    }
                };
                let value = match property {
                    VirtualPropertyValue::Long(value) => value,
                    VirtualPropertyValue::Borrowed(pointer) => {
                        if pointer.is_null()
                            || (*pointer).value_type() != ValueType::Long
                            || (*pointer).is_reference()
                        {
                            return None;
                        }
                        (*pointer).raw_long()
                    }
                    VirtualPropertyValue::Empty => return None,
                };
                slots[destination as usize].write(value);
                initialized |= 1u64 << destination;
            }
            ObjectLongOp::Arithmetic {
                kind,
                lhs,
                rhs,
                destination,
            } => {
                let lhs = resolve_object_long_source(lhs, slots, initialized)?;
                let rhs = resolve_object_long_source(rhs, slots, initialized)?;
                slots[destination as usize].write(apply_scalar_long_op(kind, lhs, rhs)?);
                initialized |= 1u64 << destination;
            }
            ObjectLongOp::Compare {
                kind,
                lhs,
                rhs,
                destination,
            } => {
                let lhs = resolve_object_long_source(lhs, slots, initialized)?;
                let rhs = resolve_object_long_source(rhs, slots, initialized)?;
                let value = apply_scalar_long_condition(kind, lhs, rhs);
                slots[destination as usize].write(value as i64);
                initialized |= 1u64 << destination;
            }
            ObjectLongOp::StringLiteralBranch {
                argument,
                literal,
                jump_when_equal,
                target,
            } => {
                let pointer = *string_arguments.get(argument as usize)?;
                if pointer.is_null() {
                    return None;
                }
                let argument = (&*pointer).as_str()?;
                let literal = callee.op_array.literals.get(literal as usize)?.as_str()?;
                if (argument == literal) == jump_when_equal {
                    ip = target as usize;
                    continue;
                }
                if matches!(operations.get(ip + 1), Some(ObjectLongOp::Noop)) {
                    ip += 2;
                    continue;
                }
            }
            ObjectLongOp::StringLength {
                argument,
                destination,
            } => {
                let pointer = *string_arguments.get(argument as usize)?;
                if pointer.is_null() {
                    return None;
                }
                let length = i64::try_from((&*pointer).as_str()?.len()).ok()?;
                slots[destination as usize].write(length);
                initialized |= 1u64 << destination;
            }
            ObjectLongOp::IntDiv {
                lhs,
                rhs,
                destination,
            } => {
                let lhs = resolve_object_long_source(lhs, slots, initialized)?;
                let rhs = resolve_object_long_source(rhs, slots, initialized)?;
                slots[destination as usize].write(lhs.checked_div(rhs)?);
                initialized |= 1u64 << destination;
            }
            ObjectLongOp::JumpIfFalse { condition, target } => {
                if resolve_object_long_source(condition, slots, initialized)? == 0 {
                    ip = target as usize;
                    continue;
                }
            }
            ObjectLongOp::JumpIfTrue { condition, target } => {
                if resolve_object_long_source(condition, slots, initialized)? != 0 {
                    ip = target as usize;
                    continue;
                }
            }
            ObjectLongOp::Jump { target } => {
                ip = target as usize;
                continue;
            }
            ObjectLongOp::Return { value } => {
                return resolve_object_long_source(value, slots, initialized);
            }
            ObjectLongOp::Bail => return None,
        }
        ip += 1;
    }
    None
}

/// Borrow a positional Send sequence and execute a guarded method that reads
/// object properties and returns a Long. A warmed, declared `FetchObjR`
/// immediately feeding a Send is also a safe borrowed argument producer.
/// Argument declarations are validated before the body plan, including unused
/// typed parameters.
#[inline(never)]
pub(crate) unsafe fn try_execute_direct_object_long_call(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    receiver: &Value,
    sends: *const Instruction,
    callee: &UserFunction,
    plan: &ObjectLongFunctionPlan,
) -> Option<(i64, *const Instruction)> {
    let common = &callee.common;
    if !common.plan.call.is_compact_user_call()
        || common.plan.ret != ReturnStrategy::Fast
        || common.sig.public_arity() != plan.public_args as u32
        || common.sig.ref_args != 0
        || common.sig.is_variadic
    {
        return None;
    }

    let declaring_class = eg.declaring_class_of(&callee.common as *const FunctionCommon);
    let mut slots = [const { std::mem::MaybeUninit::<i64>::uninit() }; 64];
    let mut initialized = 0u64;
    let mut object_arguments = [ObjectLongArgument::None; 8];
    let mut string_arguments = [std::ptr::null(); 8];
    let instruction_base = caller_op_array.instructions.as_ptr();
    let mut cursor = sends;
    for index in 0..plan.public_args as usize {
        let instruction = &*cursor;
        let (send, value) = if matches!(instruction.opcode, OpCode::SendVal | OpCode::SendVarEx) {
            let value = match instruction.op1_type {
                OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => &*(*caller).get_op_ptr(
                    instruction.op1 as u32,
                    instruction.op1_type,
                    caller_op_array,
                ),
                OpType::Unused => return None,
            };
            cursor = cursor.add(1);
            (instruction, value)
        } else if instruction.opcode == OpCode::FetchObjR {
            let send = &*cursor.add(1);
            if instruction.op2_type != OpType::Const
                || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                || !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                || send.op1_type != instruction.result_type
                || send.op1 != instruction.result
            {
                return None;
            }
            let object = match instruction.op1_type {
                OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => &*(*caller).get_op_ptr(
                    instruction.op1 as u32,
                    instruction.op1_type,
                    caller_op_array,
                ),
                OpType::Unused => return None,
            };
            if object.value_type() != ValueType::Object || object.is_reference() {
                return None;
            }
            let class_id = object.object_class_id_unchecked();
            let fetch_ip = cursor.offset_from(instruction_base);
            if class_id == 0 || fetch_ip < 0 {
                return None;
            }
            let cache = caller_op_array.cache.get(fetch_ip as usize)?;
            if cache.class_id != class_id || cache.property_flags() & 1 == 0 {
                return None;
            }
            let value = &*object.object_property_slot_unchecked(cache.property_slot());
            cursor = cursor.add(2);
            (send, value)
        } else {
            return None;
        };
        if send.op2 as u32 != common.sig.param_cv_index(index as u32) {
            return None;
        }
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

    let do_fcall_ptr = cursor;
    let do_fcall = &*do_fcall_ptr;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(
            do_fcall.result_type,
            OpType::Tmp | OpType::Var | OpType::Unused
        )
    {
        return None;
    }
    let result = evaluate_object_long_plan(
        receiver,
        &object_arguments,
        &string_arguments,
        &mut slots,
        initialized,
        callee,
        plan,
    )?;
    Some((result, do_fcall_ptr))
}

#[derive(Clone, Copy)]
enum ObjectArrayResolved {
    Long(i64),
    Borrowed(*const Value),
    Virtual(*const VirtualObject),
}

#[inline(always)]
unsafe fn object_array_property(
    owner: &UserFunction,
    receiver: &Value,
    arguments: &[ObjectLongArgument; 8],
    object: ObjectLongObjectSource,
    cache_ip: u16,
) -> Option<ObjectArrayResolved> {
    let cache = owner.op_array.cache.get(cache_ip as usize)?;
    let property = match object {
        ObjectLongObjectSource::Receiver => {
            if receiver.value_type() != ValueType::Object || receiver.is_reference() {
                return None;
            }
            let class_id = receiver.object_class_id_unchecked();
            if class_id == 0 || cache.class_id != class_id || cache.property_flags() & 1 == 0 {
                return None;
            }
            VirtualPropertyValue::Borrowed(
                receiver.object_property_slot_unchecked(cache.property_slot()),
            )
        }
        ObjectLongObjectSource::Argument(argument) => match *arguments.get(argument as usize)? {
            ObjectLongArgument::Borrowed(pointer) => {
                if pointer.is_null()
                    || (*pointer).value_type() != ValueType::Object
                    || (*pointer).is_reference()
                {
                    return None;
                }
                let class_id = (*pointer).object_class_id_unchecked();
                if class_id == 0 || cache.class_id != class_id || cache.property_flags() & 1 == 0 {
                    return None;
                }
                VirtualPropertyValue::Borrowed(
                    (*pointer).object_property_slot_unchecked(cache.property_slot()),
                )
            }
            ObjectLongArgument::Virtual(pointer) => {
                if pointer.is_null()
                    || cache.class_id != (*pointer).class_id
                    || cache.property_flags() & 1 == 0
                {
                    return None;
                }
                (*pointer).property(cache.property_slot())?
            }
            ObjectLongArgument::None => return None,
        },
    };
    match property {
        VirtualPropertyValue::Long(value) => Some(ObjectArrayResolved::Long(value)),
        VirtualPropertyValue::Borrowed(pointer) => {
            if pointer.is_null() || (*pointer).is_reference() {
                return None;
            }
            Some(ObjectArrayResolved::Borrowed(pointer))
        }
        VirtualPropertyValue::Empty => None,
    }
}

#[inline(always)]
unsafe fn resolve_object_array_source(
    source: ObjectArraySource,
    owner: &UserFunction,
    receiver: &Value,
    arguments: &[ObjectLongArgument; 8],
    slots: &[std::mem::MaybeUninit<i64>; 64],
    initialized: u64,
) -> Option<ObjectArrayResolved> {
    match source {
        ObjectArraySource::Receiver => {
            Some(ObjectArrayResolved::Borrowed(receiver as *const Value))
        }
        ObjectArraySource::Argument(argument) => match *arguments.get(argument as usize)? {
            ObjectLongArgument::Borrowed(pointer) if !pointer.is_null() => {
                Some(ObjectArrayResolved::Borrowed(pointer))
            }
            ObjectLongArgument::Virtual(pointer) if !pointer.is_null() => {
                Some(ObjectArrayResolved::Virtual(pointer))
            }
            _ => None,
        },
        ObjectArraySource::LongSlot(slot) => {
            let bit = 1u64.checked_shl(slot as u32)?;
            if initialized & bit == 0 {
                return None;
            }
            Some(ObjectArrayResolved::Long(
                slots.get(slot as usize)?.assume_init(),
            ))
        }
        ObjectArraySource::Literal(literal) => owner
            .op_array
            .literals
            .get(literal as usize)
            .map(|value| ObjectArrayResolved::Borrowed(value as *const Value)),
        ObjectArraySource::Property { object, cache_ip } => {
            object_array_property(owner, receiver, arguments, object, cache_ip)
        }
    }
}

#[inline(always)]
unsafe fn resolve_object_array_long(
    source: ObjectArraySource,
    owner: &UserFunction,
    receiver: &Value,
    arguments: &[ObjectLongArgument; 8],
    slots: &[std::mem::MaybeUninit<i64>; 64],
    initialized: u64,
) -> Option<i64> {
    match resolve_object_array_source(source, owner, receiver, arguments, slots, initialized)? {
        ObjectArrayResolved::Long(value) => Some(value),
        ObjectArrayResolved::Borrowed(pointer) => {
            if pointer.is_null()
                || (*pointer).is_reference()
                || (*pointer).value_type() != ValueType::Long
            {
                return None;
            }
            Some((*pointer).raw_long())
        }
        ObjectArrayResolved::Virtual(_) => None,
    }
}

#[derive(Clone, Copy)]
struct ResolvedObjectArrayCall {
    operation: *const ObjectArrayLongCall,
    receiver: *const Value,
    target: *const FunctionCommon,
    callee: *const UserFunction,
    plan: *const ObjectLongFunctionPlan,
    declaring_class: Option<*const str>,
}

#[cfg(feature = "quick-loops")]
impl ResolvedObjectArrayCall {
    const EMPTY: Self = Self {
        operation: std::ptr::null(),
        receiver: std::ptr::null(),
        target: std::ptr::null(),
        callee: std::ptr::null(),
        plan: std::ptr::null(),
        declaring_class: None,
    };
}

/// Resolve the invariant dispatch contract for one nested object-array call.
/// The canonical path invokes this for every call; quick virtual regions can
/// retain the result while their read-only receiver and inline caches remain
/// guarded at the region boundary.
#[inline(always)]
unsafe fn resolve_object_array_call(
    eg: &ExecutorGlobals,
    owner: &UserFunction,
    call_receiver: *const Value,
    call: &ObjectArrayLongCall,
) -> Option<ResolvedObjectArrayCall> {
    if call_receiver.is_null()
        || (*call_receiver).is_reference()
        || (*call_receiver).value_type() != ValueType::Object
    {
        return None;
    }

    let class_id = (*call_receiver).object_class_id_unchecked();
    let cache = owner.op_array.cache.get(call.cache_ip as usize)?;
    let initializer = owner.op_array.instructions.get(call.cache_ip as usize)?;
    if class_id == 0
        || cache.class_id != class_id
        || cache.func.is_null()
        || !method_return_dispatch_contract_matches(initializer, &*cache.func)
    {
        return None;
    }
    let common = &*cache.func;
    if common.fn_type != FunctionType::User
        || common.sig.public_arity() != call.arguments.len() as u32
        || common.sig.required_num_args != call.arguments.len() as u32
        || common.sig.ref_args != 0
        || common.sig.is_variadic
        || !common.plan.call.is_compact_user_call()
        || common.plan.ret != ReturnStrategy::Fast
    {
        return None;
    }
    let callee = &*(cache.func as *const UserFunction);
    let plan = callee.object_long_plan.as_deref()?;
    if plan.public_args as usize != call.arguments.len() {
        return None;
    }

    Some(ResolvedObjectArrayCall {
        operation: call,
        receiver: call_receiver,
        target: cache.func,
        callee,
        plan,
        declaring_class: eg
            .declaring_class_of(cache.func)
            .map(|class| class as *const str),
    })
}

#[inline(always)]
unsafe fn execute_resolved_object_array_call(
    eg: &ExecutorGlobals,
    owner: &UserFunction,
    receiver: &Value,
    outer_arguments: &[ObjectLongArgument; 8],
    slots: &[std::mem::MaybeUninit<i64>; 64],
    initialized: u64,
    call: &ObjectArrayLongCall,
    resolved_call: ResolvedObjectArrayCall,
) -> Option<(i64, *const FunctionCommon)> {
    if resolved_call.operation != call as *const ObjectArrayLongCall
        || resolved_call.receiver.is_null()
        || resolved_call.target.is_null()
        || resolved_call.callee.is_null()
        || resolved_call.plan.is_null()
    {
        return None;
    }
    let common = &*resolved_call.target;
    let callee = &*resolved_call.callee;
    let plan = &*resolved_call.plan;
    let declaring_class = resolved_call.declaring_class.map(|class| &*class);

    let mut callee_slots = [const { std::mem::MaybeUninit::<i64>::uninit() }; 64];
    let mut callee_initialized = 0u64;
    let mut object_arguments = [ObjectLongArgument::None; 8];
    let mut string_arguments = [std::ptr::null(); 8];
    for (index, source) in call.arguments.iter().copied().enumerate() {
        let resolved = resolve_object_array_source(
            source,
            owner,
            receiver,
            outer_arguments,
            slots,
            initialized,
        )?;
        let hint = common
            .sig
            .param_type_hints
            .get(index)
            .unwrap_or(&ParamTypeHint::None);
        let bit = 1u8 << index;
        match resolved {
            ObjectArrayResolved::Long(value) => {
                if !matches!(
                    hint,
                    ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
                ) || plan.object_argument_mask & bit != 0
                    || plan.string_argument_mask & bit != 0
                {
                    return None;
                }
                if plan.long_argument_mask & bit != 0 {
                    let slot = common.sig.param_cv_index(index as u32) as usize;
                    callee_slots[slot].write(value);
                    callee_initialized |= 1u64 << slot;
                }
            }
            ObjectArrayResolved::Borrowed(pointer) => {
                if pointer.is_null()
                    || (*pointer).is_reference()
                    || !check_type_hint(
                        &*pointer,
                        hint,
                        eg,
                        owner.op_array.strict_types,
                        declaring_class,
                    )
                {
                    return None;
                }
                if plan.long_argument_mask & bit != 0 {
                    if (*pointer).value_type() != ValueType::Long {
                        return None;
                    }
                    let slot = common.sig.param_cv_index(index as u32) as usize;
                    callee_slots[slot].write((*pointer).raw_long());
                    callee_initialized |= 1u64 << slot;
                }
                if plan.object_argument_mask & bit != 0 {
                    if (*pointer).value_type() != ValueType::Object {
                        return None;
                    }
                    object_arguments[index] = ObjectLongArgument::Borrowed(pointer);
                }
                if plan.string_argument_mask & bit != 0 {
                    if (*pointer).value_type() != ValueType::String {
                        return None;
                    }
                    string_arguments[index] = pointer;
                }
            }
            ObjectArrayResolved::Virtual(pointer) => {
                if pointer.is_null()
                    || !virtual_object_matches_hint(&*pointer, hint, eg, declaring_class)
                    || plan.long_argument_mask & bit != 0
                    || plan.string_argument_mask & bit != 0
                {
                    return None;
                }
                if plan.object_argument_mask & bit != 0 {
                    object_arguments[index] = ObjectLongArgument::Virtual(pointer);
                }
            }
        }
    }

    let result = evaluate_object_long_plan(
        &*resolved_call.receiver,
        &object_arguments,
        &string_arguments,
        &mut callee_slots,
        callee_initialized,
        callee,
        plan,
    )?;
    Some((result, resolved_call.target))
}

#[inline(always)]
unsafe fn evaluate_object_array_call(
    eg: &ExecutorGlobals,
    owner: &UserFunction,
    receiver: &Value,
    outer_arguments: &[ObjectLongArgument; 8],
    slots: &[std::mem::MaybeUninit<i64>; 64],
    initialized: u64,
    call: &ObjectArrayLongCall,
) -> Option<(i64, *const FunctionCommon)> {
    let call_receiver = match resolve_object_array_source(
        call.receiver,
        owner,
        receiver,
        outer_arguments,
        slots,
        initialized,
    )? {
        ObjectArrayResolved::Borrowed(pointer) => pointer,
        ObjectArrayResolved::Long(_) | ObjectArrayResolved::Virtual(_) => return None,
    };
    let resolved_call = resolve_object_array_call(eg, owner, call_receiver, call)?;
    execute_resolved_object_array_call(
        eg,
        owner,
        receiver,
        outer_arguments,
        slots,
        initialized,
        call,
        resolved_call,
    )
}

struct ObjectArrayEvaluated {
    values: [i64; 4],
    value_count: u8,
    called: [*const FunctionCommon; 8],
    called_count: u8,
}

impl ObjectArrayEvaluated {
    #[inline(always)]
    unsafe fn record_calls(&self) {
        for target in self.called.iter().copied().take(self.called_count as usize) {
            record_scalar_call(&*target);
        }
    }
}

/// Evaluate a guarded read-only application region into raw scalar outputs.
/// No result array is allocated here, allowing a proven caller consumer span
/// to keep the values unmaterialized.
#[inline(never)]
unsafe fn evaluate_object_array_values(
    eg: &ExecutorGlobals,
    receiver: &Value,
    arguments: &[ObjectLongArgument; 8],
    owner: &UserFunction,
    plan: &ObjectArrayFunctionPlan,
    resolved_calls: &[ResolvedObjectArrayCall],
) -> Option<ObjectArrayEvaluated> {
    if plan.slot_count as usize > 64
        || plan.operations.len() > 64
        || plan.entries.is_empty()
        || plan.entries.len() > 4
    {
        return None;
    }
    let mut slots = [const { std::mem::MaybeUninit::<i64>::uninit() }; 64];
    let mut initialized = 0u64;
    let mut called = [std::ptr::null(); 8];
    let mut called_count = 0usize;

    for operation in plan.operations.iter() {
        let (destination, value) = match operation {
            ObjectArrayLongOp::Assign {
                destination,
                source,
            } => (
                *destination,
                resolve_object_array_long(
                    *source,
                    owner,
                    receiver,
                    arguments,
                    &slots,
                    initialized,
                )?,
            ),
            ObjectArrayLongOp::Arithmetic {
                kind,
                lhs,
                rhs,
                destination,
            } => {
                let lhs = resolve_object_array_long(
                    *lhs,
                    owner,
                    receiver,
                    arguments,
                    &slots,
                    initialized,
                )?;
                let rhs = resolve_object_array_long(
                    *rhs,
                    owner,
                    receiver,
                    arguments,
                    &slots,
                    initialized,
                )?;
                (*destination, apply_scalar_long_op(*kind, lhs, rhs)?)
            }
            ObjectArrayLongOp::IntDiv {
                lhs,
                rhs,
                destination,
            } => {
                let lhs = resolve_object_array_long(
                    *lhs,
                    owner,
                    receiver,
                    arguments,
                    &slots,
                    initialized,
                )?;
                let rhs = resolve_object_array_long(
                    *rhs,
                    owner,
                    receiver,
                    arguments,
                    &slots,
                    initialized,
                )?;
                (*destination, lhs.checked_div(rhs)?)
            }
            ObjectArrayLongOp::Call(call) => {
                let resolved_call = resolved_calls
                    .get(called_count)
                    .copied()
                    .filter(|resolved| resolved.operation == call as *const ObjectArrayLongCall);
                let (value, target) = if let Some(resolved_call) = resolved_call {
                    execute_resolved_object_array_call(
                        eg,
                        owner,
                        receiver,
                        arguments,
                        &slots,
                        initialized,
                        call,
                        resolved_call,
                    )?
                } else {
                    evaluate_object_array_call(
                        eg,
                        owner,
                        receiver,
                        arguments,
                        &slots,
                        initialized,
                        call,
                    )?
                };
                *called.get_mut(called_count)? = target;
                called_count += 1;
                (call.destination, value)
            }
        };
        slots[destination as usize].write(value);
        initialized |= 1u64 << destination;
    }

    let mut values = [0i64; 4];
    for (index, entry) in plan.entries.iter().enumerate() {
        values[index] = resolve_object_array_long(
            entry.value,
            owner,
            receiver,
            arguments,
            &slots,
            initialized,
        )?;
    }

    Some(ObjectArrayEvaluated {
        values,
        value_count: plan.entries.len() as u8,
        called,
        called_count: called_count as u8,
    })
}

#[inline(always)]
unsafe fn materialize_object_array_values(
    owner: &UserFunction,
    plan: &ObjectArrayFunctionPlan,
    evaluated: &ObjectArrayEvaluated,
) -> Option<Value> {
    if evaluated.value_count as usize != plan.entries.len() {
        return None;
    }
    let mut result = PhpArray::with_hash_capacity(plan.entries.len());
    for (index, entry) in plan.entries.iter().enumerate() {
        let key = owner.op_array.literals.get(entry.key_literal as usize)?;
        if key.value_type() != ValueType::String {
            return None;
        }
        result.set_str_value(key, Value::long(evaluated.values[index]));
    }
    Some(Value::array(result))
}

#[inline(always)]
unsafe fn direct_object_array_arguments(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    receiver: &Value,
    sends: *const Instruction,
    callee: &UserFunction,
    plan: &ObjectArrayFunctionPlan,
) -> Option<([ObjectLongArgument; 8], *const Instruction)> {
    let common = &callee.common;
    if receiver.value_type() != ValueType::Object
        || receiver.is_reference()
        || common.sig.public_arity() != plan.public_args as u32
        || common.sig.required_num_args != plan.public_args as u32
        || !common.plan.call.is_compact_user_call()
        || common.plan.ret != ReturnStrategy::Fast
        || common.sig.ref_args != 0
        || common.sig.is_variadic
    {
        return None;
    }

    let declaring_class = eg.declaring_class_of(&callee.common as *const FunctionCommon);
    let mut arguments = [ObjectLongArgument::None; 8];
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
        if value.is_reference()
            || !check_type_hint(
                value,
                common
                    .sig
                    .param_type_hints
                    .get(index)
                    .unwrap_or(&ParamTypeHint::None),
                eg,
                caller_op_array.strict_types,
                declaring_class,
            )
        {
            return None;
        }
        arguments[index] = ObjectLongArgument::Borrowed(value as *const Value);
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
    Some((arguments, do_fcall_ptr))
}

/// Direct positional adapter for ObjectArrayFunctionPlan. The outer method's
/// declaration is validated before its borrowed arguments enter the region.
#[inline(never)]
pub(crate) unsafe fn try_execute_direct_object_array_call(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    receiver: &Value,
    sends: *const Instruction,
    callee: &UserFunction,
    plan: &ObjectArrayFunctionPlan,
) -> Option<(Value, *const Instruction)> {
    let (arguments, do_fcall_ptr) =
        direct_object_array_arguments(eg, caller, caller_op_array, receiver, sends, callee, plan)?;
    let evaluated = evaluate_object_array_values(eg, receiver, &arguments, callee, plan, &[])?;
    let result = materialize_object_array_values(callee, plan, &evaluated)?;
    evaluated.record_calls();
    Some((result, do_fcall_ptr))
}

#[derive(Clone, Copy)]
struct ObjectArrayAddConsumer {
    key_literal: u16,
    accumulator: u16,
}

impl ObjectArrayAddConsumer {
    const EMPTY: Self = Self {
        key_literal: 0,
        accumulator: 0,
    };
}

#[inline(always)]
fn object_array_entry_index_for_key(
    caller_op_array: &crate::compiler::OpArray,
    key_literal: u16,
    callee: &UserFunction,
    plan: &ObjectArrayFunctionPlan,
) -> Option<usize> {
    let key = caller_op_array
        .literals
        .get(key_literal as usize)?
        .as_str()?;
    for (index, entry) in plan.entries.iter().enumerate().rev() {
        if callee
            .op_array
            .literals
            .get(entry.key_literal as usize)?
            .as_str()?
            == key
        {
            return Some(index);
        }
    }
    None
}

#[inline(always)]
fn object_array_value_for_key(
    caller_op_array: &crate::compiler::OpArray,
    key_literal: u16,
    callee: &UserFunction,
    plan: &ObjectArrayFunctionPlan,
    evaluated: &ObjectArrayEvaluated,
) -> Option<i64> {
    let index = object_array_entry_index_for_key(caller_op_array, key_literal, callee, plan)?;
    evaluated.values.get(index).copied()
}

/// Commit an already-evaluated ObjectArray result into its proven immediate
/// scalar consumers.
#[inline(always)]
unsafe fn commit_object_array_consumers(
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    do_fcall_ptr: *const Instruction,
    callee: &UserFunction,
    plan: &ObjectArrayFunctionPlan,
    evaluated: &ObjectArrayEvaluated,
) -> Option<*const Instruction> {
    let do_fcall = &*do_fcall_ptr;
    if !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var) {
        return None;
    }
    let result_assign = &*do_fcall_ptr.add(1);
    if result_assign.opcode != OpCode::AssignCv
        || result_assign.op1_type != OpType::Cv
        || result_assign.op2_type != do_fcall.result_type
        || result_assign.op2 != do_fcall.result
        || result_assign.result_type != OpType::Unused
    {
        return None;
    }
    let array_cv = result_assign.op1;
    let mut consumers = [ObjectArrayAddConsumer::EMPTY; 4];
    let mut consumer_count = 0usize;
    let mut trailing = None;
    let mut cursor = do_fcall_ptr.add(2);
    let instruction_base = caller_op_array.instructions.as_ptr();
    while consumer_count + usize::from(trailing.is_some()) < 4 {
        let cursor_ip = cursor.offset_from(instruction_base);
        if cursor_ip < 0 {
            return None;
        }
        let fetch = caller_op_array.instructions.get(cursor_ip as usize)?;
        if fetch.opcode != OpCode::FetchDimR
            || fetch.op1_type != OpType::Cv
            || fetch.op1 != array_cv
            || fetch.op2_type != OpType::Const
            || !matches!(fetch.result_type, OpType::Tmp | OpType::Var)
            || caller_op_array
                .literals
                .get(fetch.op2 as usize)
                .and_then(Value::as_str)
                .is_none()
        {
            break;
        }
        let add = caller_op_array.instructions.get(cursor_ip as usize + 1);
        let assign = caller_op_array.instructions.get(cursor_ip as usize + 2);
        let accumulator = if let (Some(add), Some(assign)) = (add, assign)
            && matches!(
                add.opcode,
                OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp
            )
            && matches!(add.result_type, OpType::Tmp | OpType::Var)
            && assign.opcode == OpCode::AssignCv
            && assign.op1_type == OpType::Cv
            && assign.op2_type == add.result_type
            && assign.op2 == add.result
            && assign.result_type == OpType::Unused
        {
            if add.op1_type == OpType::Cv
                && add.op2_type == fetch.result_type
                && add.op2 == fetch.result
                && assign.op1 == add.op1
            {
                Some(add.op1)
            } else if add.op2_type == OpType::Cv
                && add.op1_type == fetch.result_type
                && add.op1 == fetch.result
                && assign.op1 == add.op2
            {
                Some(add.op2)
            } else {
                None
            }
        } else {
            None
        };
        if let Some(accumulator) = accumulator {
            consumers[consumer_count] = ObjectArrayAddConsumer {
                key_literal: fetch.op2,
                accumulator,
            };
            consumer_count += 1;
            cursor = cursor.add(3);
            continue;
        }
        trailing = Some((fetch.op2, fetch.result, fetch.result_type));
        cursor = cursor.add(1);
        break;
    }
    if consumer_count == 0 {
        return None;
    }

    let slot_base = (caller as *mut Value).add(CALL_FRAME_SLOTS);
    let mut destinations = [0u16; 4];
    let mut results = [0i64; 4];
    for (index, consumer) in consumers.iter().copied().take(consumer_count).enumerate() {
        let current = destinations[..index]
            .iter()
            .rposition(|destination| *destination == consumer.accumulator)
            .map(|previous| results[previous])
            .or_else(|| {
                let value = &*slot_base.add(consumer.accumulator as usize);
                (value.value_type() == ValueType::Long && !value.is_reference())
                    .then(|| value.raw_long())
            })?;
        let value = object_array_value_for_key(
            caller_op_array,
            consumer.key_literal,
            callee,
            plan,
            evaluated,
        )?;
        destinations[index] = consumer.accumulator;
        results[index] = current.checked_add(value)?;
    }
    let trailing_value = if let Some((key, result, result_type)) = trailing {
        Some((
            result,
            result_type,
            object_array_value_for_key(caller_op_array, key, callee, plan, evaluated)?,
        ))
    } else {
        None
    };

    for index in 0..consumer_count {
        frame_tmp_set_long(
            caller,
            slot_base.add(destinations[index] as usize),
            results[index],
        );
    }
    if let Some((result, _result_type, value)) = trailing_value {
        frame_tmp_set_long(caller, slot_base.add(result as usize), value);
    }
    evaluated.record_calls();
    (*caller).opline = cursor;
    Some(cursor)
}

/// Consume a dead, immediately-extracted ObjectArray result as raw Longs. The
/// compiler marker proves liveness; this adapter revalidates the concrete
/// instruction shape and commits only after the complete region succeeds.
#[inline(never)]
pub(crate) unsafe fn try_execute_direct_object_array_consumers(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    initializer_ptr: *const Instruction,
    receiver: &Value,
    callee: &UserFunction,
    plan: &ObjectArrayFunctionPlan,
) -> Option<*const Instruction> {
    let initializer = &*initializer_ptr;
    if initializer._pad & CALL_FLAG_OBJECT_ARRAY_CONSUMERS == 0 {
        return None;
    }
    let (arguments, do_fcall_ptr) = direct_object_array_arguments(
        eg,
        caller,
        caller_op_array,
        receiver,
        initializer_ptr.add(1),
        callee,
        plan,
    )?;
    let evaluated = evaluate_object_array_values(eg, receiver, &arguments, callee, plan, &[])?;
    commit_object_array_consumers(
        caller,
        caller_op_array,
        do_fcall_ptr,
        callee,
        plan,
        &evaluated,
    )
}

/// Execute a compiler-proven non-escaping constructor → ObjectArray consumer
/// pipeline without allocating the intermediate object. Constructor write
/// caches map borrowed/raw arguments onto virtual declared-property slots;
/// every downstream property access still validates its own canonical cache.
#[inline(never)]
unsafe fn try_execute_virtual_object_array_pipeline(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    new_ptr: *const Instruction,
) -> Option<*const Instruction> {
    let new_object = &*new_ptr;
    if new_object._pad & NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE == 0
        || new_object.opcode != OpCode::NewObj
        || new_object.op1_type != OpType::Const
        || !matches!(new_object.result_type, OpType::Tmp | OpType::Var)
        || new_object.extended_value == 0
        || new_object.extended_value > 8
    {
        return None;
    }
    let new_ip = new_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
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
        || constructor_common.sig.public_arity() != new_object.extended_value
        || constructor_common.sig.required_num_args != new_object.extended_value
        || constructor_common.sig.ref_args != 0
        || constructor_common.sig.is_variadic
        || !constructor_common.plan.call.is_compact_user_call()
        || constructor_common.plan.ret != ReturnStrategy::Fast
    {
        return None;
    }
    let constructor = &*(new_cache.func as *const UserFunction);
    let constructor_plan = constructor.property_init_plan.as_deref()?;
    if constructor_plan.public_args as u32 != new_object.extended_value
        || constructor_plan.assignments.len() > 8
    {
        return None;
    }

    let declaring_class = eg.declaring_class_of(new_cache.func);
    let mut constructor_values = [VirtualPropertyValue::Empty; 8];
    for index in 0..new_object.extended_value as usize {
        let send = &*new_ptr.add(1 + index);
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || send.op2 as u32 != constructor_common.sig.param_cv_index(index as u32)
        {
            return None;
        }
        let value = match send.op1_type {
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                &*(*caller).get_op_ptr(send.op1 as u32, send.op1_type, caller_op_array)
            }
            OpType::Unused => return None,
        };
        if value.is_reference()
            || !check_type_hint(
                value,
                constructor_common
                    .sig
                    .param_type_hints
                    .get(index)
                    .unwrap_or(&ParamTypeHint::None),
                eg,
                caller_op_array.strict_types,
                declaring_class,
            )
        {
            return None;
        }
        constructor_values[index] = if value.value_type() == ValueType::Long {
            VirtualPropertyValue::Long(value.raw_long())
        } else {
            VirtualPropertyValue::Borrowed(value as *const Value)
        };
    }
    let constructor_do_ptr = new_ptr.add(1 + new_object.extended_value as usize);
    let constructor_do = &*constructor_do_ptr;
    let object_assign = &*constructor_do_ptr.add(1);
    if constructor_do.opcode != OpCode::DoFcall
        || object_assign.opcode != OpCode::AssignCv
        || object_assign.op1_type != OpType::Cv
        || object_assign.op2_type != new_object.result_type
        || object_assign.op2 != new_object.result
        || object_assign.result_type != OpType::Unused
    {
        return None;
    }

    let mut virtual_object = VirtualObject {
        class_id: new_cache.class_id,
        class_def: class_def as *const crate::compiler::compile::ClassDef,
        property_slots: [usize::MAX; 8],
        property_values: [VirtualPropertyValue::Empty; 8],
        property_count: 0,
    };
    for assignment in constructor_plan.assignments.iter().copied() {
        let cache = constructor
            .op_array
            .cache
            .get(assignment.cache_ip as usize)?;
        if cache.class_id != virtual_object.class_id || cache.property_flags() != 3 {
            return None;
        }
        let slot = cache.property_slot();
        let value = *constructor_values.get(assignment.argument as usize)?;
        if let Some(index) = virtual_object.property_slots[..virtual_object.property_count as usize]
            .iter()
            .position(|existing| *existing == slot)
        {
            virtual_object.property_values[index] = value;
        } else {
            let index = virtual_object.property_count as usize;
            virtual_object.property_slots[index] = slot;
            virtual_object.property_values[index] = value;
            virtual_object.property_count += 1;
        }
    }

    let method_ptr = constructor_do_ptr.add(2);
    let method = &*method_ptr;
    if method.opcode != OpCode::InitMethodCall
        || method._pad & CALL_FLAG_OBJECT_ARRAY_CONSUMERS == 0
    {
        return None;
    }
    let method_receiver = match method.op1_type {
        OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
            &*(*caller).get_op_ptr(method.op1 as u32, method.op1_type, caller_op_array)
        }
        OpType::Unused => return None,
    };
    if method_receiver.value_type() != ValueType::Object || method_receiver.is_reference() {
        return None;
    }
    let receiver_class_id = method_receiver.object_class_id_unchecked();
    let method_ip = method_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
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
        || method_common.sig.public_arity() != method.extended_value
        || method_common.sig.required_num_args != method.extended_value
        || method_common.sig.ref_args != 0
        || method_common.sig.is_variadic
        || !method_common.plan.call.is_compact_user_call()
        || method_common.plan.ret != ReturnStrategy::Fast
    {
        return None;
    }
    let method_user = &*(method_cache.func as *const UserFunction);
    let method_plan = method_user.object_array_plan.as_deref()?;
    if method_plan.public_args as u32 != method.extended_value {
        return None;
    }

    let method_declaring_class = eg.declaring_class_of(method_cache.func);
    let mut method_arguments = [ObjectLongArgument::None; 8];
    let mut virtual_arguments = 0usize;
    for index in 0..method.extended_value as usize {
        let send = &*method_ptr.add(1 + index);
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || send.op2 as u32 != method_common.sig.param_cv_index(index as u32)
        {
            return None;
        }
        let hint = method_common
            .sig
            .param_type_hints
            .get(index)
            .unwrap_or(&ParamTypeHint::None);
        if send.op1_type == OpType::Cv && send.op1 == object_assign.op1 {
            if !virtual_object_matches_hint(&virtual_object, hint, eg, method_declaring_class) {
                return None;
            }
            method_arguments[index] =
                ObjectLongArgument::Virtual(&virtual_object as *const VirtualObject);
            virtual_arguments += 1;
        } else {
            let value = match send.op1_type {
                OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                    &*(*caller).get_op_ptr(send.op1 as u32, send.op1_type, caller_op_array)
                }
                OpType::Unused => return None,
            };
            if value.is_reference()
                || !check_type_hint(
                    value,
                    hint,
                    eg,
                    caller_op_array.strict_types,
                    method_declaring_class,
                )
            {
                return None;
            }
            method_arguments[index] = ObjectLongArgument::Borrowed(value as *const Value);
        }
    }
    if virtual_arguments != 1 {
        return None;
    }
    let method_do_ptr = method_ptr.add(1 + method.extended_value as usize);
    let method_do = &*method_do_ptr;
    if method_do.opcode != OpCode::DoFcall
        || !matches!(method_do.result_type, OpType::Tmp | OpType::Var)
    {
        return None;
    }

    let evaluated = evaluate_object_array_values(
        eg,
        method_receiver,
        &method_arguments,
        method_user,
        method_plan,
        &[],
    )?;
    let next = commit_object_array_consumers(
        caller,
        caller_op_array,
        method_do_ptr,
        method_user,
        method_plan,
        &evaluated,
    )?;
    record_scalar_call(constructor_common);
    record_scalar_call(method_common);
    Some(next)
}

#[inline(always)]
pub(crate) unsafe fn complete_direct_object_array_call(
    caller: *mut ExecuteData,
    do_fcall_ptr: *const Instruction,
    result: Value,
) {
    let do_fcall = &*do_fcall_ptr;
    if matches!(do_fcall.result_type, OpType::Tmp | OpType::Var) {
        let result_ptr = (caller as *mut Value).add(CALL_FRAME_SLOTS + do_fcall.result as usize);
        frame_tmp_set(caller, result_ptr, result);
    }
    (*caller).opline = do_fcall_ptr.add(1);
}
#[inline(always)]
pub(crate) unsafe fn complete_direct_scalar_long_call(
    caller: *mut ExecuteData,
    do_fcall_ptr: *const Instruction,
    result: i64,
) {
    let do_fcall = &*do_fcall_ptr;
    if matches!(do_fcall.result_type, OpType::Tmp | OpType::Var) {
        let result_ptr = (caller as *mut Value).add(CALL_FRAME_SLOTS + do_fcall.result as usize);
        frame_tmp_set_long(caller, result_ptr, result);
    }
    (*caller).opline = do_fcall_ptr.add(1);
}

#[inline(always)]
pub(crate) unsafe fn complete_direct_string_call(
    caller: *mut ExecuteData,
    do_fcall_ptr: *const Instruction,
    result: String,
) {
    let do_fcall = &*do_fcall_ptr;
    if matches!(do_fcall.result_type, OpType::Tmp | OpType::Var) {
        let result_ptr = (caller as *mut Value).add(CALL_FRAME_SLOTS + do_fcall.result as usize);
        frame_tmp_set(caller, result_ptr, Value::string(result));
    }
    (*caller).opline = do_fcall_ptr.add(1);
}

#[inline(always)]
pub(crate) unsafe fn complete_direct_scalar_double_call(
    caller: *mut ExecuteData,
    do_fcall_ptr: *const Instruction,
    result: f64,
) {
    let do_fcall = &*do_fcall_ptr;
    if matches!(do_fcall.result_type, OpType::Tmp | OpType::Var) {
        let result_ptr = (caller as *mut Value).add(CALL_FRAME_SLOTS + do_fcall.result as usize);
        frame_tmp_set(caller, result_ptr, Value::double(result));
    }
    (*caller).opline = do_fcall_ptr.add(1);
}
