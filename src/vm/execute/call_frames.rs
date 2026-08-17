// Kept in the execute module through include! so this structural split does not change visibility or code generation.
/// Check if an exception value matches a catch clause's type list.
/// PHP 8 semantics: only Throwable objects can be thrown.
/// - `catch (Exception $e)` matches Exception and subclasses only
/// - `catch (Error $e)` matches Error and subclasses (TypeError, etc.) only
/// - `catch (Throwable $e)` matches both Error and Exception hierarchies
/// For objects: checks class hierarchy via class_is_a.
fn exception_matches_catch(thrown: &Value, types: &[String], eg: &ExecutorGlobals) -> bool {
    if types.is_empty() {
        return true; // no type constraint = catch all
    }
    if let Some(obj) = thrown.as_object() {
        for type_name in types {
            if eg.class_is_a(&obj.class_name, type_name) {
                return true;
            }
        }
    }
    false
}

/// Drop all heap-backed slot values in a frame before popping it.
///
/// Three-tier cleanup:
///   1. No heap values at all (has_heap_slots == false) → skip entirely
///   2. Bitmap-driven (total slots <= 64) → iterate only heap bits via trailing_zeros
///   3. Full scan fallback (total slots > 64) → scan all slots by value type
///
/// After dropping, zeros the slot so reused stack space sees Undef.
#[inline(always)]
pub(crate) unsafe fn cleanup_frame_slots(frame: *mut ExecuteData) {
    let num_cvs = (*frame).num_cvs as usize;
    let num_temps = (*frame).num_temps as usize;
    let total = num_cvs + num_temps;

    // Tier 1: no heap values written during this invocation.
    if !(*frame).has_heap_slots {
        stats::inc_cleanup_frame(total, true);
        return;
    }

    let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);

    // Tier 2: bitmap-driven — only drop slots with heap bit set.
    if total <= 64 {
        let bitmap = (*frame).owned_heap_bitmap();
        if bitmap == 0 {
            stats::inc_cleanup_frame(total, true);
            return;
        }
        stats::inc_cleanup_frame(total, false);
        for idx in HeapSlotIter::new(bitmap) {
            let ptr = base.add(idx as usize);
            std::ptr::drop_in_place(ptr);
            std::ptr::write_bytes(ptr as *mut u8, 0, std::mem::size_of::<Value>());
        }
        return;
    }

    // Tier 3: full scan fallback for large frames (> 64 slots).
    stats::inc_cleanup_frame(total, false);
    for i in 0..total {
        let ptr = base.add(i);
        #[cfg(not(feature = "resource-lifetime"))]
        match (*ptr).value_type() {
            ValueType::String | ValueType::Array | ValueType::Object | ValueType::Closure => {
                std::ptr::drop_in_place(ptr);
                std::ptr::write_bytes(ptr as *mut u8, 0, std::mem::size_of::<Value>());
            }
            _ => {}
        }
        #[cfg(feature = "resource-lifetime")]
        match (*ptr).value_type() {
            ValueType::String
            | ValueType::Array
            | ValueType::Object
            | ValueType::Resource
            | ValueType::Closure => {
                std::ptr::drop_in_place(ptr);
                std::ptr::write_bytes(ptr as *mut u8, 0, std::mem::size_of::<Value>());
            }
            _ => {}
        }
    }
}

/// Run user destructors for direct object handles whose remaining references
/// all belong to the frame that is about to be released. The ordinary scalar
/// path remains allocation-free; object counts are built only for frames that
/// actually own heap values.
#[cold]
fn run_frame_destructors(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
) -> Result<(), VmError> {
    // SAFETY: `frame` is the live activation being released. Its compiler-sized
    // CV/TMP range remains allocated until destructor dispatch completes.
    unsafe {
        if !(*frame).has_heap_slots {
            return Ok(());
        }

        let total = ((*frame).num_cvs + (*frame).num_temps) as usize;
        let base = (frame as *const Value).add(CALL_FRAME_SLOTS);
        let candidate_indices = if total <= 64 {
            HeapSlotIter::new((*frame).owned_heap_bitmap())
                .map(|index| index as usize)
                .collect::<Vec<_>>()
        } else {
            (0..total).collect()
        };
        let mut counts = HashMap::<usize, usize>::new();
        for &index in &candidate_indices {
            let value = &*base.add(index);
            if let Some(identity) = value.object_identity() {
                *counts.entry(identity).or_default() += 1;
            }
        }

        // Function frames release local handles in slot order. The root symbol
        // table shuts down in reverse insertion order. Preserve both orders
        // explicitly instead of inheriting randomized HashMap iteration.
        let mut identities = Vec::with_capacity(counts.len());
        let mut record_identity = |index: usize| {
            if let Some(identity) = (&*base.add(index)).object_identity()
                && !identities.contains(&identity)
            {
                identities.push(identity);
            }
        };
        if (*frame).prev_execute_data.is_null() {
            for &index in candidate_indices.iter().rev() {
                record_identity(index);
            }
        } else {
            for &index in &candidate_indices {
                record_identity(index);
            }
        }

        // A destructor may release the last non-frame handle of an object that
        // was ineligible earlier in the same pass. Revisit only those deferred
        // identities until a complete pass makes no progress.
        let mut pending = identities;
        loop {
            let mut deferred = Vec::new();
            let mut progressed = false;
            for identity in pending {
                let frame_references = counts[&identity];
                let representative = candidate_indices
                    .iter()
                    .map(|index| &*base.add(*index))
                    .find(|value| value.object_identity() == Some(identity));
                let Some(representative) = representative else {
                    continue;
                };
                let class_name = representative.object_class_name_unchecked().to_string();
                if eg.find_method_info(&class_name, "__destruct").is_none() {
                    continue;
                }
                if representative.object_strong_count() != Some(frame_references) {
                    deferred.push(identity);
                    continue;
                }
                if !representative.mark_object_destructed() {
                    continue;
                }
                let receiver = representative.clone();
                let _ = call_magic_method(eg, &receiver, "__destruct", &[])?;
                progressed = true;
            }
            if !progressed {
                break;
            }
            pending = deferred;
        }
    }
    Ok(())
}

#[cold]
fn release_statement_temps(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    first: usize,
    end: usize,
) -> Result<(), VmError> {
    // SAFETY: the compiler emits a bounded statement-temporary range inside
    // this live frame; ownership bits identify which slots may be dropped.
    unsafe {
        let total = ((*frame).num_cvs + (*frame).num_temps) as usize;
        debug_assert!(first <= end && end <= total);
        let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
        let compact = total <= 64;
        let bitmap = compact.then(|| (*frame).owned_heap_bitmap());
        let is_owned = |index: usize| {
            bitmap.map_or_else(
                || (*base.add(index)).needs_cleanup(),
                |bitmap| bitmap & (1u64 << index) != 0,
            )
        };

        let mut object_counts = HashMap::<usize, usize>::new();
        for index in first..end {
            if !is_owned(index) {
                continue;
            }
            let value = &*base.add(index);
            if let Some(identity) = value.object_identity() {
                *object_counts.entry(identity).or_default() += 1;
            }
        }
        let mut identities = Vec::with_capacity(object_counts.len());
        for index in first..end {
            if !is_owned(index) {
                continue;
            }
            if let Some(identity) = (&*base.add(index)).object_identity()
                && !identities.contains(&identity)
            {
                identities.push(identity);
            }
        }
        let mut pending = identities;
        loop {
            let mut deferred = Vec::new();
            let mut progressed = false;
            for identity in pending {
                let range_references = object_counts[&identity];
                let representative = (first..end)
                    .filter(|index| is_owned(*index))
                    .map(|index| &*base.add(index))
                    .find(|value| value.object_identity() == Some(identity));
                let Some(representative) = representative else {
                    continue;
                };
                let class_name = representative.object_class_name_unchecked().to_string();
                if eg.find_method_info(&class_name, "__destruct").is_none() {
                    continue;
                }
                if representative.object_strong_count() != Some(range_references) {
                    deferred.push(identity);
                    continue;
                }
                if !representative.mark_object_destructed() {
                    continue;
                }
                let receiver = representative.clone();
                let _ = call_magic_method(eg, &receiver, "__destruct", &[])?;
                progressed = true;
            }
            if !progressed {
                break;
            }
            pending = deferred;
        }

        for index in first..end {
            if !is_owned(index) {
                continue;
            }
            let value = base.add(index);
            std::ptr::drop_in_place(value);
            std::ptr::write_bytes(value as *mut u8, 0, std::mem::size_of::<Value>());
            if compact {
                (*frame).heap_bitmap &= !(1u64 << index);
            }
        }
    }
    Ok(())
}

#[inline(always)]
unsafe fn pop_call_storage(eg: &mut ExecutorGlobals, call: *mut ExecuteData) {
    eg.discard_late_static_scope(call as usize);
    eg.discard_dynamic_scope(call as usize);
    eg.end_error_suppression(call as usize);
    eg.finally_exceptions.remove(&(call as usize));
    if (*call).deferred_scalar_call {
        eg.pending_call_stack.pop_call_frame(call);
    } else {
        eg.vm_stack.pop_call_frame(call);
    }
}

#[cold]
#[inline(never)]
fn pop_vm_call_frame(eg: &mut ExecutorGlobals, call: *mut ExecuteData) {
    eg.discard_late_static_scope(call as usize);
    eg.discard_dynamic_scope(call as usize);
    eg.end_error_suppression(call as usize);
    eg.finally_exceptions.remove(&(call as usize));
    eg.function_arguments.remove(&(call as usize));
    eg.vm_stack.pop_call_frame(call);
}

/// Append one dynamically resolved `__invoke` receiver to the packed internal
/// stack stored in the pre-existing ExecutorGlobals side-state slot.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn push_pending_invoke_this(eg: &mut ExecutorGlobals, call_key: usize, receiver: Value) {
    let pending = eg
        .pending_invoke_this
        .get_or_insert_with(|| Value::array(PhpArray::with_packed_capacity(4)));
    let stack = pending
        .as_array_mut()
        .expect("pending invoke state must remain a packed array");
    stack.push(Value::long(call_key as i64));
    stack.push(receiver);
}

/// Pop the current dynamically resolved `__invoke` receiver without
/// disturbing an outer call whose argument expression is executing.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn take_pending_invoke_this(eg: &mut ExecutorGlobals, call_key: usize) -> Option<Value> {
    let matches_current = {
        let stack = eg.pending_invoke_this.as_ref()?.as_array()?;
        let key_index = stack.len().checked_sub(2)?;
        stack.get_value_at(key_index)?.as_long()? as usize == call_key
    };
    if !matches_current {
        return None;
    }

    let (receiver, empty) = {
        let stack = eg.pending_invoke_this.as_mut()?.as_array_mut()?;
        let receiver = stack.pop()?;
        let key = stack.pop()?;
        debug_assert_eq!(key.as_long().map(|key| key as usize), Some(call_key));
        (receiver, stack.is_empty())
    };
    if empty {
        eg.pending_invoke_this = None;
    }
    Some(receiver)
}

// The high bit belongs to the late-static-scope entry sharing this packed
// sidecar. Magic-call metadata uses the next disjoint non-pointer tag.
const PENDING_MAGIC_CALL_TAG: usize = 1usize << (usize::BITS - 2);

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn push_pending_magic_call(eg: &mut ExecutorGlobals, call_key: usize, method: Value) {
    debug_assert_eq!(call_key & PENDING_MAGIC_CALL_TAG, 0);
    push_pending_invoke_this(eg, call_key | PENDING_MAGIC_CALL_TAG, method);
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn take_pending_magic_call(eg: &mut ExecutorGlobals, call_key: usize) -> Option<Value> {
    take_pending_invoke_this(eg, call_key | PENDING_MAGIC_CALL_TAG)
}

/// Initialize the sparse argument ABI on the first named send. Keeping this
/// work out of `op_send_named` prevents a correctness-only cold path from
/// displacing the quick-dispatch working set.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn prepare_named_call_frame(
    eg: &mut ExecutorGlobals,
    call: *mut ExecuteData,
    func_common: &FunctionCommon,
    positional: u32,
) {
    // Dynamic object calls are compiled before the runtime knows that the
    // target is `__invoke`, so their positional prefix initially starts at CV
    // 0. Shift only that prefix; named destinations already include `$this`.
    let call_key = call as usize;
    if let Some(this_val) = take_pending_invoke_this(eg, call_key) {
        // SAFETY: `call` is the pending live activation selected by call_key;
        // its compiler-sized CV prefix contains every positional/source slot.
        unsafe {
            for index in (0..positional).rev() {
                let value = (*call).cv(index).clone_closure_capture();
                let destination = (*call).cv_mut(index + 1) as *mut Value;
                if index + 1 == positional {
                    frame_slot_init(call, destination, value);
                } else {
                    frame_slot_set(call, destination, value);
                }
            }
            let this_slot = (*call).cv_mut(0) as *mut Value;
            if positional == 0 {
                frame_slot_init(call, this_slot, this_val);
            } else {
                frame_slot_set(call, this_slot, this_val);
            }
        }
        // Keep the call on the full DoFcall path. Undef records that `$this`
        // has already been installed and the positional prefix already moved.
        push_pending_invoke_this(eg, call_key, Value::undef());
    }

    // `push_call_frame` leaves the source argument prefix uninitialized because
    // ordinary SendVal writes every slot. Named sends can leave holes, so keep
    // preceding positional values and make every remaining parameter readable.
    // SAFETY: signature-derived CV indices are within the same pending live
    // activation; each remaining named-argument hole is initialized once.
    unsafe {
        for public_index in positional..func_common.sig.public_arity() {
            let cv_index = func_common.sig.param_cv_index(public_index);
            let slot = (*call).cv_mut(cv_index) as *mut Value;
            slot.write(Value::undef());
        }
        (*call).named_args_used = true;
    }
}

/// Abandon every not-yet-executed call owned by `frame`. This is required when
/// an argument expression throws: Init has already linked the outer call, while
/// DoFcall will never consume it. The helper also fixes the same lifetime hole
/// for pre-existing ordinary pending frames.
unsafe fn cleanup_pending_calls(eg: &mut ExecutorGlobals, frame: *mut ExecuteData) {
    let mut call = (*frame).call;
    (*frame).call = std::ptr::null_mut();
    while !call.is_null() {
        let next = (*call).call;
        let call_key = call as usize;
        eg.pending_named_variadic.remove(&call_key);
        eg.pending_closure_captures.remove(&call_key);
        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        eg.discard_generic_member_call(call_key);
        let _ = take_pending_invoke_this(eg, call_key);
        let _ = take_pending_magic_call(eg, call_key);
        cleanup_frame_slots(call);
        pop_call_storage(eg, call);
        call = next;
    }
    #[cfg(feature = "php-generics-reified")]
    eg.discard_pending_reified_binding_scopes(frame as usize);
}

/// Clean up a pending call frame and throw a catchable exception.
/// Removes per-call side state, unlinks the call from the call chain, cleans up
/// CV/TMP slots, pops the call frame, and delegates to throw_in_frame.
///
/// SAFETY: `frame` and `call` must be valid ExecuteData pointers.
///         `call` must be the current pending call on `frame` (i.e. `(*frame).call == call`).
unsafe fn cleanup_call_and_throw<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    call: *mut ExecuteData,
    err: Value,
) -> ThrowResult<'a> {
    let call_key = call as usize;
    eg.pending_named_variadic.remove(&call_key);
    eg.pending_closure_captures.remove(&call_key);
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    eg.discard_generic_member_call(call_key);
    #[cfg(feature = "php-generics-reified")]
    {
        eg.discard_pending_reified_binding_scopes(frame as usize);
    }
    let _ = take_pending_invoke_this(eg, call_key);
    let _ = take_pending_magic_call(eg, call_key);
    (*frame).call = (*call).call;
    cleanup_frame_slots(call);
    pop_call_storage(eg, call);
    throw_in_frame(eg, frame, err)
}

/// Snapshot an exception trace while an internal call frame is still live.
/// Internal handlers execute synchronously and their frame is otherwise
/// released before the shared throw boundary sees the exception.
#[cold]
#[inline(never)]
fn attach_internal_call_trace_if_missing(
    throwable: &Value,
    call: *mut ExecuteData,
    caller: *mut ExecuteData,
    eg: &ExecutorGlobals,
) {
    let missing_trace = throwable.as_object().is_some_and(|object| {
        let has_origin = object
            .get_property("file")
            .and_then(Value::as_str)
            .is_some_and(|file| !file.is_empty())
            && object
                .get_property("line")
                .and_then(Value::as_long)
                .is_some_and(|line| line > 0);
        !has_origin
            && object
                .get_property("trace")
                .and_then(Value::as_array)
                .is_none_or(PhpArray::is_empty)
    });
    if !missing_trace {
        return;
    }
    let ignore_arguments = crate::stdlib::ini_default(eg, "zend.exception_ignore_args")
        .as_deref()
        .is_some_and(crate::stdlib::ini_boolean);
    let trace_options = if ignore_arguments { 2 } else { 0 };

    // SAFETY: call and caller are the linked, live synchronous frames passed
    // by DoFcall. The caller opline belongs to its immutable op-array and is
    // restored before either frame can execute or be released.
    let trace = unsafe {
        // collect_debug_backtrace expects a caller to point one instruction
        // past the active call so it can recover the call-site location.
        let caller_opline = (*caller).opline;
        let caller_op_array = (*caller).op_array();
        let caller_index = caller_opline.offset_from(caller_op_array.instructions.as_ptr());
        let can_advance = usize::try_from(caller_index)
            .ok()
            .filter(|index| *index < caller_op_array.instructions.len())
            .is_some();
        if can_advance {
            (*caller).opline = caller_opline.add(1);
        }
        let trace = crate::stdlib::collect_debug_backtrace(call, trace_options, 0, eg, true);
        if can_advance {
            (*caller).opline = caller_opline;
        }
        trace
    };
    if let Some(mut object) = throwable.as_object_mut() {
        object.set_property("trace", Value::array(trace));
    }
}

/// Call a magic method on an object.
/// Looks up `classname::method_name` in the function table and, if found,
/// pushes a temporary call frame, executes it, and returns the result.
/// `obj_val` must be an Object value (caller ensures this).
/// `args` are the explicit arguments to pass (excluding $this).
fn call_magic_method(
    eg: &mut ExecutorGlobals,
    obj_val: &Value,
    method_name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class_name = {
        let obj = obj_val.as_object().unwrap();
        obj.class_name.clone()
    };
    let full_name = format!("{}::{}", class_name.to_lowercase(), method_name);
    let func_ptr = match eg.find_function(&full_name) {
        Some(ptr) => ptr,
        None => return Ok(None),
    };

    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(obj_val.clone());
    call_args.extend_from_slice(args);
    Ok(Some(call_function(eg, func_ptr, &call_args)?))
}

const PROPERTY_GUARD_GET: u8 = 1;
const PROPERTY_GUARD_SET: u8 = 1 << 1;
const PROPERTY_GUARD_ISSET: u8 = 1 << 2;
const PROPERTY_GUARD_UNSET: u8 = 1 << 3;

#[inline]
fn property_guard_active(object: &Value, name: &str, operation: u8) -> bool {
    object
        .as_object()
        .is_some_and(|object| object.property_guard_active(name, operation))
}

#[inline]
fn set_property_guard(object: &Value, name: &str, operation: u8, active: bool) {
    if let Some(mut object) = object.as_object_mut() {
        object.set_property_guard(name, operation, active);
    }
}

/// Invoke one guarded magic-property operation and always release its guard,
/// including when the user method throws or the VM reports an execution error.
fn call_guarded_property_magic_method(
    eg: &mut ExecutorGlobals,
    object: &Value,
    name: &str,
    operation: u8,
    method: &str,
    arguments: &[Value],
) -> Result<Option<Value>, VmError> {
    // The user method may rebind the CV/global slot from which `object` was
    // borrowed. Retain the receiver before setting the guard so re-entrant
    // writes cannot turn the borrowed slot into a scalar underneath the
    // follow-up call or guard cleanup.
    let receiver = object.clone();
    if property_guard_active(&receiver, name, operation) {
        return Ok(None);
    }
    set_property_guard(&receiver, name, operation, true);
    let result = call_magic_method(eg, &receiver, method, arguments);
    set_property_guard(&receiver, name, operation, false);
    result
}

/// Reuse PHP object string conversion from internal handlers.
pub(crate) fn call_object_string_conversion(
    eg: &mut ExecutorGlobals,
    object: &Value,
) -> Result<Option<Value>, VmError> {
    call_magic_method(eg, object, "__tostring", &[])
}

/// Execute a top-level script.
/// Result of throw_in_frame: either the exception was handled (new frame + op_array)
/// or it was not and should propagate via eg.exception.
enum ThrowResult<'a> {
    Handled(*mut ExecuteData, &'a crate::compiler::OpArray),
    Unhandled(Value),
}

/// Append an exception displaced by an escaping finally failure to the tail
/// of the new Throwable's explicit previous chain. PHP preserves an explicitly
/// supplied previous value first and adds the displaced exception after it.
#[cold]
fn append_replaced_exception(
    thrown: &Value,
    displaced: &Value,
    eg: &ExecutorGlobals,
) {
    let Some(displaced_identity) = displaced.object_identity() else {
        return;
    };
    if !displaced.as_object().is_some_and(|object| {
        eg.class_is_a(&object.class_name, "Throwable")
    }) {
        return;
    }
    let Some(thrown_identity) = thrown.object_identity() else {
        return;
    };
    // Do not create a cycle when the displaced exception already names the
    // newly escaping Throwable somewhere in its explicit previous chain.
    let mut probe = displaced.clone();
    let mut probed = std::collections::HashSet::new();
    loop {
        let Some(identity) = probe.object_identity() else {
            break;
        };
        if identity == thrown_identity {
            return;
        }
        if !probed.insert(identity) {
            break;
        }
        let Some(object) = probe.as_object() else {
            break;
        };
        let class_name = object.class_name.to_string();
        let previous_key = eg
            .find_property_visibility(&class_name, "previous")
            .map_or_else(
                || "previous".to_string(),
                |(_, declaring_class)| {
                    crate::runtime::mangle_private_prop(&declaring_class, "previous")
                },
            );
        let previous = object
            .get_property(&previous_key)
            .filter(|value| {
                value.as_object().is_some_and(|previous| {
                    eg.class_is_a(&previous.class_name, "Throwable")
                })
            })
            .cloned();
        drop(object);
        let Some(previous) = previous else {
            break;
        };
        probe = previous;
    }
    let mut current = thrown.clone();
    let mut seen = std::collections::HashSet::new();
    loop {
        let Some(identity) = current.object_identity() else {
            return;
        };
        if identity == displaced_identity || !seen.insert(identity) {
            return;
        }
        let Some(object) = current.as_object() else {
            return;
        };
        let class_name = object.class_name.to_string();
        let previous_key = eg
            .find_property_visibility(&class_name, "previous")
            .map_or_else(
                || "previous".to_string(),
                |(_, declaring_class)| {
                    crate::runtime::mangle_private_prop(&declaring_class, "previous")
                },
            );
        let previous = object
            .get_property(&previous_key)
            .filter(|value| {
                value.as_object().is_some_and(|previous| {
                    eg.class_is_a(&previous.class_name, "Throwable")
                })
            })
            .cloned();
        drop(object);
        if let Some(previous) = previous {
            current = previous;
            continue;
        }
        if let Some(mut object) = current.as_object_mut() {
            object.set_property(&previous_key, displaced.clone());
        }
        return;
    }
}

/// Attach the immutable creation/raise origin that PHP exposes through
/// Throwable::getFile()/getLine(). Existing metadata wins so rethrowing an
/// object never moves its origin. The trace is captured at the same creation
/// site and therefore also survives a later throw or rethrow unchanged.
fn attach_throwable_origin(
    throwable: &Value,
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    instruction_index: usize,
) {
    if throwable.as_object().is_some_and(|object| {
        object
            .get_property("file")
            .and_then(Value::as_str)
            .is_some_and(|file| !file.is_empty())
            && object
                .get_property("line")
                .and_then(Value::as_long)
                .is_some_and(|line| line > 0)
    }) {
        return;
    }
    let ignore_arguments = crate::stdlib::ini_default(eg, "zend.exception_ignore_args")
        .as_deref()
        .is_some_and(crate::stdlib::ini_boolean);
    let trace_options = if ignore_arguments { 2 } else { 0 };
    // SAFETY: opcode dispatch keeps the complete synchronous frame chain live
    // for the duration of this cold metadata snapshot. A compiler-synthesized
    // implicit Return has no source line, so its still-live caller provides
    // the observable raise location without changing the captured frame chain.
    let (origin_op_array, line, trace) = unsafe {
        let mut origin_op_array = op_array;
        let mut origin_index = instruction_index;
        if origin_op_array.source_line(origin_index).is_none()
            && origin_op_array.instructions.get(origin_index).is_some_and(|instruction| {
                instruction.opcode == OpCode::Return && instruction.extended_value == 0
            })
        {
            let caller = (*frame).prev_execute_data;
            if !caller.is_null() {
                let caller_op_array = (*caller).op_array();
                let caller_ip = (*caller)
                    .opline
                    .offset_from(caller_op_array.instructions.as_ptr())
                    as usize;
                if let Some(caller_origin) = caller_op_array
                    .instructions
                    .len()
                    .checked_sub(1)
                    .and_then(|last| (0..=caller_ip.min(last))
                        .rev()
                        .find(|index| caller_op_array.source_line(*index).is_some()))
                {
                    origin_op_array = caller_op_array;
                    origin_index = caller_origin;
                }
            }
        }
        let Some(line) = origin_op_array.source_line(origin_index) else {
            return;
        };
        if origin_op_array.source_file.is_empty() {
            return;
        }
        let trace = crate::stdlib::collect_debug_backtrace(frame, trace_options, 0, eg, true);
        (origin_op_array, line, trace)
    };
    let Some(mut object) = throwable.as_object_mut() else {
        return;
    };
    if object
        .get_property("file")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        object.set_property(
            "file",
            Value::shared_string(origin_op_array.source_file.clone()),
        );
    }
    if object
        .get_property("line")
        .and_then(Value::as_long)
        .is_none_or(|line| line <= 0)
    {
        object.set_property("line", Value::long(line as i64));
    }
    if !object.contains_property("trace") {
        object.set_property("trace", Value::array(trace));
    }
}

/// Argument verification happens while the callee frame is pending. PHP
/// exposes the declaration as the Throwable origin but retains that pending
/// call as frame zero of the trace, so snapshot both before releasing it.
fn attach_argument_type_error_origin(
    throwable: &Value,
    source_file: std::rc::Rc<String>,
    declaration_line: usize,
    mut trace: PhpArray,
    caller_op_array: &crate::compiler::OpArray,
    call_instruction: &Instruction,
) {
    let call_index = caller_op_array
        .instructions
        .iter()
        .position(|instruction| std::ptr::eq(instruction, call_instruction));
    if let Some(call_line) = call_index.and_then(|index| caller_op_array.source_line(index))
        && !caller_op_array.source_file.is_empty()
        && let Some(mut first) = trace.get_value_at(0).cloned()
        && let Some(entry) = first.as_array_mut()
    {
        entry.set_str(
            "file",
            Value::shared_string(caller_op_array.source_file.clone()),
        );
        entry.set_str("line", Value::long(call_line as i64));
        trace.set_int(0, first);
    }
    let Some(mut object) = throwable.as_object_mut() else {
        return;
    };
    object.set_property("file", Value::shared_string(source_file));
    object.set_property("line", Value::long(declaration_line as i64));
    object.set_property("trace", Value::array(trace));
}

/// Walk frames starting from `frame` looking for a try/catch handler for `thrown`.
/// On success: unwinds frames and returns the handler frame + op_array.
/// On failure: returns Unhandled with the original exception value.
fn throw_in_frame<'a>(
    eg: &mut ExecutorGlobals,
    mut frame: *mut ExecuteData,
    thrown: Value,
) -> ThrowResult<'a> {
    // Runtime helpers commonly construct Error/TypeError immediately before
    // entering this shared throw boundary. Stamp that first raise site here so
    // every catchable runtime error exposes the same immutable file, line and
    // trace metadata as an explicit `throw`. Existing metadata wins inside the
    // helper, which preserves the original site for rethrows and exceptions
    // propagating out of a callee.
    // SAFETY: `frame` is the live frame entering the shared throw boundary;
    // its opline points into the immutable instruction slice of its op-array.
    // SAFETY: every traversed pointer belongs to the live caller chain rooted
    // at `frame`; the chain remains allocated for the whole unwind search.
    let (origin_op_array, origin_ip, displaced_exception) = unsafe {
        let origin_op_array = (*frame).op_array();
        let origin_ip =
        (*frame)
            .opline
            .offset_from(origin_op_array.instructions.as_ptr()) as usize;
        let mut pending_owner = frame;
        let displaced_exception = loop {
            if let Some(pending) = eg
                .finally_exceptions
                .get(&(pending_owner as usize))
                .and_then(|pending| pending.last())
                .filter(|pending| pending.object_identity() != thrown.object_identity())
            {
                break Some((pending_owner, pending.clone()));
            }
            let previous = (*pending_owner).prev_execute_data;
            if previous.is_null() {
                break None;
            }
            pending_owner = previous;
        };
        (origin_op_array, origin_ip, displaced_exception)
    };
    attach_throwable_origin(&thrown, eg, frame, origin_op_array, origin_ip);

    let mut search_frame = frame;
    loop {
        let sf_op_array = unsafe { (*search_frame).op_array() };
        // An exception raised while a finally block is completing replaces a
        // pending goto/break continuation in that frame.
        finally_jump_state(search_frame, sf_op_array, FINALLY_JUMP_CLEAR, 0, false);
        let current_ip = unsafe {
            (*search_frame)
                .opline
                .offset_from(sf_op_array.instructions.as_ptr()) as u32
        };

        let mut matched_entry: Option<&crate::compiler::compile::TryEntry> = None;
        for entry in &sf_op_array.try_entries {
            if current_ip >= entry.try_start && current_ip < entry.try_end {
                matched_entry = Some(entry);
                break;
            }
        }

        if let Some(entry) = matched_entry {
            let matched_catch = entry
                .catches
                .iter()
                .find(|c| exception_matches_catch(&thrown, &c.types, eg));

            if let Some(catch) = matched_catch {
                if let Some((owner, displaced)) = displaced_exception.as_ref() {
                    let catch_stays_in_finally = search_frame == *owner
                        && sf_op_array.try_entries.iter().any(|active| {
                            active.finally_start != u32::MAX
                                && catch.catch_start >= active.finally_start
                                && catch.catch_start < active.finally_end
                        });
                    if !catch_stays_in_finally {
                        append_replaced_exception(&thrown, displaced, eg);
                        if let Some(pending) = eg.finally_exceptions.get_mut(&(*owner as usize)) {
                            pending.pop();
                            if pending.is_empty() {
                                eg.finally_exceptions.remove(&(*owner as usize));
                            }
                        }
                    }
                }
                while frame != search_frame {
                    let prev = unsafe { (*frame).prev_execute_data };
                    eg.current_execute_data.set(prev);
                    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                    eg.discard_generic_member_call(frame as usize);
                    #[cfg(feature = "php-generics-reified")]
                    {
                        eg.discard_active_reified_binding_scope(frame as usize);
                    }
                    unsafe {
                        cleanup_pending_calls(eg, frame);
                        cleanup_frame_slots(frame);
                    };
                    pop_vm_call_frame(eg, frame);
                    frame = prev;
                }
                unsafe { cleanup_pending_calls(eg, search_frame) };
                let base_ptr = sf_op_array.instructions.as_ptr();
                if let Some(catch_cv) = catch.catch_cv {
                    let catch_cv_ptr =
                        unsafe { (*search_frame).get_op_mut(catch_cv, OpType::Cv) };
                    unsafe { slot_set(catch_cv_ptr, thrown.clone()) };
                }
                unsafe { (*frame).opline = base_ptr.add(catch.catch_start as usize) };
                let new_op_array = unsafe { (*frame).op_array() };
                return ThrowResult::Handled(frame, new_op_array);
            } else if entry.finally_start != 0xFFFFFFFF {
                while frame != search_frame {
                    let prev = unsafe { (*frame).prev_execute_data };
                    eg.current_execute_data.set(prev);
                    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                    eg.discard_generic_member_call(frame as usize);
                    #[cfg(feature = "php-generics-reified")]
                    {
                        eg.discard_active_reified_binding_scope(frame as usize);
                    }
                    unsafe {
                        cleanup_pending_calls(eg, frame);
                        cleanup_frame_slots(frame);
                    };
                    pop_vm_call_frame(eg, frame);
                    frame = prev;
                }
                unsafe { cleanup_pending_calls(eg, search_frame) };
                let base_ptr = sf_op_array.instructions.as_ptr();
                if let Some((owner, displaced)) = displaced_exception.as_ref() {
                    append_replaced_exception(&thrown, displaced, eg);
                    if let Some(pending) = eg.finally_exceptions.get_mut(&(*owner as usize)) {
                        pending.pop();
                        if pending.is_empty() {
                            eg.finally_exceptions.remove(&(*owner as usize));
                        }
                    }
                }
                eg.finally_exceptions
                    .entry(frame as usize)
                    .or_default()
                    .push(thrown.clone());
                unsafe { (*frame).opline = base_ptr.add(entry.finally_start as usize) };
                let new_op_array = unsafe { (*frame).op_array() };
                return ThrowResult::Handled(frame, new_op_array);
            }
        }

        let prev = unsafe { (*search_frame).prev_execute_data };
        if prev.is_null() {
            break;
        }
        search_frame = prev;
    }

    if let Some((owner, displaced)) = displaced_exception.as_ref() {
        append_replaced_exception(&thrown, displaced, eg);
        if let Some(pending) = eg.finally_exceptions.get_mut(&(*owner as usize)) {
            pending.pop();
            if pending.is_empty() {
                eg.finally_exceptions.remove(&(*owner as usize));
            }
        }
    }
    ThrowResult::Unhandled(thrown)
}
