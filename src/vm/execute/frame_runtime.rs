// Kept in the execute module through include! so this structural split does not change visibility or code generation.
/// Update a globals entry in-place if the key exists, otherwise insert.
/// Avoids String clone + HashMap rehash on the hot path (key already present).
#[inline(always)]
fn globals_set(globals: &mut HashMap<String, Value>, key: &str, val: Value) {
    if let Some(slot) = globals.get_mut(key) {
        *slot = val;
    } else {
        globals.insert(key.to_string(), val);
    }
}

// ── Slot write API ──
//
// Per-slot bitmap tracking: each write helper maintains heap_bitmap (u64) alongside
// the has_heap_slots flag. Cleanup uses bitmap to drop only truly-heap slots instead
// of scanning all. Bitmap ops are gated behind has_heap_slots — scalar-only frames
// (like fib) skip bitmap entirely for zero overhead.
//
// For frames with > 64 total slots, bitmap path is skipped (u64 covers 64 bits).
// The has_heap_slots flag is always kept in sync as fallback.
//
// slot_set:              Overwrite any initialized slot. Drops old. No frame tracking.
// frame_slot_set:        Overwrite a frame slot. Per-slot drop via bitmap.
// frame_slot_init:       First write to uninitialized arg slot. No drop.
// frame_set_this:        Write $this into CV[0].
// frame_tmp_set:         Write to frame TMP slot. Per-slot drop via bitmap. (hot path)
// frame_tmp_set_long:    Write Long directly to TMP. No Value construction. (hot path)
// frame_tmp_set_bool:    Write Bool directly to TMP. No Value construction. (hot path)
// frame_return_set:      Write a callee result into its caller's tracked TMP.
// frame_tmp_prepare_external_write / finish_external_write:
//                        Bracket internal handlers that write through raw pointers.

/// Compute absolute slot index from frame pointer and slot pointer.
#[inline(always)]
unsafe fn slot_idx(frame: *const ExecuteData, ptr: *const Value) -> u32 {
    ptr.offset_from((frame as *const Value).add(CALL_FRAME_SLOTS)) as u32
}

/// Overwrite a slot, dropping the old value. No frame heap tracking.
/// For external targets (return_value, globals) or mixed contexts.
/// SAFETY: `ptr` must point to a valid, initialized Value.
#[inline(always)]
unsafe fn slot_set(ptr: *mut Value, val: Value) {
    stats::inc_write_val();
    std::ptr::drop_in_place(ptr);
    ptr.write(val);
}

/// Perform an ordinary PHP assignment into a value storage location.
///
/// Array elements and object properties may hold an explicit PHP reference
/// cell. Assigning to that element/property updates the referenced value while
/// preserving every alias; materialization paths that intentionally replace a
/// reference wrapper continue to use `slot_set` or their container setter.
#[inline(always)]
fn assignment_slot_set(slot: &mut Value, val: Value) {
    // SAFETY: `slot` is an exclusive initialized Value. A reference Value's
    // target is live by its representation contract; otherwise the target is
    // the slot itself. `slot_set` preserves initialization after replacement.
    unsafe {
        let target = if slot.is_reference() {
            slot.as_ref_ptr()
        } else {
            slot as *mut Value
        };
        slot_set(target, val);
    }
}

/// Bitmap slow path: drop + update bitmap for a TMP slot overwrite.
/// Outlined to keep hot inline code small.
#[inline(never)]
#[cold]
unsafe fn bitmap_drop_and_update(frame: *mut ExecuteData, ptr: *mut Value, heap: bool) {
    let total = (*frame).num_cvs + (*frame).num_temps;
    if total <= 64 {
        let idx = slot_idx(frame, ptr);
        let bit = 1u64 << idx;
        if (*frame).heap_bitmap & bit != 0 {
            std::ptr::drop_in_place(ptr);
        }
        if heap {
            (*frame).heap_bitmap |= bit;
        } else {
            (*frame).heap_bitmap &= !bit;
        }
    } else {
        std::ptr::drop_in_place(ptr);
    }
}

/// Bitmap slow path: drop a scalar overwrite of a heap slot.
#[inline(never)]
#[cold]
unsafe fn bitmap_drop_scalar(frame: *mut ExecuteData, ptr: *mut Value) {
    let total = (*frame).num_cvs + (*frame).num_temps;
    if total <= 64 {
        let idx = slot_idx(frame, ptr);
        let bit = 1u64 << idx;
        if (*frame).heap_bitmap & bit != 0 {
            std::ptr::drop_in_place(ptr);
            (*frame).heap_bitmap &= !bit;
        }
    } else {
        std::ptr::drop_in_place(ptr);
    }
}

/// Bitmap slow path: mark a slot as heap (first heap write in frame).
#[inline(never)]
#[cold]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
unsafe fn bitmap_mark_heap(frame: *mut ExecuteData, ptr: *const Value) {
    let total = (*frame).num_cvs + (*frame).num_temps;
    if total <= 64 {
        let idx = slot_idx(frame, ptr);
        (*frame).heap_bitmap |= 1u64 << idx;
    }
}

/// Write to a frame TMP result slot.
/// Hot-path: when has_heap_slots is false (scalar-only frame), identical to original.
/// Cold-path: bitmap-driven per-slot drop, outlined for icache efficiency.
#[inline(always)]
unsafe fn frame_tmp_set(frame: *mut ExecuteData, ptr: *mut Value, val: Value) {
    let heap = val.needs_cleanup();
    if (*frame).has_heap_slots {
        bitmap_drop_and_update(frame, ptr, heap);
        ptr.write(val);
    } else {
        ptr.write(val);
        if heap {
            (*frame).has_heap_slots = true;
            bitmap_mark_heap(frame, ptr);
        }
    }
}

/// Write a Long value directly to a frame TMP slot. Zero overhead for scalar frames.
#[inline(always)]
pub(super) unsafe fn frame_tmp_set_long(frame: *mut ExecuteData, ptr: *mut Value, v: i64) {
    if (*frame).has_heap_slots {
        bitmap_drop_scalar(frame, ptr);
    }
    Value::write_long(ptr, v);
}

/// Write a Bool value directly to a frame TMP slot.
#[inline(always)]
unsafe fn frame_tmp_set_bool(frame: *mut ExecuteData, ptr: *mut Value, v: bool) {
    if (*frame).has_heap_slots {
        bitmap_drop_scalar(frame, ptr);
    }
    Value::write_bool(ptr, v);
}

/// Write a synchronous callee result into its caller-owned return slot.
///
/// DoFcall results are compiler-owned TMP/VAR slots.  Routing the overwrite
/// through the caller bitmap is important because stack reuse does not
/// initialize small-frame TMP bytes: an unset bitmap bit means "no live heap
/// value", even when the stale bytes are not a valid `Value`.
#[inline(always)]
pub(super) unsafe fn frame_return_set(frame: *mut ExecuteData, ptr: *mut Value, val: Value) {
    let caller = (*frame).prev_execute_data;
    if caller.is_null() {
        slot_set(ptr, val);
    } else {
        frame_tmp_set(caller, ptr, val);
    }
}

/// Copy a proven scalar return without constructing or cloning a `Value`.
#[inline(always)]
pub(super) unsafe fn frame_return_copy_scalar(
    frame: *mut ExecuteData,
    ptr: *mut Value,
    source: *const Value,
) {
    let caller = (*frame).prev_execute_data;
    if caller.is_null() {
        slot_set(ptr, (*source).clone());
    } else {
        if (*caller).has_heap_slots {
            bitmap_drop_scalar(caller, ptr);
        }
        Value::raw_copy(source, ptr);
    }
}

/// Write a proven Long return directly into the caller slot.
#[inline(always)]
pub(super) unsafe fn frame_return_set_long(frame: *mut ExecuteData, ptr: *mut Value, value: i64) {
    let caller = (*frame).prev_execute_data;
    if caller.is_null() {
        slot_set(ptr, Value::long(value));
    } else {
        if (*caller).has_heap_slots {
            bitmap_drop_scalar(caller, ptr);
        }
        Value::write_long(ptr, value);
    }
}

/// Prepare a caller TMP for an internal handler that writes with `ptr.write`.
#[inline(always)]
unsafe fn frame_tmp_prepare_external_write(frame: *mut ExecuteData, ptr: *mut Value) {
    if (*frame).has_heap_slots {
        bitmap_drop_scalar(frame, ptr);
    }
    ptr.write(Value::undef());
}

/// Record a heap-backed value written directly by an internal handler.
#[inline(always)]
unsafe fn frame_tmp_finish_external_write(frame: *mut ExecuteData, ptr: *mut Value) {
    if (*ptr).needs_cleanup() {
        (*frame).has_heap_slots = true;
        bitmap_mark_heap(frame, ptr);
    }
}

/// Prepare any DoFcall result operand for a raw internal-handler write.
#[inline(always)]
unsafe fn frame_result_prepare_external_write(
    frame: *mut ExecuteData,
    ptr: *mut Value,
    result_type: OpType,
) {
    if matches!(result_type, OpType::Tmp | OpType::Var) {
        frame_tmp_prepare_external_write(frame, ptr);
    } else {
        slot_set(ptr, Value::undef());
    }
}

/// Finish tracking a raw internal-handler write to a caller-owned result.
#[inline(always)]
unsafe fn frame_result_finish_external_write(
    frame: *mut ExecuteData,
    ptr: *mut Value,
    result_type: OpType,
) {
    if matches!(result_type, OpType::Tmp | OpType::Var) {
        frame_tmp_finish_external_write(frame, ptr);
    }
}

/// Write a fully materialized DoFcall result through the appropriate owner.
#[inline(always)]
unsafe fn frame_result_set(
    frame: *mut ExecuteData,
    ptr: *mut Value,
    result_type: OpType,
    value: Value,
) {
    if matches!(result_type, OpType::Tmp | OpType::Var) {
        frame_tmp_set(frame, ptr, value);
    } else {
        slot_set(ptr, value);
    }
}

/// Overwrite a frame slot (CV or TMP). Per-slot drop via bitmap when heap present.
#[inline(always)]
pub(super) unsafe fn frame_slot_set(frame: *mut ExecuteData, ptr: *mut Value, val: Value) {
    let heap = val.needs_cleanup();
    stats::inc_write_frame_slot(heap);
    if (*frame).has_heap_slots {
        bitmap_drop_and_update(frame, ptr, heap);
        ptr.write(val);
    } else {
        ptr.write(val);
        if heap {
            (*frame).has_heap_slots = true;
            bitmap_mark_heap(frame, ptr);
        }
    }
}

/// Init an unwritten frame slot (arg CV during SendVal/SendRef/SendNamed).
/// No drop — slot is uninitialized. Bitmap + has_heap_slots only if heap value.
#[inline(always)]
unsafe fn frame_slot_init(frame: *mut ExecuteData, ptr: *mut Value, val: Value) {
    let heap = val.needs_cleanup();
    stats::inc_write_frame_slot(heap);
    ptr.write(val);
    if heap {
        (*frame).has_heap_slots = true;
        bitmap_mark_heap(frame, ptr);
    }
}

/// Initialize a sequential argument in a freshly pushed callback frame.
/// The slot index is already known, so heap bookkeeping can set the bitmap
/// directly instead of calling the outlined pointer-to-index slow path.
#[inline(always)]
unsafe fn callback_arg_init(frame: *mut ExecuteData, index: usize, val: Value) {
    let heap = val.needs_cleanup();
    stats::inc_write_frame_slot(heap);
    let ptr = (frame as *mut Value).add(CALL_FRAME_SLOTS + index);
    ptr.write(val);
    if heap {
        (*frame).has_heap_slots = true;
        if (*frame).num_cvs + (*frame).num_temps <= 64 {
            (*frame).heap_bitmap |= 1u64 << index;
        }
    }
}

/// Restore a saved CV/TMP slot into a freshly pushed generator frame.
/// No drop — slot is uninitialized. Track heap via bitmap.
#[inline(always)]
unsafe fn frame_restore_slot(frame: *mut ExecuteData, ptr: *mut Value, val: Value) {
    let heap = val.needs_cleanup();
    ptr.write(val);
    if heap {
        (*frame).has_heap_slots = true;
        bitmap_mark_heap(frame, ptr);
    }
}

/// Write $this into CV[0] of a method frame.
#[inline(always)]
unsafe fn frame_set_this(frame: *mut ExecuteData, val: Value) {
    let heap = val.needs_cleanup();
    let ptr = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    ptr.write(val);
    if heap {
        (*frame).has_heap_slots = true;
        let total = (*frame).num_cvs + (*frame).num_temps;
        if total <= 64 {
            (*frame).heap_bitmap |= 1u64;
        }
    }
}

/// Borrow `$this` from the caller for the lifetime of a synchronous method
/// frame. The slot deliberately stays out of heap cleanup bookkeeping.
///
/// SAFETY: the caller's source Value must outlive `frame`, and the compiled
/// call plan must prove that CV 0 is never returned directly.
#[inline(always)]
unsafe fn frame_set_borrowed_this(frame: *mut ExecuteData, val: *const Value) {
    let ptr = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    Value::raw_copy(val, ptr);
}

/// Initialize a by-value heap parameter as a synchronous borrow. The caller
/// keeps the owning Value alive until DoFcall returns, and the callee's proof
/// excludes direct transfer, rebinding and String/Array COW mutation.
#[inline(always)]
unsafe fn try_init_borrowed_heap_arg(
    call: *mut ExecuteData,
    public_param: u32,
    source: *const Value,
    destination: *mut Value,
) -> bool {
    let common = &*(*call).func;
    if common.fn_type != FunctionType::User
        || public_param >= 64
        || common.sig.is_param_by_ref(public_param)
    {
        return false;
    }
    let user = &*((*call).func as *const UserFunction);
    if user.borrowable_heap_args & (1u64 << public_param) == 0 || !(*source).needs_cleanup() {
        return false;
    }
    Value::raw_copy(source, destination);
    true
}

/// Return a stable alias for a CV exposed through a PHP reference. Ordinary
/// locals are promoted to request-owned cells so a reference captured by a
/// returned closure can outlive every forwarding call frame. A pre-existing
/// borrowed reference remains a borrowed alias; canonical SendRef paths create
/// owned cells at their first caller boundary.
#[inline(always)]
unsafe fn materialize_reference_alias(frame: *mut ExecuteData, ptr: *mut Value) -> Value {
    if (*ptr).is_owned_reference() {
        return (*ptr).clone_owned_reference_alias();
    }
    if (*ptr).is_reference() {
        return Value::reference((*ptr).as_ref_ptr());
    }

    let total = (*frame).num_cvs + (*frame).num_temps;
    if total <= 64 && (*ptr).needs_cleanup() {
        let idx = slot_idx(frame, ptr);
        let bit = 1u64 << idx;
        if (*frame).heap_bitmap & bit == 0 {
            let owned = (*ptr).clone();
            ptr.write(owned);
            (*frame).has_heap_slots = true;
            (*frame).heap_bitmap |= bit;
        }
    }

    let current = std::mem::replace(&mut *ptr, Value::undef());
    let binding = Value::owned_reference(current);
    frame_slot_set(frame, ptr, binding.clone_owned_reference_alias());
    binding
}

/// Copy a scalar argument operand directly into a pending call frame.
#[inline(always)]
unsafe fn try_copy_scalar_arg(
    frame: *mut ExecuteData,
    call: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    send: &Instruction,
) -> bool {
    let source = match send.op1_type {
        OpType::Tmp | OpType::Var => {
            (frame as *const Value).add(CALL_FRAME_SLOTS + send.op1 as usize)
        }
        OpType::Cv => {
            let cv = (*frame).cv(send.op1 as u32);
            if cv.is_reference() {
                cv.as_ref_ptr() as *const Value
            } else {
                cv as *const Value
            }
        }
        OpType::Const => &op_array.literals()[send.op1 as usize] as *const Value,
        OpType::Unused => return false,
    };
    let value = &*source;
    if value.needs_cleanup() || value.is_reference() {
        return false;
    }

    let destination = (call as *mut Value).add(CALL_FRAME_SLOTS + send.op2 as usize);
    if value.is_undef() {
        Value::write_null(destination);
        return true;
    }
    Value::raw_copy(source, destination);
    true
}

/// Fast path used by ordinary function calls. Keeping the SendVal-only wrapper
/// separate avoids adding method/reference branches to this hotter path.
#[inline(always)]
pub(crate) unsafe fn try_send_scalar_arg(
    frame: *mut ExecuteData,
    call: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    send: &Instruction,
) -> bool {
    debug_assert!(send.opcode == OpCode::SendVal);
    try_copy_scalar_arg(frame, call, op_array, send)
}

/// Method-call variant that also accepts SendVarEx after proving the resolved
/// scalar callee cannot require a reference.
#[inline(always)]
pub(crate) unsafe fn try_send_scalar_method_arg(
    frame: *mut ExecuteData,
    call: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    send: &Instruction,
) -> bool {
    match send.opcode {
        OpCode::SendVal => try_copy_scalar_arg(frame, call, op_array, send),
        OpCode::SendVarEx => {
            let common = &*(*call).func;
            common.supports_scalar_long_plan()
                && !common.sig.is_param_by_ref(send.extended_value)
                && try_copy_scalar_arg(frame, call, op_array, send)
        }
        _ => false,
    }
}

/// Consume scalar sends immediately following a call initializer.
///
/// Argument expressions may insert other opcodes (including nested calls), so
/// only the contiguous prefix is fused. Returns the number of Send opcodes
/// whose argument slots were initialized.
#[inline(always)]
unsafe fn bind_contiguous_scalar_args(
    frame: *mut ExecuteData,
    call: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    mut next: *const Instruction,
    max_args: u32,
    fast_scalar: bool,
) -> usize {
    let mut bound = 0usize;
    while bound < max_args as usize {
        let send = &*next;
        let copied = match send.opcode {
            OpCode::SendVal => try_copy_scalar_arg(frame, call, op_array, send),
            // FastScalar construction proves that no formal parameter is by
            // reference, so SendVarEx needs only the scalar-value guard.
            OpCode::SendVarEx if fast_scalar => try_copy_scalar_arg(frame, call, op_array, send),
            _ => false,
        };
        if !copied {
            break;
        }
        bound += 1;
        next = next.add(1);
    }
    bound
}

#[inline(always)]
fn resolve_long_plan_source(source: LongPlanSource, arguments: &[i64; 8]) -> i64 {
    match source {
        LongPlanSource::Argument(index) => arguments[index as usize],
        LongPlanSource::Constant(value) => value,
    }
}

/// Execute a compiler-proven integer property method without allocating a VM
/// frame. Every guard and arithmetic operation completes before the first
/// write, so a failed fast path can safely restart through ordinary DoFcall.
#[inline(always)]
pub(crate) unsafe fn try_execute_long_property_method(
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    receiver: &Value,
    sends: *const Instruction,
    plan: &LongPropertyMethodPlan,
    callee: &UserFunction,
) -> bool {
    let mut arguments = [0i64; 8];
    for (index, argument) in arguments
        .iter_mut()
        .enumerate()
        .take(plan.public_args as usize)
    {
        let send = &*sends.add(index);
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || send.op2 as usize != index + 1
        {
            return false;
        }
        let value = match send.op1_type {
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                &*(*caller).get_op_ptr(send.op1 as u32, send.op1_type, caller_op_array)
            }
            OpType::Unused => return false,
        };
        if value.value_type() != ValueType::Long {
            return false;
        }
        *argument = value.raw_long();
    }

    try_execute_long_property_plan(receiver, &arguments, plan, callee)
}

/// Hot-executor boundary for the property evaluator. The baseline interpreter
/// benefits from inlining this short call protocol, while recursively-entered
/// hot frames must not inherit its argument workspace in their host stack
/// frame.
#[inline(never)]
pub(crate) unsafe fn try_execute_hot_long_property_method(
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    receiver: &Value,
    sends: *const Instruction,
    plan: &LongPropertyMethodPlan,
    callee: &UserFunction,
) -> bool {
    try_execute_long_property_method(caller, caller_op_array, receiver, sends, plan, callee)
}

/// Execute the guarded/transactional portion shared by contiguous and compact
/// deferred calls after their public arguments have been normalized to Long.
#[inline(always)]
unsafe fn try_execute_long_property_plan(
    receiver: &Value,
    arguments: &[i64; 8],
    plan: &LongPropertyMethodPlan,
    callee: &UserFunction,
) -> bool {
    if plan.properties.len() == 1 {
        return try_execute_single_long_property_plan(receiver, arguments, plan, callee);
    }
    try_execute_multi_long_property_plan(receiver, arguments, plan, callee)
}

/// Dominant property-method shape: one declared slot, optionally updated more
/// than once. Keeping it separate avoids reserving the two eight-element
/// transactional arrays in every hot-executor activation.
#[inline(always)]
unsafe fn try_execute_single_long_property_plan(
    receiver: &Value,
    arguments: &[i64; 8],
    plan: &LongPropertyMethodPlan,
    callee: &UserFunction,
) -> bool {
    let class_id = receiver.object_class_id_unchecked();
    if class_id == 0 {
        return false;
    }
    let property = &plan.properties[0];
    let cache = &callee.op_array.cache[property.cache_ip as usize];
    let flags = cache.property_flags();
    let required = property.required_flags as u32;
    if cache.class_id != class_id
        || (flags & required != required
            && !(required == 3 && instance_property_cache_accepts_long_write(cache)))
    {
        return false;
    }
    let slot = cache.property_slot();
    let property_value = &*receiver.object_property_slot_unchecked(slot);
    if property_value.value_type() != ValueType::Long {
        return false;
    }
    let mut value = property_value.raw_long();
    let mut written = false;

    for operation in plan.operations.iter().copied() {
        match operation {
            LongPropertyOp::Add { property, rhs } => {
                if property != 0 {
                    return false;
                }
                let Some(updated) = value.checked_add(resolve_long_plan_source(rhs, arguments))
                else {
                    return false;
                };
                value = updated;
                written = true;
            }
            LongPropertyOp::Sub { property, rhs } => {
                if property != 0 {
                    return false;
                }
                let Some(updated) = value.checked_sub(resolve_long_plan_source(rhs, arguments))
                else {
                    return false;
                };
                value = updated;
                written = true;
            }
            LongPropertyOp::Min {
                property,
                candidate,
            } => {
                if property != 0 {
                    return false;
                }
                let candidate = resolve_long_plan_source(candidate, arguments);
                if candidate < value {
                    value = candidate;
                    written = true;
                }
            }
            LongPropertyOp::Max {
                property,
                candidate,
            } => {
                if property != 0 {
                    return false;
                }
                let candidate = resolve_long_plan_source(candidate, arguments);
                if candidate > value {
                    value = candidate;
                    written = true;
                }
            }
            LongPropertyOp::Set {
                property,
                value: source,
            } => {
                if property != 0 {
                    return false;
                }
                value = resolve_long_plan_source(source, arguments);
                written = true;
            }
        }
    }

    if written {
        Value::write_long(
            receiver.object_property_slot_unchecked(slot) as *mut Value,
            value,
        );
    }
    true
}

/// General multi-property transaction remains out of line. It is important
/// for real methods such as statistics accumulators, but should not inflate the
/// host stack frame of every scalar-recursive hot call.
#[inline(never)]
unsafe fn try_execute_multi_long_property_plan(
    receiver: &Value,
    arguments: &[i64; 8],
    plan: &LongPropertyMethodPlan,
    callee: &UserFunction,
) -> bool {
    let class_id = receiver.object_class_id_unchecked();
    if class_id == 0 {
        return false;
    }
    let mut property_values = [0i64; 8];
    let mut property_slots = [0usize; 8];
    for (index, property) in plan.properties.iter().enumerate() {
        let cache = &callee.op_array.cache[property.cache_ip as usize];
        let flags = cache.property_flags();
        let required = property.required_flags as u32;
        if cache.class_id != class_id
            || (flags & required != required
                && !(required == 3 && instance_property_cache_accepts_long_write(cache)))
        {
            return false;
        }
        let slot = cache.property_slot();
        let value = &*receiver.object_property_slot_unchecked(slot);
        if value.value_type() != ValueType::Long {
            return false;
        }
        property_slots[index] = slot;
        property_values[index] = value.raw_long();
    }

    let mut written = 0u8;
    for operation in plan.operations.iter().copied() {
        match operation {
            LongPropertyOp::Add { property, rhs } => {
                let target = &mut property_values[property as usize];
                let Some(value) = target.checked_add(resolve_long_plan_source(rhs, arguments))
                else {
                    return false;
                };
                *target = value;
                written |= 1 << property;
            }
            LongPropertyOp::Sub { property, rhs } => {
                let target = &mut property_values[property as usize];
                let Some(value) = target.checked_sub(resolve_long_plan_source(rhs, arguments))
                else {
                    return false;
                };
                *target = value;
                written |= 1 << property;
            }
            LongPropertyOp::Min {
                property,
                candidate,
            } => {
                let candidate = resolve_long_plan_source(candidate, arguments);
                let target = &mut property_values[property as usize];
                if candidate < *target {
                    *target = candidate;
                    written |= 1 << property;
                }
            }
            LongPropertyOp::Max {
                property,
                candidate,
            } => {
                let candidate = resolve_long_plan_source(candidate, arguments);
                let target = &mut property_values[property as usize];
                if candidate > *target {
                    *target = candidate;
                    written |= 1 << property;
                }
            }
            LongPropertyOp::Set { property, value } => {
                property_values[property as usize] = resolve_long_plan_source(value, arguments);
                written |= 1 << property;
            }
        }
    }

    for index in 0..plan.properties.len() {
        if written & (1 << index) != 0 {
            Value::write_long(
                receiver.object_property_slot_unchecked(property_slots[index]) as *mut Value,
                property_values[index],
            );
        }
    }
    true
}

/// Closed-region variant whose class/layout/property guards were resolved at
/// entry. Values remain transactional: all checked operations complete before
/// any declared property slot is updated.
#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn try_execute_resolved_long_property_plan(
    receiver: &Value,
    arguments: &[i64; 8],
    plan: &LongPropertyMethodPlan,
    property_slots: &[usize; 8],
    property_count: u8,
) -> bool {
    if property_count as usize != plan.properties.len() || property_count > 8 {
        return false;
    }
    let mut property_values = [0i64; 8];
    for index in 0..property_count as usize {
        let value = &*receiver.object_property_slot_unchecked(property_slots[index]);
        if value.value_type() != ValueType::Long {
            return false;
        }
        property_values[index] = value.raw_long();
    }

    let mut written = 0u8;
    for operation in plan.operations.iter().copied() {
        match operation {
            LongPropertyOp::Add { property, rhs } => {
                let Some(target) = property_values.get_mut(property as usize) else {
                    return false;
                };
                let Some(value) = target.checked_add(resolve_long_plan_source(rhs, arguments))
                else {
                    return false;
                };
                *target = value;
                written |= 1 << property;
            }
            LongPropertyOp::Sub { property, rhs } => {
                let Some(target) = property_values.get_mut(property as usize) else {
                    return false;
                };
                let Some(value) = target.checked_sub(resolve_long_plan_source(rhs, arguments))
                else {
                    return false;
                };
                *target = value;
                written |= 1 << property;
            }
            LongPropertyOp::Min {
                property,
                candidate,
            } => {
                let Some(target) = property_values.get_mut(property as usize) else {
                    return false;
                };
                let candidate = resolve_long_plan_source(candidate, arguments);
                if candidate < *target {
                    *target = candidate;
                    written |= 1 << property;
                }
            }
            LongPropertyOp::Max {
                property,
                candidate,
            } => {
                let Some(target) = property_values.get_mut(property as usize) else {
                    return false;
                };
                let candidate = resolve_long_plan_source(candidate, arguments);
                if candidate > *target {
                    *target = candidate;
                    written |= 1 << property;
                }
            }
            LongPropertyOp::Set { property, value } => {
                let Some(target) = property_values.get_mut(property as usize) else {
                    return false;
                };
                *target = resolve_long_plan_source(value, arguments);
                written |= 1 << property;
            }
        }
    }

    for index in 0..property_count as usize {
        if written & (1 << index) != 0 {
            Value::write_long(
                receiver.object_property_slot_unchecked(property_slots[index]) as *mut Value,
                property_values[index],
            );
        }
    }
    true
}

/// Materialize a compiler-proven `return $this->property` call directly into
/// the caller's DoFcall result.  The callee's FetchObjR cache is authoritative:
/// only a declared public property resolved for this exact receiver class can
/// enter the path.  A cold, polymorphic, dynamic, or non-public access simply
/// resumes through the ordinary method frame.
#[inline(always)]
pub(crate) unsafe fn try_execute_direct_property_getter(
    caller: *mut ExecuteData,
    receiver: &Value,
    do_fcall_ptr: *const Instruction,
    callee: &UserFunction,
    plan: &PropertyGetterMethodPlan,
) -> bool {
    if !direct_property_getters_enabled() {
        return false;
    }
    let do_fcall = &*do_fcall_ptr;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(
            do_fcall.result_type,
            OpType::Unused | OpType::Tmp | OpType::Var
        )
    {
        return false;
    }

    let class_id = receiver.object_class_id_unchecked();
    if class_id == 0 {
        return false;
    }
    let cache = &callee.op_array.cache[plan.cache_ip as usize];
    if cache.class_id != class_id || cache.property_flags() & 1 == 0 {
        return false;
    }
    let property_slot = cache.property_slot();
    let property = &*receiver.object_property_slot_unchecked(property_slot);
    if property.is_undef() {
        // A getter cache is shared by all instances of the class. The
        // baseline read must construct the catchable uninitialized typed
        // property Error for a different, not-yet-initialized receiver.
        return false;
    }

    if matches!(do_fcall.result_type, OpType::Tmp | OpType::Var) {
        let result_ptr = (caller as *mut Value).add(CALL_FRAME_SLOTS + do_fcall.result as usize);
        if property.needs_cleanup() || property.is_reference() {
            frame_slot_set(caller, result_ptr, property.clone());
        } else {
            // Canonical FastScalar return also transfers scalar slots as a raw
            // Value.  Preserve that zero-clone path while clearing a possible
            // previous heap bit on reused caller temporaries.
            if (*caller).has_heap_slots {
                bitmap_drop_scalar(caller, result_ptr);
            }
            Value::raw_copy(property as *const Value, result_ptr);
        }
    }
    record_scalar_call(&callee.common);
    (*caller).opline = do_fcall_ptr.add(1);
    true
}

/// Keep heap-aware getter materialization out of recursively-entered hot host
/// frames; baseline retains the inlined form.
#[inline(never)]
pub(crate) unsafe fn try_execute_hot_property_getter(
    caller: *mut ExecuteData,
    receiver: &Value,
    do_fcall_ptr: *const Instruction,
    callee: &UserFunction,
    plan: &PropertyGetterMethodPlan,
) -> bool {
    try_execute_direct_property_getter(caller, receiver, do_fcall_ptr, callee, plan)
}

/// Fuse the exact call-site shape `longPropertyMutator(propertyGetter())`
/// after both method and property inline caches have independently proven the
/// dispatch.  The getter read is captured before the transactional outer plan
/// starts, preserving PHP argument evaluation and same-object aliasing.
#[inline(never)]
pub(crate) unsafe fn try_execute_composed_long_property_call(
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    outer_init_ptr: *const Instruction,
    outer_receiver: &Value,
    outer_user: &UserFunction,
    outer_plan: &LongPropertyMethodPlan,
) -> bool {
    if !composed_property_calls_enabled() || outer_plan.public_args != 1 {
        return false;
    }
    let base = caller_op_array.instructions.as_ptr();
    let outer_ip = outer_init_ptr.offset_from(base);
    if outer_ip < 0 || outer_ip as usize + 4 >= caller_op_array.instructions.len() {
        return false;
    }

    let inner_init_ptr = outer_init_ptr.add(1);
    let inner_do_ptr = outer_init_ptr.add(2);
    let send_ptr = outer_init_ptr.add(3);
    let outer_do_ptr = outer_init_ptr.add(4);
    let inner_init = &*inner_init_ptr;
    let inner_do = &*inner_do_ptr;
    let send = &*send_ptr;
    let outer_do = &*outer_do_ptr;
    if inner_init.opcode != OpCode::InitMethodCall
        || inner_init.extended_value != 0
        || inner_do.opcode != OpCode::DoFcall
        || !matches!(inner_do.result_type, OpType::Tmp | OpType::Var)
        || !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
        || send.op1_type != inner_do.result_type
        || send.op1 != inner_do.result
        || send.op2 != 1
        || outer_do.opcode != OpCode::DoFcall
        || outer_do.result_type != OpType::Unused
    {
        return false;
    }

    let inner_receiver = match inner_init.op1_type {
        OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
            &*(*caller).get_op_ptr(inner_init.op1 as u32, inner_init.op1_type, caller_op_array)
        }
        OpType::Unused => return false,
    };
    let inner_receiver = if inner_receiver.is_reference() {
        &*inner_receiver.as_ref_ptr()
    } else {
        inner_receiver
    };
    if inner_receiver.value_type() != ValueType::Object {
        return false;
    }
    let inner_class_id = inner_receiver.object_class_id_unchecked();
    if inner_class_id == 0 {
        return false;
    }
    let inner_ic = &caller_op_array.cache[outer_ip as usize + 1];
    if inner_ic.func.is_null()
        || inner_ic.class_id != inner_class_id
        || !inner_ic.method_has_property_getter_plan()
    {
        return false;
    }
    let inner_common = &*inner_ic.func;
    if inner_common.fn_type != FunctionType::User || inner_common.sig.public_arity() != 0 {
        return false;
    }
    let inner_user = &*(inner_ic.func as *const UserFunction);
    let Some(getter_plan) = inner_user.property_getter_plan.as_ref() else {
        return false;
    };
    let getter_cache = &inner_user.op_array.cache[getter_plan.cache_ip as usize];
    if getter_cache.class_id != inner_class_id || getter_cache.property_flags() & 1 == 0 {
        return false;
    }
    let argument = &*inner_receiver.object_property_slot_unchecked(getter_cache.property_slot());
    if argument.value_type() != ValueType::Long || argument.is_reference() {
        return false;
    }
    let mut arguments = [0i64; 8];
    arguments[0] = argument.raw_long();
    if !try_execute_long_property_plan(outer_receiver, &arguments, outer_plan, outer_user) {
        return false;
    }

    record_scalar_call(inner_common);
    record_scalar_call(&outer_user.common);
    (*caller).opline = outer_do_ptr.add(1);
    true
}
