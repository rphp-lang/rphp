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
    eg.end_error_suppression(call as usize);
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
    eg.end_error_suppression(call as usize);
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
        for index in (0..positional).rev() {
            let value = unsafe { (*call).cv(index).clone() };
            let destination = unsafe { (*call).cv_mut(index + 1) } as *mut Value;
            unsafe { frame_slot_set(call, destination, value) };
        }
        let this_slot = unsafe { (*call).cv_mut(0) } as *mut Value;
        unsafe { frame_slot_set(call, this_slot, this_val) };
        // Keep the call on the full DoFcall path. Undef records that `$this`
        // has already been installed and the positional prefix already moved.
        push_pending_invoke_this(eg, call_key, Value::undef());
    }

    // `push_call_frame` leaves the source argument prefix uninitialized because
    // ordinary SendVal writes every slot. Named sends can leave holes, so keep
    // preceding positional values and make every remaining parameter readable.
    for public_index in positional..func_common.sig.public_arity() {
        let cv_index = func_common.sig.param_cv_index(public_index);
        let slot = unsafe { (*call).cv_mut(cv_index) } as *mut Value;
        unsafe { slot.write(Value::undef()) };
    }
    unsafe { (*call).named_args_used = true };
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
    (*frame).call = (*call).call;
    cleanup_frame_slots(call);
    pop_call_storage(eg, call);
    throw_in_frame(eg, frame, err)
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

    let func_common = unsafe { &*func_ptr };
    if func_common.fn_type != FunctionType::User {
        return Ok(None);
    }

    let user = unsafe { &*(func_ptr as *const UserFunction) };
    let num_explicit_args = args.len() as u32;

    // Push a call frame: +1 for $this at CV 0
    let call = eg.vm_stack.push_call_frame(
        func_ptr,
        num_explicit_args + 1,
        num_explicit_args,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    let mut return_value = Value::null();
    unsafe {
        (*call).return_value = &mut return_value;
        (*call).opline = user.op_array.instructions.as_ptr();
        // Write $this directly — cleanup handles it separately.
        frame_set_this(call, obj_val.clone());
        // Set arguments in CV slots starting at 1 (after $this)
        // These are fresh uninitialized slots (within num_args range), use init.
        for (i, arg) in args.iter().enumerate() {
            let cv = (*call).cv_mut(1 + i as u32);
            frame_slot_init(call, cv as *mut Value, arg.clone());
        }
    }

    let saved_execute_data = eg.current_execute_data.get();
    eg.current_execute_data.set(call);
    let result = execute_ex(eg, call);
    eg.current_execute_data.set(saved_execute_data);

    if result.is_ok() {
        run_frame_destructors(eg, call)?;
    }
    unsafe { cleanup_frame_slots(call) };
    pop_vm_call_frame(eg, call);

    match result {
        Ok(()) => Ok(Some(return_value)),
        Err(e) => Err(e),
    }
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
    if property_guard_active(object, name, operation) {
        return Ok(None);
    }
    set_property_guard(object, name, operation, true);
    let result = call_magic_method(eg, object, method, arguments);
    set_property_guard(object, name, operation, false);
    result
}

/// Reuse PHP object string conversion from feature-only internal handlers.
#[cfg(any(feature = "stream-line", feature = "stream-registry"))]
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

/// Attach the immutable creation/raise origin that PHP exposes through
/// Throwable::getFile()/getLine(). Existing metadata wins so rethrowing an
/// object never moves its origin. An empty trace is recorded only for a root
/// creation site; nested frame rendering stays disabled until call-site source
/// locations can produce the complete PHP trace rather than a plausible fake.
fn attach_throwable_origin(
    throwable: &Value,
    op_array: &crate::compiler::OpArray,
    instruction_index: usize,
    is_root_frame: bool,
) {
    let Some(line) = op_array.source_line(instruction_index) else {
        return;
    };
    if op_array.source_file.is_empty() {
        return;
    }
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
    let Some(mut object) = throwable.as_object_mut() else {
        return;
    };
    if object
        .get_property("file")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        object.set_property("file", Value::shared_string(op_array.source_file.clone()));
    }
    if object
        .get_property("line")
        .and_then(Value::as_long)
        .is_none_or(|line| line <= 0)
    {
        object.set_property("line", Value::long(line as i64));
    }
    if is_root_frame && !object.contains_property("trace") {
        object.set_property("trace", Value::array(PhpArray::new()));
    }
}

/// Walk frames starting from `frame` looking for a try/catch handler for `thrown`.
/// On success: unwinds frames and returns the handler frame + op_array.
/// On failure: returns Unhandled with the original exception value.
fn throw_in_frame<'a>(
    eg: &mut ExecutorGlobals,
    mut frame: *mut ExecuteData,
    thrown: Value,
) -> ThrowResult<'a> {
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
                eg.exception = Some(thrown.clone());
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

    ThrowResult::Unhandled(thrown)
}
