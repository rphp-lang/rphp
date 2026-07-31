use std::collections::HashMap;
use std::sync::atomic::Ordering;
#[cfg(feature = "vm-stats")]
use std::sync::OnceLock;

use crate::value::{Value, PhpArray, PhpClosure, PhpObject, ArrayKey, ValueType, make_error_value};
use crate::runtime::ExecutorGlobals;
use crate::parser::Visibility;
use crate::vm::stats;
use super::opcode::OpCode;
use super::instruction::{Instruction, OpType};
use super::frame::{ExecuteData, HeapSlotIter, CALL_FRAME_SLOTS};
use super::function::{FunctionCommon, FunctionType, UserFunction, CallStrategy, ReturnStrategy, ParamTypeHint, HotStatus, FUNC_HOT_THRESHOLD};
use super::quick::{
    QuickArrayIndex, QuickIncrementKind, QuickLongAccumulateLoop, QuickLongBound, QuickLongCondition,
    QuickLongInductionLoop, QuickLongOp, QuickLongOperand, QuickLongOpsLoop, QuickLongTarget,
    QuickLongTerm,
    QUICK_LOOP_COUNTER_STRIDE, QUICK_LOOP_DISABLED, QUICK_LOOP_FAILURE_LIMIT,
    QUICK_LOOP_HOT_THRESHOLD, QUICK_STRING_FETCH_CACHE_LIMIT,
};
// Planner module is kept as scaffolding for future hot-executor architecture.
// Not used in baseline dispatch loop — will be integrated via function-entry dispatch.

#[inline(always)]
fn direct_user_calls_enabled() -> bool {
    #[cfg(feature = "vm-stats")]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("RPHP_DISABLE_DIRECT_USER_CALLS").is_none())
    }
    #[cfg(not(feature = "vm-stats"))]
    {
        true
    }
}

/// Get the current caller's **lexical** (declaring) class name from the frame.
/// Uses the `method_declaring_class` map on EG rather than runtime $this,
/// so that `private` checks use the class that defines the code, not the
/// dynamic receiver.  Returns None if in top-level code or a plain function.
#[inline]
fn get_caller_class(frame: *mut ExecuteData, eg: &ExecutorGlobals) -> Option<String> {
    if frame.is_null() {
        return None;
    }
    let func = unsafe { (*frame).func };
    if func.is_null() {
        return None;
    }
    eg.declaring_class_of(func).map(|s| s.to_string())
}

/// Check a value against a parameter type hint. Returns true if the value satisfies the hint.
/// Check a value against a type hint.
/// `callee_class`: the declaring class of the function whose hint is being checked.
/// Used to resolve `self`, `parent`, `static` pseudo-types.
/// Pass `None` for global functions.
fn check_type_hint(val: &Value, hint: &crate::vm::function::ParamTypeHint, eg: &ExecutorGlobals, strict: bool, callee_class: Option<&str>) -> bool {
    use crate::vm::function::ParamTypeHint;
    match hint {
        ParamTypeHint::None => true,
        ParamTypeHint::Int => val.value_type() == ValueType::Long,
        ParamTypeHint::Float => {
            if strict {
                val.value_type() == ValueType::Double
            } else {
                matches!(val.value_type(), ValueType::Double | ValueType::Long)
            }
        }
        ParamTypeHint::String => val.value_type() == ValueType::String,
        ParamTypeHint::Bool => matches!(val.value_type(), ValueType::True | ValueType::False),
        ParamTypeHint::Array => val.value_type() == ValueType::Array,
        ParamTypeHint::Callable => {
            // Simplified: string (function name), array [obj, method], or closure
            matches!(val.value_type(), ValueType::String | ValueType::Array)
        }
        ParamTypeHint::ClassName(class_name) => {
            if let Some(obj) = val.as_object() {
                // Resolve `self`, `parent`, `static` pseudo-types using callee's declaring class
                let resolved = match class_name.as_str() {
                    "self" | "static" => {
                        callee_class.unwrap_or(class_name.as_str())
                    }
                    "parent" => {
                        if let Some(decl) = callee_class {
                            if let Some(class_def) = eg.class_table.get(decl) {
                                class_def.parent.as_deref().unwrap_or(class_name.as_str())
                            } else {
                                class_name.as_str()
                            }
                        } else {
                            class_name.as_str()
                        }
                    }
                    _ => class_name.as_str(),
                };
                eg.class_is_a(&obj.class_name, resolved)
            } else {
                false
            }
        }
        ParamTypeHint::Nullable(inner) => {
            if val.value_type() == ValueType::Null {
                true
            } else {
                check_type_hint(val, inner, eg, strict, callee_class)
            }
        }
        ParamTypeHint::Void => false,
        ParamTypeHint::Mixed => true,
        ParamTypeHint::Never => false,
        ParamTypeHint::Union(types) => {
            types.iter().any(|t| check_type_hint(val, t, eg, strict, callee_class))
        }
    }
}

/// VM error — replaces panic! in all runtime paths
#[derive(Debug)]
pub enum VmError {
    Fatal(String),
    UnimplementedOpcode(OpCode),
    /// `exit($code)` / `die($msg)` — clean script termination.
    Exit(i32),
}

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
// mark_caller_heap_return: Propagate heap flag to caller frame.

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
        if heap { (*frame).heap_bitmap |= bit; } else { (*frame).heap_bitmap &= !bit; }
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
unsafe fn frame_tmp_set_long(frame: *mut ExecuteData, ptr: *mut Value, v: i64) {
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

/// Overwrite a frame slot (CV or TMP). Per-slot drop via bitmap when heap present.
#[inline(always)]
unsafe fn frame_slot_set(frame: *mut ExecuteData, ptr: *mut Value, val: Value) {
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
/// FastScalar callee cannot require a reference.
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
            common.plan.call == CallStrategy::FastScalar
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
) -> usize {
    let mut bound = 0usize;
    while bound < max_args as usize {
        let send = &*next;
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || !try_send_scalar_method_arg(frame, call, op_array, send)
        {
            break;
        }
        bound += 1;
        next = next.add(1);
    }
    bound
}

/// Propagate heap-backed return values into the caller's cleanup bookkeeping.
///
/// SAFETY: `return_value` must point into the caller's frame slot area.
/// This is guaranteed today because the compiler always emits DoFcall with
/// result_type = Tmp/Var. If the optimizer ever writes return values into
/// a dereferenced CV reference (outside frame), the bitmap update would be
/// unsound. The debug_assert below guards against this.
#[inline(always)]
unsafe fn mark_caller_heap_return(frame: *mut ExecuteData, val: &Value) {
    if val.needs_cleanup() {
        let prev = (*frame).prev_execute_data;
        if !prev.is_null() {
            (*prev).has_heap_slots = true;
            let return_ptr = (*frame).return_value;
            if !return_ptr.is_null() {
                let total = (*prev).num_cvs + (*prev).num_temps;
                if total <= 64 {
                    let idx = slot_idx(prev, return_ptr);
                    debug_assert!(
                        (idx as u32) < total,
                        "mark_caller_heap_return: return_value slot idx {} out of bounds (total={})",
                        idx, total
                    );
                    (*prev).heap_bitmap |= 1u64 << idx;
                }
            }
        }
    }
}

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
        let bitmap = (*frame).heap_bitmap;
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
        match (*ptr).value_type() {
            ValueType::String | ValueType::Array | ValueType::Object | ValueType::Closure => {
                std::ptr::drop_in_place(ptr);
                std::ptr::write_bytes(ptr as *mut u8, 0, std::mem::size_of::<Value>());
            }
            _ => {}
        }
    }
}

/// Clean up a pending call frame and throw a catchable exception.
/// Removes pending_named_variadic entries, unlinks the call from the call chain,
/// cleans up CV/TMP slots, pops the call frame, and delegates to throw_in_frame.
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
    (*frame).call = (*call).call;
    cleanup_frame_slots(call);
    eg.vm_stack.pop_call_frame(call);
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
    let call = eg.vm_stack.push_call_frame(func_ptr, num_explicit_args + 1);
    let mut return_value = Value::null();
    unsafe {
        (*call).num_args = num_explicit_args;
        (*call).return_value = &mut return_value;
        (*call).prev_execute_data = std::ptr::null_mut();
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

    match result {
        Ok(()) => Ok(Some(return_value)),
        Err(e) => Err(e),
    }
}

/// Execute a top-level script.
/// Result of throw_in_frame: either the exception was handled (new frame + op_array)
/// or it was not and should propagate via eg.exception.
enum ThrowResult<'a> {
    Handled(*mut ExecuteData, &'a crate::compiler::OpArray),
    Unhandled(Value),
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
        let current_ip = unsafe {
            (*search_frame).opline.offset_from(sf_op_array.instructions.as_ptr()) as u32
        };

        let mut matched_entry: Option<&crate::compiler::compile::TryEntry> = None;
        for entry in &sf_op_array.try_entries {
            if current_ip >= entry.try_start && current_ip < entry.try_end {
                matched_entry = Some(entry);
                break;
            }
        }

        if let Some(entry) = matched_entry {
            let matched_catch = entry.catches.iter().find(|c| {
                exception_matches_catch(&thrown, &c.types, eg)
            });

            if let Some(catch) = matched_catch {
                while frame != search_frame {
                    let prev = unsafe { (*frame).prev_execute_data };
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                }
                let base_ptr = sf_op_array.instructions.as_ptr();
                let catch_cv_ptr = unsafe { (*search_frame).get_op_mut(catch.catch_cv, OpType::Cv) };
                unsafe { slot_set(catch_cv_ptr, thrown.clone()) };
                unsafe { (*frame).opline = base_ptr.add(catch.catch_start as usize) };
                let new_op_array = unsafe { (*frame).op_array() };
                return ThrowResult::Handled(frame, new_op_array);
            } else if entry.finally_start != 0xFFFFFFFF {
                while frame != search_frame {
                    let prev = unsafe { (*frame).prev_execute_data };
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                }
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

pub fn execute(eg: &mut ExecutorGlobals, main_func: &UserFunction) -> Result<Value, VmError> {
    let func_ptr = &main_func.common as *const FunctionCommon;
    let frame = eg.vm_stack.push_call_frame(func_ptr, 0);

    let mut return_value = Value::null();
    unsafe {
        (*frame).return_value = &mut return_value;
        (*frame).opline = main_func.op_array.instructions.as_ptr();
        (*frame).prev_execute_data = eg.current_execute_data.get();
    }
    eg.current_execute_data.set(frame);

    execute_ex(eg, frame)?;

    #[cfg(debug_assertions)]
    super::hot::dump_bail_stats();

    eg.current_execute_data.set(unsafe { (*frame).prev_execute_data });
    unsafe { cleanup_frame_slots(frame) };
    eg.vm_stack.pop_call_frame(frame);

    // Check for uncaught exception that propagated through execute_ex
    if let Some(exc) = eg.exception.take() {
        let (class_name, message) = if let Some(obj) = exc.as_object() {
            let cls = obj.class_name.clone();
            let msg = obj.get_property("message")
                .map(|v| v.echo_to_string())
                .unwrap_or_default();
            (cls, msg)
        } else {
            ("Exception".to_string(), exc.echo_to_string())
        };
        return Err(VmError::Fatal(format!("Uncaught {}: {}", class_name, message)));
    }

    Ok(return_value)
}

/// Call a PHP function by FunctionCommon pointer with given arguments.
/// Used by stdlib functions like array_map/array_filter for callback invocation.
pub fn call_function(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    args: &[Value],
) -> Result<Value, VmError> {
    if unsafe { (*func_ptr).fn_type } == FunctionType::Internal {
        let internal = unsafe {
            &*(func_ptr as *const super::function::InternalFunction)
        };
        if let Some(handler) = internal.direct_handler {
            let common = &internal.common;
            let arity_ok = args.len() >= common.sig.required_num_args as usize
                && (common.sig.is_variadic || args.len() <= common.sig.public_arity() as usize);
            if arity_ok {
                return handler(args);
            }
        }
    }
    call_function_iter(eg, func_ptr, args.len(), args.iter())
}

/// Call a PHP function from borrowed arguments without first materializing an
/// intermediate `Vec<Value>`. Each value is cloned exactly once, directly into
/// its destination CV slot in the new call frame.
pub fn call_function_iter<'a, I>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
) -> Result<Value, VmError>
where
    I: Iterator<Item = &'a Value>,
{
    let (return_value, _) =
        call_function_value_iter::<_, false>(eg, func_ptr, num_args, args.cloned())?;
    Ok(return_value)
}

/// Call a PHP function from owned arguments, moving every value directly into
/// the new frame. This is used after named-argument normalization and by
/// callback consumers that already own their temporary arguments.
pub fn call_function_owned_iter<I>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
) -> Result<Value, VmError>
where
    I: Iterator<Item = Value>,
{
    let (return_value, _) =
        call_function_value_iter::<_, false>(eg, func_ptr, num_args, args)?;
    Ok(return_value)
}

/// Shared callback invocation path. `READBACK_ARG0` keeps the ordinary path
/// free of the extra first-public-argument clone required by `array_walk`.
fn call_function_value_iter<I, const READBACK_ARG0: bool>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    mut args: I,
) -> Result<(Value, Option<Value>), VmError>
where
    I: Iterator<Item = Value>,
{
    let saved_execute_data = eg.current_execute_data.get();
    let frame = eg.vm_stack.push_call_frame(func_ptr, num_args as u32);
    let mut return_value = Value::null();

    unsafe {
        (*frame).return_value = &mut return_value;
        // prev=null so Return exits execute_ex instead of continuing in caller
        (*frame).prev_execute_data = std::ptr::null_mut();
        (*frame).num_args = num_args as u32;
    }

    // Write args into CV slots — fresh uninitialized slots, use init (no drop).
    for i in 0..num_args {
        let arg = args
            .next()
            .expect("callback argument iterator shorter than declared length");
        unsafe { callback_arg_init(frame, i, arg) };
    }
    debug_assert!(
        args.next().is_none(),
        "callback argument iterator longer than declared length"
    );

    let execution_result = match unsafe { (*func_ptr).fn_type } {
        FunctionType::User => {
            let user = unsafe { &*(func_ptr as *const UserFunction) };
            unsafe { (*frame).opline = user.op_array.instructions.as_ptr() };
            eg.current_execute_data.set(frame);
            execute_ex(eg, frame)
        }
        FunctionType::Internal => {
            let internal = unsafe {
                &*(func_ptr as *const super::function::InternalFunction)
            };
            unsafe { std::ptr::drop_in_place(&mut return_value as *mut Value) };
            (internal.handler)(frame, &mut return_value, eg)
        }
        FunctionType::Undef => {
            eg.exception = Some(make_error_value("Error", "Call to undefined function"));
            Ok(())
        }
    };

    let arg0 = if READBACK_ARG0 {
        let arg0_cv = unsafe { (*func_ptr).sig.param_cv_index(0) } as usize;
        Some(if num_args > arg0_cv {
            unsafe { (*frame).cv(arg0_cv as u32).clone() }
        } else {
            Value::null()
        })
    } else {
        None
    };
    let callback_threw = eg.exception.is_some();

    // Always restore and pop the callback frame, including fatal/error paths.
    eg.current_execute_data.set(saved_execute_data);
    unsafe { cleanup_frame_slots(frame) };
    eg.vm_stack.pop_call_frame(frame);

    execution_result?;

    // A PHP exception stays in ExecutorGlobals for the calling opcode to
    // handle. Callback consumers stop iterating and ignore the partial return.
    if callback_threw {
        Ok((Value::null(), arg0))
    } else {
        Ok((return_value, arg0))
    }
}

/// Like `call_function`, but reads back the first public argument before frame
/// cleanup (CV(0) for functions, CV(1) after a method's hidden `$this`).
/// Used by `array_walk` to capture mutations made by `function (&$val, $key)` callbacks.
/// Returns `(return_value, modified_arg0)`.
pub fn call_function_readback_arg0(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    args: &[Value],
) -> Result<(Value, Value), VmError> {
    call_function_readback_arg0_iter(eg, func_ptr, args.len(), args.iter())
}

/// Borrowed-argument form of `call_function_readback_arg0`.
pub fn call_function_readback_arg0_iter<'a, I>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
) -> Result<(Value, Value), VmError>
where
    I: Iterator<Item = &'a Value>,
{
    let (return_value, arg0) =
        call_function_value_iter::<_, true>(eg, func_ptr, num_args, args.cloned())?;
    Ok((return_value, arg0.unwrap_or_else(Value::null)))
}

/// Resume a generator: set up frame, copy state, execute until yield/return.
/// The generator's state is updated in place.
pub fn resume_generator(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    send_value: Value,
) -> Result<(), VmError> {
    use crate::vm::generator::GeneratorState;

    {
        let gen_data = gen_ref.borrow();
        match gen_data.state {
            GeneratorState::Completed => return Ok(()),
            GeneratorState::Running => {
                return Err(VmError::Fatal("Cannot resume an already running generator".into()));
            }
            _ => {}
        }
    }

    // Handle yield from delegation
    {
        use crate::vm::generator::YieldFromDelegate;
        let has_delegate = gen_ref.borrow().delegate.is_some();
        if has_delegate {
            let delegate = gen_ref.borrow_mut().delegate.take();
            match delegate {
                Some(YieldFromDelegate::Generator(inner_gen_ref)) => {
                    // Forward send value to inner generator
                    resume_generator(eg, &inner_gen_ref, send_value)?;

                    let inner_state = inner_gen_ref.borrow().state;
                    if inner_state == GeneratorState::Completed {
                        // Inner generator done — remove delegate, resume outer with return value
                        let ret_val = inner_gen_ref.borrow().return_value.clone();
                        gen_ref.borrow_mut().delegate = None;

                        // Resume the outer generator at the YieldFrom instruction
                        // It will advance past it. We need to write the return value
                        // to the result slot. We'll do this by resuming normally
                        // but first advancing ip past the YieldFrom and writing result.
                        {
                            let mut gen_data = gen_ref.borrow_mut();
                            // ip_offset points to YieldFrom instruction, advance past it
                            gen_data.ip_offset += 1;
                            gen_data.state = GeneratorState::Suspended;
                            // Store return value in send_value to be written to result slot
                            // We'll handle this by writing it after frame setup below
                        }

                        // Now do a normal resume, but we need to write ret_val to the
                        // YieldFrom result TMP. We handle this by writing it after frame setup.
                        // Actually, let's just set ip_offset-1 to point to YieldFrom so the
                        // send value write logic handles it... but it checks for OpCode::Yield.
                        // Better approach: resume the generator normally and write ret_val
                        // to the YieldFrom's result TMP slot manually.
                        let func_ptr = gen_ref.borrow().func;
                        let user = unsafe { &*(func_ptr as *const UserFunction) };

                        gen_ref.borrow_mut().state = GeneratorState::Running;
                        let saved_execute_data = eg.current_execute_data.get();
                        let frame = eg.vm_stack.push_call_frame(func_ptr, 0);
                        let mut dummy_return = Value::null();
                        unsafe {
                            (*frame).return_value = &mut dummy_return;
                            (*frame).prev_execute_data = std::ptr::null_mut();
                        }

                        {
                            let gen_data = gen_ref.borrow();
                            for (i, val) in gen_data.cv_values.iter().enumerate() {
                                let slot = unsafe { (*frame).cv_mut(i as u32) };
                                unsafe { frame_restore_slot(frame, slot as *mut Value, val.clone()) };
                            }
                            for (i, val) in gen_data.tmp_values.iter().enumerate() {
                                let slot = unsafe { (*frame).tmp_mut(i as u32) };
                                unsafe { frame_restore_slot(frame, slot as *mut Value, val.clone()) };
                            }
                            unsafe {
                                (*frame).opline = user.op_array.instructions.as_ptr().add(gen_data.ip_offset);
                            }
                        }

                        // Write return value to the YieldFrom result slot
                        {
                            let result_slot = gen_ref.borrow().yield_from_result_slot;
                            let yield_from_instr = &user.op_array.instructions[gen_ref.borrow().ip_offset - 1];
                            if yield_from_instr.result_type != OpType::Unused {
                                let slot = unsafe { (*frame).slot_mut(result_slot) };
                                unsafe { frame_restore_slot(frame, slot as *mut Value, ret_val) };
                            }
                        }

                        let saved_active = eg.active_generator.take();
                        eg.active_generator = Some(gen_ref.clone());
                        eg.current_execute_data.set(frame);
                        let result = execute_ex(eg, frame);
                        eg.current_execute_data.set(saved_execute_data);
                        eg.active_generator = saved_active;
                        if result.is_err() {
                            gen_ref.borrow_mut().state = GeneratorState::Completed;
                        }
                        return result;
                    } else {
                        // Inner generator yielded again — copy its value/key to outer
                        let mut gen_data = gen_ref.borrow_mut();
                        let inner = inner_gen_ref.borrow();
                        gen_data.value = inner.value.clone();
                        gen_data.key = inner.key.clone();
                        drop(inner);
                        gen_data.delegate = Some(YieldFromDelegate::Generator(inner_gen_ref));
                        gen_data.state = GeneratorState::Suspended;
                        return Ok(());
                    }
                }
                Some(YieldFromDelegate::Array(entries, pos)) => {
                    if pos >= entries.len() {
                        // Array exhausted — remove delegate, resume outer
                        gen_ref.borrow_mut().delegate = None;
                        {
                            let mut gen_data = gen_ref.borrow_mut();
                            gen_data.ip_offset += 1;
                            gen_data.state = GeneratorState::Suspended;
                        }

                        let func_ptr = gen_ref.borrow().func;
                        let user = unsafe { &*(func_ptr as *const UserFunction) };

                        gen_ref.borrow_mut().state = GeneratorState::Running;
                        let saved_execute_data = eg.current_execute_data.get();
                        let frame = eg.vm_stack.push_call_frame(func_ptr, 0);
                        let mut dummy_return = Value::null();
                        unsafe {
                            (*frame).return_value = &mut dummy_return;
                            (*frame).prev_execute_data = std::ptr::null_mut();
                        }

                        {
                            let gen_data = gen_ref.borrow();
                            for (i, val) in gen_data.cv_values.iter().enumerate() {
                                let slot = unsafe { (*frame).cv_mut(i as u32) };
                                unsafe { frame_restore_slot(frame, slot as *mut Value, val.clone()) };
                            }
                            for (i, val) in gen_data.tmp_values.iter().enumerate() {
                                let slot = unsafe { (*frame).tmp_mut(i as u32) };
                                unsafe { frame_restore_slot(frame, slot as *mut Value, val.clone()) };
                            }
                            unsafe {
                                (*frame).opline = user.op_array.instructions.as_ptr().add(gen_data.ip_offset);
                            }
                        }

                        // Write null to YieldFrom result (arrays return null)
                        {
                            let result_slot = gen_ref.borrow().yield_from_result_slot;
                            let yield_from_instr = &user.op_array.instructions[gen_ref.borrow().ip_offset - 1];
                            if yield_from_instr.result_type != OpType::Unused {
                                let slot = unsafe { (*frame).slot_mut(result_slot) };
                                unsafe { frame_restore_slot(frame, slot as *mut Value, Value::null()) };
                            }
                        }

                        let saved_active = eg.active_generator.take();
                        eg.active_generator = Some(gen_ref.clone());
                        eg.current_execute_data.set(frame);
                        let result = execute_ex(eg, frame);
                        eg.current_execute_data.set(saved_execute_data);
                        eg.active_generator = saved_active;
                        if result.is_err() {
                            gen_ref.borrow_mut().state = GeneratorState::Completed;
                        }
                        return result;
                    } else {
                        // Yield next array element
                        let mut gen_data = gen_ref.borrow_mut();
                        let (ref key, ref val) = entries[pos];
                        gen_data.value = val.clone();
                        gen_data.key = match key {
                            crate::value::ArrayKey::Int(i) => Value::long(*i),
                            crate::value::ArrayKey::String(s) => Value::string(s.clone()),
                        };
                        gen_data.delegate = Some(YieldFromDelegate::Array(entries, pos + 1));
                        gen_data.state = GeneratorState::Suspended;
                        return Ok(());
                    }
                }
                None => unreachable!(),
            }
        }
    }

    // Mark as running
    gen_ref.borrow_mut().state = GeneratorState::Running;

    let func_ptr = gen_ref.borrow().func;
    let user = unsafe { &*(func_ptr as *const UserFunction) };
    let saved_execute_data = eg.current_execute_data.get();

    // Push a frame for the generator
    let frame = eg.vm_stack.push_call_frame(func_ptr, 0);
    let mut dummy_return = Value::null();
    unsafe {
        (*frame).return_value = &mut dummy_return;
        (*frame).prev_execute_data = std::ptr::null_mut();
    }

    // Copy saved CV values into frame
    {
        let gen_data = gen_ref.borrow();
        for (i, val) in gen_data.cv_values.iter().enumerate() {
            let slot = unsafe { (*frame).cv_mut(i as u32) };
            unsafe { frame_restore_slot(frame, slot as *mut Value, val.clone()) };
        }
        for (i, val) in gen_data.tmp_values.iter().enumerate() {
            let slot = unsafe { (*frame).tmp_mut(i as u32) };
            unsafe { frame_restore_slot(frame, slot as *mut Value, val.clone()) };
        }

        // Set instruction pointer
        unsafe {
            (*frame).opline = user.op_array.instructions.as_ptr().add(gen_data.ip_offset);
        }
    }

    // If resuming from a yield (not first call), write send value to the
    // previous yield's result TMP. The yield instruction at ip_offset-1
    // told us its result slot.
    {
        let gen_data = gen_ref.borrow();
        if gen_data.state == GeneratorState::Running && gen_data.ip_offset > 0 {
            // The yield instruction is at ip_offset - 1
            let yield_instr = &user.op_array.instructions[gen_data.ip_offset - 1];
            if yield_instr.opcode == crate::vm::opcode::OpCode::Yield
                && yield_instr.result_type != OpType::Unused
            {
                let tmp_slot = unsafe { (*frame).slot_mut(yield_instr.result as u32) };
                unsafe { frame_restore_slot(frame, tmp_slot as *mut Value, send_value.clone()) };
            }
        }
    }

    // Set active generator so Yield/Return can find it
    let saved_active = eg.active_generator.take();
    eg.active_generator = Some(gen_ref.clone());

    eg.current_execute_data.set(frame);
    let result = execute_ex(eg, frame);

    // Restore state
    eg.current_execute_data.set(saved_execute_data);
    eg.active_generator = saved_active;

    // Clean up frame (CV/TMP already saved by Yield handler)
    // Note: Yield handler already cleaned up the frame, but if Return happened
    // or an error occurred, the frame might still be allocated.
    // The Yield/Return handlers pop the frame themselves, so we only need
    // to handle the error case.
    if result.is_err() {
        gen_ref.borrow_mut().state = GeneratorState::Completed;
    }

    result
}

// ── Cold opcode helpers ──────────────────────────────────────────────
// Extracted from execute_ex to reduce icache pressure on the hot dispatch loop.
// Each helper is #[inline(never)] so LLVM keeps their code out of the jump table.

/// Returns true if the caller should `continue` (skip opline advance).
#[inline(never)]
fn op_include(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &crate::vm::instruction::Instruction,
) -> Result<bool, VmError> {
    let path_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let path_str = path_val.echo_to_string();
    let is_require = (opline.extended_value & 1) != 0;
    let is_once = (opline.extended_value & 2) != 0;

    let resolved_path = if std::path::Path::new(&path_str).is_absolute() {
        path_str.clone()
    } else {
        let base_dir = {
            let op_name = &op_array.name;
            let p = std::path::Path::new(op_name);
            if p.is_file() {
                p.parent().map(|d| d.to_path_buf())
            } else {
                None
            }
        }.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        base_dir.join(&path_str).to_string_lossy().to_string()
    };

    let canonical = std::fs::canonicalize(&resolved_path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| resolved_path.clone());

    if is_once && eg.included_files.contains(&canonical) {
        return Ok(false);
    }

    let source = match std::fs::read_to_string(&resolved_path) {
        Ok(s) => s,
        Err(e) => {
            if is_require {
                return Err(VmError::Fatal(format!(
                    "require({}): Failed opening required '{}' ({})",
                    path_str, resolved_path, e
                )));
            } else {
                let warning = format!(
                    "Warning: include({}): Failed opening '{}' for inclusion ({})\n",
                    path_str, resolved_path, e
                );
                eg.write_output(warning.as_bytes());
                unsafe { (*frame).opline = (*frame).opline.add(1); }
                return Ok(true); // continue
            }
        }
    };

    if is_once {
        eg.included_files.insert(canonical);
    }

    let tokens = crate::lexer::Lexer::new(&source).tokenize()
        .map_err(|e| VmError::Fatal(format!("Syntax error in {}: {}", resolved_path, e)))?;
    let stmts = crate::parser::Parser::new(tokens).parse()
        .map_err(|e| VmError::Fatal(format!("Parse error in {}: {}", resolved_path, e)))?;
    let compile_result = crate::compiler::compile::Compiler::new().compile(&stmts)
        .map_err(|e| VmError::Fatal(format!("Compile error in {}: {}", resolved_path, e)))?;

    for (name, func) in compile_result.functions {
        let boxed = Box::new(func);
        let ptr = &boxed.common as *const FunctionCommon;
        eg.included_functions.push(boxed);
        let _ = eg.register_function(&name, ptr);
    }
    for class_def in compile_result.class_defs {
        eg.register_class(class_def).map_err(|e| VmError::Fatal(e))?;
    }

    let mut inc_op_array_main = compile_result.main;
    inc_op_array_main.name = resolved_path.clone();
    let main_func_boxed = Box::new(crate::compiler::make_user_function(inc_op_array_main));
    eg.included_functions.push(main_func_boxed);
    let main_func: &UserFunction = unsafe {
        &*(&**eg.included_functions.last().unwrap() as *const UserFunction)
    };

    let scope_vars: Vec<(u32, String)> = if !op_array.all_cvs.is_empty() {
        op_array.all_cvs.clone()
    } else {
        op_array.main_scope_vars.clone()
    };
    for (cv_idx, var_name) in &scope_vars {
        if var_name == "this" { continue; }
        let cv_ptr = unsafe { (*frame).get_op_ptr(*cv_idx, OpType::Cv, op_array) };
        let val = unsafe { (*cv_ptr).clone() };
        globals_set(&mut eg.globals, var_name, val);
    }

    let inc_func_ptr = &main_func.common as *const FunctionCommon;
    let mut inc_return_value = Value::null();
    let inc_frame = eg.vm_stack.push_call_frame(inc_func_ptr, 0);
    unsafe {
        (*inc_frame).return_value = &mut inc_return_value;
        (*inc_frame).opline = main_func.op_array.instructions.as_ptr();
        (*inc_frame).prev_execute_data = std::ptr::null_mut();
    }
    for (cv_idx, var_name) in &main_func.op_array.main_scope_vars {
        if let Some(val) = eg.globals.get(var_name) {
            let cv_ptr = unsafe { (*inc_frame).get_op_mut(*cv_idx, OpType::Cv) };
            unsafe { slot_set(cv_ptr, val.clone()) };
        }
    }

    let prev_ed = eg.current_execute_data.get();
    eg.current_execute_data.set(inc_frame);
    let inc_result = execute_ex(eg, inc_frame);

    let inc_op_array = unsafe { (*inc_frame).op_array() };
    let inc_scope = if !inc_op_array.all_cvs.is_empty() {
        &inc_op_array.all_cvs
    } else {
        &inc_op_array.main_scope_vars
    };
    for (cv_idx, var_name) in inc_scope {
        let cv_ptr = unsafe { (*inc_frame).get_op_mut(*cv_idx, OpType::Cv) };
        let val = unsafe { (*cv_ptr).clone() };
        globals_set(&mut eg.globals, var_name, val);
    }

    eg.current_execute_data.set(prev_ed);
    unsafe { cleanup_frame_slots(inc_frame) };
    eg.vm_stack.pop_call_frame(inc_frame);

    for (cv_idx, var_name) in &scope_vars {
        if var_name == "this" { continue; }
        if let Some(val) = eg.globals.get(var_name) {
            let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
            unsafe { slot_set(cv_ptr, val.clone()) };
        }
    }

    if let Some(exc) = eg.exception.take() {
        let (class_name, message) = if let Some(obj) = exc.as_object() {
            let cls = obj.class_name.clone();
            let msg = obj.get_property("message")
                .map(|v| v.echo_to_string())
                .unwrap_or_default();
            (cls, msg)
        } else {
            ("Exception".to_string(), exc.echo_to_string())
        };
        return Err(VmError::Fatal(format!("Uncaught {}: {}", class_name, message)));
    }

    let new_op_array = unsafe { (*frame).op_array() };
    for (cv_idx, var_name) in &new_op_array.main_scope_vars {
        if let Some(val) = eg.globals.get(var_name) {
            let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
            unsafe { slot_set(cv_ptr, val.clone()) };
        }
    }

    inc_result?;
    Ok(false)
}

/// Result type for cold opcode helpers that may change the VM frame (e.g. via throw_in_frame).
enum ColdResult<'a> {
    /// Normal completion — advance opline as usual.
    Done,
    /// Skip opline advance (already advanced or jumped).
    Continue,
    /// Frame changed (exception was caught by a handler in a different frame).
    NewFrame(*mut ExecuteData, &'a crate::compiler::OpArray),
    /// Unhandled exception — propagate via eg.exception and return from execute_ex.
    Unhandled(Value),
    /// Generator suspend / return — execute_ex should return Ok(()).
    Return,
}

// ── Additional cold opcode helpers ─────────────────────────────────────

#[inline(never)]
fn op_throw<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    // PHP 8: only Throwable objects can be thrown
    if val.as_object().is_none() || {
        let obj = val.as_object().unwrap();
        !eg.class_is_a(&obj.class_name, "Throwable")
    } {
        let type_name = match val.value_type() {
            ValueType::Long => "int",
            ValueType::Double => "float",
            ValueType::String => "string",
            ValueType::True | ValueType::False => "bool",
            ValueType::Null | ValueType::Undef => "null",
            ValueType::Array => "array",
            ValueType::Object => {
                // Object but not Throwable
                let obj = val.as_object().unwrap();
                return Err(VmError::Fatal(format!(
                    "Cannot throw objects that do not implement Throwable (class {})", obj.class_name
                )));
            }
            _ => "unknown",
        };
        return Err(VmError::Fatal(format!(
            "Can only throw objects implementing Throwable, {} given", type_name
        )));
    }
    let thrown = val.clone();

    match throw_in_frame(eg, frame, thrown) {
        ThrowResult::Handled(new_frame, new_op_array) => {
            Ok(ColdResult::NewFrame(new_frame, new_op_array))
        }
        ThrowResult::Unhandled(exc) => {
            Ok(ColdResult::Unhandled(exc))
        }
    }
}

#[inline(never)]
fn op_new_obj<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let class_name = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let name = class_name.as_str().unwrap_or("");
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

    // Reject instantiation of interfaces, abstract classes, and internal-only classes
    if name == "Generator" {
        return Err(VmError::Fatal(
            "The \"Generator\" class is reserved for internal use and cannot be manually instantiated".into()
        ));
    }
    if let Some(class_def) = eg.class_table.get(name) {
        if class_def.is_interface {
            return Err(VmError::Fatal(format!(
                "Cannot instantiate interface {}",
                name
            )));
        }
        if class_def.is_abstract {
            return Err(VmError::Fatal(format!(
                "Cannot instantiate abstract class {}",
                name
            )));
        }
        if class_def.is_enum {
            let err = make_error_value("Error", &format!(
                "Cannot instantiate enum {}",
                name
            ));
            match throw_in_frame(eg, frame, err) {
                ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
            }
        }
    }

    // Create compact declared-property slots from the class layout.
    let (property_layout, property_values) = if let Some(class_def) = eg.class_table.get(name) {
        let mut values = Vec::with_capacity(class_def.properties.len());
        for (prop_name, default_val, _vis, _declaring) in &class_def.properties {
            let is_readonly = class_def.readonly_props.contains(prop_name);
            let val = default_val.as_ref()
                .map(|v| v.clone())
                .unwrap_or(if is_readonly { Value::undef() } else { Value::null() });
            values.push(val);
        }
        (class_def.property_layout.clone(), values)
    } else {
        (std::rc::Rc::new(crate::value::ObjectLayout::empty()), Vec::new())
    };
    let obj = PhpObject::with_layout(
        name.to_string(),
        eg.class_id_of(name),
        property_layout,
        property_values,
    );
    unsafe { slot_set(result_ptr, Value::object(obj)) };

    // Check for __construct — set up call frame if it exists
    let num_args = opline.extended_value;
    let construct_name = format!("{}::__construct", name);
    if let Some(func_ptr) = eg.find_function(&construct_name) {
        // +1 for $this at CV 0; SendVal writes args to CV 1..N
        let call = eg.vm_stack.push_call_frame(func_ptr, num_args + 1);
        unsafe {
            (*call).num_args = num_args; // restore explicit arg count for DoFcall arity check
            (*call).prev_execute_data = frame;
            (*call).call = (*frame).call;
            (*frame).call = call;
            // Write $this directly — cleanup handles it separately.
            let obj_ref = &*result_ptr;
            frame_set_this(call, obj_ref.clone());
        }
    } else {
        // No constructor — skip num_args SendVals + 1 DoFcall.
        // Arg expressions were compiled before NewObj so side effects
        // have already executed; we just discard the values.
        let skip = num_args + 1; // SendVals + DoFcall
        let base_ptr = op_array.instructions.as_ptr();
        let current_ip = unsafe { (*frame).opline.offset_from(base_ptr) } as usize;
        unsafe { (*frame).opline = base_ptr.add(current_ip + 1 + skip as usize) };
        return Ok(ColdResult::Continue);
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_fetch_obj_r(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let prop_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

    if obj_val.value_type() != ValueType::Object {
        return Err(VmError::Fatal("Attempt to read property on non-object".into()));
    }

    let name = prop_name.as_str().unwrap_or("");
    let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
    let ic = &op_array.cache[ip];
    let object_class_id = unsafe { obj_val.object_class_id_unchecked() };

    // Cache hit: direct class guard + slot load. No RefCell borrow or hash lookup.
    if ic.property_flags() & 1 != 0
        && ic.class_id == object_class_id
        && object_class_id != 0
    {
        let property_ptr = unsafe {
            obj_val.object_property_slot_unchecked(ic.property_slot())
        };
        unsafe { slot_set(result_ptr, (*property_ptr).clone()) };
        return Ok(());
    }

    if let Some(obj) = obj_val.as_object() {

        // ── Full resolution (cache miss or private/protected) ──
        let caller_class = get_caller_class(frame, eg);

        // Private property early binding is only valid when the receiver
        // is in the same inheritance hierarchy as the caller.  When
        // accessing an unrelated object, the caller's private property
        // must NOT leak — use target-only key resolution.
        let receiver_in_scope = caller_class.as_ref().map_or(false, |cc| {
            eg.class_is_a(&obj.class_name, cc)
        });
        let effective_caller = if receiver_in_scope { caller_class.as_deref() } else { None };

        // Resolve storage key (mangled for private properties)
        let key = crate::runtime::resolve_property_key(eg, &obj.class_name, name, effective_caller);

        // Determine if property is public (for caching)
        let mut is_public = true;
        // Visibility check
        if let Some((vis, defining_class)) = eg.find_property_visibility(&obj.class_name, name) {
            if vis != Visibility::Public {
                is_public = false;
                // Skip check if the caller owns the defining class AND
                // the receiver is in that scope (same hierarchy).
                let own_private = receiver_in_scope && caller_class.as_ref().map_or(false, |cc| {
                    vis == Visibility::Private && defining_class.eq_ignore_ascii_case(cc)
                });
                // Also skip if caller's class declares its own private
                // with same name AND the receiver is in scope.
                let caller_has_own = receiver_in_scope && caller_class.as_ref().map_or(false, |cc| {
                    if let Some((Visibility::Private, ref dc)) = eg.find_property_visibility(cc, name) {
                        dc.eq_ignore_ascii_case(cc)
                    } else {
                        false
                    }
                });
                if !own_private && !caller_has_own {
                    if !eg.check_visibility(caller_class.as_deref(), &defining_class, vis) {
                        let vis_str = match vis { Visibility::Protected => "protected", Visibility::Private => "private", _ => "public" };
                        return Err(VmError::Fatal(format!(
                            "Cannot access {} property {}::${}",
                            vis_str, defining_class, name
                        )));
                    }
                }
            }
        }

        // Cache only declared public properties. Dynamic properties have no
        // stable slot and remain on the cold lookup path.
        if is_public && key == name && obj.class_id != 0 {
            if let Some(slot) = obj.property_slot(&key) {
                let ic_mut = unsafe { &mut *(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache) };
                let mut flags: u32 = 1; // read-safe
                let writable = eg.class_table.get(&obj.class_name).is_none_or(|cd| {
                    !cd.is_enum && !cd.readonly_props.iter().any(|prop| prop == name)
                });
                if writable {
                    flags |= 2;
                }
                ic_mut.set_property(obj.class_id, slot, flags);
            }
        }

        let found_val = obj.get_property(&key).cloned();
        drop(obj); // Release borrow before potential magic method call
        if let Some(val) = found_val {
            unsafe { slot_set(result_ptr, val) };
        } else {
            // Property not found — try __get magic method
            if let Some(result) = call_magic_method(eg, obj_val, "__get", &[Value::string(name)])? {
                unsafe { slot_set(result_ptr, result) };
            } else {
                unsafe { slot_set(result_ptr, Value::null()) };
            }
        }
    }
    Ok(())
}

#[inline(never)]
fn op_assign_obj_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let prop_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
    let cloned = val.clone();
    let name = prop_name.as_str().unwrap_or("").to_string();
    let obj_ptr = unsafe { (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let obj = unsafe { &*obj_ptr };

    if let Some(mut php_obj) = obj.as_object_mut() {
        let caller_class = get_caller_class(frame, eg);

        // Same receiver-in-scope guard as FetchObjR — only allow
        // private bypass when the receiver is in the caller's hierarchy.
        let receiver_in_scope = caller_class.as_ref().map_or(false, |cc| {
            eg.class_is_a(&php_obj.class_name, cc)
        });
        let effective_caller = if receiver_in_scope { caller_class.as_deref() } else { None };

        // Visibility check — use declaring class, not receiver class
        let mut prop_is_public = true;
        if let Some((vis, defining_class)) = eg.find_property_visibility(&php_obj.class_name, &name) {
            if vis != Visibility::Public {
                prop_is_public = false;
                let own_private = receiver_in_scope && caller_class.as_ref().map_or(false, |cc| {
                    vis == Visibility::Private && defining_class.eq_ignore_ascii_case(cc)
                });
                let caller_has_own = receiver_in_scope && caller_class.as_ref().map_or(false, |cc| {
                    if let Some((Visibility::Private, ref dc)) = eg.find_property_visibility(cc, &name) {
                        dc.eq_ignore_ascii_case(cc)
                    } else {
                        false
                    }
                });
                if !own_private && !caller_has_own {
                    if !eg.check_visibility(caller_class.as_deref(), &defining_class, vis) {
                        let vis_str = match vis { Visibility::Protected => "protected", Visibility::Private => "private", _ => "public" };
                        return Err(VmError::Fatal(format!(
                            "Cannot access {} property {}::${}",
                            vis_str, defining_class, name
                        )));
                    }
                }
            }
        }
        // Enum guard: enum cases are sealed — no property writes allowed
        // Track writability for cache population — enum/readonly are not cacheable for writes.
        let mut prop_is_writable = true;
        if let Some(class_def) = eg.class_table.get(&php_obj.class_name) {
            if class_def.is_enum {
                let err = make_error_value("Error", &format!(
                    "Cannot modify readonly property {}::${}",
                    php_obj.class_name, name
                ));
                drop(php_obj);
                match throw_in_frame(eg, frame, err) {
                    ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                    ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                }
            }
        }
        // Readonly property check
        if let Some(class_def) = eg.class_table.get(&php_obj.class_name) {
            if class_def.readonly_props.contains(&name) {
                prop_is_writable = false;
                let key_check = crate::runtime::resolve_property_key(eg, &php_obj.class_name, &name, effective_caller);
                let already_init = php_obj.get_property(&key_check)
                    .map_or(false, |v| !v.is_undef());
                if already_init {
                    // Already initialized — always error
                    let err = make_error_value("Error", &format!(
                        "Cannot modify readonly property {}::${}",
                        php_obj.class_name, name
                    ));
                    drop(php_obj);
                    match throw_in_frame(eg, frame, err) {
                        ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                        ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                    }
                } else {
                    // First initialization — only allowed from declaring class scope
                    let in_declaring_scope = caller_class.as_ref().map_or(false, |cc| {
                        cc.eq_ignore_ascii_case(&php_obj.class_name)
                    });
                    if !in_declaring_scope {
                        let err = make_error_value("Error", &format!(
                            "Cannot initialize readonly property {}::${} from {}",
                            php_obj.class_name, name,
                            caller_class.as_deref().map_or("global scope".to_string(), |c| format!("scope {}", c))
                        ));
                        drop(php_obj);
                        match throw_in_frame(eg, frame, err) {
                            ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                            ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                        }
                    }
                }
            }
        }
        // Resolve storage key (mangled for private properties)
        let key = crate::runtime::resolve_property_key(eg, &php_obj.class_name, &name, effective_caller);

        // Cache: if public, not enum, not readonly, key == name → mark for write fast path.
        if prop_is_public && prop_is_writable && key == name && php_obj.class_id != 0 {
            let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
            let ic_mut = unsafe { &mut *(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache) };
            if let Some(slot) = php_obj.property_slot(&key) {
                ic_mut.set_property(php_obj.class_id, slot, 3);
            }
        }

        let prop_exists = php_obj.contains_property(&key);
        if prop_exists {
            php_obj.set_property(&key, cloned);
        } else {
            drop(php_obj); // Release borrow before potential magic method call
            // Property not found — try __set magic method
            if call_magic_method(eg, obj, "__set", &[Value::string(name.clone()), cloned.clone()])?.is_none() {
                // No __set — fall back to direct insert
                if let Some(mut php_obj) = obj.as_object_mut() {
                    php_obj.set_property(&key, cloned);
                }
            }
        }
    } else {
        return Err(VmError::Fatal("Attempt to assign property on non-object".into()));
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_init_method_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };

    if let Some(obj) = obj_val.as_object() {
        let obj_class_id = obj.class_id;

        // Inline cache: if same class_id as last time, reuse resolved func_ptr
        // — avoids class_name.clone() and full method resolution on cache hit.
        let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
        let ic = &op_array.cache[ip];
        let func_ptr = if !ic.func.is_null() && ic.class_id == obj_class_id && obj_class_id != 0 {
            drop(obj); // release borrow — class_name not needed on cache hit
            ic.func
        } else {
            let target_class_name = obj.class_name.clone();
            drop(obj); // release borrow before lookup
            let method_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
            let method = method_name.as_str().unwrap_or("");
            let caller_class = get_caller_class(frame, eg);

            let dispatch_class = if let Some(ref cc) = caller_class {
                if let Some((Visibility::Private, ref defining)) = eg.find_method_visibility(cc, method) {
                    if defining.eq_ignore_ascii_case(cc)
                        && eg.class_is_a(&target_class_name, cc)
                    {
                        cc.clone()
                    } else {
                        target_class_name.clone()
                    }
                } else {
                    target_class_name.clone()
                }
            } else {
                target_class_name.clone()
            };

            let full_name = format!("{}::{}", dispatch_class, method);
            let resolved = match eg.find_function(&full_name) {
                Some(ptr) => ptr,
                None => {
                    let err = make_error_value("Error", &format!("Call to undefined method {}::{}()", dispatch_class, method));
                    match throw_in_frame(eg, frame, err) {
                        ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                        ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                    }
                }
            };

            // Visibility check
            if let Some((vis, defining_class)) = eg.find_method_visibility(&dispatch_class, method) {
                if vis != Visibility::Public {
                    if !eg.check_visibility(caller_class.as_deref(), &defining_class, vis) {
                        let vis_str = match vis {
                            Visibility::Protected => "protected",
                            Visibility::Private => "private",
                            _ => "public",
                        };
                        return Err(VmError::Fatal(format!(
                            "Call to {} method {}::{}() from scope {}",
                            vis_str, defining_class, method,
                            caller_class.as_deref().unwrap_or("global")
                        )));
                    }
                }
            }

            // Cache the resolution (don't cache if class_id is 0 = unknown)
            if obj_class_id != 0 {
                let ic_mut = unsafe { &mut *(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache) };
                ic_mut.func = resolved;
                ic_mut.class_id = obj_class_id;
            }
            resolved
        };

        let num_args = opline.extended_value;
        let call = eg.vm_stack.push_call_frame(func_ptr, num_args + 1);
        unsafe {
            (*call).num_args = num_args;
            (*call).prev_execute_data = frame;
            (*call).call = (*frame).call;
            (*frame).call = call;
            let common = &*func_ptr;
            if common.plan.borrow_this {
                frame_set_borrowed_this(call, obj_val as *const Value);
            } else {
                frame_set_this(call, obj_val.clone());
            }
        }
    } else {
        let method_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
        let method = method_name.as_str().unwrap_or("");
        let err = make_error_value("Error", &format!("Call to member function {}() on non-object", method));
        match throw_in_frame(eg, frame, err) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                return Ok(ColdResult::NewFrame(new_frame, new_op_array));
            }
            ThrowResult::Unhandled(thrown) => {
                return Ok(ColdResult::Unhandled(thrown));
            }
        }
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_init_static_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // Inline cache: static calls have constant class+method — cache resolved func_ptr.
    // Visibility is checked on first resolve only (same instruction = same caller context).
    let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
    let cached = op_array.cache[ip].func;
    let func_ptr = if !cached.is_null() {
        cached
    } else {
        let class_name = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
        let method_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
        let class = class_name.as_str().unwrap_or("");
        let method = method_name.as_str().unwrap_or("");

        let full_name = format!("{}::{}", class, method);
        let resolved = match eg.find_function(&full_name) {
            Some(ptr) => ptr,
            None => {
                let err = make_error_value("Error", &format!("Call to undefined method {}::{}()", class, method));
                match throw_in_frame(eg, frame, err) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        return Ok(ColdResult::NewFrame(new_frame, new_op_array));
                    }
                    ThrowResult::Unhandled(thrown) => {
                        return Ok(ColdResult::Unhandled(thrown));
                    }
                }
            }
        };

        // Visibility check on first resolve
        if let Some((vis, defining_class)) = eg.find_method_visibility(class, method) {
            if vis != Visibility::Public {
                let caller_class = get_caller_class(frame, eg);
                if !eg.check_visibility(caller_class.as_deref(), &defining_class, vis) {
                    let vis_str = match vis { Visibility::Protected => "protected", Visibility::Private => "private", _ => "public" };
                    return Err(VmError::Fatal(format!(
                        "Call to {} method {}::{}() from scope {}",
                        vis_str, defining_class, method,
                        caller_class.as_deref().unwrap_or("global")
                    )));
                }
            }
        }

        // Cache for subsequent calls
        unsafe { (*(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache)).func = resolved; }
        resolved
    };

    let num_args = opline.extended_value;
    // +1 for $this at CV 0 (compiler allocates $this even for static calls)
    let call = eg.vm_stack.push_call_frame(func_ptr, num_args + 1);
    unsafe {
        (*call).num_args = num_args; // restore explicit arg count for DoFcall arity check
        (*call).prev_execute_data = frame;
        (*call).call = (*frame).call;
        (*frame).call = call;
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_init_user_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let resolved = match resolve_user_call_at_opline(eg, frame, op_array, opline) {
        Some(resolved) => resolved,
        None => {
            let callback_raw = unsafe {
                &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
            };
            let callback = if callback_raw.is_reference() {
                unsafe { &*callback_raw.as_ref_ptr() }
            } else {
                callback_raw
            };
            let description = callback.echo_to_string();
            let error = make_error_value("TypeError", &format!(
                "call_user_func(): Argument #1 ($callback) must be a valid callback, function \"{}\" not found or not callable",
                description,
            ));
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
    };

    init_resolved_user_call(eg, frame, opline.extended_value, resolved);
    Ok(ColdResult::Done)
}

#[inline]
fn resolve_user_call_at_opline(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Option<crate::stdlib::ResolvedCallback> {
    let callback_raw = unsafe {
        &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
    };
    let callback = if callback_raw.is_reference() {
        unsafe { &*callback_raw.as_ref_ptr() }
    } else {
        callback_raw
    };
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let cache_slot = unsafe {
        op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache
    };
    let caller_class = get_caller_class(frame, eg);
    crate::stdlib::resolve_callback_with_cache(
        callback,
        eg,
        caller_class.as_deref(),
        Some(cache_slot),
    )
}

#[inline]
fn init_resolved_user_call(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    explicit_args: u32,
    resolved: crate::stdlib::ResolvedCallback,
) {
    let signature = unsafe { &(*resolved.func_ptr).sig };
    let public_end = signature.this_offset + explicit_args;
    let capture_end = signature.num_args + resolved.use_vars.len() as u32;
    let storage_slots = public_end.max(capture_end);
    let call = eg.vm_stack.push_call_frame(resolved.func_ptr, storage_slots);
    unsafe {
        // DoFcall validates only the public arguments. Hidden `$this` and
        // closure captures occupy CV slots but are not part of public arity.
        (*call).num_args = explicit_args;
        (*call).prev_execute_data = frame;
        (*call).call = (*frame).call;
        (*frame).call = call;
    }

    // push_call_frame leaves the whole requested argument prefix
    // uninitialized. Optional declared parameters between the supplied
    // arguments and closure captures must remain readable Undef slots.
    for index in public_end..signature.num_args {
        let destination = unsafe { (*call).cv_mut(index) } as *mut Value;
        unsafe { destination.write(Value::undef()) };
    }

    for (index, value) in resolved.prepend_args.into_iter().enumerate() {
        let destination = unsafe { (*call).cv_mut(index as u32) } as *mut Value;
        unsafe { frame_slot_init(call, destination, value) };
    }

    let capture_offset = signature.num_args;
    for (index, value) in resolved.use_vars.into_iter().enumerate() {
        let destination = unsafe { (*call).cv_mut(capture_offset + index as u32) } as *mut Value;
        unsafe { frame_slot_init(call, destination, value) };
    }
}

#[inline(never)]
fn op_init_dynamic_call(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    let callable = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };

    if let Some(closure) = callable.as_closure() {
        // Fast path: Closure value — direct function pointer, no string lookup.
        let func_ptr = closure.func;
        let num_args = opline.extended_value;
        let call = eg.vm_stack.push_call_frame(func_ptr, num_args);
        unsafe {
            (*call).prev_execute_data = frame;
            (*call).call = (*frame).call;
            (*frame).call = call;
        }

        // Copy captured use_vars into CV slots after declared params
        let func = unsafe { &*func_ptr };
        let use_var_offset = func.sig.num_args;
        let n_captures = closure.captures.len();
        if n_captures > 0 {
            if !closure.has_heap_captures {
                // Scalar-only fast path: all captures are Long/Double/Bool/Null.
                // Raw memcpy — no clone overhead, no needs_cleanup checks.
                unsafe {
                    let src = closure.captures.as_ptr();
                    let dst = (*call).cv_mut(use_var_offset) as *mut Value;
                    std::ptr::copy_nonoverlapping(src, dst, n_captures);
                }
                // No heap flag needed — all scalars.
            } else {
                // General path: at least one heap capture, clone each.
                for (i, captured) in closure.captures.iter().enumerate() {
                    let cv_slot = unsafe { (*call).cv_mut(use_var_offset + i as u32) };
                    unsafe { frame_slot_init(call, cv_slot as *mut Value, captured.clone()) };
                }
            }
        }
    } else if let Some(arr) = callable.as_array() {
        // Legacy array callable: [class_or_object, method_name]
        let arr_len = arr.len();
        if arr_len == 0 {
            return Err(VmError::Fatal("Array is not callable".into()));
        }
        let func_name = arr.get_value_at(0)
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                VmError::Fatal("Closure descriptor must start with function name".into())
            })?;

        let func_ptr = eg.find_function(func_name).ok_or_else(|| {
            VmError::Fatal(format!("Call to undefined function {}()", func_name))
        })?;

        let num_args = opline.extended_value;
        let call = eg.vm_stack.push_call_frame(func_ptr, num_args);
        unsafe {
            (*call).prev_execute_data = frame;
            (*call).call = (*frame).call;
            (*frame).call = call;
        }

        // Copy captured use_vars into CV slots after params
        let func = unsafe { &*func_ptr };
        let use_var_offset = func.sig.num_args;
        for i in 1..arr_len {
            let captured_val = arr.get_value_at(i).unwrap().clone();
            let cv_slot = unsafe { (*call).cv_mut(use_var_offset + (i as u32 - 1)) };
            unsafe { frame_slot_set(call, cv_slot as *mut Value, captured_val) };
        }
    } else if let Some(func_name) = callable.as_str() {
        // Simple string function call: $func = "my_func"; $func()
        let func_ptr = eg.find_function(func_name).ok_or_else(|| {
            VmError::Fatal(format!("Call to undefined function {}()", func_name))
        })?;

        let num_args = opline.extended_value;
        let call = eg.vm_stack.push_call_frame(func_ptr, num_args);
        unsafe {
            (*call).prev_execute_data = frame;
            (*call).call = (*frame).call;
            (*frame).call = call;
        }
    } else if callable.value_type() == ValueType::Object {
        // Object with __invoke: set up as method call to __invoke
        let obj = callable.as_object().unwrap();
        let class_name = obj.class_name.clone();
        drop(obj);
        let full_name = format!("{}::__invoke", class_name.to_lowercase());
        let func_ptr = match eg.find_function(&full_name) {
            Some(ptr) => ptr,
            None => return Err(VmError::Fatal(format!("Call to undefined method {}::__invoke()", class_name))),
        };

        let num_args = opline.extended_value;
        // +1 for $this at CV 0; but don't write $this yet because
        // SendVal will write args to CV 0..N-1 (compiler doesn't know
        // it's a method call). We'll shift args in DoFcall.
        let call = eg.vm_stack.push_call_frame(func_ptr, num_args + 1);
        unsafe {
            (*call).num_args = num_args;
            (*call).num_cvs = num_args + 1; // track total CVs needed
            (*call).prev_execute_data = frame;
            (*call).call = (*frame).call;
            (*frame).call = call;
        }
        // Stash $this object for injection in DoFcall
        eg.pending_invoke_this = Some(callable.clone());
    } else {
        return Err(VmError::Fatal(format!("Value of type {:?} is not callable", callable.value_type())));
    }
    Ok(())
}

#[inline(never)]
fn op_foreach_init(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<bool, VmError> {
    let arr_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };

    // Check for Generator object
    let is_generator = if let Some(obj) = arr_val.as_object() {
        obj.class_name == "Generator" && arr_val.as_object_rc().map_or(false, |rc| rc.borrow().generator.is_some())
    } else {
        false
    };

    if is_generator {
        // Start the generator (rewind)
        let gen_ref = arr_val.as_object_rc().unwrap().borrow().generator.clone().unwrap();
        {
            let state = gen_ref.borrow().state;
            if state == crate::vm::generator::GeneratorState::Created {
                resume_generator(eg, &gen_ref, Value::null())?;
            }
        }
        let is_valid = gen_ref.borrow().state != crate::vm::generator::GeneratorState::Completed;
        if !is_valid {
            let target = opline.op2 as usize;
            let base_ptr = op_array.instructions.as_ptr();
            unsafe { (*frame).opline = base_ptr.add(target) };
            return Ok(true); // continue
        }
        // Store generator object in result TMP
        let cloned = arr_val.clone();
        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
        unsafe { slot_set(result_ptr, cloned) };
        // Set position TMP to 0 (0 = first iteration, don't call next)
        let pos_ptr = unsafe { (*frame).get_op_mut(opline.extended_value, OpType::Tmp) };
        unsafe { slot_set(pos_ptr, Value::long(0)) };
    } else {
        let is_empty = match arr_val.as_array() {
            Some(arr) => arr.is_empty(),
            None => {
                eg.write_output(b"\nWarning: foreach() argument must be of type array|object, ");
                let type_name = match arr_val.value_type() {
                    ValueType::Null => "null",
                    ValueType::True | ValueType::False => "bool",
                    ValueType::Long => "int",
                    ValueType::Double => "float",
                    ValueType::String => "string",
                    _ => "unknown",
                };
                eg.write_output(type_name.as_bytes());
                eg.write_output(b" given\n");
                true
            }
        };
        if is_empty {
            let target = opline.op2 as usize;
            let base_ptr = op_array.instructions.as_ptr();
            unsafe { (*frame).opline = base_ptr.add(target) };
            return Ok(true); // continue
        }
        // Copy array to result TMP
        let cloned = arr_val.clone();
        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
        unsafe { slot_set(result_ptr, cloned) };
        // Set position TMP to 0
        let pos_ptr = unsafe { (*frame).get_op_mut(opline.extended_value, OpType::Tmp) };
        unsafe { slot_set(pos_ptr, Value::long(0)) };
    }
    Ok(false)
}

#[inline(never)]
fn op_foreach_next(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    let val_cv = (opline.extended_value & 0xFFFF) as u32;
    let key_encoded = (opline.extended_value >> 16) as u32;

    let arr_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };

    // Check for Generator object
    let gen_ref_opt = if let Some(obj) = arr_val.as_object() {
        if obj.class_name == "Generator" {
            arr_val.as_object_rc().and_then(|rc| rc.borrow().generator.clone())
        } else { None }
    } else { None };

    let has_more = if let Some(gen_ref) = gen_ref_opt {
        let pos_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
        let pos = pos_val.as_long().unwrap_or(0);

        // On first iteration (pos=0), generator is already started by ForeachInit
        // On subsequent iterations, call next()
        if pos > 0 {
            let state = gen_ref.borrow().state;
            if state == crate::vm::generator::GeneratorState::Suspended {
                resume_generator(eg, &gen_ref, Value::null())?;
            }
        }

        let gen_data = gen_ref.borrow();
        if gen_data.state != crate::vm::generator::GeneratorState::Completed {
            // Write current value to value_cv
            let val_ptr = unsafe { (*frame).get_op_mut(val_cv, OpType::Cv) };
            unsafe { slot_set(val_ptr, gen_data.value.clone()) };
            // Write key if requested
            if key_encoded > 0 {
                let key_cv = key_encoded - 1;
                let key_ptr = unsafe { (*frame).get_op_mut(key_cv, OpType::Cv) };
                unsafe { slot_set(key_ptr, gen_data.key.clone()) };
            }
            drop(gen_data);
            // Increment position
            let pos_ptr = unsafe { (*frame).get_op_mut(opline.op2 as u32, opline.op2_type) };
            unsafe { slot_set(pos_ptr, Value::long(pos + 1)) };
            true
        } else {
            false
        }
    } else {
        let pos_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
        let pos = pos_val.as_long().unwrap_or(0) as usize;

        if let Some(arr) = arr_val.as_array() {
            if pos < arr.len() {
                if key_encoded > 0 {
                    // Need both key and value — use get_at()
                    let (val, key) = arr.get_at(pos).unwrap();
                    let val_ptr = unsafe { (*frame).get_op_mut(val_cv, OpType::Cv) };
                    unsafe { slot_set(val_ptr, val.clone()) };
                    let key_cv = key_encoded - 1;
                    let key_val = match key {
                        ArrayKey::Int(k) => Value::long(k),
                        ArrayKey::String(k) => Value::string(k),
                    };
                    let key_ptr = unsafe { (*frame).get_op_mut(key_cv, OpType::Cv) };
                    unsafe { slot_set(key_ptr, key_val) };
                } else {
                    // Only value needed — use get_value_at() (avoids key clone)
                    let val = arr.get_value_at(pos).unwrap();
                    let val_ptr = unsafe { (*frame).get_op_mut(val_cv, OpType::Cv) };
                    unsafe { slot_set(val_ptr, val.clone()) };
                }
                let pos_ptr = unsafe { (*frame).get_op_mut(opline.op2 as u32, opline.op2_type) };
                unsafe { slot_set(pos_ptr, Value::long((pos + 1) as i64)) };
                true
            } else {
                false
            }
        } else {
            false
        }
    };

    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
    unsafe { slot_set(result_ptr, Value::bool(has_more)) };
    Ok(())
}

#[inline(never)]
fn op_yield<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    use crate::vm::generator::GeneratorState;

    let yielded_value = if opline.op1_type != OpType::Unused {
        unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) }.clone()
    } else {
        Value::null()
    };

    let yielded_key = if opline.op2_type != OpType::Unused {
        Some(unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) }.clone())
    } else {
        None
    };

    if let Some(gen_ref) = eg.active_generator.take() {
        let mut gen_data = gen_ref.borrow_mut();

        // Set yielded value/key
        gen_data.value = yielded_value;
        if let Some(key) = yielded_key {
            gen_data.key = key;
        } else {
            gen_data.key = Value::long(gen_data.implicit_key);
            gen_data.implicit_key += 1;
        }

        // Save frame state back to generator
        let num_cvs = unsafe { (*frame).num_cvs } as usize;
        let num_temps = unsafe { (*frame).num_temps } as usize;
        gen_data.cv_values.clear();
        for i in 0..num_cvs {
            gen_data.cv_values.push(unsafe { (*frame).cv(i as u32) }.clone());
        }
        gen_data.tmp_values.clear();
        for i in 0..num_temps {
            gen_data.tmp_values.push(unsafe { (*frame).tmp(i as u32) }.clone());
        }

        // Save instruction pointer (advance past yield for resume)
        let base = op_array.instructions.as_ptr();
        gen_data.ip_offset = unsafe { (*frame).opline.offset_from(base) as usize + 1 };
        gen_data.state = GeneratorState::Suspended;

        gen_data.send_value = Value::null();

        drop(gen_data);
        eg.active_generator = Some(gen_ref);
    }

    // Return from generator frame (like OpCode::Return)
    let prev = unsafe { (*frame).prev_execute_data };
    if prev.is_null() {
        return Ok(ColdResult::Return);
    }
    eg.current_execute_data.set(prev);
    unsafe { cleanup_frame_slots(frame) };
    eg.vm_stack.pop_call_frame(frame);
    Ok(ColdResult::NewFrame(prev, unsafe { (*prev).op_array() }))
}

#[inline(never)]
fn op_yield_from<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    use crate::vm::generator::{GeneratorState, YieldFromDelegate};

    let source_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) }.clone();

    if let Some(gen_ref) = eg.active_generator.take() {
        let result_slot = opline.result as u32;

        // Determine delegate type
        if let Some(obj_data) = source_val.as_object() {
            if obj_data.class_name == "Generator" {
                if let Some(inner_gen_ref) = obj_data.generator.clone() {
                    drop(obj_data);
                    // Start inner generator if needed
                    {
                        let inner_state: GeneratorState = inner_gen_ref.borrow().state;
                        if inner_state == GeneratorState::Created {
                            eg.active_generator = Some(gen_ref.clone());
                            drop(eg.active_generator.take());
                            resume_generator(eg, &inner_gen_ref, Value::null())?;
                        }
                    }

                    let inner_state: GeneratorState = inner_gen_ref.borrow().state;
                    if inner_state == GeneratorState::Completed {
                        // Sub-generator already done, write return value to result
                        let ret_val = inner_gen_ref.borrow().return_value.clone();
                        eg.active_generator = Some(gen_ref);
                        // Write result to TMP and continue (don't suspend)
                        if opline.result_type != OpType::Unused {
                            let slot = unsafe { (*frame).slot_mut(result_slot) };
                            unsafe { frame_tmp_set(frame, slot as *mut Value, ret_val) };
                        }
                        unsafe { (*frame).opline = (*frame).opline.add(1); }
                        return Ok(ColdResult::Continue);
                    }

                    // Set up delegation
                    {
                        let mut gen_data = gen_ref.borrow_mut();
                        gen_data.delegate = Some(YieldFromDelegate::Generator(inner_gen_ref.clone()));
                        gen_data.yield_from_result_slot = result_slot;

                        // Copy inner generator's current value/key to outer
                        let inner = inner_gen_ref.borrow();
                        gen_data.value = inner.value.clone();
                        gen_data.key = inner.key.clone();

                        // Save frame state
                        let num_cvs = unsafe { (*frame).num_cvs } as usize;
                        let num_temps = unsafe { (*frame).num_temps } as usize;
                        gen_data.cv_values.clear();
                        for i in 0..num_cvs {
                            gen_data.cv_values.push(unsafe { (*frame).cv(i as u32) }.clone());
                        }
                        gen_data.tmp_values.clear();
                        for i in 0..num_temps {
                            gen_data.tmp_values.push(unsafe { (*frame).tmp(i as u32) }.clone());
                        }
                        let base = op_array.instructions.as_ptr();
                        gen_data.ip_offset = unsafe { (*frame).opline.offset_from(base) as usize };
                        gen_data.state = GeneratorState::Suspended;
                    }

                    eg.active_generator = Some(gen_ref);

                    // Pop frame like Yield
                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(ColdResult::Return);
                    }
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    return Ok(ColdResult::NewFrame(prev, unsafe { (*prev).op_array() }));
                }
            }
            drop(obj_data);
            eg.active_generator = Some(gen_ref);
            let err = make_error_value("Error", "Can use \"yield from\" only with arrays and Traversables");
            match throw_in_frame(eg, frame, err) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    return Ok(ColdResult::NewFrame(new_frame, new_op_array));
                }
                ThrowResult::Unhandled(exc) => {
                    return Ok(ColdResult::Unhandled(exc));
                }
            }
        } else if let Some(arr) = source_val.as_array() {
            let entries: Vec<(crate::value::ArrayKey, Value)> = arr.iter().map(|(k, v)| (k, v.clone())).collect();

            if entries.is_empty() {
                // Empty array — result is null, continue
                eg.active_generator = Some(gen_ref);
                if opline.result_type != OpType::Unused {
                    let slot = unsafe { (*frame).slot_mut(result_slot) };
                    unsafe { frame_tmp_set(frame, slot as *mut Value, Value::null()) };
                }
                unsafe { (*frame).opline = (*frame).opline.add(1); }
                return Ok(ColdResult::Continue);
            }

            // Set up array delegation
            {
                let mut gen_data = gen_ref.borrow_mut();
                // Yield first element
                let (ref key, ref val) = entries[0];
                gen_data.value = val.clone();
                gen_data.key = match key {
                    crate::value::ArrayKey::Int(i) => Value::long(*i),
                    crate::value::ArrayKey::String(s) => Value::string(s.clone()),
                };
                gen_data.delegate = Some(YieldFromDelegate::Array(entries, 1)); // position after first
                gen_data.yield_from_result_slot = result_slot;

                // Save frame state
                let num_cvs = unsafe { (*frame).num_cvs } as usize;
                let num_temps = unsafe { (*frame).num_temps } as usize;
                gen_data.cv_values.clear();
                for i in 0..num_cvs {
                    gen_data.cv_values.push(unsafe { (*frame).cv(i as u32) }.clone());
                }
                gen_data.tmp_values.clear();
                for i in 0..num_temps {
                    gen_data.tmp_values.push(unsafe { (*frame).tmp(i as u32) }.clone());
                }
                let base = op_array.instructions.as_ptr();
                gen_data.ip_offset = unsafe { (*frame).opline.offset_from(base) as usize };
                gen_data.state = GeneratorState::Suspended;
            }

            eg.active_generator = Some(gen_ref);

            // Pop frame like Yield
            let prev = unsafe { (*frame).prev_execute_data };
            if prev.is_null() {
                return Ok(ColdResult::Return);
            }
            eg.current_execute_data.set(prev);
            unsafe { cleanup_frame_slots(frame) };
            eg.vm_stack.pop_call_frame(frame);
            return Ok(ColdResult::NewFrame(prev, unsafe { (*prev).op_array() }));
        } else {
            eg.active_generator = Some(gen_ref);
            let err = make_error_value("Error", "Can use \"yield from\" only with arrays and Traversables");
            match throw_in_frame(eg, frame, err) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    return Ok(ColdResult::NewFrame(new_frame, new_op_array));
                }
                ThrowResult::Unhandled(exc) => {
                    return Ok(ColdResult::Unhandled(exc));
                }
            }
        }
    } else {
        return Err(VmError::Fatal("yield from outside generator".into()));
    }
}

#[inline(never)]
fn op_send_named<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // Named argument: op1=value, op2=CONST name string
    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let name = name_val.as_str().unwrap_or("");
    let call = unsafe { (*frame).call };
    debug_assert!(!call.is_null());
    let func_common = unsafe { &*(*call).func };

    // Find the parameter position by name
    let mut resolved_idx: Option<u32> = None;
    for (idx, pname) in func_common.sig.param_names.iter().enumerate() {
        if pname == name {
            resolved_idx = Some(idx as u32);
            break;
        }
    }

    // Determine if the resolved index targets the variadic parameter itself.
    let public_max = func_common.sig.public_arity();
    let is_variadic_target = func_common.sig.is_variadic && match resolved_idx {
        Some(idx) => idx >= public_max,
        None => true,
    };

    if is_variadic_target {
        if !func_common.sig.is_variadic
            || func_common.fn_type == crate::vm::function::FunctionType::Internal
        {
            let err = make_error_value("Error", &format!(
                "Unknown named parameter ${}", name
            ));
            match unsafe { cleanup_call_and_throw(eg, frame, call, err) } {
                ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
            }
        }

        // Duplicate check: scan the pending buffer for this name
        let call_key = call as usize;
        if let Some(existing) = eg.pending_named_variadic.get(&call_key) {
            if existing.iter().any(|(n, _)| n == name) {
                let err = make_error_value("Error", &format!(
                    "Named parameter ${} overwrites previous argument", name
                ));
                match unsafe { cleanup_call_and_throw(eg, frame, call, err) } {
                    ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                    ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                }
            }
        }

        let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
        let cloned = val.clone();
        eg.pending_named_variadic
            .entry(call_key)
            .or_insert_with(Vec::new)
            .push((name.to_string(), cloned));
        // This named arg doesn't occupy a CV slot, so decrement
        // num_args so DoFcall's positional variadic count is correct.
        unsafe {
            if (*call).num_args > 0 {
                (*call).num_args -= 1;
            }
        }
    } else {
        // Mark frame as having named args — FastScalar uses this to skip holes check.
        unsafe { (*call).named_args_used = true; }
        match resolved_idx {
            Some(idx) => {
                let cv_idx = func_common.sig.param_cv_index(idx);

                // Check for duplicate: if CV slot already has a non-undef value,
                // the parameter was already passed (positionally or by a prior named arg).
                let existing = unsafe { &*(*call).cv(cv_idx) };
                if !existing.is_undef() {
                    let err = make_error_value("Error", &format!(
                        "Named parameter ${} overwrites previous argument", name
                    ));
                    match unsafe { cleanup_call_and_throw(eg, frame, call, err) } {
                        ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                        ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                    }
                }

                let is_ref = func_common.sig.is_param_by_ref(idx);

                if is_ref && opline.op1_type == OpType::Cv {
                    // By-reference: same logic as SendRef
                    let caller_cv_ptr = unsafe {
                        let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
                        let raw_ptr = base.add(opline.op1 as usize);
                        if (*raw_ptr).is_reference() {
                            (*raw_ptr).as_ref_ptr()
                        } else {
                            raw_ptr
                        }
                    };
                    let arg_slot = unsafe { (*call).cv_mut(cv_idx) };
                    unsafe { frame_slot_init(call, arg_slot as *mut Value, Value::reference(caller_cv_ptr)) };
                } else {
                    // By-value: same logic as SendVal
                    let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                    let cloned = val.clone();
                    let arg_slot = unsafe { (*call).cv_mut(cv_idx) };
                    unsafe { frame_slot_init(call, arg_slot as *mut Value, cloned) };
                }

                // Update num_args to cover this position
                let public_pos = idx + 1; // 1-based count
                unsafe {
                    if (*call).num_args < public_pos {
                        (*call).num_args = public_pos;
                    }
                }
            }
            None => {
                let err = make_error_value("Error", &format!(
                    "Unknown named parameter ${}", name
                ));
                match unsafe { cleanup_call_and_throw(eg, frame, call, err) } {
                    ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                    ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                }
            }
        }
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_nullsafe_check(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<bool, VmError> {
    let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let is_null = val.value_type() == ValueType::Null;
    let is_non_object = !is_null && val.as_object().is_none();

    if is_null {
        // null ?-> anything  =>  null (short-circuit)
        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
        unsafe { slot_set(result_ptr, Value::null()) };
        let target = opline.op2 as usize;
        unsafe {
            (*frame).opline = op_array.instructions.as_ptr().add(target);
        }
        return Ok(true); // continue
    } else if is_non_object {
        // extended_value: 0 = property access (warning + null), 1 = method call (fatal)
        if opline.extended_value == 1 {
            // Method call on scalar: fatal error (like PHP)
            return Err(VmError::Fatal(
                "Call to a member function on a non-object".into()
            ));
        } else {
            // Property access on scalar: warning + null (like PHP)
            eg.write_output(b"Warning: Attempt to read property on non-object\n");
            let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
            unsafe { slot_set(result_ptr, Value::null()) };
            let target = opline.op2 as usize;
            unsafe {
                (*frame).opline = op_array.instructions.as_ptr().add(target);
            }
            return Ok(true); // continue
        }
    }
    Ok(false)
}

#[inline(never)]
fn op_clone_obj<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let src_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

    if src_val.value_type() != ValueType::Object {
        return Err(VmError::Fatal(
            "__clone method called on non-object".into()
        ));
    }

    // Enum cases are singletons — cloning is forbidden
    {
        let obj = src_val.as_object().unwrap();
        if let Some(class_def) = eg.class_table.get(&obj.class_name) {
            if class_def.is_enum {
                let err = make_error_value("Error", &format!(
                    "Trying to clone an uncloneable object of class {}", obj.class_name
                ));
                drop(obj);
                match throw_in_frame(eg, frame, err) {
                    ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                    ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                }
            }
        }
    }

    let cloned_obj = {
        let obj = src_val.as_object().unwrap();
        PhpObject {
            class_name: obj.class_name.clone(),
            class_id: obj.class_id,
            property_layout: obj.property_layout.clone(),
            property_values: obj.property_values.clone(),
            dynamic_properties: obj.dynamic_properties.clone(),
            generator: None,
        }
    };
    let cloned_val = Value::object(cloned_obj);

    let _ = call_magic_method(eg, &cloned_val, "__clone", &[])?;

    // If __clone threw an exception, propagate it
    if let Some(exc) = eg.exception.take() {
        match throw_in_frame(eg, frame, exc) {
            ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
            ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
        }
    }

    unsafe { slot_set(result_ptr, cloned_val) };
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_concat(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

    // Fast path: both operands are strings — avoid echo_to_string() heap allocation.
    if op1.value_type() == ValueType::String && op2.value_type() == ValueType::String {
        let s1 = op1.as_str().unwrap();
        let s2 = op2.as_str().unwrap();
        let mut concatenated = String::with_capacity(s1.len() + s2.len());
        concatenated.push_str(s1);
        concatenated.push_str(s2);
        unsafe { frame_tmp_set(frame, result_ptr, Value::string(concatenated)) };
        return Ok(());
    }

    // Fast path: string . int — avoids echo_to_string heap alloc for the int.
    if op1.value_type() == ValueType::String && op2.value_type() == ValueType::Long {
        let s1 = op1.as_str().unwrap();
        use std::fmt::Write;
        let mut concatenated = String::with_capacity(s1.len() + 20);
        concatenated.push_str(s1);
        write!(concatenated, "{}", unsafe { op2.raw_long() }).unwrap();
        unsafe { frame_tmp_set(frame, result_ptr, Value::string(concatenated)) };
        return Ok(());
    }

    // Fast path: int . string
    if op1.value_type() == ValueType::Long && op2.value_type() == ValueType::String {
        let s2 = op2.as_str().unwrap();
        use std::fmt::Write;
        let mut concatenated = String::with_capacity(20 + s2.len());
        write!(concatenated, "{}", unsafe { op1.raw_long() }).unwrap();
        concatenated.push_str(s2);
        unsafe { frame_tmp_set(frame, result_ptr, Value::string(concatenated)) };
        return Ok(());
    }

    // Slow path: at least one operand is non-string/non-int (object, float, etc).
    // Stringify each, then concatenate with pre-allocated capacity.
    let s1 = if op1.value_type() == ValueType::Object {
        if let Some(result) = call_magic_method(eg, op1, "__tostring", &[])? {
            result.echo_to_string()
        } else {
            op1.echo_to_string()
        }
    } else {
        op1.echo_to_string()
    };
    let s2 = if op2.value_type() == ValueType::Object {
        if let Some(result) = call_magic_method(eg, op2, "__tostring", &[])? {
            result.echo_to_string()
        } else {
            op2.echo_to_string()
        }
    } else {
        op2.echo_to_string()
    };
    let mut concatenated = String::with_capacity(s1.len() + s2.len());
    concatenated.push_str(&s1);
    concatenated.push_str(&s2);
    unsafe { frame_tmp_set(frame, result_ptr, Value::string(concatenated)) };
    Ok(())
}

#[cfg(feature = "quick-loops")]
pub(super) enum QuickLoopOutcome {
    Completed,
    Deoptimized,
    GuardFailed,
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
pub(super) unsafe fn quick_loop_slot_has_heap(frame: *mut ExecuteData, slot: u16) -> bool {
    (*frame).heap_bitmap & (1u64 << slot) != 0
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_induction_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: QuickLongInductionLoop,
) -> Result<QuickLoopOutcome, VmError> {
    if (*frame).num_cvs != op_array.num_cvs
        || (*frame).num_cvs + (*frame).num_temps > 64
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let slot_base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    let induction_ptr = slot_base.add(plan.induction_cv as usize);
    let condition_ptr = plan
        .condition_tmp
        .map(|slot| slot_base.add(slot as usize));
    let increment_ptr = plan
        .increment_tmp
        .map(|slot| slot_base.add(slot as usize));
    let bound_ptr = match plan.bound {
        QuickLongBound::Cv(slot) => Some(slot_base.add(slot as usize)),
        QuickLongBound::Const(_) => None,
    };

    if quick_loop_slot_has_heap(frame, plan.induction_cv)
        || plan
            .condition_tmp
            .is_some_and(|slot| quick_loop_slot_has_heap(frame, slot))
        || plan
            .increment_tmp
            .is_some_and(|slot| quick_loop_slot_has_heap(frame, slot))
        || matches!(plan.bound, QuickLongBound::Cv(slot) if quick_loop_slot_has_heap(frame, slot))
        || (*induction_ptr).value_type() != ValueType::Long
        || condition_ptr.is_some_and(|ptr| {
            !matches!((*ptr).value_type(), ValueType::True | ValueType::False)
        })
        || increment_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
        || bound_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let mut induction = (*induction_ptr).raw_long();
    let bound = match plan.bound {
        QuickLongBound::Cv(_) => (*bound_ptr.unwrap_unchecked()).raw_long(),
        QuickLongBound::Const(value) => value,
    };
    let mut iterations = 0u64;
    let mut last_increment_result = 0i64;
    let mut completed_iteration = false;

    loop {
        if induction >= bound {
            Value::write_long(induction_ptr, induction);
            if let Some(ptr) = condition_ptr {
                Value::write_bool(ptr, false);
            }
            if completed_iteration {
                if let Some(ptr) = increment_ptr {
                    Value::write_long(ptr, last_increment_result);
                }
            }
            (*frame).opline = op_array.instructions.as_ptr().add(plan.exit_ip);
            stats::inc_quick_loop_completed(iterations);
            return Ok(QuickLoopOutcome::Completed);
        }

        let next_induction = match induction.checked_add(1) {
            Some(value) => value,
            None => {
                Value::write_long(induction_ptr, induction);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.increment_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
        };

        last_increment_result = match plan.increment_kind {
            QuickIncrementKind::Pre => next_induction,
            QuickIncrementKind::Post => induction,
        };
        induction = next_induction;
        completed_iteration = true;
        iterations += 1;

        // The baseline region has four instructions, so checking every 64
        // iterations preserves execute_ex's 256-opcode interrupt interval.
        if iterations & 63 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            Value::write_long(induction_ptr, induction);
            if let Some(ptr) = condition_ptr {
                Value::write_bool(ptr, true);
            }
            if let Some(ptr) = increment_ptr {
                Value::write_long(ptr, last_increment_result);
            }
            (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
            handle_interrupt(eg)?;
        }
    }
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_accumulate_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: QuickLongAccumulateLoop,
) -> Result<QuickLoopOutcome, VmError> {
    if (*frame).num_cvs != op_array.num_cvs
        || (*frame).num_cvs + (*frame).num_temps > 64
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let slot_base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    let induction_ptr = slot_base.add(plan.induction_cv as usize);
    let accumulator_ptr = slot_base.add(plan.accumulator_cv as usize);
    let condition_ptr = plan
        .condition_tmp
        .map(|slot| slot_base.add(slot as usize));
    let term_ptr = match plan.term {
        QuickLongTerm::Induction => None,
        QuickLongTerm::InductionPlusConst { term_tmp, .. }
        | QuickLongTerm::InductionPlusCv { term_tmp, .. }
        | QuickLongTerm::ArrayIndex { term_tmp, .. }
        | QuickLongTerm::StringLength { term_tmp, .. }
        | QuickLongTerm::AbsLong { term_tmp, .. } => {
            Some(slot_base.add(term_tmp as usize))
        }
    };
    let term_destination_ptr = match plan.term {
        QuickLongTerm::ArrayIndex {
            destination: Some(destination),
            ..
        } => Some(slot_base.add(destination as usize)),
        _ => None,
    };
    let addend_ptr = match plan.term {
        QuickLongTerm::Induction
        | QuickLongTerm::InductionPlusConst { .. }
        | QuickLongTerm::ArrayIndex { .. }
        | QuickLongTerm::StringLength { .. }
        | QuickLongTerm::AbsLong { .. } => None,
        QuickLongTerm::InductionPlusCv { addend_cv, .. } => {
            Some(slot_base.add(addend_cv as usize))
        }
    };
    let array_ptr = match plan.term {
        QuickLongTerm::ArrayIndex { array_cv, .. } => {
            Some(slot_base.add(array_cv as usize))
        }
        _ => None,
    };
    let string_ptr = match plan.term {
        QuickLongTerm::StringLength { string_cv, .. } => {
            Some(slot_base.add(string_cv as usize))
        }
        _ => None,
    };
    let abs_operand_ptr = match plan.term {
        QuickLongTerm::AbsLong { operand_cv, .. } => {
            Some(slot_base.add(operand_cv as usize))
        }
        _ => None,
    };
    let sum_ptr = slot_base.add(plan.sum_tmp as usize);
    let increment_ptr = plan
        .increment_tmp
        .map(|slot| slot_base.add(slot as usize));

    let bound_ptr = match plan.bound {
        QuickLongBound::Cv(slot) => Some(slot_base.add(slot as usize)),
        QuickLongBound::Const(_) => None,
    };

    if quick_loop_slot_has_heap(frame, plan.induction_cv)
        || quick_loop_slot_has_heap(frame, plan.accumulator_cv)
        || plan
            .condition_tmp
            .is_some_and(|slot| quick_loop_slot_has_heap(frame, slot))
        || matches!(
            plan.term,
            QuickLongTerm::InductionPlusConst { term_tmp, .. }
                | QuickLongTerm::InductionPlusCv { term_tmp, .. }
                | QuickLongTerm::ArrayIndex { term_tmp, .. }
                | QuickLongTerm::StringLength { term_tmp, .. }
                | QuickLongTerm::AbsLong { term_tmp, .. }
                if quick_loop_slot_has_heap(frame, term_tmp)
        )
        || matches!(
            plan.term,
            QuickLongTerm::InductionPlusCv { addend_cv, .. }
                if quick_loop_slot_has_heap(frame, addend_cv)
        )
        || matches!(
            plan.term,
            QuickLongTerm::AbsLong { operand_cv, .. }
                if quick_loop_slot_has_heap(frame, operand_cv)
        )
        || matches!(
            plan.term,
            QuickLongTerm::ArrayIndex {
                destination: Some(destination),
                ..
            } if quick_loop_slot_has_heap(frame, destination)
        )
        || quick_loop_slot_has_heap(frame, plan.sum_tmp)
        || plan
            .increment_tmp
            .is_some_and(|slot| quick_loop_slot_has_heap(frame, slot))
        || matches!(plan.bound, QuickLongBound::Cv(slot) if quick_loop_slot_has_heap(frame, slot))
        || (*induction_ptr).value_type() != ValueType::Long
        || (*accumulator_ptr).value_type() != ValueType::Long
        || condition_ptr.is_some_and(|ptr| {
            !matches!((*ptr).value_type(), ValueType::True | ValueType::False)
        })
        || term_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
        || term_destination_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
        || addend_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
        || array_ptr.is_some_and(|ptr| (*ptr).as_array().is_none())
        || string_ptr.is_some_and(|ptr| (*ptr).as_str().is_none())
        || abs_operand_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
        || (*sum_ptr).value_type() != ValueType::Long
        || increment_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
        || bound_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let mut induction = (*induction_ptr).raw_long();
    let mut accumulator = (*accumulator_ptr).raw_long();
    let bound = match plan.bound {
        QuickLongBound::Cv(_) => (*bound_ptr.unwrap_unchecked()).raw_long(),
        QuickLongBound::Const(value) => value,
    };
    let invariant_addend = addend_ptr.map(|ptr| (*ptr).raw_long());
    let quick_array = array_ptr.map(|ptr| {
        QuickLongArray::from_array((*ptr).as_array().unwrap_unchecked())
    });
    let invariant_string_length = string_ptr.map(|ptr| {
        (*ptr).as_str().unwrap_unchecked().len() as i64
    });
    let invariant_abs = match plan.term {
        QuickLongTerm::AbsLong { operand_cv, .. } if operand_cv != plan.induction_cv => {
            (*abs_operand_ptr.unwrap_unchecked()).raw_long().checked_abs()
        }
        _ => Some(0),
    };
    let invariant_array_term = match plan.term {
        QuickLongTerm::ArrayIndex {
            index: QuickArrayIndex::Long(QuickLongOperand::Const(index)),
            ..
        } => quick_array.unwrap_unchecked().long_at_int(index),
        QuickLongTerm::ArrayIndex {
            index: QuickArrayIndex::StringLiteral(literal),
            ..
        } => {
            let key = op_array
                .literals
                .get_unchecked(literal as usize)
                .as_str()
                .unwrap_unchecked();
            quick_array.unwrap_unchecked().long_at_str(key)
        }
        QuickLongTerm::ArrayIndex {
            index: QuickArrayIndex::ValueSlot(slot),
            ..
        } => match value_to_array_key_ref(&*slot_base.add(slot as usize)).ok() {
            Some(ArrayKeyRef::Int(key)) => quick_array.unwrap_unchecked().long_at_int(key),
            Some(ArrayKeyRef::String(key)) => quick_array.unwrap_unchecked().long_at_str(key),
            None => None,
        },
        _ => Some(0),
    };
    if invariant_array_term.is_none() || invariant_abs.is_none() {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }
    let mut iterations = 0u64;
    let mut last_term = 0i64;
    let mut last_increment_result = 0i64;
    let mut completed_iteration = false;

    loop {
        if induction >= bound {
            Value::write_long(induction_ptr, induction);
            Value::write_long(accumulator_ptr, accumulator);
            if let Some(ptr) = condition_ptr {
                Value::write_bool(ptr, false);
            }
            if completed_iteration {
                if let Some(ptr) = term_ptr {
                    Value::write_long(ptr, last_term);
                }
                if let Some(ptr) = term_destination_ptr {
                    Value::write_long(ptr, last_term);
                }
                Value::write_long(sum_ptr, accumulator);
                if let Some(ptr) = increment_ptr {
                    Value::write_long(ptr, last_increment_result);
                }
            }
            (*frame).opline = op_array.instructions.as_ptr().add(plan.exit_ip);
            stats::inc_quick_loop_completed(iterations);
            return Ok(QuickLoopOutcome::Completed);
        }

        let term = match plan.term {
            QuickLongTerm::Induction => induction,
            QuickLongTerm::InductionPlusConst {
                addend, term_ip, ..
            } => match induction.checked_add(addend) {
                Some(value) => value,
                None => {
                    Value::write_long(induction_ptr, induction);
                    Value::write_long(accumulator_ptr, accumulator);
                    if let Some(ptr) = condition_ptr {
                        Value::write_bool(ptr, true);
                    }
                    (*frame).opline = op_array.instructions.as_ptr().add(term_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                }
            },
            QuickLongTerm::InductionPlusCv { term_ip, .. } => {
                let addend = invariant_addend.unwrap_unchecked();
                match induction.checked_add(addend) {
                    Some(value) => value,
                    None => {
                        Value::write_long(induction_ptr, induction);
                        Value::write_long(accumulator_ptr, accumulator);
                        if let Some(ptr) = condition_ptr {
                            Value::write_bool(ptr, true);
                        }
                        (*frame).opline = op_array.instructions.as_ptr().add(term_ip);
                        stats::inc_quick_loop_deoptimized(iterations);
                        return Ok(QuickLoopOutcome::Deoptimized);
                    }
                }
            }
            QuickLongTerm::ArrayIndex {
                index, fetch_ip, ..
            } => {
                let fetched = match index {
                    QuickArrayIndex::Long(QuickLongOperand::Slot(_)) => {
                        quick_array.unwrap_unchecked().long_at_int(induction)
                    }
                    QuickArrayIndex::Long(QuickLongOperand::Const(_))
                    | QuickArrayIndex::StringLiteral(_)
                    | QuickArrayIndex::ValueSlot(_) => invariant_array_term,
                };
                let Some(fetched) = fetched else {
                    Value::write_long(induction_ptr, induction);
                    Value::write_long(accumulator_ptr, accumulator);
                    if let Some(ptr) = condition_ptr {
                        Value::write_bool(ptr, true);
                    }
                    (*frame).opline = op_array.instructions.as_ptr().add(fetch_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                };
                fetched
            }
            QuickLongTerm::StringLength { .. } => {
                invariant_string_length.unwrap_unchecked()
            }
            QuickLongTerm::AbsLong {
                operand_cv,
                term_ip,
                ..
            } => {
                if operand_cv != plan.induction_cv {
                    invariant_abs.unwrap_unchecked()
                } else {
                    match induction.checked_abs() {
                        Some(value) => value,
                        None => {
                            Value::write_long(induction_ptr, induction);
                            Value::write_long(accumulator_ptr, accumulator);
                            if let Some(ptr) = condition_ptr {
                                Value::write_bool(ptr, true);
                            }
                            (*frame).opline =
                                op_array.instructions.as_ptr().add(term_ip);
                            stats::inc_quick_loop_deoptimized(iterations);
                            return Ok(QuickLoopOutcome::Deoptimized);
                        }
                    }
                }
            }
        };

        let next_accumulator = match accumulator.checked_add(term) {
            Some(value) => value,
            None => {
                Value::write_long(induction_ptr, induction);
                Value::write_long(accumulator_ptr, accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if let Some(ptr) = term_ptr {
                    Value::write_long(ptr, term);
                }
                if let Some(ptr) = term_destination_ptr {
                    Value::write_long(ptr, term);
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.sum_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
        };

        let next_induction = match induction.checked_add(1) {
            Some(value) => value,
            None => {
                Value::write_long(induction_ptr, induction);
                Value::write_long(accumulator_ptr, next_accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if let Some(ptr) = term_ptr {
                    Value::write_long(ptr, term);
                }
                if let Some(ptr) = term_destination_ptr {
                    Value::write_long(ptr, term);
                }
                Value::write_long(sum_ptr, next_accumulator);
                (*frame).opline = op_array.instructions.as_ptr().add(plan.increment_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
        };

        last_increment_result = match plan.increment_kind {
            QuickIncrementKind::Pre => next_induction,
            QuickIncrementKind::Post => induction,
        };
        last_term = term;
        induction = next_induction;
        accumulator = next_accumulator;
        completed_iteration = true;
        iterations += 1;

        // One quick iteration represents seven or eight baseline instructions.
        // Checking every 32 iterations preserves approximately the same
        // interrupt bound as execute_ex's 256-opcode batch.
        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            Value::write_long(induction_ptr, induction);
            Value::write_long(accumulator_ptr, accumulator);
            if let Some(ptr) = condition_ptr {
                Value::write_bool(ptr, true);
            }
            if let Some(ptr) = term_ptr {
                Value::write_long(ptr, last_term);
            }
            if let Some(ptr) = term_destination_ptr {
                Value::write_long(ptr, last_term);
            }
            Value::write_long(sum_ptr, accumulator);
            if let Some(ptr) = increment_ptr {
                Value::write_long(ptr, last_increment_result);
            }
            (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
            handle_interrupt(eg)?;
        }
    }
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn commit_quick_long_ops_slots(
    slot_base: *mut Value,
    slots: &[i64; 64],
    mut dirty_long_mask: u64,
    mut dirty_bool_mask: u64,
) {
    while dirty_long_mask != 0 {
        let slot = dirty_long_mask.trailing_zeros() as usize;
        dirty_long_mask &= dirty_long_mask - 1;
        Value::write_long(slot_base.add(slot), slots[slot]);
    }
    while dirty_bool_mask != 0 {
        let slot = dirty_bool_mask.trailing_zeros() as usize;
        dirty_bool_mask &= dirty_bool_mask - 1;
        Value::write_bool(slot_base.add(slot), slots[slot] != 0);
    }
}

/// Borrowed view of an immutable PHP array for one guarded region.
///
/// The planner rejects writes and calls in the region, the array slot cannot
/// overlap a scalar output, and PHP array aliases detach through copy-on-write.
/// The source `Value` therefore keeps this allocation alive and stable until
/// the region completes or takes a side exit.
#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
enum QuickLongArray {
    Empty,
    Packed {
        values: *const Value,
        len: usize,
    },
    Hash {
        array: *const PhpArray,
    },
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickStringFetchCacheEntry {
    key_data: *const u8,
    key_len: usize,
    array_slot: u16,
    value: i64,
    valid: bool,
}

#[cfg(feature = "quick-loops")]
impl QuickStringFetchCacheEntry {
    const EMPTY: Self = Self {
        key_data: std::ptr::null(),
        key_len: 0,
        array_slot: 0,
        value: 0,
        valid: false,
    };
}

#[cfg(feature = "quick-loops")]
struct QuickStringFetchCache {
    entries: [QuickStringFetchCacheEntry; QUICK_STRING_FETCH_CACHE_LIMIT],
    capacity: usize,
    next: usize,
}

/// Retained string CV state for one closed quick region. The frame keeps the
/// original string alive while the region runs; assignments only redirect a
/// slot to immutable OpArray literals. Drop commits every dirty CV on all
/// completion and deoptimization returns.
#[cfg(feature = "quick-loops")]
struct QuickStringSlotState {
    slot_base: *mut Value,
    values: [*const Value; 64],
    dirty_mask: u64,
}

#[cfg(feature = "quick-loops")]
impl QuickStringSlotState {
    #[inline]
    unsafe fn new(slot_base: *mut Value, mut input_mask: u64) -> Self {
        let mut values = [std::ptr::null(); 64];
        while input_mask != 0 {
            let slot = input_mask.trailing_zeros() as usize;
            input_mask &= input_mask - 1;
            values[slot] = slot_base.add(slot);
        }
        Self {
            slot_base,
            values,
            dirty_mask: 0,
        }
    }

    #[inline(always)]
    unsafe fn value(&self, slot: u16) -> &Value {
        &*self.values[slot as usize]
    }

    #[inline(always)]
    fn assign_literal(&mut self, slot: u16, value: *const Value) {
        debug_assert!(!value.is_null());
        self.values[slot as usize] = value;
        self.dirty_mask |= 1u64 << slot;
    }

    #[inline(always)]
    fn assign_slot(&mut self, destination: u16, source: u16) {
        let value = self.values[source as usize];
        debug_assert!(!value.is_null());
        self.values[destination as usize] = value;
        self.dirty_mask |= 1u64 << destination;
    }

    #[inline]
    unsafe fn commit(&mut self) {
        while self.dirty_mask != 0 {
            let slot = self.dirty_mask.trailing_zeros() as usize;
            self.dirty_mask &= self.dirty_mask - 1;
            let value = (&*self.values[slot]).clone();
            debug_assert_eq!(value.value_type(), ValueType::String);
            slot_set(self.slot_base.add(slot), value);
        }
    }
}

#[cfg(feature = "quick-loops")]
impl Drop for QuickStringSlotState {
    fn drop(&mut self) {
        unsafe { self.commit() };
    }
}

#[cfg(feature = "quick-loops")]
impl QuickStringFetchCache {
    #[inline]
    const fn new(capacity: u8) -> Self {
        Self {
            entries: [QuickStringFetchCacheEntry::EMPTY; QUICK_STRING_FETCH_CACHE_LIMIT],
            capacity: capacity as usize,
            next: 0,
        }
    }

    /// Cache a successful long fetch by immutable string allocation identity.
    /// The planner proves that both the array slot and string key can only be
    /// read or replaced by immutable literals for the lifetime of this region.
    #[inline(always)]
    unsafe fn long_at(
        &mut self,
        array_slot: u16,
        array: QuickLongArray,
        key: &str,
    ) -> Option<i64> {
        let key_data = key.as_ptr();
        let key_len = key.len();
        if self.capacity != 0
            && self.entries[0].valid
            && self.entries[0].array_slot == array_slot
            && self.entries[0].key_data == key_data
            && self.entries[0].key_len == key_len
        {
            return Some(self.entries[0].value);
        }
        if self.capacity > 1
            && self.entries[1].valid
            && self.entries[1].array_slot == array_slot
            && self.entries[1].key_data == key_data
            && self.entries[1].key_len == key_len
        {
            return Some(self.entries[1].value);
        }
        if self.capacity > 2
            && self.entries[2].valid
            && self.entries[2].array_slot == array_slot
            && self.entries[2].key_data == key_data
            && self.entries[2].key_len == key_len
        {
            return Some(self.entries[2].value);
        }
        if self.capacity > 3
            && self.entries[3].valid
            && self.entries[3].array_slot == array_slot
            && self.entries[3].key_data == key_data
            && self.entries[3].key_len == key_len
        {
            return Some(self.entries[3].value);
        }

        let value = match canonical_decimal_array_key(key) {
            Some(key) => array.long_at_int(key),
            None => array.long_at_str(key),
        }?;
        if self.capacity != 0 {
            self.entries[self.next] = QuickStringFetchCacheEntry {
                key_data,
                key_len,
                array_slot,
                value,
                valid: true,
            };
            self.next += 1;
            if self.next == self.capacity {
                self.next = 0;
            }
        }
        Some(value)
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongIntPositionHint {
    first_key: i64,
    stride: i64,
}

#[cfg(feature = "quick-loops")]
impl QuickLongArray {
    const EMPTY: Self = Self::Empty;

    #[inline]
    fn from_array(array: &PhpArray) -> Self {
        match array.packed_values() {
            Some(values) => Self::Packed {
                values: values.as_ptr(),
                len: values.len(),
            },
            None => Self::Hash {
                array: array as *const PhpArray,
            },
        }
    }

    #[inline(always)]
    unsafe fn long_at_int(self, index: i64) -> Option<i64> {
        let value = match self {
            Self::Packed { values, len } if index >= 0 && (index as usize) < len => {
                &*values.add(index as usize)
            }
            Self::Hash { array } => (*array).get_int(index)?,
            Self::Empty | Self::Packed { .. } => return None,
        };
        (value.value_type() == ValueType::Long).then(|| value.raw_long())
    }

    #[inline(always)]
    unsafe fn long_at_str(self, key: &str) -> Option<i64> {
        let Self::Hash { array } = self else {
            return None;
        };
        let value = (*array).get_str(key)?;
        (value.value_type() == ValueType::Long).then(|| value.raw_long())
    }

    #[inline(always)]
    unsafe fn long_at(
        self,
        index: QuickArrayIndex,
        slots: &[i64; 64],
        op_array: &crate::compiler::OpArray,
    ) -> Option<i64> {
        match index {
            QuickArrayIndex::Long(index) => {
                self.long_at_int(quick_long_operand(slots, index))
            }
            QuickArrayIndex::StringLiteral(literal) => self.long_at_str(
                op_array
                    .literals
                    .get_unchecked(literal as usize)
                    .as_str()
                    .unwrap_unchecked(),
            ),
            QuickArrayIndex::ValueSlot(_) => None,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongConditionalKernel {
    header_lhs: u16,
    header_rhs: QuickLongOperand,
    header_condition_tmp: Option<u16>,
    add_lhs: u16,
    add_rhs: u16,
    add_result: u16,
    destination: u16,
    add_resume_ip: usize,
    post_value: u16,
    post_result: Option<u16>,
    post_resume_ip: usize,
    body_target: QuickLongTarget,
    exit_target: QuickLongTarget,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
enum QuickLongConditionalBody {
    LessThan {
        lhs: u16,
        rhs: QuickLongOperand,
        condition_tmp: Option<u16>,
    },
    ModuloEqual {
        value: u16,
        divisor: i64,
        result: u16,
        resume_ip: usize,
        lhs: u16,
        rhs: QuickLongOperand,
        condition_tmp: Option<u16>,
    },
}

const QUICK_LONG_BRANCH_CONDITION_LIMIT: usize = 8;

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongBranchCondition {
    lhs: u16,
    rhs: QuickLongOperand,
    condition_tmp: Option<u16>,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongBranchOnlyKernel {
    header_lhs: u16,
    header_rhs: QuickLongOperand,
    header_condition_tmp: Option<u16>,
    conditions: [QuickLongBranchCondition; QUICK_LONG_BRANCH_CONDITION_LIMIT],
    condition_count: u8,
    post_value: u16,
    post_result: Option<u16>,
    post_resume_ip: usize,
    body_target: QuickLongTarget,
    exit_target: QuickLongTarget,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongAddAssignKernel {
    lhs: u16,
    rhs: u16,
    result: u16,
    destination: u16,
    resume_ip: usize,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongAddAddAssignKernel {
    first_lhs: u16,
    first_rhs: u16,
    first_result: u16,
    second_lhs: u16,
    second_rhs: u16,
    second_result: u16,
    destination: u16,
    first_resume_ip: usize,
    second_resume_ip: usize,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongConditionalAddAssignKernel {
    condition: QuickLongCondition,
    condition_tmp: Option<u16>,
    lhs: u16,
    rhs: u16,
    result: u16,
    destination: u16,
    add_resume_ip: usize,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
enum QuickLongArrayBodyKernel {
    OneAdd {
        add: QuickLongAddAssignKernel,
    },
    TwoAdds {
        first: QuickLongAddAssignKernel,
        second: QuickLongAddAssignKernel,
    },
    AddFusedAddAdd {
        first: QuickLongAddAssignKernel,
        middle: QuickLongAddAddAssignKernel,
        last: QuickLongAddAssignKernel,
    },
    ConditionalAdd {
        first: QuickLongConditionalAddAssignKernel,
        second: QuickLongAddAssignKernel,
    },
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongArrayLoopKernel {
    header_lhs: u16,
    header_rhs: QuickLongOperand,
    header_condition_tmp: Option<u16>,
    array: u16,
    index: QuickArrayIndex,
    fetch_result: u16,
    fetch_destination: Option<u16>,
    fetch_resume_ip: usize,
    post_value: u16,
    post_result: Option<u16>,
    post_resume_ip: usize,
    body_target: QuickLongTarget,
    exit_target: QuickLongTarget,
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
fn quick_long_array_loop_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<(QuickLongArrayLoopKernel, QuickLongArrayBodyKernel)> {
    if plan.entry_op != 0 || plan.string_input_mask != 0 || plan.string_output_mask != 0 {
        return None;
    }

    let [
        QuickLongOp::BranchUnlessLt {
            lhs: header_lhs,
            rhs: header_rhs,
            condition_tmp: header_condition_tmp,
            false_target: header_false_target,
            next_target: header_next_target,
            ..
        },
        QuickLongOp::FetchArrayLong {
            array,
            index,
            result: fetch_result,
            destination: fetch_destination,
            next_target: fetch_next_target,
            resume_ip: fetch_resume_ip,
        },
        body_ops @ ..,
        QuickLongOp::PostIncLoopLt {
            value: post_value,
            result: post_result,
            condition_lhs: post_condition_lhs,
            condition_rhs: post_condition_rhs,
            condition_tmp: post_condition_tmp,
            body_target,
            exit_target,
            resume_ip: post_resume_ip,
        },
    ] = plan.ops.as_slice()
    else {
        return None;
    };

    let post_index = plan.ops.len() - 1;
    let body = match body_ops {
        [QuickLongOp::AddAssign {
            lhs,
            rhs,
            result,
            destination,
            next_target,
            add_resume_ip,
        }] if next_target.op_index() == Some(post_index) => {
            QuickLongArrayBodyKernel::OneAdd {
                add: QuickLongAddAssignKernel {
                    lhs: *lhs,
                    rhs: *rhs,
                    result: *result,
                    destination: *destination,
                    resume_ip: *add_resume_ip,
                },
            }
        }
        [
            QuickLongOp::AddAssign {
                lhs: first_lhs,
                rhs: first_rhs,
                result: first_result,
                destination: first_destination,
                next_target: first_next_target,
                add_resume_ip: first_resume_ip,
            },
            QuickLongOp::AddAssign {
                lhs: second_lhs,
                rhs: second_rhs,
                result: second_result,
                destination: second_destination,
                next_target: second_next_target,
                add_resume_ip: second_resume_ip,
            },
        ] if first_next_target.op_index() == Some(3)
            && second_next_target.op_index() == Some(post_index) =>
        {
            QuickLongArrayBodyKernel::TwoAdds {
                first: QuickLongAddAssignKernel {
                    lhs: *first_lhs,
                    rhs: *first_rhs,
                    result: *first_result,
                    destination: *first_destination,
                    resume_ip: *first_resume_ip,
                },
                second: QuickLongAddAssignKernel {
                    lhs: *second_lhs,
                    rhs: *second_rhs,
                    result: *second_result,
                    destination: *second_destination,
                    resume_ip: *second_resume_ip,
                },
            }
        }
        [
            QuickLongOp::AddAssign {
                lhs: first_lhs,
                rhs: first_rhs,
                result: first_result,
                destination: first_destination,
                next_target: first_next_target,
                add_resume_ip: first_resume_ip,
            },
            QuickLongOp::AddAddAssign {
                first_lhs: middle_first_lhs,
                first_rhs: middle_first_rhs,
                first_result: middle_first_result,
                second_lhs: middle_second_lhs,
                second_rhs: middle_second_rhs,
                second_result: middle_second_result,
                destination: middle_destination,
                next_target: middle_next_target,
                first_resume_ip: middle_first_resume_ip,
                second_resume_ip: middle_second_resume_ip,
            },
            QuickLongOp::AddAssign {
                lhs: last_lhs,
                rhs: last_rhs,
                result: last_result,
                destination: last_destination,
                next_target: last_next_target,
                add_resume_ip: last_resume_ip,
            },
        ] if first_next_target.op_index() == Some(3)
            && middle_next_target.op_index() == Some(4)
            && last_next_target.op_index() == Some(post_index) =>
        {
            QuickLongArrayBodyKernel::AddFusedAddAdd {
                first: QuickLongAddAssignKernel {
                    lhs: *first_lhs,
                    rhs: *first_rhs,
                    result: *first_result,
                    destination: *first_destination,
                    resume_ip: *first_resume_ip,
                },
                middle: QuickLongAddAddAssignKernel {
                    first_lhs: *middle_first_lhs,
                    first_rhs: *middle_first_rhs,
                    first_result: *middle_first_result,
                    second_lhs: *middle_second_lhs,
                    second_rhs: *middle_second_rhs,
                    second_result: *middle_second_result,
                    destination: *middle_destination,
                    first_resume_ip: *middle_first_resume_ip,
                    second_resume_ip: *middle_second_resume_ip,
                },
                last: QuickLongAddAssignKernel {
                    lhs: *last_lhs,
                    rhs: *last_rhs,
                    result: *last_result,
                    destination: *last_destination,
                    resume_ip: *last_resume_ip,
                },
            }
        }
        [
            QuickLongOp::ConditionalAddAssign {
                condition,
                condition_tmp,
                lhs,
                rhs,
                result,
                destination,
                next_target: first_next_target,
                add_resume_ip,
                ..
            },
            QuickLongOp::AddAssign {
                lhs: second_lhs,
                rhs: second_rhs,
                result: second_result,
                destination: second_destination,
                next_target: second_next_target,
                add_resume_ip: second_resume_ip,
            },
        ] if first_next_target.op_index() == Some(3)
            && second_next_target.op_index() == Some(post_index) =>
        {
            QuickLongArrayBodyKernel::ConditionalAdd {
                first: QuickLongConditionalAddAssignKernel {
                    condition: *condition,
                    condition_tmp: *condition_tmp,
                    lhs: *lhs,
                    rhs: *rhs,
                    result: *result,
                    destination: *destination,
                    add_resume_ip: *add_resume_ip,
                },
                second: QuickLongAddAssignKernel {
                    lhs: *second_lhs,
                    rhs: *second_rhs,
                    result: *second_result,
                    destination: *second_destination,
                    resume_ip: *second_resume_ip,
                },
            }
        }
        _ => return None,
    };

    header_false_target.exit_ip()?;
    if header_next_target.op_index() != Some(1)
        || fetch_next_target.op_index() != Some(2)
        || body_target.op_index() != Some(1)
        || exit_target != header_false_target
        || post_condition_lhs != header_lhs
        || post_condition_rhs != header_rhs
        || post_condition_tmp != header_condition_tmp
    {
        return None;
    }

    Some((
        QuickLongArrayLoopKernel {
            header_lhs: *header_lhs,
            header_rhs: *header_rhs,
            header_condition_tmp: *header_condition_tmp,
            array: *array,
            index: *index,
            fetch_result: *fetch_result,
            fetch_destination: *fetch_destination,
            fetch_resume_ip: *fetch_resume_ip,
            post_value: *post_value,
            post_result: *post_result,
            post_resume_ip: *post_resume_ip,
            body_target: *body_target,
            exit_target: *exit_target,
        },
        body,
    ))
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
fn quick_long_branch_only_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<QuickLongBranchOnlyKernel> {
    if plan.entry_op != 0 || plan.ops.len() < 3 {
        return None;
    }

    let (
        header_lhs,
        header_rhs,
        header_condition_tmp,
        header_false_target,
        header_next_target,
    ) = match *plan.ops.first()? {
        QuickLongOp::BranchUnlessLt {
            lhs,
            rhs,
            condition_tmp,
            false_target,
            next_target,
            ..
        } => (lhs, rhs, condition_tmp, false_target, next_target),
        _ => return None,
    };
    header_false_target.exit_ip()?;

    let post_index = plan.ops.len() - 1;
    let (
        post_value,
        post_result,
        post_header_lhs,
        post_header_rhs,
        post_header_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
    ) = match *plan.ops.last()? {
        QuickLongOp::PostIncLoopLt {
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        } => (
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        ),
        _ => return None,
    };
    if post_header_lhs != header_lhs
        || post_header_rhs != header_rhs
        || post_header_condition_tmp != header_condition_tmp
        || header_next_target.op_index() != Some(1)
        || body_target.op_index() != Some(1)
        || exit_target != header_false_target
    {
        return None;
    }

    let empty_condition = QuickLongBranchCondition {
        lhs: 0,
        rhs: QuickLongOperand::Const(0),
        condition_tmp: None,
    };
    let mut conditions = [empty_condition; QUICK_LONG_BRANCH_CONDITION_LIMIT];
    let mut condition_count = 0usize;
    let mut index = 1usize;
    while index < post_index {
        if condition_count == conditions.len() {
            return None;
        }
        let (lhs, rhs, condition_tmp, false_target, true_target) =
            match *plan.ops.get(index)? {
                QuickLongOp::BranchUnlessEq {
                    lhs,
                    rhs,
                    condition_tmp,
                    false_target,
                    next_target,
                    ..
                } => (lhs, rhs, condition_tmp, false_target, next_target),
                _ => return None,
            };
        conditions[condition_count] = QuickLongBranchCondition {
            lhs,
            rhs,
            condition_tmp,
        };
        condition_count += 1;

        let false_index = false_target.op_index()?;
        let true_index = true_target.op_index()?;
        if true_index == post_index {
            if false_index != post_index {
                return None;
            }
            index = post_index;
        } else {
            if true_index != index + 1 || false_index != index + 2 {
                return None;
            }
            match *plan.ops.get(true_index)? {
                QuickLongOp::Jump { target }
                    if target.op_index() == Some(post_index) => {}
                _ => return None,
            }
            index = false_index;
        }
    }
    if condition_count == 0 {
        return None;
    }

    Some(QuickLongBranchOnlyKernel {
        header_lhs,
        header_rhs,
        header_condition_tmp,
        conditions,
        condition_count: condition_count as u8,
        post_value,
        post_result,
        post_resume_ip,
        body_target,
        exit_target,
    })
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
fn quick_long_conditional_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<(QuickLongConditionalKernel, QuickLongConditionalBody)> {
    if plan.entry_op != 0 {
        return None;
    }

    let (
        header_lhs,
        header_rhs,
        header_condition_tmp,
        header_false_target,
        header_next_target,
    ) = match *plan.ops.first()? {
        QuickLongOp::BranchUnlessLt {
            lhs,
            rhs,
            condition_tmp,
            false_target,
            next_target,
            ..
        } => (lhs, rhs, condition_tmp, false_target, next_target),
        _ => return None,
    };
    header_false_target.exit_ip()?;

    let (
        body,
        add_lhs,
        add_rhs,
        add_result,
        destination,
        add_resume_ip,
        post_index,
        body_index,
    ) = match plan.ops.as_slice() {
        [
            _,
            QuickLongOp::ConditionalAddAssign {
                condition: QuickLongCondition::Lt {
                    lhs: condition_lhs,
                    rhs: condition_rhs,
                },
                condition_tmp,
                lhs,
                rhs,
                result,
                destination,
                next_target,
                add_resume_ip,
                ..
            },
            QuickLongOp::PostIncLoopLt { .. },
        ] if next_target.op_index() == Some(2) => (
            QuickLongConditionalBody::LessThan {
                lhs: *condition_lhs,
                rhs: *condition_rhs,
                condition_tmp: *condition_tmp,
            },
            *lhs,
            *rhs,
            *result,
            *destination,
            *add_resume_ip,
            2,
            1,
        ),
        [
            _,
            QuickLongOp::ModConst {
                value,
                divisor,
                result,
                next_target: mod_next_target,
                resume_ip,
            },
            QuickLongOp::ConditionalAddAssign {
                condition: QuickLongCondition::Eq {
                    lhs: condition_lhs,
                    rhs: condition_rhs,
                },
                condition_tmp,
                lhs,
                rhs,
                result: add_result,
                destination,
                next_target,
                add_resume_ip,
                ..
            },
            QuickLongOp::PostIncLoopLt { .. },
        ] if mod_next_target.op_index() == Some(2) && next_target.op_index() == Some(3) => (
            QuickLongConditionalBody::ModuloEqual {
                value: *value,
                divisor: *divisor,
                result: *result,
                resume_ip: *resume_ip,
                lhs: *condition_lhs,
                rhs: *condition_rhs,
                condition_tmp: *condition_tmp,
            },
            *lhs,
            *rhs,
            *add_result,
            *destination,
            *add_resume_ip,
            3,
            1,
        ),
        _ => return None,
    };

    let (
        post_value,
        post_result,
        post_condition_lhs,
        post_condition_rhs,
        post_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
    ) = match *plan.ops.get(post_index)? {
        QuickLongOp::PostIncLoopLt {
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        } => (
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        ),
        _ => return None,
    };

    if header_next_target.op_index() != Some(body_index)
        || body_target.op_index() != Some(body_index)
        || exit_target != header_false_target
        || post_condition_lhs != header_lhs
        || post_condition_rhs != header_rhs
        || post_condition_tmp != header_condition_tmp
    {
        return None;
    }

    Some((
        QuickLongConditionalKernel {
            header_lhs,
            header_rhs,
            header_condition_tmp,
            add_lhs,
            add_rhs,
            add_result,
            destination,
            add_resume_ip,
            post_value,
            post_result,
            post_resume_ip,
            body_target,
            exit_target,
        },
        body,
    ))
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn quick_long_operand(slots: &[i64; 64], operand: QuickLongOperand) -> i64 {
    match operand {
        QuickLongOperand::Slot(slot) => slots[slot as usize],
        QuickLongOperand::Const(value) => value,
    }
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn deopt_quick_long_kernel(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    slot_base: *mut Value,
    slots: &[i64; 64],
    dirty_long_mask: u64,
    dirty_bool_mask: u64,
    resume_ip: usize,
    iterations: u64,
) -> QuickLoopOutcome {
    commit_quick_long_ops_slots(
        slot_base,
        slots,
        dirty_long_mask,
        dirty_bool_mask,
    );
    (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
    stats::inc_quick_loop_deoptimized(iterations);
    QuickLoopOutcome::Deoptimized
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_array_loop_kernel<Fetch, Body>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    kernel: QuickLongArrayLoopKernel,
    mut fetch: Fetch,
    mut execute_body: Body,
) -> Result<QuickLoopOutcome, VmError>
where
    Fetch: FnMut(&[i64; 64]) -> Option<i64>,
    Body: FnMut(&mut [i64; 64], &mut u64, &mut u64) -> Result<(), usize>,
{
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut iterations = 0u64;

    let mut continue_loop =
        slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
    if let Some(slot) = kernel.header_condition_tmp {
        slots[slot as usize] = i64::from(continue_loop);
        dirty_bool_mask |= 1u64 << slot;
    }

    while continue_loop {
        let Some(fetched) = fetch(&slots) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.fetch_resume_ip,
                iterations,
            ));
        };
        slots[kernel.fetch_result as usize] = fetched;
        dirty_long_mask |= 1u64 << kernel.fetch_result;
        if let Some(destination) = kernel.fetch_destination {
            slots[destination as usize] = fetched;
            dirty_long_mask |= 1u64 << destination;
        }

        if let Err(resume_ip) =
            execute_body(&mut slots, &mut dirty_long_mask, &mut dirty_bool_mask)
        {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                resume_ip,
                iterations,
            ));
        }

        let Some(incremented) = slots[kernel.post_value as usize].checked_add(1) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.post_resume_ip,
                iterations,
            ));
        };
        if let Some(result) = kernel.post_result {
            slots[result as usize] = slots[kernel.post_value as usize];
            dirty_long_mask |= 1u64 << result;
        }
        slots[kernel.post_value as usize] = incremented;
        dirty_long_mask |= 1u64 << kernel.post_value;

        continue_loop =
            slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(continue_loop);
            dirty_bool_mask |= 1u64 << slot;
        }
        iterations += 1;

        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            commit_quick_long_ops_slots(
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
            );
            let next_target = if continue_loop {
                kernel.body_target
            } else {
                kernel.exit_target
            };
            let next_ip = plan.target_ip(next_target).unwrap_unchecked();
            (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
            handle_interrupt(eg)?;
        }
    }

    commit_quick_long_ops_slots(
        slot_base,
        &slots,
        dirty_long_mask,
        dirty_bool_mask,
    );
    let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn execute_quick_long_add_assign(
    slots: &mut [i64; 64],
    dirty_long_mask: &mut u64,
    kernel: QuickLongAddAssignKernel,
) -> Result<(), usize> {
    let value = slots[kernel.lhs as usize]
        .checked_add(slots[kernel.rhs as usize])
        .ok_or(kernel.resume_ip)?;
    slots[kernel.result as usize] = value;
    slots[kernel.destination as usize] = value;
    *dirty_long_mask |= (1u64 << kernel.result) | (1u64 << kernel.destination);
    Ok(())
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn execute_quick_long_add_add_assign(
    slots: &mut [i64; 64],
    dirty_long_mask: &mut u64,
    kernel: QuickLongAddAddAssignKernel,
) -> Result<(), usize> {
    let first = slots[kernel.first_lhs as usize]
        .checked_add(slots[kernel.first_rhs as usize])
        .ok_or(kernel.first_resume_ip)?;
    slots[kernel.first_result as usize] = first;
    *dirty_long_mask |= 1u64 << kernel.first_result;

    let second = slots[kernel.second_lhs as usize]
        .checked_add(slots[kernel.second_rhs as usize])
        .ok_or(kernel.second_resume_ip)?;
    slots[kernel.second_result as usize] = second;
    slots[kernel.destination as usize] = second;
    *dirty_long_mask |= (1u64 << kernel.second_result) | (1u64 << kernel.destination);
    Ok(())
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
fn execute_quick_long_conditional_add_assign(
    slots: &mut [i64; 64],
    dirty_long_mask: &mut u64,
    dirty_bool_mask: &mut u64,
    kernel: QuickLongConditionalAddAssignKernel,
    condition: bool,
) -> Result<(), usize> {
    if let Some(slot) = kernel.condition_tmp {
        slots[slot as usize] = i64::from(condition);
        *dirty_bool_mask |= 1u64 << slot;
    }
    if condition {
        let value = slots[kernel.lhs as usize]
            .checked_add(slots[kernel.rhs as usize])
            .ok_or(kernel.add_resume_ip)?;
        slots[kernel.result as usize] = value;
        slots[kernel.destination as usize] = value;
        *dirty_long_mask |= (1u64 << kernel.result) | (1u64 << kernel.destination);
    }
    Ok(())
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_indexed_array_two_adds_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    array: *const PhpArray,
    index: u16,
    kernel: QuickLongArrayLoopKernel,
    first: QuickLongAddAssignKernel,
    second: QuickLongAddAssignKernel,
) -> Result<QuickLoopOutcome, VmError> {
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut iterations = 0u64;

    let mut continue_loop =
        slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
    if let Some(slot) = kernel.header_condition_tmp {
        slots[slot as usize] = i64::from(continue_loop);
        dirty_bool_mask |= 1u64 << slot;
    }

    while continue_loop {
        let fetched = (*array)
            .get_indexed_int(slots[index as usize])
            .and_then(|value| {
                (value.value_type() == ValueType::Long).then(|| value.raw_long())
            });
        let Some(fetched) = fetched else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.fetch_resume_ip,
                iterations,
            ));
        };
        slots[kernel.fetch_result as usize] = fetched;
        dirty_long_mask |= 1u64 << kernel.fetch_result;

        let Some(first_value) = slots[first.lhs as usize]
            .checked_add(slots[first.rhs as usize])
        else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                first.resume_ip,
                iterations,
            ));
        };
        slots[first.result as usize] = first_value;
        slots[first.destination as usize] = first_value;
        dirty_long_mask |= (1u64 << first.result) | (1u64 << first.destination);

        let Some(second_value) = slots[second.lhs as usize]
            .checked_add(slots[second.rhs as usize])
        else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                second.resume_ip,
                iterations,
            ));
        };
        slots[second.result as usize] = second_value;
        slots[second.destination as usize] = second_value;
        dirty_long_mask |= (1u64 << second.result) | (1u64 << second.destination);

        let Some(incremented) = slots[kernel.post_value as usize].checked_add(1) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.post_resume_ip,
                iterations,
            ));
        };
        if let Some(result) = kernel.post_result {
            slots[result as usize] = slots[kernel.post_value as usize];
            dirty_long_mask |= 1u64 << result;
        }
        slots[kernel.post_value as usize] = incremented;
        dirty_long_mask |= 1u64 << kernel.post_value;

        continue_loop =
            slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(continue_loop);
            dirty_bool_mask |= 1u64 << slot;
        }
        iterations += 1;

        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            commit_quick_long_ops_slots(
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
            );
            let next_target = if continue_loop {
                kernel.body_target
            } else {
                kernel.exit_target
            };
            let next_ip = plan.target_ip(next_target).unwrap_unchecked();
            (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
            handle_interrupt(eg)?;
        }
    }

    commit_quick_long_ops_slots(
        slot_base,
        &slots,
        dirty_long_mask,
        dirty_bool_mask,
    );
    let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_array_two_adds_kernel<Fetch>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    kernel: QuickLongArrayLoopKernel,
    first: QuickLongAddAssignKernel,
    second: QuickLongAddAssignKernel,
    mut fetch: Fetch,
) -> Result<QuickLoopOutcome, VmError>
where
    Fetch: FnMut(&[i64; 64]) -> Option<i64>,
{
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut iterations = 0u64;

    let mut continue_loop =
        slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
    if let Some(slot) = kernel.header_condition_tmp {
        slots[slot as usize] = i64::from(continue_loop);
        dirty_bool_mask |= 1u64 << slot;
    }

    while continue_loop {
        let Some(fetched) = fetch(&slots) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.fetch_resume_ip,
                iterations,
            ));
        };
        slots[kernel.fetch_result as usize] = fetched;
        dirty_long_mask |= 1u64 << kernel.fetch_result;
        if let Some(destination) = kernel.fetch_destination {
            slots[destination as usize] = fetched;
            dirty_long_mask |= 1u64 << destination;
        }

        if let Err(resume_ip) =
            execute_quick_long_add_assign(&mut slots, &mut dirty_long_mask, first)
        {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                resume_ip,
                iterations,
            ));
        }
        if let Err(resume_ip) =
            execute_quick_long_add_assign(&mut slots, &mut dirty_long_mask, second)
        {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                resume_ip,
                iterations,
            ));
        }

        let Some(incremented) = slots[kernel.post_value as usize].checked_add(1) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.post_resume_ip,
                iterations,
            ));
        };
        if let Some(result) = kernel.post_result {
            slots[result as usize] = slots[kernel.post_value as usize];
            dirty_long_mask |= 1u64 << result;
        }
        slots[kernel.post_value as usize] = incremented;
        dirty_long_mask |= 1u64 << kernel.post_value;

        continue_loop =
            slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(continue_loop);
            dirty_bool_mask |= 1u64 << slot;
        }
        iterations += 1;

        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            commit_quick_long_ops_slots(
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
            );
            let next_target = if continue_loop {
                kernel.body_target
            } else {
                kernel.exit_target
            };
            let next_ip = plan.target_ip(next_target).unwrap_unchecked();
            (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
            handle_interrupt(eg)?;
        }
    }

    commit_quick_long_ops_slots(
        slot_base,
        &slots,
        dirty_long_mask,
        dirty_bool_mask,
    );
    let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn dispatch_quick_long_array_body_kernel<Fetch>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: [i64; 64],
    kernel: QuickLongArrayLoopKernel,
    body: QuickLongArrayBodyKernel,
    fetch: Fetch,
) -> Result<QuickLoopOutcome, VmError>
where
    Fetch: FnMut(&[i64; 64]) -> Option<i64>,
{
    match body {
        QuickLongArrayBodyKernel::OneAdd { add } => {
            run_quick_long_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                fetch,
                move |slots, dirty_long_mask, _| {
                    execute_quick_long_add_assign(slots, dirty_long_mask, add)
                },
            )
        }
        QuickLongArrayBodyKernel::TwoAdds { first, second } => {
            run_quick_long_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                fetch,
                move |slots, dirty_long_mask, _| {
                    execute_quick_long_add_assign(slots, dirty_long_mask, first)?;
                    execute_quick_long_add_assign(slots, dirty_long_mask, second)
                },
            )
        }
        QuickLongArrayBodyKernel::AddFusedAddAdd {
            first,
            middle,
            last,
        } => run_quick_long_array_loop_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            fetch,
            move |slots, dirty_long_mask, _| {
                execute_quick_long_add_assign(slots, dirty_long_mask, first)?;
                execute_quick_long_add_add_assign(slots, dirty_long_mask, middle)?;
                execute_quick_long_add_assign(slots, dirty_long_mask, last)
            },
        ),
        QuickLongArrayBodyKernel::ConditionalAdd { first, second } => match first.condition {
            QuickLongCondition::Lt { lhs, rhs } => run_quick_long_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                fetch,
                move |slots, dirty_long_mask, dirty_bool_mask| {
                    let condition =
                        slots[lhs as usize] < quick_long_operand(slots, rhs);
                    execute_quick_long_conditional_add_assign(
                        slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                        first,
                        condition,
                    )?;
                    execute_quick_long_add_assign(slots, dirty_long_mask, second)
                },
            ),
            QuickLongCondition::Eq { lhs, rhs } => run_quick_long_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                fetch,
                move |slots, dirty_long_mask, dirty_bool_mask| {
                    let condition =
                        slots[lhs as usize] == quick_long_operand(slots, rhs);
                    execute_quick_long_conditional_add_assign(
                        slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                        first,
                        condition,
                    )?;
                    execute_quick_long_add_assign(slots, dirty_long_mask, second)
                },
            ),
        },
    }
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn dispatch_quick_long_array_loop_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: [i64; 64],
    arrays: &[QuickLongArray; 64],
    int_position_hints: &[Option<QuickLongIntPositionHint>; 64],
    indexed_int_array_mask: u64,
    kernel: QuickLongArrayLoopKernel,
    body: QuickLongArrayBodyKernel,
) -> Result<QuickLoopOutcome, VmError> {
    let array = arrays[kernel.array as usize];
    let int_position_hint = int_position_hints[kernel.array as usize];
    if let QuickLongArrayBodyKernel::TwoAdds { first, second } = body {
        if let (
            Some(position_hint),
            QuickLongArray::Hash { array },
            QuickArrayIndex::Long(index),
        ) = (int_position_hint, array, kernel.index)
        {
            return run_quick_long_array_two_adds_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                first,
                second,
                move |slots| {
                    (*array)
                        .get_positioned_int(
                            quick_long_operand(slots, index),
                            position_hint.first_key,
                            position_hint.stride,
                        )
                        .and_then(|value| {
                            (value.value_type() == ValueType::Long)
                                .then(|| value.raw_long())
                        })
                },
            );
        }
        if indexed_int_array_mask & (1u64 << kernel.array) != 0 {
            if let (
                QuickLongArray::Hash { array },
                QuickArrayIndex::Long(QuickLongOperand::Slot(index)),
                None,
            ) = (array, kernel.index, kernel.fetch_destination)
            {
                return run_quick_long_indexed_array_two_adds_kernel(
                    eg,
                    frame,
                    op_array,
                    plan,
                    slot_base,
                    slots,
                    array,
                    index,
                    kernel,
                    first,
                    second,
                );
            }
            if let (
                QuickLongArray::Hash { array },
                QuickArrayIndex::Long(index),
            ) = (array, kernel.index)
            {
                return run_quick_long_array_two_adds_kernel(
                    eg,
                    frame,
                    op_array,
                    plan,
                    slot_base,
                    slots,
                    kernel,
                    first,
                    second,
                    move |slots| {
                        (*array)
                            .get_indexed_int(quick_long_operand(slots, index))
                            .and_then(|value| {
                                (value.value_type() == ValueType::Long)
                                    .then(|| value.raw_long())
                            })
                    },
                );
            }
        }
        return run_quick_long_array_two_adds_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            first,
            second,
            move |slots| array.long_at(kernel.index, slots, op_array),
        );
    }

    if let (
        Some(position_hint),
        QuickLongArray::Hash { array },
        QuickArrayIndex::Long(index),
    ) = (int_position_hint, array, kernel.index)
    {
        return dispatch_quick_long_array_body_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            body,
            move |slots| {
                (*array)
                    .get_positioned_int(
                        quick_long_operand(slots, index),
                        position_hint.first_key,
                        position_hint.stride,
                    )
                    .and_then(|value| {
                        (value.value_type() == ValueType::Long)
                            .then(|| value.raw_long())
                    })
            },
        );
    }

    if indexed_int_array_mask & (1u64 << kernel.array) != 0 {
        if let (
            QuickLongArray::Hash { array },
            QuickArrayIndex::Long(index),
        ) = (array, kernel.index)
        {
            return dispatch_quick_long_array_body_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                kernel,
                body,
                move |slots| {
                    (*array)
                        .get_indexed_int(quick_long_operand(slots, index))
                        .and_then(|value| {
                            (value.value_type() == ValueType::Long)
                                .then(|| value.raw_long())
                        })
                },
            );
        }
    }

    dispatch_quick_long_array_body_kernel(
        eg,
        frame,
        op_array,
        plan,
        slot_base,
        slots,
        kernel,
        body,
        move |slots| array.long_at(kernel.index, slots, op_array),
    )
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_conditional_kernel<F>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    kernel: QuickLongConditionalKernel,
    mut evaluate_body_condition: F,
) -> Result<QuickLoopOutcome, VmError>
where
    F: FnMut(&mut [i64; 64], &mut u64, &mut u64) -> Result<bool, usize>,
{
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut iterations = 0u64;

    let mut continue_loop =
        slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
    if let Some(slot) = kernel.header_condition_tmp {
        slots[slot as usize] = i64::from(continue_loop);
        dirty_bool_mask |= 1u64 << slot;
    }

    while continue_loop {
        let body_condition = match evaluate_body_condition(
            &mut slots,
            &mut dirty_long_mask,
            &mut dirty_bool_mask,
        ) {
            Ok(condition) => condition,
            Err(resume_ip) => {
                commit_quick_long_ops_slots(
                    slot_base,
                    &slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
        };

        if body_condition {
            let value = match slots[kernel.add_lhs as usize]
                .checked_add(slots[kernel.add_rhs as usize])
            {
                Some(value) => value,
                None => {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    (*frame).opline = op_array
                        .instructions
                        .as_ptr()
                        .add(kernel.add_resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                }
            };
            slots[kernel.add_result as usize] = value;
            slots[kernel.destination as usize] = value;
            dirty_long_mask |=
                (1u64 << kernel.add_result) | (1u64 << kernel.destination);
        }

        let incremented = match slots[kernel.post_value as usize].checked_add(1) {
            Some(value) => value,
            None => {
                commit_quick_long_ops_slots(
                    slot_base,
                    &slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(kernel.post_resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
        };
        if let Some(result) = kernel.post_result {
            slots[result as usize] = slots[kernel.post_value as usize];
            dirty_long_mask |= 1u64 << result;
        }
        slots[kernel.post_value as usize] = incremented;
        dirty_long_mask |= 1u64 << kernel.post_value;

        continue_loop =
            slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(continue_loop);
            dirty_bool_mask |= 1u64 << slot;
        }
        iterations += 1;

        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            commit_quick_long_ops_slots(
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
            );
            let next_target = if continue_loop {
                kernel.body_target
            } else {
                kernel.exit_target
            };
            let next_ip = plan.target_ip(next_target).unwrap_unchecked();
            (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
            handle_interrupt(eg)?;
        }
    }

    commit_quick_long_ops_slots(
        slot_base,
        &slots,
        dirty_long_mask,
        dirty_bool_mask,
    );
    let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_branch_only_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    kernel: QuickLongBranchOnlyKernel,
) -> Result<QuickLoopOutcome, VmError> {
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut iterations = 0u64;
    let mut continue_loop =
        slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
    if let Some(slot) = kernel.header_condition_tmp {
        slots[slot as usize] = i64::from(continue_loop);
        dirty_bool_mask |= 1u64 << slot;
    }

    while continue_loop {
        for condition in &kernel.conditions[..kernel.condition_count as usize] {
            let condition_result = slots[condition.lhs as usize]
                == quick_long_operand(&slots, condition.rhs);
            if let Some(slot) = condition.condition_tmp {
                slots[slot as usize] = i64::from(condition_result);
                dirty_bool_mask |= 1u64 << slot;
            }
            if condition_result {
                break;
            }
        }

        let incremented = match slots[kernel.post_value as usize].checked_add(1) {
            Some(value) => value,
            None => {
                commit_quick_long_ops_slots(
                    slot_base,
                    &slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(kernel.post_resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
        };
        if let Some(result) = kernel.post_result {
            slots[result as usize] = slots[kernel.post_value as usize];
            dirty_long_mask |= 1u64 << result;
        }
        slots[kernel.post_value as usize] = incremented;
        dirty_long_mask |= 1u64 << kernel.post_value;

        continue_loop =
            slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(continue_loop);
            dirty_bool_mask |= 1u64 << slot;
        }
        iterations += 1;

        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            commit_quick_long_ops_slots(
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
            );
            let next_target = if continue_loop {
                kernel.body_target
            } else {
                kernel.exit_target
            };
            let next_ip = plan.target_ip(next_target).unwrap_unchecked();
            (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
            handle_interrupt(eg)?;
        }
    }

    commit_quick_long_ops_slots(
        slot_base,
        &slots,
        dirty_long_mask,
        dirty_bool_mask,
    );
    let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn dispatch_quick_long_conditional_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: [i64; 64],
    kernel: QuickLongConditionalKernel,
    body: QuickLongConditionalBody,
) -> Result<QuickLoopOutcome, VmError> {
    match body {
        QuickLongConditionalBody::LessThan {
            lhs,
            rhs,
            condition_tmp,
        } => run_quick_long_conditional_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            move |slots, _, dirty_bool_mask| {
                let condition = slots[lhs as usize] < quick_long_operand(slots, rhs);
                if let Some(slot) = condition_tmp {
                    slots[slot as usize] = i64::from(condition);
                    *dirty_bool_mask |= 1u64 << slot;
                }
                Ok(condition)
            },
        ),
        QuickLongConditionalBody::ModuloEqual {
            value,
            divisor,
            result,
            resume_ip,
            lhs,
            rhs,
            condition_tmp,
        } => run_quick_long_conditional_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            move |slots, dirty_long_mask, dirty_bool_mask| {
                let remainder = slots[value as usize]
                    .checked_rem(divisor)
                    .ok_or(resume_ip)?;
                slots[result as usize] = remainder;
                *dirty_long_mask |= 1u64 << result;
                let condition = slots[lhs as usize] == quick_long_operand(slots, rhs);
                if let Some(slot) = condition_tmp {
                    slots[slot as usize] = i64::from(condition);
                    *dirty_bool_mask |= 1u64 << slot;
                }
                Ok(condition)
            },
        ),
    }
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_ops_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
) -> Result<QuickLoopOutcome, VmError> {
    if (*frame).num_cvs != op_array.num_cvs
        || (*frame).num_cvs + (*frame).num_temps > 64
        || (*frame).heap_bitmap
            & (plan.involved_mask & !(plan.array_input_mask | plan.string_input_mask))
            != 0
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let slot_base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    let mut slots = [0i64; 64];
    let mut input_mask = plan.long_input_mask;
    while input_mask != 0 {
        let slot = input_mask.trailing_zeros() as usize;
        input_mask &= input_mask - 1;
        let value = slot_base.add(slot);
        if (*value).value_type() != ValueType::Long {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        }
        slots[slot] = (*value).raw_long();
    }

    let mut string_mask = plan.string_input_mask;
    while string_mask != 0 {
        let slot = string_mask.trailing_zeros() as usize;
        string_mask &= string_mask - 1;
        if (*slot_base.add(slot)).value_type() != ValueType::String {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        }
    }

    if let Some(kernel) = quick_long_branch_only_kernel(plan) {
        return run_quick_long_branch_only_kernel(
            eg, frame, op_array, plan, slot_base, slots, kernel,
        );
    }

    if let Some((kernel, body)) = quick_long_conditional_kernel(plan) {
        return dispatch_quick_long_conditional_kernel(
            eg, frame, op_array, plan, slot_base, slots, kernel, body,
        );
    }

    let mut arrays = [QuickLongArray::EMPTY; 64];
    let mut int_position_hints = [None; 64];
    let mut indexed_int_array_mask = 0u64;
    let mut array_mask = plan.array_input_mask;
    while array_mask != 0 {
        let slot = array_mask.trailing_zeros() as usize;
        array_mask &= array_mask - 1;
        let value = &*slot_base.add(slot);
        let Some(array) = value.as_array() else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        };
        let quick_array = QuickLongArray::from_array(array);
        if matches!(quick_array, QuickLongArray::Hash { .. }) {
            if let Some((first_key, stride)) = array.integer_position_hint() {
                int_position_hints[slot] =
                    Some(QuickLongIntPositionHint { first_key, stride });
            } else {
                indexed_int_array_mask |= 1u64 << slot;
            }
        }
        arrays[slot] = quick_array;
    }

    if let Some((kernel, body)) = quick_long_array_loop_kernel(plan) {
        return dispatch_quick_long_array_loop_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            &arrays,
            &int_position_hints,
            indexed_int_array_mask,
            kernel,
            body,
        );
    }

    let mut string_fetch_cache = QuickStringFetchCache::new(plan.string_cache_capacity);
    let mut string_state = QuickStringSlotState::new(slot_base, plan.string_input_mask);
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut iterations = 0u64;
    let mut op_index = plan.entry_op as usize;

    loop {
        let mut completed_backedge = false;
        let next_target = match *plan.ops.get_unchecked(op_index) {
            QuickLongOp::BranchUnlessLt {
                lhs,
                rhs,
                condition_tmp,
                false_target,
                next_target,
                ..
            } => {
                let rhs = match rhs {
                    QuickLongOperand::Slot(slot) => slots[slot as usize],
                    QuickLongOperand::Const(value) => value,
                };
                let condition = slots[lhs as usize] < rhs;
                if let Some(slot) = condition_tmp {
                    slots[slot as usize] = i64::from(condition);
                    dirty_bool_mask |= 1u64 << slot;
                }
                if condition { next_target } else { false_target }
            }
            QuickLongOp::BranchUnlessEq {
                lhs,
                rhs,
                condition_tmp,
                false_target,
                next_target,
                ..
            } => {
                let rhs = match rhs {
                    QuickLongOperand::Slot(slot) => slots[slot as usize],
                    QuickLongOperand::Const(value) => value,
                };
                let condition = slots[lhs as usize] == rhs;
                if let Some(slot) = condition_tmp {
                    slots[slot as usize] = i64::from(condition);
                    dirty_bool_mask |= 1u64 << slot;
                }
                if condition { next_target } else { false_target }
            }
            QuickLongOp::ModConst {
                value,
                divisor,
                result,
                next_target,
                resume_ip,
            } => match slots[value as usize].checked_rem(divisor) {
                Some(remainder) => {
                    slots[result as usize] = remainder;
                    dirty_long_mask |= 1u64 << result;
                    next_target
                }
                None => {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                }
            },
            QuickLongOp::FetchArrayLong {
                array,
                index,
                result,
                destination,
                next_target,
                resume_ip,
            } => {
                let fetched = match index {
                    QuickArrayIndex::ValueSlot(slot) => {
                        let key = string_state.value(slot).as_str().unwrap_unchecked();
                        string_fetch_cache.long_at(array, arrays[array as usize], key)
                    }
                    _ => arrays[array as usize].long_at(index, &slots, op_array),
                };
                let Some(fetched) = fetched else {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                };
                slots[result as usize] = fetched;
                dirty_long_mask |= 1u64 << result;
                if let Some(destination) = destination {
                    slots[destination as usize] = fetched;
                    dirty_long_mask |= 1u64 << destination;
                }
                next_target
            }
            QuickLongOp::Add {
                lhs,
                rhs,
                result,
                next_target,
                resume_ip,
            } => match slots[lhs as usize].checked_add(slots[rhs as usize]) {
                Some(value) => {
                    slots[result as usize] = value;
                    dirty_long_mask |= 1u64 << result;
                    next_target
                }
                None => {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                }
            },
            QuickLongOp::AddAssign {
                lhs,
                rhs,
                result,
                destination,
                next_target,
                add_resume_ip,
            } => match slots[lhs as usize].checked_add(slots[rhs as usize]) {
                Some(value) => {
                    slots[result as usize] = value;
                    slots[destination as usize] = value;
                    dirty_long_mask |= (1u64 << result) | (1u64 << destination);
                    next_target
                }
                None => {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    (*frame).opline =
                        op_array.instructions.as_ptr().add(add_resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                }
            },
            QuickLongOp::ConditionalAddAssign {
                condition,
                condition_tmp,
                lhs,
                rhs,
                result,
                destination,
                next_target,
                add_resume_ip,
                ..
            } => {
                let condition = match condition {
                    QuickLongCondition::Lt { lhs, rhs } => {
                        let rhs = match rhs {
                            QuickLongOperand::Slot(slot) => slots[slot as usize],
                            QuickLongOperand::Const(value) => value,
                        };
                        slots[lhs as usize] < rhs
                    }
                    QuickLongCondition::Eq { lhs, rhs } => {
                        let rhs = match rhs {
                            QuickLongOperand::Slot(slot) => slots[slot as usize],
                            QuickLongOperand::Const(value) => value,
                        };
                        slots[lhs as usize] == rhs
                    }
                };
                if let Some(slot) = condition_tmp {
                    slots[slot as usize] = i64::from(condition);
                    dirty_bool_mask |= 1u64 << slot;
                }
                if !condition {
                    next_target
                } else {
                    match slots[lhs as usize].checked_add(slots[rhs as usize]) {
                        Some(value) => {
                            slots[result as usize] = value;
                            slots[destination as usize] = value;
                            dirty_long_mask |=
                                (1u64 << result) | (1u64 << destination);
                            next_target
                        }
                        None => {
                            commit_quick_long_ops_slots(
                                slot_base,
                                &slots,
                                dirty_long_mask,
                                dirty_bool_mask,
                            );
                            (*frame).opline =
                                op_array.instructions.as_ptr().add(add_resume_ip);
                            stats::inc_quick_loop_deoptimized(iterations);
                            return Ok(QuickLoopOutcome::Deoptimized);
                        }
                    }
                }
            }
            QuickLongOp::AddAddAssign {
                first_lhs,
                first_rhs,
                first_result,
                second_lhs,
                second_rhs,
                second_result,
                destination,
                next_target,
                first_resume_ip,
                second_resume_ip,
            } => {
                let first = match slots[first_lhs as usize]
                    .checked_add(slots[first_rhs as usize])
                {
                    Some(value) => value,
                    None => {
                        commit_quick_long_ops_slots(
                            slot_base,
                            &slots,
                            dirty_long_mask,
                            dirty_bool_mask,
                        );
                        (*frame).opline =
                            op_array.instructions.as_ptr().add(first_resume_ip);
                        stats::inc_quick_loop_deoptimized(iterations);
                        return Ok(QuickLoopOutcome::Deoptimized);
                    }
                };
                slots[first_result as usize] = first;
                dirty_long_mask |= 1u64 << first_result;

                let second = match slots[second_lhs as usize]
                    .checked_add(slots[second_rhs as usize])
                {
                    Some(value) => value,
                    None => {
                        commit_quick_long_ops_slots(
                            slot_base,
                            &slots,
                            dirty_long_mask,
                            dirty_bool_mask,
                        );
                        (*frame).opline =
                            op_array.instructions.as_ptr().add(second_resume_ip);
                        stats::inc_quick_loop_deoptimized(iterations);
                        return Ok(QuickLoopOutcome::Deoptimized);
                    }
                };
                slots[second_result as usize] = second;
                slots[destination as usize] = second;
                dirty_long_mask |=
                    (1u64 << second_result) | (1u64 << destination);
                next_target
            }
            QuickLongOp::Assign {
                destination,
                source,
                next_target,
            } => {
                slots[destination as usize] = slots[source as usize];
                dirty_long_mask |= 1u64 << destination;
                next_target
            }
            QuickLongOp::AssignStringLiteral {
                destination,
                literal,
                next_target,
            } => {
                let value = op_array.literals.as_ptr().add(literal as usize);
                debug_assert_eq!((*value).value_type(), ValueType::String);
                string_state.assign_literal(destination, value);
                next_target
            }
            QuickLongOp::AssignStringSlot {
                destination,
                source,
                next_target,
            } => {
                string_state.assign_slot(destination, source);
                next_target
            }
            QuickLongOp::PostInc {
                value,
                result,
                next_target,
                resume_ip,
            }
            | QuickLongOp::PostIncJump {
                value,
                result,
                target: next_target,
                resume_ip,
            } => match slots[value as usize].checked_add(1) {
                Some(incremented) => {
                    if let Some(result) = result {
                        slots[result as usize] = slots[value as usize];
                        dirty_long_mask |= 1u64 << result;
                    }
                    slots[value as usize] = incremented;
                    dirty_long_mask |= 1u64 << value;
                    next_target
                }
                None => {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                }
            },
            QuickLongOp::PostIncLoopLt {
                value,
                result,
                condition_lhs,
                condition_rhs,
                condition_tmp,
                body_target,
                exit_target,
                resume_ip,
            } => match slots[value as usize].checked_add(1) {
                Some(incremented) => {
                    if let Some(result) = result {
                        slots[result as usize] = slots[value as usize];
                        dirty_long_mask |= 1u64 << result;
                    }
                    slots[value as usize] = incremented;
                    dirty_long_mask |= 1u64 << value;

                    let rhs = match condition_rhs {
                        QuickLongOperand::Slot(slot) => slots[slot as usize],
                        QuickLongOperand::Const(value) => value,
                    };
                    let condition = slots[condition_lhs as usize] < rhs;
                    if let Some(slot) = condition_tmp {
                        slots[slot as usize] = i64::from(condition);
                        dirty_bool_mask |= 1u64 << slot;
                    }
                    completed_backedge = true;
                    if condition { body_target } else { exit_target }
                }
                None => {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                }
            },
            QuickLongOp::Jump { target } => target,
        };

        if completed_backedge || next_target.op_index() == Some(plan.entry_op as usize) {
            iterations += 1;
            if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
                commit_quick_long_ops_slots(
                    slot_base,
                    &slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                string_state.commit();
                let next_ip = plan.target_ip(next_target).unwrap_unchecked();
                (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
                handle_interrupt(eg)?;
            }
        }

        if let Some(next_index) = next_target.op_index() {
            op_index = next_index;
            continue;
        }

        commit_quick_long_ops_slots(
            slot_base,
            &slots,
            dirty_long_mask,
            dirty_bool_mask,
        );
        let next_ip = next_target.exit_ip().unwrap_unchecked();
        (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
        stats::inc_quick_loop_completed(iterations);
        return Ok(QuickLoopOutcome::Completed);
    }
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn execute_quick_loop_backedge(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    let target = opline.op1 as usize;
    let block_idx = opline.extended_value as usize - 1;

    if let Some(plan) = op_array.block_plans.get(block_idx) {
        let hot_counter = &op_array.block_counters[block_idx];
        let count = hot_counter.get();
        if count == QUICK_LOOP_DISABLED {
            (*frame).opline = op_array.instructions().as_ptr().add(target);
            return Ok(());
        }
        let hot_progress = count % QUICK_LOOP_COUNTER_STRIDE;
        if hot_progress >= QUICK_LOOP_HOT_THRESHOLD {
            let outcome = match plan {
                super::planner::BlockPlan::QuickLongInduction(plan) => {
                    run_quick_long_induction_loop(eg, frame, op_array, *plan)?
                }
                super::planner::BlockPlan::QuickLongAccumulate(plan) => {
                    run_quick_long_accumulate_loop(eg, frame, op_array, *plan)?
                }
                super::planner::BlockPlan::QuickForeachLongAccumulate(plan) => {
                    super::quick_foreach::run_quick_foreach_long_accumulate_loop(
                        eg, frame, op_array, *plan,
                    )?
                }
                super::planner::BlockPlan::QuickLongOps(plan) => {
                    run_quick_long_ops_loop(eg, frame, op_array, plan)?
                }
                _ => {
                    (*frame).opline = op_array.instructions().as_ptr().add(target);
                    return Ok(());
                }
            };
            match outcome {
                QuickLoopOutcome::Completed => {
                    hot_counter.set(QUICK_LOOP_HOT_THRESHOLD);
                    return Ok(());
                }
                QuickLoopOutcome::Deoptimized => {
                    let failures = count / QUICK_LOOP_COUNTER_STRIDE + 1;
                    hot_counter.set(if failures >= QUICK_LOOP_FAILURE_LIMIT {
                        QUICK_LOOP_DISABLED
                    } else {
                        failures * QUICK_LOOP_COUNTER_STRIDE
                    });
                    return Ok(());
                }
                QuickLoopOutcome::GuardFailed => {
                    let failures = count / QUICK_LOOP_COUNTER_STRIDE + 1;
                    hot_counter.set(if failures >= QUICK_LOOP_FAILURE_LIMIT {
                        QUICK_LOOP_DISABLED
                    } else {
                        failures * QUICK_LOOP_COUNTER_STRIDE
                    });
                }
            }
        } else {
            hot_counter.set(count + 1);
        }
    }

    (*frame).opline = op_array.instructions().as_ptr().add(target);
    Ok(())
}

/// Inner execute loop — equivalent to zend_execute_ex.
fn execute_ex(eg: &mut ExecutorGlobals, initial_frame: *mut ExecuteData) -> Result<(), VmError> {
    let mut frame = initial_frame;
    let mut op_array = unsafe { (*frame).op_array() };
    let mut tick: u8 = 255; // First iteration checks immediately (wraps to 0)
    'vm: loop {
        // Batch interrupt check: every 256 opcodes instead of every opcode.
        // Placed at loop top so all `continue` paths also pass through it.
        tick = tick.wrapping_add(1);
        if tick == 0 {
            if eg.vm_interrupt.load(Ordering::Relaxed) {
                handle_interrupt(eg)?;
            }
        }

        let mut opline_ptr: *const Instruction = unsafe { (*frame).opline };
        let opline = unsafe { &*opline_ptr };
        stats::inc_opcode(opline.opcode as usize);

        // Check for pending return or exception after finally block ends
        let frame_pending = unsafe { (*frame).pending_return_after_finally };
        let check_finally = frame_pending || eg.exception.is_some();
        if check_finally {
            let current_ip = unsafe {
                (*frame).opline.offset_from(op_array.instructions.as_ptr()) as u32
            };
            let at_finally_end = op_array.try_entries.iter().any(|e| {
                e.finally_start != 0xFFFFFFFF && current_ip == e.finally_end
            });
            if at_finally_end {
                if frame_pending {
                    unsafe { (*frame).pending_return_after_finally = false; }
                    // Deferred return — pop frame now (return value already written)
                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    continue;
                } else {
                    // Real exception — re-enter throw/unwind to find outer handler
                    let pending = eg.exception.take().unwrap();
                    // Start from current frame (outer try/catch may be in same frame)
                    let mut search_frame = frame;
                    let mut found = false;
                    loop {
                        let sf_op_array = unsafe { (*search_frame).op_array() };
                        let sf_ip = unsafe {
                            (*search_frame).opline.offset_from(sf_op_array.instructions.as_ptr()) as u32
                        };
                        for entry in &sf_op_array.try_entries {
                            // Skip the entry whose finally we just finished
                            if entry.finally_start != 0xFFFFFFFF && sf_ip == entry.finally_end {
                                continue;
                            }
                            if sf_ip >= entry.try_start && sf_ip < entry.try_end {
                                // Unwind frames between current and search_frame
                                while frame != search_frame {
                                    let prev = unsafe { (*frame).prev_execute_data };
                                    eg.current_execute_data.set(prev);
                                    unsafe { cleanup_frame_slots(frame) };
                                    eg.vm_stack.pop_call_frame(frame);
                                    frame = prev;
                                }
                                let base_ptr = sf_op_array.instructions.as_ptr();
                                let matched_catch = entry.catches.iter().find(|c| {
                                    exception_matches_catch(&pending, &c.types, eg)
                                });
                                if let Some(catch) = matched_catch {
                                    let catch_cv_ptr = unsafe { (*search_frame).get_op_mut(catch.catch_cv, OpType::Cv) };
                                    unsafe { slot_set(catch_cv_ptr, pending.clone()) };
                                    unsafe { (*frame).opline = base_ptr.add(catch.catch_start as usize) };
                                } else if entry.finally_start != 0xFFFFFFFF {
                                    eg.exception = Some(pending.clone());
                                    unsafe { (*frame).opline = base_ptr.add(entry.finally_start as usize) };
                                }
                                found = true;
                                break;
                            }
                        }
                        if found { break; }
                        let prev = unsafe { (*search_frame).prev_execute_data };
                        if prev.is_null() { break; }
                        search_frame = prev;
                    }
                    if found {
                        op_array = unsafe { (*frame).op_array() };
                        continue;
                    }
                    // Propagate via eg.exception for re-entry boundary crossing
                    eg.exception = Some(pending);
                    return Ok(());
                }
            }
        }

        match opline.opcode {
            OpCode::AssignCv => {
                // ASSIGN_CV op1=CV(dest), op2=value, result=optional copy
                let val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let cloned = val.clone();
                let dest = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                if opline.result_type != OpType::Unused {
                    // Need two copies: one for dest, one for result
                    unsafe { slot_set(dest, cloned.clone()) };
                    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                    unsafe { slot_set(result_ptr, cloned) };
                } else {
                    // Common path: just move the single clone into dest
                    unsafe { slot_set(dest, cloned) };
                }
            }

            OpCode::AssignConcat => {
                // $x .= expr: in-place string append
                // COW: if dest is sole owner, push_str in place (no allocation).
                // If shared, as_string_mut() detaches first.
                let rhs = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let dest = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                let dest_ref = unsafe { &mut *dest };
                if dest_ref.value_type() == ValueType::String {
                    // Fast path: avoid echo_to_string() allocation when RHS is string
                    if rhs.value_type() == ValueType::String {
                        let rhs_s = rhs.as_str().unwrap();
                        let s = unsafe { dest_ref.as_string_mut().unwrap_unchecked() };
                        s.push_str(rhs_s);
                    } else {
                        let rhs_str = rhs.echo_to_string();
                        let s = unsafe { dest_ref.as_string_mut().unwrap_unchecked() };
                        s.push_str(&rhs_str);
                    }
                } else {
                    let lhs_str = dest_ref.echo_to_string();
                    let rhs_str = if rhs.value_type() == ValueType::String {
                        rhs.as_str().unwrap().to_string()
                    } else {
                        rhs.echo_to_string()
                    };
                    let mut new_s = lhs_str;
                    new_s.push_str(&rhs_str);
                    unsafe { slot_set(dest, Value::string(new_s)) };
                }
            }

            OpCode::Echo => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if val.value_type() == ValueType::String {
                    // Fast path: string → write bytes directly, no allocation
                    eg.write_output(val.as_str().unwrap().as_bytes());
                } else if val.value_type() == ValueType::Long {
                    // Fast path: integer → stack-local write, no heap allocation
                    use std::io::Write;
                    let mut buf = [0u8; 20]; // i64 max is 19 digits + sign
                    let s = {
                        let mut cursor = std::io::Cursor::new(&mut buf[..]);
                        write!(cursor, "{}", unsafe { val.raw_long() }).unwrap();
                        cursor.position() as usize
                    };
                    eg.write_output(&buf[..s]);
                } else if val.value_type() == ValueType::Object {
                    if let Some(result) = call_magic_method(eg, val, "__tostring", &[])? {
                        let output = result.echo_to_string();
                        eg.write_output(output.as_bytes());
                    } else {
                        let output = val.echo_to_string();
                        eg.write_output(output.as_bytes());
                    }
                } else {
                    let output = val.echo_to_string();
                    eg.write_output(output.as_bytes());
                }
            }

            // ── Specialized arithmetic opcodes ──────────────────────────
            // Inline operand access: no get_op_ptr match, no ref check.
            // Fall through to general handler on non-Long operands.

            OpCode::Add_TmpTmp => {
                let base = frame as *const Value;
                let op1 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op1 as usize) };
                let op2 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op2 as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    if let Some(sum) = l1.checked_add(l2) {
                        // Peek ahead: if next is Return consuming our result TMP,
                        // and frame is FastScalar with no heap — skip TMP write + Return dispatch.
                        // Write sum directly to caller's return_value, pop frame inline.
                        let next = unsafe { &*opline_ptr.add(1) };
                        if next.opcode == OpCode::Return
                            && next.op1_type == OpType::Tmp
                            && next.op1 == opline.result
                            && !unsafe { (*frame).has_heap_slots }
                        {
                            let return_target = unsafe { (*frame).return_value };
                            if !return_target.is_null() {
                                let prev = unsafe { (*frame).prev_execute_data };
                                if !prev.is_null() && unsafe { (*prev).has_heap_slots } {
                                    unsafe { std::ptr::drop_in_place(return_target) };
                                }
                                unsafe { Value::write_long(return_target, sum) };
                            }
                            stats::inc_return_fast();
                            let prev = unsafe { (*frame).prev_execute_data };
                            if prev.is_null() {
                                return Ok(());
                            }
                            if frame == initial_frame {
                                eg.current_execute_data.set(prev);
                                eg.vm_stack.pop_call_frame(frame);
                                return Ok(());
                            }
                            eg.current_execute_data.set(prev);
                            eg.vm_stack.pop_call_frame(frame);
                            frame = prev;
                            op_array = unsafe { (*frame).op_array() };
            
                            continue;
                        }
                        // Normal path: write to TMP
                        let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                        unsafe { frame_tmp_set_long(frame, result_ptr, sum) };
                    } else {
                        let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                        unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 + l2 as f64))
                        };
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 + d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for +".into()));
                }
            }

            OpCode::Add_CvTmp => {
                let base = frame as *const Value;
                let cv_ptr = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op1 as usize) };
                let op1 = if cv_ptr.is_reference() { unsafe { &*cv_ptr.as_ref_ptr() } } else { cv_ptr };
                let op2 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op2 as usize) };
                let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_add(l2) {
                        Some(sum) => unsafe { frame_tmp_set_long(frame, result_ptr, sum) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 + l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 + d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for +".into()));
                }
            }

            OpCode::Sub_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    // Peek ahead: if next instruction is SendVal consuming our TMP result,
                    // write directly to the call arg slot and skip the SendVal dispatch.
                    let next = unsafe { &*opline_ptr.add(1) };
                    if next.opcode == OpCode::SendVal
                        && next.op1_type == OpType::Tmp
                        && next.op1 == opline.result
                    {
                        let call = unsafe { (*frame).call };
                        let dst = unsafe {
                            (call as *mut Value).add(CALL_FRAME_SLOTS + next.op2 as usize)
                        };
                        match l1.checked_sub(l2) {
                            Some(diff) => unsafe { Value::write_long(dst, diff) },
                            None => unsafe { dst.write(Value::double(l1 as f64 - l2 as f64)) },
                        }
                        // Skip SendVal: advance local ptr +1, loop bottom adds +1 → net +2
                        opline_ptr = unsafe { opline_ptr.add(1) };
                    } else {
                        let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                        match l1.checked_sub(l2) {
                            Some(diff) => unsafe { frame_tmp_set_long(frame, result_ptr, diff) },
                            None => unsafe {
                                frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 - l2 as f64))
                            },
                        }
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 - d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for -".into()));
                }
            }

            OpCode::Sub_TmpTmp => {
                let base = frame as *const Value;
                let op1 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op1 as usize) };
                let op2 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op2 as usize) };
                let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_sub(l2) {
                        Some(diff) => unsafe { frame_tmp_set_long(frame, result_ptr, diff) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 - l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 - d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for -".into()));
                }
            }

            OpCode::IsSmaller_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, l1 < l2) };
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, d1 < d2) };
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, s1 < s2) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                }
            }

            OpCode::IsSmallerOrEqual_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, l1 <= l2) };
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, d1 <= d2) };
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, s1 <= s2) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                }
            }

            OpCode::IsEqual_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, l1 == l2) };
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, d1 == d2) };
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, s1 == s2) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                }
            }

            // ── Superinstructions: fused comparison + conditional jump ──
            // Eliminates TMP write/read and one dispatch cycle.
            // On fall-through, advances opline by 2 (skipping the dead JmpZ/JmpNZ).

            OpCode::JmpZ_Le_CvConst => {
                // Fused: IsSmallerOrEqual_CvConst + JmpZ
                // Jump to result if !(CV <= Const), else fall through (+2).
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let cmp_result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1 <= l2
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1 <= d2
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1 <= s2
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };
                if !cmp_result {
                    unsafe { (*frame).opline = op_array.instructions().as_ptr().add(opline.result as usize) };
    
                    continue;
                }
                // Fall through: advance local +1, loop bottom adds +1 more → net +2
                opline_ptr = unsafe { opline_ptr.add(1) };

            }

            OpCode::JmpNZ_Le_CvConst => {
                // Fused: IsSmallerOrEqual_CvConst + JmpNZ
                // Jump to result if CV <= Const, else fall through (+2).
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let cmp_result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1 <= l2
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1 <= d2
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1 <= s2
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };
                if cmp_result {
                    unsafe { (*frame).opline = op_array.instructions().as_ptr().add(opline.result as usize) };
    
                    continue;
                }
                opline_ptr = unsafe { opline_ptr.add(1) };

            }

            OpCode::JmpZ_Lt_CvConst => {
                // Fused: IsSmaller_CvConst + JmpZ
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let cmp_result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1 < l2
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1 < d2
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1 < s2
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };
                if !cmp_result {
                    unsafe { (*frame).opline = op_array.instructions().as_ptr().add(opline.result as usize) };
    
                    continue;
                }
                opline_ptr = unsafe { opline_ptr.add(1) };

            }

            OpCode::JmpNZ_Lt_CvConst => {
                // Fused: IsSmaller_CvConst + JmpNZ
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let cmp_result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1 < l2
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1 < d2
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1 < s2
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };
                if cmp_result {
                    unsafe { (*frame).opline = op_array.instructions().as_ptr().add(opline.result as usize) };

                    continue;
                }
                opline_ptr = unsafe { opline_ptr.add(1) };

            }

            OpCode::JmpZ_Eq_CvConst => {
                // Fused: IsEqual_CvConst + JmpZ
                // Jump to result if !(CV == Const), else fall through (+2).
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let cmp_result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1 == l2
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1 == d2
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1 == s2
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };
                if !cmp_result {
                    unsafe { (*frame).opline = op_array.instructions().as_ptr().add(opline.result as usize) };

                    continue;
                }
                opline_ptr = unsafe { opline_ptr.add(1) };

            }

            OpCode::JmpNZ_Eq_CvConst => {
                // Fused: IsEqual_CvConst + JmpNZ
                // Jump to result if CV == Const, else fall through (+2).
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let cmp_result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1 == l2
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1 == d2
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1 == s2
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };
                if cmp_result {
                    unsafe { (*frame).opline = op_array.instructions().as_ptr().add(opline.result as usize) };

                    continue;
                }
                opline_ptr = unsafe { opline_ptr.add(1) };

            }

            OpCode::Add => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_add(l2) {
                        Some(sum) => unsafe { frame_tmp_set_long(frame, result_ptr, sum) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 + l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 + d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for +".into()));
                }
            }

            OpCode::Sub => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_sub(l2) {
                        Some(diff) => unsafe { frame_tmp_set_long(frame, result_ptr, diff) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 - l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 - d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for -".into()));
                }
            }

            OpCode::Mul => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_mul(l2) {
                        Some(prod) => unsafe { frame_tmp_set_long(frame, result_ptr, prod) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 * l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 * d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for *".into()));
                }
            }

            OpCode::Div => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    if d2 == 0.0 {
                        return Err(VmError::Fatal("Division by zero".into()));
                    }
                    // PHP: if both are long and divisible, result is long
                    if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                        if l2 != 0 && l1 % l2 == 0 {
                            unsafe { frame_tmp_set_long(frame, result_ptr, l1 / l2) };
                        } else {
                            unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 / d2)) };
                        }
                    } else {
                        unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 / d2)) };
                    }
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for /".into()));
                }
            }

            OpCode::Mod => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    if l2 == 0 {
                        return Err(VmError::Fatal("Division by zero".into()));
                    }
                    unsafe { frame_tmp_set_long(frame, result_ptr, l1 % l2) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for %".into()));
                }
            }

            OpCode::Concat => {
                op_concat(eg, frame, op_array, opline)?;
            }

            OpCode::Spaceship => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let cmp = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1.cmp(&l2)
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1.partial_cmp(&d2).unwrap_or(std::cmp::Ordering::Equal)
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1.cmp(s2)
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for <=>".into()));
                };
                let val = match cmp {
                    std::cmp::Ordering::Less => -1i64,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                };
                unsafe { frame_tmp_set_long(frame, result_ptr, val) };
            }

            OpCode::Pow => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    if l2 >= 0 {
                        unsafe { frame_tmp_set_long(frame, result_ptr, l1.wrapping_pow(l2 as u32)) };
                    } else {
                        unsafe { frame_tmp_set(frame, result_ptr, Value::double((l1 as f64).powf(l2 as f64))) };
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1.powf(d2))) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for **".into()));
                }
            }

            OpCode::BitwiseAnd => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let l1 = op1.to_long_val();
                let l2 = op2.to_long_val();
                unsafe { frame_tmp_set_long(frame, result_ptr, l1 & l2) };
            }

            OpCode::BitwiseOr => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let l1 = op1.to_long_val();
                let l2 = op2.to_long_val();
                unsafe { frame_tmp_set_long(frame, result_ptr, l1 | l2) };
            }

            OpCode::BitwiseXor => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let l1 = op1.to_long_val();
                let l2 = op2.to_long_val();
                unsafe { frame_tmp_set_long(frame, result_ptr, l1 ^ l2) };
            }

            OpCode::ShiftLeft => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let l1 = op1.to_long_val();
                let l2 = op2.to_long_val();
                unsafe { frame_tmp_set_long(frame, result_ptr, l1.wrapping_shl(l2 as u32)) };
            }

            OpCode::ShiftRight => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let l1 = op1.to_long_val();
                let l2 = op2.to_long_val();
                unsafe { frame_tmp_set_long(frame, result_ptr, l1.wrapping_shr(l2 as u32)) };
            }

            OpCode::BitwiseNot => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let l = val.to_long_val();
                unsafe { frame_tmp_set_long(frame, result_ptr, !l) };
            }

            OpCode::IsEqual | OpCode::IsNotEqual | OpCode::IsSmaller | OpCode::IsSmallerOrEqual => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match opline.opcode {
                        OpCode::IsEqual => l1 == l2,
                        OpCode::IsNotEqual => l1 != l2,
                        OpCode::IsSmaller => l1 < l2,
                        OpCode::IsSmallerOrEqual => l1 <= l2,
                        _ => unreachable!(),
                    }
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    match opline.opcode {
                        OpCode::IsEqual => s1 == s2,
                        OpCode::IsNotEqual => s1 != s2,
                        OpCode::IsSmaller => s1 < s2,
                        OpCode::IsSmallerOrEqual => s1 <= s2,
                        _ => unreachable!(),
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    match opline.opcode {
                        OpCode::IsEqual => d1 == d2,
                        OpCode::IsNotEqual => d1 != d2,
                        OpCode::IsSmaller => d1 < d2,
                        OpCode::IsSmallerOrEqual => d1 <= d2,
                        _ => unreachable!(),
                    }
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };

                unsafe { frame_tmp_set_bool(frame, result_ptr, result) };
            }

            OpCode::IsIdentical | OpCode::IsNotIdentical => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let identical = values_identical(op1, op2);

                let result = match opline.opcode {
                    OpCode::IsIdentical => identical,
                    _ => !identical,
                };
                unsafe { frame_tmp_set_bool(frame, result_ptr, result) };
            }

            OpCode::Isset => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let is_set = val.value_type() != ValueType::Undef && val.value_type() != ValueType::Null;
                unsafe { frame_tmp_set_bool(frame, result_ptr, is_set) };
            }

            OpCode::Cast => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let casted = match opline.extended_value {
                    0 => Value::long(val.to_long_val()),    // (int)
                    1 => Value::double(val.to_float_val()), // (float)
                    2 => {                                   // (string)
                        if val.value_type() == ValueType::Object {
                            if let Some(result) = call_magic_method(eg, val, "__tostring", &[])? {
                                Value::string(result.echo_to_string())
                            } else {
                                Value::string(val.echo_to_string())
                            }
                        } else {
                            Value::string(val.echo_to_string())
                        }
                    }
                    3 => Value::bool(val.is_truthy()),      // (bool)
                    4 => {                                   // (array)
                        match val.value_type() {
                            ValueType::Array => val.clone(),
                            ValueType::Null | ValueType::Undef => Value::array(PhpArray::new()),
                            _ => {
                                let mut arr = PhpArray::new();
                                arr.push(val.clone());
                                Value::array(arr)
                            }
                        }
                    }
                    _ => val.clone(),
                };
                unsafe { frame_tmp_set(frame, result_ptr, casted) };
            }

            OpCode::BoolNot => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let negated = !val.is_truthy();
                unsafe { frame_tmp_set_bool(frame, result_ptr, negated) };
            }

            OpCode::Jmp => {
                // op1 = absolute instruction index to jump to
                let target = opline.op1 as usize;
                unsafe {
                    (*frame).opline = op_array.instructions().as_ptr().add(target);
                }

                continue; // skip normal advance
            }

            #[cfg(feature = "quick-loops")]
            OpCode::QuickLongLoopJmp => {
                unsafe { execute_quick_loop_backedge(eg, frame, op_array, opline)? };
                continue;
            }

            #[cfg(not(feature = "quick-loops"))]
            OpCode::QuickLongLoopJmp => {
                let target = opline.op1 as usize;
                unsafe {
                    (*frame).opline = op_array.instructions().as_ptr().add(target);
                }
                continue;
            }

            OpCode::JmpZ => {
                // op1 = value to test, op2 = absolute jump target
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if !val.is_truthy() {
                    let target = opline.op2 as usize;
                    unsafe {
                        (*frame).opline = op_array.instructions().as_ptr().add(target);
                    }
    
                    continue;
                }
                // Fall-through after JmpZ is also a block leader

            }

            OpCode::JmpNZ => {
                // op1 = value to test, op2 = absolute jump target
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if val.is_truthy() {
                    let target = opline.op2 as usize;
                    unsafe {
                        (*frame).opline = op_array.instructions().as_ptr().add(target);
                    }
    
                    continue;
                }
                // Fall-through after JmpNZ is also a block leader

            }

            OpCode::DirectInternalCall1 => {
                // The handler ID is emitted from the same metadata used to
                // register the direct ABI. No function lookup, cache probe or
                // FunctionType check remains in this hot path.
                let argument = unsafe {
                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                let Some(kind) = crate::builtin_metadata::DirectInternalKind::from_id(
                    opline.extended_value,
                ) else {
                    return Err(VmError::Fatal(
                        "Invalid direct internal handler ID".into(),
                    ));
                };
                let result = crate::stdlib::invoke_direct_internal1(kind, argument)?;

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    if kind.result_may_need_cleanup() && opline.result_type == OpType::Tmp {
                        unsafe { frame_tmp_set(frame, result_ptr, result) };
                    } else {
                        // Scalar direct kinds always overwrite their own unique
                        // TMP, so its previous value is Undef or scalar too.
                        unsafe { result_ptr.write(result) };
                    }
                }
            }

            OpCode::Strlen => {
                let argument = unsafe {
                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                let length = crate::stdlib::direct_strlen_len(argument);

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                        unsafe { Value::write_long(result_ptr, length) };
                    } else {
                        unsafe { slot_set(result_ptr, Value::long(length)) };
                    }
                }
            }

            OpCode::Strlen_Cv => {
                let argument = unsafe { (*frame).cv(opline.op1 as u32) };
                let length = crate::stdlib::direct_strlen_len(argument);

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    debug_assert!(matches!(opline.result_type, OpType::Tmp | OpType::Var));
                    unsafe { Value::write_long(result_ptr, length) };
                }
            }

            OpCode::CallUserFuncArray => {
                // Compiler-lowered call_user_func_array(callback, args). The
                // callback is resolved at this opcode's own call site and the
                // packed-array/direct-internal path can therefore avoid both
                // the stdlib wrapper frame and the callback frame.
                let callback_raw = unsafe {
                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                let callback = if callback_raw.is_reference() {
                    unsafe { &*callback_raw.as_ref_ptr() }
                } else {
                    callback_raw
                };
                let args_raw = unsafe {
                    &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
                };
                let args = if args_raw.is_reference() {
                    unsafe { &*args_raw.as_ref_ptr() }
                } else {
                    args_raw
                };

                let ip = unsafe {
                    (opline as *const Instruction)
                        .offset_from(op_array.instructions.as_ptr()) as usize
                };
                let cache_slot = unsafe {
                    op_array.cache.as_ptr().add(ip)
                        as *mut crate::vm::instruction::InlineCache
                };
                let caller_class = get_caller_class(frame, eg);
                let result = crate::stdlib::invoke_call_user_func_array(
                    callback,
                    args,
                    eg,
                    caller_class.as_deref(),
                    Some(cache_slot),
                )?;

                if let Some(exc) = eg.exception.take() {
                    match throw_in_frame(eg, frame, exc) {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            frame = new_frame;
                            op_array = new_op_array;
                            continue;
                        }
                        ThrowResult::Unhandled(thrown) => {
                            eg.exception = Some(thrown);
                            return Ok(());
                        }
                    }
                }

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    unsafe { slot_set(result_ptr, result) };
                }
            }

            OpCode::InitUserCall => {
                // A one-argument call_user_func()/call_user_func_array() with
                // a simple argument compiles to an adjacent
                // InitUserCall + SendUser + DoFcall sequence. Once its runtime
                // callback names a pure direct-ABI internal function,
                // invoke that handler on the caller's borrowed value and skip
                // the callback frame and the next two VM dispatches entirely.
                let next = unsafe { &*opline_ptr.add(1) };
                let direct_shape = direct_user_calls_enabled()
                    && opline.extended_value == 1
                    && next.opcode == OpCode::SendUser
                    && next.extended_value == 0
                    && unsafe { (*opline_ptr.add(2)).opcode == OpCode::DoFcall };
                let mut initialized = false;

                if direct_shape {
                    let next2 = unsafe { &*opline_ptr.add(2) };
                    let callback_raw = unsafe {
                        &*(*frame).get_op_ptr(
                            opline.op1 as u32,
                            opline.op1_type,
                            op_array,
                        )
                    };
                    let callback = if callback_raw.is_reference() {
                        unsafe { &*callback_raw.as_ref_ptr() }
                    } else {
                        callback_raw
                    };
                    let direct_kind = callback.as_str().and_then(|name| {
                        crate::builtin_metadata::direct_internal_spec(name)
                            .filter(|spec| spec.required_args <= 1 && spec.max_args >= 1)
                            .map(|spec| spec.kind)
                    });

                    if let Some(kind) = direct_kind {
                        let argument = unsafe {
                            &*(*frame).get_op_ptr(
                                next.op1 as u32,
                                next.op1_type,
                                op_array,
                            )
                        };
                        let result = crate::stdlib::invoke_direct_internal1(kind, argument)?;
                        if next2.result_type != OpType::Unused {
                            let result_ptr = unsafe {
                                (*frame).get_op_mut(
                                    next2.result as u32,
                                    next2.result_type,
                                )
                            };
                            if matches!(next2.result_type, OpType::Tmp | OpType::Var) {
                                unsafe { frame_tmp_set(frame, result_ptr, result) };
                            } else {
                                unsafe { frame_slot_set(frame, result_ptr, result) };
                            }
                        }
                        // Loop-bottom advance adds one more instruction.
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        initialized = true;
                    } else if let Some(resolved) =
                        resolve_user_call_at_opline(eg, frame, op_array, opline)
                    {
                        init_resolved_user_call(
                            eg,
                            frame,
                            opline.extended_value,
                            resolved,
                        );
                        initialized = true;
                    }
                }

                if !initialized {
                    match op_init_user_call(eg, frame, op_array, opline)? {
                        ColdResult::NewFrame(new_frame, new_op_array) => {
                            frame = new_frame;
                            op_array = new_op_array;
                            continue;
                        }
                        ColdResult::Unhandled(thrown) => {
                            eg.exception = Some(thrown);
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }

            OpCode::InitFcall => {
                // op1 = num_args
                // op2 = CONST index pointing to function name string
                // extended_value = CONST index of fallback name (for unqualified calls in namespace), 0 = no fallback

                // Inline cache: if we resolved this function before, reuse the pointer
                let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
                let cached = op_array.cache[ip].func;
                let func_ptr = if !cached.is_null() {
                    cached
                } else {
                    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                    let name = name_val.as_str().unwrap_or_else(|| {
                        panic!("INIT_FCALL: op2 must be a string");
                    });
                    let func_ptr_opt = eg.find_function(name).or_else(|| {
                        if opline.extended_value != 0 {
                            let fallback_val = unsafe { &*(*frame).get_op_ptr(opline.extended_value, OpType::Const, op_array) };
                            if let Some(fallback_name) = fallback_val.as_str() {
                                return eg.find_function(fallback_name);
                            }
                        }
                        None
                    });
                    match func_ptr_opt {
                        Some(ptr) => {
                            // Cache for next time (don't cache failures — function may be defined later via include)
                            unsafe { (*(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache)).func = ptr; }
                            ptr
                        }
                        None => {
                            let err = make_error_value("Error", &format!("Call to undefined function {}()", name_val.as_str().unwrap_or("?")));
                            match throw_in_frame(eg, frame, err) {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue;
                                }
                                ThrowResult::Unhandled(thrown) => {
                                    eg.exception = Some(thrown);
                                    return Ok(());
                                }
                            }
                        }
                    }
                };

                let num_args = opline.op1 as u32;
                let call = eg.vm_stack.push_call_frame(func_ptr, num_args);
                unsafe {
                    (*call).prev_execute_data = frame;
                    (*call).call = (*frame).call;
                    (*frame).call = call;
                }

                // Peek ahead: if next is Sub_CvConst whose result feeds SendVal,
                // inline the subtraction + arg write, skip 2 instructions.
                let next = unsafe { &*opline_ptr.add(1) };
                if next.opcode == OpCode::Sub_CvConst {
                    let next2 = unsafe { &*opline_ptr.add(2) };
                    if next2.opcode == OpCode::SendVal
                        && next2.op1_type == OpType::Tmp
                        && next2.op1 == next.result
                    {
                        let op1_cv = unsafe { (*frame).cv(next.op1 as u32) };
                        let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                        let op2 = &op_array.literals()[next.op2 as usize];
                        if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                            let dst = unsafe {
                                (call as *mut Value).add(CALL_FRAME_SLOTS + next2.op2 as usize)
                            };
                            match l1.checked_sub(l2) {
                                Some(diff) => unsafe { Value::write_long(dst, diff) },
                                None => unsafe { dst.write(Value::double(l1 as f64 - l2 as f64)) },
                            }
                            // Skip Sub_CvConst + SendVal: advance local +2, loop bottom adds +1 → net +3
                            opline_ptr = unsafe { opline_ptr.add(2) };
                        }
                    }
                } else if next.opcode == OpCode::SendVal
                    && unsafe { try_send_scalar_arg(frame, call, op_array, next) }
                {
                    // InitFcall + scalar SendVal: argument is already in the
                    // callee frame. Loop-bottom advance skips the SendVal.
                    opline_ptr = unsafe { opline_ptr.add(1) };
                }
            }

            OpCode::SendVal => {
                // Send value to pending call frame
                // op1 = value to send, op2 = argument number (0-based)
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                let dst = unsafe {
                    (call as *mut Value).add(CALL_FRAME_SLOTS + opline.op2 as usize)
                };
                // For TMP/Var operands that are provably scalar (Long, Double, Bool, Null),
                // use raw 16-byte bitwise copy — no clone/drop overhead.
                // TMP values are consumed (not read again), so move semantics are valid.
                // IMPORTANT: heap types (String, Array, Object, Closure) and References
                // MUST go through clone to maintain refcount / avoid double-free.
                if opline.op1_type == OpType::Tmp || opline.op1_type == OpType::Var {
                    let src = unsafe {
                        (frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                    };
                    let src_val = unsafe { &*src };
                    if !src_val.needs_cleanup() && !src_val.is_reference() {
                        // Scalar TMP/Var: safe bitwise move
                        unsafe { Value::raw_copy(src, dst) };
                    } else {
                        // Heap or reference TMP/Var: must clone + mark callee heap bits
                        let cloned = src_val.clone();
                        unsafe { dst.write(cloned) };
                        unsafe {
                            (*call).has_heap_slots = true;
                            let total = (*call).num_cvs + (*call).num_temps;
                            if total <= 64 {
                                (*call).heap_bitmap |= 1u64 << opline.op2;
                            }
                        }
                    }
                } else {
                    let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                    let cloned = val.clone();
                    unsafe { dst.write(cloned) };
                    // Mark heap bit if needed
                    if unsafe { (*dst).needs_cleanup() } {
                        unsafe {
                            (*call).has_heap_slots = true;
                            let total = (*call).num_cvs + (*call).num_temps;
                            if total <= 64 {
                                (*call).heap_bitmap |= 1u64 << opline.op2;
                            }
                        }
                    }
                }
            }

            OpCode::SendRef => {
                // Send reference to caller's CV into callee frame
                // op1 = CV index in caller, op1_type must be CV
                // op2 = argument number in callee (0-based)
                debug_assert!(opline.op1_type == OpType::Cv);
                let caller_cv_ptr = unsafe {
                    let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
                    let raw_ptr = base.add(opline.op1 as usize);
                    // If caller's CV is itself a reference, forward the target
                    if (*raw_ptr).is_reference() {
                        (*raw_ptr).as_ref_ptr()
                    } else {
                        raw_ptr
                    }
                };
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                let arg_slot = unsafe { (*call).cv_mut(opline.op2 as u32) };
                unsafe { frame_slot_init(call, arg_slot as *mut Value, Value::reference(caller_cv_ptr)) };
            }

            OpCode::SendVarEx => {
                // Runtime-checked send: by-ref if callee expects it AND op1 is CV, else by-val
                // op2 = CV slot in callee, extended_value = parameter index for ref_args check
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                let param_idx = opline.extended_value;
                let func_common = unsafe { &*(*call).func };
                let is_ref = func_common.sig.is_param_by_ref(param_idx);

                if is_ref && opline.op1_type == OpType::Cv {
                    // Same logic as SendRef
                    let caller_cv_ptr = unsafe {
                        let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
                        let raw_ptr = base.add(opline.op1 as usize);
                        if (*raw_ptr).is_reference() {
                            (*raw_ptr).as_ref_ptr()
                        } else {
                            raw_ptr
                        }
                    };
                    let arg_slot = unsafe { (*call).cv_mut(opline.op2 as u32) };
                    unsafe { frame_slot_init(call, arg_slot as *mut Value, Value::reference(caller_cv_ptr)) };
                } else {
                    // Same logic as SendVal
                    let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                    let cloned = val.clone();
                    let arg_slot = unsafe { (*call).cv_mut(opline.op2 as u32) };
                    unsafe { frame_slot_init(call, arg_slot as *mut Value, cloned) };
                }
            }

            OpCode::SendUser => {
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                let func_common = unsafe { &*(*call).func };
                let destination_index = func_common
                    .sig
                    .param_cv_index(opline.extended_value);
                let value = unsafe {
                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                // call_user_func forwards ordinary arguments by value. Follow
                // an existing reference for the read, but do not create a new
                // reference merely because the callback parameter is by-ref.
                let value = if value.is_reference() {
                    unsafe { &*value.as_ref_ptr() }
                } else {
                    value
                };
                let destination = unsafe { (*call).cv_mut(destination_index) };
                unsafe { frame_slot_init(call, destination as *mut Value, value.clone()) };
            }

            OpCode::SendNamed => {
                match op_send_named(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::DoFcall => {
                // Execute the pending call
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                // Restore previous pending call from the chain
                unsafe { (*frame).call = (*call).call };

                // ── FastScalar path: tightest call protocol ──
                // Preconditions guaranteed at compile time: fixed arity, no by-ref,
                // no variadics, no generator, no globals, no type hints, no return type.
                // Runtime: only check fn_type + plan + no pending edge cases.
                let func_common_fast = unsafe { &*(*call).func };

                // ── Fast path for fixed-signature internal functions ──
                // Internal handlers still receive their ordinary ExecuteData
                // frame, so this changes no stdlib ABI or argument ownership.
                // It only avoids the generic type/variadic/class validation
                // path when the constructor proved those features absent.
                if func_common_fast.fn_type == FunctionType::Internal
                    && func_common_fast.plan.call == CallStrategy::Fast
                    && eg.pending_invoke_this.is_none()
                    && eg.pending_named_variadic.is_empty()
                {
                    let num_args_fast = unsafe { (*call).num_args };
                    let arity_ok = num_args_fast >= func_common_fast.sig.required_num_args
                        && num_args_fast <= func_common_fast.sig.public_arity();
                    let required_args_present = !unsafe { (*call).named_args_used } || {
                        let mut all_present = true;
                        for i in 0..func_common_fast.sig.required_num_args {
                            let cv_idx = func_common_fast.sig.param_cv_index(i);
                            if unsafe { (*(*call).cv(cv_idx)).is_undef() } {
                                all_present = false;
                                break;
                            }
                        }
                        all_present
                    };

                    if arity_ok && required_args_present {
                        stats::inc_do_fcall_fast();
                        let return_value_ptr = match opline.result_type {
                            OpType::Tmp | OpType::Var => unsafe {
                                (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                            },
                            OpType::Unused => std::ptr::null_mut(),
                            _ => unsafe {
                                (*frame).get_op_mut(opline.result as u32, opline.result_type)
                            },
                        };
                        unsafe { (*call).return_value = return_value_ptr };

                        let internal = unsafe {
                            &*((*call).func as *const super::function::InternalFunction)
                        };
                        if !return_value_ptr.is_null() {
                            unsafe { std::ptr::drop_in_place(return_value_ptr) };
                        }
                        let handler_result = (internal.handler)(call, return_value_ptr, eg);
                        unsafe { cleanup_frame_slots(call) };
                        eg.vm_stack.pop_call_frame(call);

                        if let Some(exc) = eg.exception.take() {
                            match throw_in_frame(eg, frame, exc) {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue;
                                }
                                ThrowResult::Unhandled(thrown) => {
                                    eg.exception = Some(thrown);
                                    return Ok(());
                                }
                            }
                        }
                        if let Err(e) = handler_result {
                            return Err(e);
                        }

                        unsafe { (*frame).opline = opline_ptr.add(1) };
                        continue 'vm;
                    }
                }

                if func_common_fast.fn_type == FunctionType::User
                    && func_common_fast.plan.call == CallStrategy::FastScalar
                    // FastScalar is fixed-arity by construction, so the
                    // required public count is also its exact arity. This
                    // avoids recomputing public_arity() on every normal call
                    // while still excluding the hidden method `$this`.
                    && unsafe { (*call).num_args } == func_common_fast.sig.required_num_args
                    && eg.pending_invoke_this.is_none()
                    && eg.pending_named_variadic.is_empty()
                {
                    let has_hole = unsafe { (*call).named_args_used } && {
                        let mut hole = false;
                        for i in 0..func_common_fast.sig.public_arity() {
                            let cv_idx = func_common_fast.sig.param_cv_index(i);
                            if unsafe { (*(*call).cv(cv_idx)).is_undef() } {
                                hole = true;
                                break;
                            }
                        }
                        hole
                    };
                    if !has_hole {
                    stats::inc_do_fcall_fast();

                    // Function-level hotness tracking.
                    // Promotion uses can_promote_to_hot() — single source of truth.
                    let cc = func_common_fast.call_count.get();
                    if cc < u32::MAX { func_common_fast.call_count.set(cc + 1); }
                    if cc == FUNC_HOT_THRESHOLD && func_common_fast.hot_status.get() == HotStatus::Cold {
                        if func_common_fast.can_promote_to_hot() {
                            func_common_fast.hot_status.set(HotStatus::Hot);
                        }
                    }

                    let user = unsafe { &*((*call).func as *const UserFunction) };
                    let return_value_ptr = match opline.result_type {
                        OpType::Tmp | OpType::Var => unsafe {
                            (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                        },
                        OpType::Unused => std::ptr::null_mut(),
                        _ => unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) },
                    };
                    unsafe { (*call).return_value = return_value_ptr };
                    unsafe {
                        (*call).opline = user.op_array.instructions.as_ptr();
                        (*frame).opline = opline_ptr.add(1);
                    }
                    eg.current_execute_data.set(call);

                    // Hot executor dispatch: if callee is hot, run in specialized executor.
                    // On Completed: callee returned, frame popped — restore caller state.
                    // On Bailout: callee still active — switch to it in baseline loop.
                    if func_common_fast.hot_status.get() == HotStatus::Hot {
                        match super::hot::execute_hot_frame(eg, call)? {
                            super::hot::HotResult::Completed => {
                                // Callee done. eg.current_execute_data is our caller (frame).
                                // (*frame).opline was already set to DoFcall+1 above.
                                // op_array unchanged (same caller function).
                                continue;
                            }
                            super::hot::HotResult::Bailout => {
                                // Callee bailed. It's the active frame with opline at bailout point.
                                frame = eg.current_execute_data.get();
                                op_array = unsafe { (*frame).op_array() };
                                continue;
                            }
                        }
                    } else {
                        // Cold function — baseline interpreter
                        frame = call;
                        op_array = unsafe { (*frame).op_array() };
                        continue;
                    }
                    }
                }

                // ── Fast path for simple user function calls ──
                if func_common_fast.fn_type == FunctionType::User
                    && func_common_fast.plan.call == CallStrategy::Fast
                    && eg.pending_invoke_this.is_none()
                    && eg.pending_named_variadic.is_empty()
                {
                    let num_args_fast = unsafe { (*call).num_args };
                    let user = unsafe { &*((*call).func as *const UserFunction) };
                    let mut has_required_holes = false;
                    if func_common_fast.sig.required_num_args > 0 {
                        for i in 0..func_common_fast.sig.required_num_args {
                            let cv_idx = func_common_fast.sig.param_cv_index(i);
                            let val = unsafe { &*(*call).cv(cv_idx) };
                            if val.is_undef() {
                                has_required_holes = true;
                                break;
                            }
                        }
                    }
                    if !user.op_array.is_generator
                        && !has_required_holes
                        && num_args_fast >= func_common_fast.sig.required_num_args
                        && num_args_fast <= func_common_fast.sig.public_arity()
                    {
                        let mut type_ok = true;
                        let hints = &func_common_fast.sig.param_type_hints;
                        let caller_strict = op_array.strict_types;
                        let has_typed_params = !hints.is_empty() && hints.iter().any(|h| !matches!(h, ParamTypeHint::None | ParamTypeHint::Mixed));
                        if caller_strict && has_typed_params {
                            type_ok = false;
                        } else if !hints.is_empty() {
                            let check_count = std::cmp::min(num_args_fast as usize, hints.len());
                            for i in 0..check_count {
                                let hint = &hints[i];
                                if matches!(hint, ParamTypeHint::None | ParamTypeHint::Mixed) {
                                    continue;
                                }
                                let cv_idx = func_common_fast.sig.param_cv_index(i as u32);
                                let val = unsafe { &*(*call).cv(cv_idx) };
                                let ok = match hint {
                                    ParamTypeHint::Int => val.as_long().is_some(),
                                    ParamTypeHint::Float => val.value_type() == ValueType::Double || val.as_long().is_some(),
                                    ParamTypeHint::Bool => val.value_type() == ValueType::True || val.value_type() == ValueType::False,
                                    ParamTypeHint::String => val.as_str().is_some(),
                                    _ => true,
                                };
                                if !ok {
                                    type_ok = false;
                                    break;
                                }
                            }
                        }
                        if !type_ok {
                            // Fall through to full path for proper TypeError
                        } else {
                        stats::inc_do_fcall_fast();
                        let return_value_ptr = match opline.result_type {
                            OpType::Tmp | OpType::Var => unsafe {
                                (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                            },
                            OpType::Unused => std::ptr::null_mut(),
                            _ => unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) },
                        };
                        unsafe { (*call).return_value = return_value_ptr };
                        if user.op_array.may_access_globals
                            && (!op_array.main_scope_vars.is_empty() || !op_array.global_vars.is_empty())
                        {
                            let vars_to_sync = if !op_array.main_scope_vars.is_empty() {
                                &op_array.main_scope_vars
                            } else {
                                &op_array.global_vars
                            };
                            for (cv_idx, var_name) in vars_to_sync {
                                let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                                let val = unsafe { (*cv_ptr).clone() };
                                globals_set(&mut eg.globals, var_name, val);
                            }
                        }
                        unsafe {
                            (*call).opline = user.op_array.instructions.as_ptr();
                            (*frame).opline = opline_ptr.add(1);
                        }
                        // Function-level hotness tracking.
                        // Promotion uses can_promote_to_hot() — single source of truth.
                        let cc = func_common_fast.call_count.get();
                        if cc < u32::MAX { func_common_fast.call_count.set(cc + 1); }
                        if cc == FUNC_HOT_THRESHOLD && func_common_fast.hot_status.get() == HotStatus::Cold {
                            if func_common_fast.can_promote_to_hot() {
                                func_common_fast.hot_status.set(HotStatus::Hot);
                            }
                        }

                        eg.current_execute_data.set(call);

                        // Hot executor dispatch: Hot status implies eligible (promotion guard above).
                        if func_common_fast.hot_status.get() == HotStatus::Hot {
                            match super::hot::execute_hot_frame(eg, call)? {
                                super::hot::HotResult::Completed => {
                                    continue;
                                }
                                super::hot::HotResult::Bailout => {
                                    frame = eg.current_execute_data.get();
                                    op_array = unsafe { (*frame).op_array() };
                                    continue;
                                }
                            }
                        } else {
                            frame = call;
                            op_array = unsafe { (*frame).op_array() };
                            continue;
                        }
                    } // else: type_ok
                    } // if arity/generator ok
                }

                // ── Full path (handles all edge cases) ──
                stats::inc_do_fcall_full();

                // Set up return value in result slot if used
                let return_value_ptr = if opline.result_type != OpType::Unused {
                    unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) }
                } else {
                    std::ptr::null_mut()
                };
                unsafe { (*call).return_value = return_value_ptr };

                // Eagerly extract any pending named variadic args so they don't
                // leak on error paths (TypeError, arity, etc.).
                let call_key = call as usize;
                let pending_named = eg.pending_named_variadic.remove(&call_key);

                // __invoke dispatch: SendVal wrote args to CV 0..N-1 but the
                // method expects $this at CV 0 and args at CV 1..N.
                // Shift args right by 1 and place $this at CV 0.
                if let Some(this_val) = eg.pending_invoke_this.take() {
                    let num = unsafe { (*call).num_args };
                    // Shift args from CV[N-1] down to CV[0] into CV[N]..CV[1]
                    for i in (0..num).rev() {
                        let val = unsafe { (*call).cv(i).clone() };
                        let dst = unsafe { (*call).cv_mut(i + 1) };
                        unsafe { frame_slot_set(call, dst as *mut Value, val) };
                    }
                    // Place $this at CV 0
                    let this_slot = unsafe { (*call).cv_mut(0) };
                    unsafe { frame_slot_set(call, this_slot as *mut Value, this_val) };
                    // num_args stays as the public arg count (excluding $this)
                    // — same convention as InitMethodCall
                }

                // Validate argument count
                // `num_args` is the explicit (public) arg count from the call site.
                // `public_arity()` = declared param count excluding hidden $this.
                let func_common = unsafe { &*(*call).func };
                let num_args = unsafe { (*call).num_args };
                let public_max = func_common.sig.public_arity();
                if num_args < func_common.sig.required_num_args {
                    return Err(VmError::Fatal(format!(
                        "Too few arguments, {} passed and exactly {} expected",
                        num_args, func_common.sig.required_num_args
                    )));
                }
                if !func_common.sig.is_variadic && num_args > public_max {
                    return Err(VmError::Fatal(format!(
                        "Too many arguments, {} passed and at most {} expected",
                        num_args, public_max
                    )));
                }

                // Named args can skip required positional params; verify no holes
                // in the required range. A required param is one at index < required_num_args.
                if func_common.sig.required_num_args > 0 {
                    for i in 0..func_common.sig.required_num_args {
                        let cv_idx = func_common.sig.param_cv_index(i);
                        let val = unsafe { &*(*call).cv(cv_idx) };
                        if val.is_undef() {
                            return Err(VmError::Fatal(format!(
                                "Too few arguments, {} passed and exactly {} expected",
                                num_args, func_common.sig.required_num_args
                            )));
                        }
                    }
                }

                // Resolve callee's declaring class for self/parent/static type hints
                let callee_class = eg.declaring_class_of(unsafe { (*call).func }).map(|s| s.to_string());
                let callee_class_ref = callee_class.as_deref();

                // Type-check arguments against declared type hints
                if !func_common.sig.param_type_hints.is_empty() {
                    let mut type_error: Option<Value> = None;
                    for (i, hint) in func_common.sig.param_type_hints.iter().enumerate() {
                        if matches!(hint, crate::vm::function::ParamTypeHint::None) { continue; }
                        let cv_idx = func_common.sig.param_cv_index(i as u32);
                        if (i as u32) >= num_args { break; }
                        let val = unsafe { &*(*call).cv(cv_idx) };
                        if val.is_undef() { continue; }
                        if !check_type_hint(val, hint, eg, op_array.strict_types, callee_class_ref) {
                            type_error = Some(make_error_value("TypeError", &format!(
                                "Argument #{} must be of type {}, {} given",
                                i + 1,
                                hint.display_name(),
                                val.type_name()
                            )));
                            break;
                        }
                    }
                    if let Some(err) = type_error {
                        // Clean up call frame before throwing
                        unsafe { cleanup_frame_slots(call) };
                        eg.vm_stack.pop_call_frame(call);
                        match throw_in_frame(eg, frame, err) {
                            ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue; }
                            ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                        }
                    }
                }

                // Pack extra arguments into variadic parameter array
                if func_common.sig.is_variadic {
                    let extra_count = num_args.saturating_sub(public_max);
                    let mut variadic_arr = crate::value::PhpArray::new();
                    let cv_start = func_common.sig.variadic_cv_index;
                    for i in 0..extra_count {
                        let arg = unsafe { (*call).cv(cv_start + i) }.clone();
                        variadic_arr.push(arg);
                    }
                    // Merge any named variadic args (extracted at start of DoFcall)
                    if let Some(named_extras) = pending_named {
                        // Type-check each named extra against the variadic param's type hint
                        let variadic_hint_idx = public_max as usize; // index in param_type_hints
                        let variadic_hint = func_common.sig.param_type_hints.get(variadic_hint_idx);
                        for (name, val) in named_extras {
                            if let Some(hint) = variadic_hint {
                                if !matches!(hint, crate::vm::function::ParamTypeHint::None)
                                    && !check_type_hint(&val, hint, eg, op_array.strict_types, callee_class_ref)
                                {
                                    let type_err = make_error_value("TypeError", &format!(
                                        "Named parameter ${} must be of type {}, {} given",
                                        name,
                                        hint.display_name(),
                                        val.type_name()
                                    ));
                                    unsafe { cleanup_frame_slots(call) };
                                    eg.vm_stack.pop_call_frame(call);
                                    match throw_in_frame(eg, frame, type_err) {
                                        ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                        ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                                    }
                                }
                            }
                            variadic_arr.set_str(&name, val);
                        }
                    }
                    // Overwrite the variadic CV slot with the packed array
                    let variadic_slot = unsafe { (*call).cv_mut(cv_start) };
                    unsafe {
                        frame_slot_set(call, variadic_slot as *mut Value, crate::value::Value::array(variadic_arr));
                    }
                }

                // Direct fn_type check — avoids Function wrapper overhead
                let call_fn_type = unsafe { (*(*call).func).fn_type };
                match call_fn_type {
                    FunctionType::User => {
                        let user = unsafe { &*((*call).func as *const UserFunction) };

                        // Generator function — create Generator object instead of executing
                        if user.op_array.is_generator {
                            use crate::vm::generator::{Generator, new_generator_ref};

                            // Collect argument values from the call frame CV slots
                            let mut args = Vec::new();
                            let num_cvs = user.op_array.num_cvs as usize;
                            for i in 0..num_cvs {
                                let val = unsafe { (*call).cv(i as u32) }.clone();
                                args.push(val);
                            }

                            let generator = Generator::new(
                                unsafe { (*call).func },
                                args,
                                user.op_array.num_cvs,
                                user.op_array.num_temps,
                            );
                            let gen_ref = new_generator_ref(generator);
                            let mut gen_obj = PhpObject::dynamic(
                                "Generator".to_string(),
                                0,
                                std::collections::HashMap::new(),
                            );
                            gen_obj.generator = Some(gen_ref);
                            let gen_val = Value::object(gen_obj);

                            // Write generator object as return value
                            if !return_value_ptr.is_null() {
                                unsafe { slot_set(return_value_ptr, gen_val) };
                            }

                            // Clean up the call frame (we didn't execute it)
                            unsafe { cleanup_frame_slots(call) };
                            eg.vm_stack.pop_call_frame(call);
                        } else {
                            // Sync caller's scope vars to eg.globals — only when callee may reach globals.
                            if user.op_array.may_access_globals {
                                let vars_to_sync = if !op_array.main_scope_vars.is_empty() {
                                    &op_array.main_scope_vars
                                } else {
                                    &op_array.global_vars
                                };
                                for (cv_idx, var_name) in vars_to_sync {
                                    let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                                    let val = unsafe { (*cv_ptr).clone() };
                                    globals_set(&mut eg.globals, var_name, val);
                                }
                            }
                            unsafe {
                                (*call).opline = user.op_array.instructions.as_ptr();
                                (*frame).opline = opline_ptr.add(1);
                            }
                            eg.current_execute_data.set(call);
                            frame = call;
                            op_array = unsafe { (*frame).op_array() };
            
                            continue;
                        }
                    }
                    FunctionType::Internal => {
                        let internal = unsafe {
                            &*((*call).func as *const super::function::InternalFunction)
                        };
                        if !return_value_ptr.is_null() {
                            unsafe { std::ptr::drop_in_place(return_value_ptr) };
                        }
                        let handler_result = (internal.handler)(call, return_value_ptr, eg);
                        unsafe { cleanup_frame_slots(call) };
                        eg.vm_stack.pop_call_frame(call);
                        // 1) eg.exception set (real PHP throw from callback) → catchable
                        if let Some(exc) = eg.exception.take() {
                            match throw_in_frame(eg, frame, exc) {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue;
                                }
                                ThrowResult::Unhandled(thrown) => {
                                    eg.exception = Some(thrown);
                                    return Ok(());
                                }
                            }
                        }
                        // 2) Handler returned Err (hard fatal) → not catchable
                        if let Err(e) = handler_result {
                            return Err(e);
                        }
                    }
                    FunctionType::Undef => {
                        let err = make_error_value("Error", "Call to undefined function");
                        match throw_in_frame(eg, frame, err) {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue;
                            }
                            ThrowResult::Unhandled(thrown) => {
                                eg.exception = Some(thrown);
                                return Ok(());
                            }
                        }
                    }
                }
            }

            OpCode::PreInc => {
                // ++$var: increment CV in place, result = new value
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
                let old = unsafe { &*cv_ptr };
                let new_val = if let Some(n) = old.as_long() {
                    match n.checked_add(1) {
                        Some(v) => Value::long(v),
                        None => Value::double(n as f64 + 1.0),
                    }
                } else if let Some(d) = old.to_double() {
                    Value::double(d + 1.0)
                } else {
                    Value::long(1) // PHP: null++ = 1
                };
                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                    unsafe { slot_set(result_ptr, new_val.clone()) };
                }
                unsafe { slot_set(cv_ptr, new_val) };
            }

            OpCode::PreDec => {
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
                let old = unsafe { &*cv_ptr };
                if let Some(n) = old.as_long() {
                    let new_val = match n.checked_sub(1) {
                        Some(v) => Value::long(v),
                        None => Value::double(n as f64 - 1.0),
                    };
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                        unsafe { slot_set(result_ptr, new_val.clone()) };
                    }
                    unsafe { slot_set(cv_ptr, new_val) };
                } else if let Some(d) = old.to_double() {
                    let new_val = Value::double(d - 1.0);
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                        unsafe { slot_set(result_ptr, new_val.clone()) };
                    }
                    unsafe { slot_set(cv_ptr, new_val) };
                } else {
                    // PHP: null-- has no effect, value stays null
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                        unsafe { slot_set(result_ptr, Value::null()) };
                    }
                }
            }

            OpCode::PostInc => {
                // $var++: increment CV in place, result = old value
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
                let old = unsafe { &*cv_ptr };
                let old_val = old.clone();
                let new_val = if let Some(n) = old.as_long() {
                    match n.checked_add(1) {
                        Some(v) => Value::long(v),
                        None => Value::double(n as f64 + 1.0),
                    }
                } else if let Some(d) = old.to_double() {
                    Value::double(d + 1.0)
                } else {
                    Value::long(1)
                };
                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                    unsafe { slot_set(result_ptr, old_val) };
                }
                unsafe { slot_set(cv_ptr, new_val) };
            }

            OpCode::PostDec => {
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
                let old = unsafe { &*cv_ptr };
                let old_val = old.clone();
                if let Some(n) = old.as_long() {
                    let new_val = match n.checked_sub(1) {
                        Some(v) => Value::long(v),
                        None => Value::double(n as f64 - 1.0),
                    };
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                        unsafe { slot_set(result_ptr, old_val) };
                    }
                    unsafe { slot_set(cv_ptr, new_val) };
                } else if let Some(d) = old.to_double() {
                    let new_val = Value::double(d - 1.0);
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                        unsafe { slot_set(result_ptr, old_val) };
                    }
                    unsafe { slot_set(cv_ptr, new_val) };
                } else {
                    // PHP: null-- has no effect, value stays null
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                        unsafe { slot_set(result_ptr, Value::null()) };
                    }
                }
            }

            OpCode::InitArray => {
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                unsafe { slot_set(result_ptr, Value::array(PhpArray::new())) };
            }

            OpCode::AddArrayElement => {
                // op1 = array TMP, op2 = value, result = key (or Unused for auto-key)
                let val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let cloned_val = val.clone();
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                let php_arr = arr.as_array_mut().ok_or_else(|| {
                    VmError::Fatal("AddArrayElement: operand is not an array".into())
                })?;
                if opline.result_type != OpType::Unused {
                    let key_val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
                    let key = value_to_array_key(key_val)?;
                    php_arr.set(key, cloned_val);
                } else {
                    php_arr.push(cloned_val);
                }
            }

            OpCode::FetchDimR => {
                // result = op1[op2]
                let arr_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let idx_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let Some(arr) = arr_val.as_array() {
                    let fetched = match value_to_array_key_ref(idx_val)? {
                        ArrayKeyRef::Int(key) => arr.get_int(key),
                        ArrayKeyRef::String(key) => arr.get_str(key),
                    };
                    let val = fetched.cloned().unwrap_or(Value::null());
                    unsafe { slot_set(result_ptr, val) };
                } else if let Some(s) = arr_val.as_str() {
                    // String offset access: $s[0] — PHP strings are byte-oriented
                    let bytes = s.as_bytes();
                    if let Some(idx) = idx_val.as_long() {
                        let pos = if idx >= 0 {
                            idx as usize
                        } else {
                            let len = bytes.len() as i64;
                            let p = len + idx;
                            if p >= 0 { p as usize } else { usize::MAX }
                        };
                        let val = if pos < bytes.len() {
                            // Single byte as a string
                            Value::string(String::from(bytes[pos] as char))
                        } else {
                            Value::string("")
                        };
                        unsafe { slot_set(result_ptr, val) };
                    } else {
                        unsafe { slot_set(result_ptr, Value::null()) };
                    }
                } else {
                    unsafe { slot_set(result_ptr, Value::null()) };
                }
            }

            OpCode::AssignDim => {
                // op1[op2] = result (value source encoded in result/result_type)
                let idx_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let key = value_to_array_key(idx_val)?;
                let val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
                let cloned_val = val.clone();
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                // Auto-create array if variable is null/undef
                if arr.value_type() == ValueType::Null || arr.value_type() == ValueType::Undef {
                    unsafe { slot_set(arr_ptr, Value::array(PhpArray::new())) };
                    let arr = unsafe { &mut *arr_ptr };
                    arr.as_array_mut().unwrap().set(key, cloned_val);
                } else if let Some(php_arr) = arr.as_array_mut() {
                    php_arr.set(key, cloned_val);
                } else {
                    return Err(VmError::Fatal("Cannot use a scalar value as an array".into()));
                }
            }

            OpCode::ArrayPushOp => {
                // op1[] = op2
                let val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let cloned_val = val.clone();
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                // Auto-create array if variable is null/undef
                if arr.value_type() == ValueType::Null || arr.value_type() == ValueType::Undef {
                    unsafe { slot_set(arr_ptr, Value::array(PhpArray::new())) };
                    let arr = unsafe { &mut *arr_ptr };
                    arr.as_array_mut().unwrap().push(cloned_val);
                } else if let Some(php_arr) = arr.as_array_mut() {
                    php_arr.push(cloned_val);
                } else {
                    return Err(VmError::Fatal("[] operator not supported for non-array".into()));
                }
            }

            OpCode::UnsetDim => {
                // Remove key op2 from array op1
                let idx_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let key = value_to_array_key(idx_val)?;
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                match arr.value_type() {
                    ValueType::Array => {
                        arr.as_array_mut().unwrap().remove(&key);
                    }
                    ValueType::Undef | ValueType::Null => {
                        // PHP silently ignores unset on undef/null
                    }
                    _ => {
                        return Err(VmError::Fatal(
                            "Cannot unset offset in a non-array variable".into(),
                        ));
                    }
                }
            }

            OpCode::ForeachInit => {
                if op_foreach_init(eg, frame, op_array, opline)? {
                    continue;
                }
            }

            OpCode::ForeachNext => {
                op_foreach_next(eg, frame, op_array, opline)?;
            }

            OpCode::Throw => {
                match op_throw(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::NewObj => {
                match op_new_obj(eg, frame, op_array, opline)? {
                    ColdResult::Continue => { continue; }
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::FetchObjR => {
                op_fetch_obj_r(eg, frame, op_array, opline)?;
            }

            OpCode::AssignObjProp => {
                // ── Cache-hit fast path for public, non-enum, non-readonly properties ──
                let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if obj_val.value_type() == ValueType::Object {
                    let obj_class_id = unsafe { obj_val.object_class_id_unchecked() };
                    let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
                    let ic = &op_array.cache[ip];
                    // flags == 3: read-safe + write-safe declared property slot.
                    if ic.property_flags() == 3 && ic.class_id == obj_class_id && obj_class_id != 0 {
                        let val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
                        let cloned = val.clone();
                        unsafe {
                            obj_val.object_set_property_slot_unchecked(ic.property_slot(), cloned);
                        };
                    } else {
                        match op_assign_obj_prop(eg, frame, op_array, opline)? {
                            ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                            ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                            _ => {}
                        }
                    }
                } else {
                    match op_assign_obj_prop(eg, frame, op_array, opline)? {
                        ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                        ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                        _ => {}
                    }
                }
            }

            OpCode::AssignObjDim => {
                // $obj->prop[$key] = val
                let obj_ptr = unsafe { (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let key_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
                let prop_name_val = &op_array.literals[opline.extended_value as usize];
                let prop_name = prop_name_val.as_str().unwrap_or("").to_string();
                let key = key_val.clone();
                let new_val = val.clone();

                let arr_key = value_to_array_key(&key)?;
                let obj = unsafe { &*obj_ptr };
                if let Some(mut php_obj) = obj.as_object_mut() {
                    let caller_class = get_caller_class(frame, eg);
                    let receiver_in_scope = caller_class.as_ref().map_or(false, |cc| {
                        eg.class_is_a(&php_obj.class_name, cc)
                    });
                    let effective_caller = if receiver_in_scope { caller_class.as_deref() } else { None };
                    let storage_key = crate::runtime::resolve_property_key(eg, &php_obj.class_name, &prop_name, effective_caller);

                    if let Some(arr_val) = php_obj.get_property_mut(&storage_key) {
                        // Property exists — mutate the array in place
                        if let Some(arr) = arr_val.as_array_mut() {
                            arr.set(arr_key, new_val);
                        } else {
                            return Err(VmError::Fatal(format!(
                                "Cannot use object of type {} as array", php_obj.class_name
                            )));
                        }
                    } else {
                        // Property doesn't exist — create new array
                        let mut new_arr = crate::value::PhpArray::new();
                        new_arr.set(arr_key, new_val);
                        php_obj.set_property(&storage_key, Value::array(new_arr));
                    }
                } else {
                    return Err(VmError::Fatal("Attempt to assign property on non-object".into()));
                }
            }

            OpCode::InitMethodCall => {
                // ── Cache-hit fast path (inlined) ──
                // Most method calls hit the monomorphic inline cache.
                // Bypass the #[inline(never)] helper entirely on cache hit.
                let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if obj_val.value_type() == ValueType::Object {
                    let obj_class_id = unsafe { obj_val.object_class_id_unchecked() };
                    let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
                    let ic = &op_array.cache[ip];
                    if !ic.func.is_null() && ic.class_id == obj_class_id && obj_class_id != 0 {
                        let func_ptr = ic.func;
                        let num_args = opline.extended_value;
                        let call = eg.vm_stack.push_call_frame(func_ptr, num_args + 1);
                        unsafe {
                            (*call).num_args = num_args;
                            (*call).prev_execute_data = frame;
                            (*call).call = (*frame).call;
                            (*frame).call = call;
                            let common = &*func_ptr;
                            if common.plan.borrow_this {
                                frame_set_borrowed_this(call, obj_val as *const Value);
                            } else {
                                frame_set_this(call, obj_val.clone());
                            }
                        }

                        // Bind the contiguous scalar argument prefix while the
                        // new frame is hot in registers. Nested argument
                        // expressions naturally stop the fusion.
                        let bound = unsafe {
                            bind_contiguous_scalar_args(
                                frame,
                                call,
                                op_array,
                                opline_ptr.add(1),
                                num_args,
                            )
                        };
                        if bound != 0 {
                            opline_ptr = unsafe { opline_ptr.add(bound) };
                        }
                    } else {
                        // Cache miss — full resolution in cold helper
                        match op_init_method_call(eg, frame, op_array, opline)? {
                            ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                            ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                            _ => {}
                        }
                    }
                } else {
                    // Non-object — cold path (error or __invoke)
                    match op_init_method_call(eg, frame, op_array, opline)? {
                        ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                        ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                        _ => {}
                    }
                }
            }

            OpCode::InitStaticCall => {
                match op_init_static_call(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::InitDynamicCall => {
                op_init_dynamic_call(eg, frame, op_array, opline)?;
            }

            OpCode::FetchStaticProp => {
                // Look up static property from class definition (used for enum cases)
                let class_name_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let prop_name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let cls = class_name_val.as_str().unwrap_or("");
                let prop = prop_name_val.as_str().unwrap_or("");

                let mut found = false;
                if let Some(class_def) = eg.class_table.get(cls) {
                    for (pname, default, _vis, _declaring) in &class_def.properties {
                        if pname == prop {
                            if let Some(val) = default {
                                unsafe { slot_set(result_ptr, val.clone()) };
                                found = true;
                            }
                            break;
                        }
                    }
                }
                if !found {
                    unsafe { slot_set(result_ptr, Value::null()) };
                }
            }

            OpCode::Instanceof => {
                let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let class_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let target = class_name.as_str().unwrap_or("");
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let is_instance = if let Some(obj) = obj_val.as_object() {
                    eg.class_is_a(&obj.class_name, target)
                } else {
                    false
                };
                unsafe { slot_set(result_ptr, Value::bool(is_instance)) };
            }

            OpCode::FetchConst => {
                if opline.extended_value == 1 {
                    // Define mode: const FOO = value;
                    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                    let value_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                    let name = name_val.as_str().unwrap_or("").to_string();
                    let value = value_val.clone();
                    eg.define_constant(&name, value).map_err(|e| VmError::Fatal(e))?;
                } else {
                    // Read mode: fetch constant value
                    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                    let name = name_val.as_str().unwrap_or("");
                    let value = eg.find_constant(name).ok_or_else(|| {
                        VmError::Fatal(format!("Undefined constant \"{}\"", name))
                    })?;
                    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                    unsafe { slot_set(result_ptr, value) };
                }
            }

            OpCode::BindDefaultParam => {
                // If CV slot is NOT undef (arg was passed), skip default init
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
                let is_undef = unsafe { (*cv_ptr).is_undef() };
                if !is_undef {
                    // Jump past the default expr computation + AssignCv
                    let target = opline.op2 as usize;
                    unsafe {
                        (*frame).opline = op_array.instructions.as_ptr().add(target);
                    }
                    continue;
                }
                // Otherwise fall through — next instructions compute and assign default
            }

            OpCode::BindGlobal => {
                // Bind a CV to a global variable: copy eg.globals[name] into CV
                let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let name = name_val.as_str().unwrap_or("").to_string();
                if let Some(val) = eg.globals.get(&name) {
                    let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
                    unsafe { slot_set(cv_ptr, val.clone()) };
                }
                // If not in globals, CV stays undef/null — that's fine, it will be written back on return
            }

            OpCode::BindStatic => {
                // Bind a CV to a static variable
                let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let var_name = name_val.as_str().unwrap_or("").to_string();
                let func_name_val = &op_array.literals[opline.extended_value as usize];
                let func_name = func_name_val.as_str().unwrap_or("").to_string();

                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
                if let Some(func_statics) = eg.static_vars.get(&func_name) {
                    if let Some(val) = func_statics.get(&var_name) {
                        unsafe { slot_set(cv_ptr, val.clone()) };
                    } else {
                        // First call — initialize with default value
                        if opline.result_type != OpType::Unused {
                            let default_val = unsafe {
                                &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array)
                            };
                            unsafe { slot_set(cv_ptr, default_val.clone()) };
                        } else {
                            unsafe { slot_set(cv_ptr, Value::null()) };
                        }
                    }
                } else {
                    // First call — no statics for this function yet, use default
                    if opline.result_type != OpType::Unused {
                        let default_val = unsafe {
                            &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array)
                        };
                        unsafe { slot_set(cv_ptr, default_val.clone()) };
                    } else {
                        unsafe { slot_set(cv_ptr, Value::null()) };
                    }
                }
            }

            OpCode::Return => {
                let func_common_ret = unsafe { &*(*frame).func };

                // ── FastScalar return: tightest path ──
                // No return type check, no globals sync, no dirty_globals propagation.
                // Guaranteed by FastScalar invariant: no globals, no statics, no try/finally,
                // no return type, no generator, may_access_globals == false.
                if func_common_ret.plan.call == CallStrategy::FastScalar
                    && func_common_ret.plan.ret == ReturnStrategy::Fast
                    && eg.exception.is_none()
                {
                    stats::inc_return_fast();
                    if opline.op1_type != OpType::Unused {
                        let return_target = unsafe { (*frame).return_value };
                        if !return_target.is_null() {
                            let frame_no_heap = !unsafe { (*frame).has_heap_slots };
                            if frame_no_heap && opline.op1_type != OpType::Const {
                                let retval_ptr = unsafe {
                                    (frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                                };
                                let src = if opline.op1_type == OpType::Cv {
                                    let cv_val = unsafe { &*retval_ptr };
                                    if cv_val.is_reference() {
                                        unsafe { cv_val.as_ref_ptr() as *const Value }
                                    } else {
                                        retval_ptr
                                    }
                                } else {
                                    retval_ptr
                                };
                                let prev = unsafe { (*frame).prev_execute_data };
                                if !prev.is_null() && unsafe { (*prev).has_heap_slots } {
                                    unsafe { std::ptr::drop_in_place(return_target) };
                                }
                                unsafe { Value::raw_copy(src, return_target) };
                            } else {
                                let retval = unsafe {
                                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                                };
                                unsafe { mark_caller_heap_return(frame, retval) };
                                unsafe { slot_set(return_target, retval.clone()) };
                            }
                        }
                    }
                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    // Recursive execute_ex boundary: callee done → return to caller's macro loop
                    if frame == initial_frame {
                        eg.current_execute_data.set(prev);
                        unsafe { cleanup_frame_slots(frame) };
                        eg.vm_stack.pop_call_frame(frame);
                        return Ok(());
                    }
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    // No dirty_globals check: FastScalar callee never touches globals,
                    // and may_access_globals == false means no deeper callee did either.
    
                    continue;
                }

                // ── Fast return path ──
                // Single precomputed flag check replaces 6 runtime conditions.
                // ReturnStrategy::Fast = no globals, no statics, no return type, no try/finally, not generator.
                if func_common_ret.plan.ret == ReturnStrategy::Fast
                    && eg.exception.is_none()
                {
                    // Inline return type validation for scalar hints.
                    // strict_types callers fall through to full path.
                    let ret_hint = &func_common_ret.sig.return_type_hint;
                    let has_return_type = !matches!(ret_hint, ParamTypeHint::None | ParamTypeHint::Mixed);
                    // strict_types with return type → use full path for proper enforcement.
                    if has_return_type && op_array.strict_types {
                        // Fall through to full return path.
                    } else {
                    if has_return_type && opline.op1_type != OpType::Unused {
                        let retval = unsafe {
                            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                        };
                        let type_ok = match ret_hint {
                            ParamTypeHint::Int => retval.as_long().is_some(),
                            ParamTypeHint::Float => retval.to_double().is_some(),
                            ParamTypeHint::Bool => retval.value_type() == ValueType::True || retval.value_type() == ValueType::False,
                            ParamTypeHint::String => retval.as_str().is_some(),
                            _ => true,
                        };
                        if !type_ok {
                            let err = make_error_value("TypeError", &format!(
                                "Return value must be of type {}, {} returned",
                                ret_hint.display_name(),
                                retval.type_name()
                            ));
                            match throw_in_frame(eg, frame, err) {
                                ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                            }
                        }
                    }
                    stats::inc_return_fast();
                    if opline.op1_type != OpType::Unused {
                        let return_target = unsafe { (*frame).return_value };
                        if !return_target.is_null() {
                            // Scalar-frame fast path: if frame has no heap slots and
                            // operand is a slot (not Const), ALL values are scalar.
                            // Skip clone/needs_cleanup entirely — raw 16-byte copy.
                            let frame_no_heap = !unsafe { (*frame).has_heap_slots };
                            if frame_no_heap && opline.op1_type != OpType::Const {
                                let retval_ptr = unsafe {
                                    (frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                                };
                                // CV ref check: even in scalar frame, CV could be a ref.
                                // But for Fast return path, function has no by-ref params
                                // and no globals, so refs are rare. Check anyway for safety.
                                let src = if opline.op1_type == OpType::Cv {
                                    let cv_val = unsafe { &*retval_ptr };
                                    if cv_val.is_reference() {
                                        unsafe { cv_val.as_ref_ptr() as *const Value }
                                    } else {
                                        retval_ptr
                                    }
                                } else {
                                    retval_ptr
                                };
                                // Caller's target: drop old only if caller has heap slots.
                                let prev = unsafe { (*frame).prev_execute_data };
                                if !prev.is_null() && unsafe { (*prev).has_heap_slots } {
                                    unsafe { std::ptr::drop_in_place(return_target) };
                                }
                                unsafe { Value::raw_copy(src, return_target) };
                            } else {
                                let retval = unsafe {
                                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                                };
                                unsafe { mark_caller_heap_return(frame, retval) };
                                unsafe { slot_set(return_target, retval.clone()) };
                            }
                        }
                    }
                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    // Recursive execute_ex boundary: callee done → return to caller's macro loop
                    if frame == initial_frame {
                        eg.current_execute_data.set(prev);
                        unsafe { cleanup_frame_slots(frame) };
                        eg.vm_stack.pop_call_frame(frame);
                        return Ok(());
                    }
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    // Fast-return functions don't sync globals themselves, but a deeper
                    // callee (via full return) may have left dirty entries that need to
                    // propagate up to the main scope or a function with `global` bindings.
                    if !eg.dirty_globals.is_empty()
                        && (!op_array.main_scope_vars.is_empty() || !op_array.global_vars.is_empty())
                    {
                        let vars_to_check = if !op_array.main_scope_vars.is_empty() {
                            &op_array.main_scope_vars
                        } else {
                            &op_array.global_vars
                        };
                        for (cv_idx, var_name) in vars_to_check {
                            if eg.dirty_globals.contains(var_name) {
                                if let Some(val) = eg.globals.get(var_name) {
                                    let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                                    unsafe { slot_set(cv_ptr, val.clone()) };
                                }
                            }
                        }
                        eg.dirty_globals.clear();
                    }
    
                    continue;
                    } // else: not strict with return type
                }

                // ── Full return path ──
                stats::inc_return_full();
                // Note: don't clear dirty_globals here — deeper callees may have set entries
                // that need to propagate up to the main scope. Clearing happens in the
                // caller's "after return" handler when it actually consumes the dirty set.
                if !op_array.global_vars.is_empty() {
                    for (cv_idx, var_name) in &op_array.global_vars {
                        let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                        let val = unsafe { (*cv_ptr).clone() };
                        globals_set(&mut eg.globals, var_name, val);
                        eg.dirty_globals.insert(var_name.clone());
                    }
                }
                if !op_array.static_vars.is_empty() {
                    let func_name = op_array.name.clone();
                    for (cv_idx, var_name) in &op_array.static_vars {
                        let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                        let val = unsafe { (*cv_ptr).clone() };
                        eg.static_vars.entry(func_name.clone())
                            .or_insert_with(HashMap::new)
                            .insert(var_name.clone(), val);
                    }
                }

                // ── Return type validation ──
                let func_common = unsafe { &*(*frame).func };
                let return_hint = &func_common.sig.return_type_hint;
                if !matches!(return_hint, crate::vm::function::ParamTypeHint::None) {
                    let has_explicit_value = opline.extended_value == 1;
                    match return_hint {
                        crate::vm::function::ParamTypeHint::Void => {
                            if has_explicit_value {
                                // Any explicit `return expr;` in a void function is an error,
                                // including `return null;` (PHP rejects it).
                                // Only bare `return;` (extended_value=0) is allowed.
                                let err = make_error_value("TypeError",
                                    "A void function must not return a value");
                                match throw_in_frame(eg, frame, err) {
                                    ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                    ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                                }
                            }
                            // bare "return;" is OK for void
                        }
                        crate::vm::function::ParamTypeHint::Never => {
                            let err = make_error_value("TypeError",
                                "A never-returning function must not return");
                            match throw_in_frame(eg, frame, err) {
                                ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                            }
                        }
                        hint => {
                            if opline.op1_type != OpType::Unused {
                                let retval = unsafe {
                                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                                };
                                let ret_callee_class = eg.declaring_class_of(unsafe { (*frame).func });
                                if !check_type_hint(retval, hint, eg, op_array.strict_types, ret_callee_class) {
                                    let err = make_error_value("TypeError", &format!(
                                        "Return value must be of type {}, {} returned",
                                        hint.display_name(),
                                        retval.type_name()
                                    ));
                                    match throw_in_frame(eg, frame, err) {
                                        ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                        ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                                    }
                                }
                            }
                        }
                    }
                }

                // Check if we're inside a try region with a finally block
                let current_ip = unsafe {
                    (*frame).opline.offset_from(op_array.instructions.as_ptr()) as u32
                };
                let mut need_finally: Option<u32> = None;
                for entry in &op_array.try_entries {
                    if current_ip >= entry.try_start && current_ip < entry.finally_end
                        && entry.finally_start != 0xFFFFFFFF
                        // Don't re-enter finally if we're already inside it
                        && current_ip < entry.finally_start
                    {
                        need_finally = Some(entry.finally_start);
                        break;
                    }
                }

                if let Some(finally_ip) = need_finally {
                    // Write return value now (so it's available after finally)
                    if opline.op1_type != OpType::Unused {
                        let retval = unsafe {
                            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                        };
                        let return_target = unsafe { (*frame).return_value };
                        if !return_target.is_null() {
                            unsafe { mark_caller_heap_return(frame, retval) };
                            unsafe { slot_set(return_target, retval.clone()) };
                        }
                    }
                    // Jump to finally; after finally ends, the pending return
                    // will be detected by the finally_end check
                    eg.exception = None; // no exception, just deferred return
                    let base_ptr = op_array.instructions.as_ptr();
                    unsafe { (*frame).opline = base_ptr.add(finally_ip as usize) };
                    // Mark that we need to return after finally completes (per-frame)
                    unsafe { (*frame).pending_return_after_finally = true; }
                    continue;
                }

                // If returning from inside a finally block while an exception
                // is pending, the return suppresses the exception (PHP semantics).
                if eg.exception.is_some() {
                    eg.exception = None;
                }

                // Generator return — save return value and mark completed
                if op_array.is_generator {
                    if let Some(gen_ref) = eg.active_generator.take() {
                        let mut gen_data = gen_ref.borrow_mut();
                        if opline.op1_type != OpType::Unused {
                            let retval = unsafe {
                                &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                            };
                            gen_data.return_value = retval.clone();
                        }
                        gen_data.state = crate::vm::generator::GeneratorState::Completed;
                        gen_data.value = Value::null();
                        gen_data.key = Value::null();
                        drop(gen_data);
                        eg.active_generator = Some(gen_ref);
                    }

                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    continue;
                }

                if opline.op1_type != OpType::Unused {
                    let retval = unsafe {
                        &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                    };
                    let return_target = unsafe { (*frame).return_value };
                    if !return_target.is_null() {
                        unsafe { mark_caller_heap_return(frame, retval) };
                        unsafe { slot_set(return_target, retval.clone()) };
                    }
                }

                let prev = unsafe { (*frame).prev_execute_data };
                if prev.is_null() {
                    return Ok(());
                }
                // Recursive execute_ex boundary: callee done → return to caller's macro loop
                if frame == initial_frame {
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    return Ok(());
                }

                eg.current_execute_data.set(prev);
                unsafe { cleanup_frame_slots(frame) };
                eg.vm_stack.pop_call_frame(frame);
                frame = prev;
                op_array = unsafe { (*frame).op_array() };
                // After callee returns, selectively re-read globals that the callee modified.
                // Only update caller CVs for variables the callee wrote back via `global` keyword.
                // This avoids overwriting by-ref modifications to other variables.
                if !eg.dirty_globals.is_empty() {
                    let vars_to_check = if !op_array.main_scope_vars.is_empty() {
                        &op_array.main_scope_vars
                    } else {
                        &op_array.global_vars
                    };
                    for (cv_idx, var_name) in vars_to_check {
                        if eg.dirty_globals.contains(var_name) {
                            if let Some(val) = eg.globals.get(var_name) {
                                let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                                unsafe { slot_set(cv_ptr, val.clone()) };
                            }
                        }
                    }
                    // Clear dirty set once consumed by a scope that tracks globals.
                    // Intermediate frames without main_scope_vars/global_vars leave
                    // dirty_globals intact so changes propagate up to main scope.
                    if !op_array.main_scope_vars.is_empty() || !op_array.global_vars.is_empty() {
                        eg.dirty_globals.clear();
                    }
                }

                continue;
            }

            OpCode::Yield => {
                match op_yield(eg, frame, op_array, opline)? {
                    ColdResult::Return => { return Ok(()); }
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    _ => {}
                }
            }

            OpCode::YieldFrom => {
                match op_yield_from(eg, frame, op_array, opline)? {
                    ColdResult::Return => { return Ok(()); }
                    ColdResult::Continue => { continue; }
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    ColdResult::Done => {}
                }
            }

            OpCode::GeneratorReturn => {
                return Err(VmError::Fatal("GeneratorReturn outside generator context".into()));
            }

            OpCode::Include => {
                if op_include(eg, frame, op_array, opline)? {
                    continue;
                }
                // Refresh op_array — include may have changed frame context.
                op_array = unsafe { (*frame).op_array() };
            }

            OpCode::CloneObj => {
                match op_clone_obj(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::CreateClosure => {
                // op1 = CONST function name, result = TMP(closure value)
                // Resolve function pointer via inline cache (first call does string lookup,
                // subsequent calls use cached pointer).
                let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
                let cached = op_array.cache[ip].func;
                let func_ptr = if !cached.is_null() {
                    cached
                } else {
                    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                    let name = name_val.as_str().unwrap_or_else(|| {
                        panic!("CreateClosure: op1 must be a function name string");
                    });
                    let ptr = eg.find_function(name).unwrap_or_else(|| {
                        panic!("CreateClosure: closure function {} not found", name);
                    });
                    // Cache for next time (closures in loops)
                    unsafe { (*(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache)).func = ptr; }
                    ptr
                };
                let num_captures = opline.extended_value as usize;
                let closure = PhpClosure {
                    func: func_ptr,
                    captures: Vec::with_capacity(num_captures),
                    has_heap_captures: false,
                };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                unsafe { frame_tmp_set(frame, result_ptr, Value::closure(closure)) };
            }

            OpCode::ClosureUseVar => {
                // op1 = TMP(closure), op2 = CV(captured variable)
                let val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let cloned_val = val.clone();
                let closure_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                let closure_val = unsafe { &mut *closure_ptr };
                let php_closure = closure_val.as_closure_mut().expect("ClosureUseVar: op1 must be a closure");
                if cloned_val.needs_cleanup() {
                    php_closure.has_heap_captures = true;
                }
                php_closure.captures.push(cloned_val);
            }

            OpCode::NullSafeCheck => {
                if op_nullsafe_check(eg, frame, op_array, opline)? {
                    continue;
                }
            }

            // All opcodes handled — new opcodes must be added above
        }

        // Advance to next instruction.
        // Use local opline_ptr to avoid redundant memory load of (*frame).opline.
        unsafe { (*frame).opline = opline_ptr.add(1); }
    }
}

#[derive(Clone, Copy)]
enum ArrayKeyRef<'a> {
    Int(i64),
    String(&'a str),
}

/// Normalize an array offset while borrowing string storage from the source
/// `Value`. Read paths can therefore probe `PhpArray` without allocating an
/// owned `ArrayKey` for every access.
fn value_to_array_key_ref(val: &Value) -> Result<ArrayKeyRef<'_>, VmError> {
    match val.value_type() {
        ValueType::Long => Ok(ArrayKeyRef::Int(val.as_long().unwrap())),
        ValueType::String => {
            let value = val.as_str().unwrap();
            match canonical_decimal_array_key(value) {
                Some(value) => Ok(ArrayKeyRef::Int(value)),
                None => Ok(ArrayKeyRef::String(value)),
            }
        }
        ValueType::Null => Ok(ArrayKeyRef::String("")),
        ValueType::True => Ok(ArrayKeyRef::Int(1)),
        ValueType::False => Ok(ArrayKeyRef::Int(0)),
        ValueType::Double => Ok(ArrayKeyRef::Int(val.as_double().unwrap() as i64)),
        other => Err(VmError::Fatal(format!("Illegal offset type {:?}", other))),
    }
}

/// PHP converts only canonical decimal strings that fit in `i64` to integer
/// array keys. Checking the syntax first avoids allocating `i64::to_string()`
/// merely to reject leading zeroes, a plus sign, whitespace, or `-0`.
#[inline]
fn canonical_decimal_array_key(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    let digits = match bytes {
        [b'0'] => return Some(0),
        [b'1'..=b'9', rest @ ..] => rest,
        [b'-', b'1'..=b'9', rest @ ..] => rest,
        _ => return None,
    };
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    value.parse().ok()
}

#[cfg(test)]
mod array_key_normalization_tests {
    use super::canonical_decimal_array_key;

    #[test]
    fn canonical_decimal_keys_match_php_array_rules_without_allocation() {
        for (source, expected) in [
            ("0", Some(0)),
            ("1", Some(1)),
            ("-3", Some(-3)),
            ("9223372036854775807", Some(i64::MAX)),
            ("-9223372036854775808", Some(i64::MIN)),
            ("", None),
            ("01", None),
            ("-0", None),
            ("+1", None),
            (" 1", None),
            ("1a", None),
            ("9223372036854775808", None),
        ] {
            assert_eq!(canonical_decimal_array_key(source), expected, "{source}");
        }
    }
}

/// Convert a Value to an ArrayKey.
fn value_to_array_key(val: &Value) -> Result<ArrayKey, VmError> {
    match value_to_array_key_ref(val)? {
        ArrayKeyRef::Int(value) => Ok(ArrayKey::Int(value)),
        ArrayKeyRef::String(value) => Ok(ArrayKey::String(value.to_string())),
    }
}

/// PHP === comparison: same type and same value (recursive for arrays).
fn values_identical(a: &Value, b: &Value) -> bool {
    if a.value_type() != b.value_type() {
        return false;
    }
    match a.value_type() {
        ValueType::Undef | ValueType::Null => true,
        ValueType::True | ValueType::False => true,
        ValueType::Long => a.as_long() == b.as_long(),
        ValueType::Double => a.as_double() == b.as_double(),
        ValueType::String => a.as_str() == b.as_str(),
        ValueType::Array => {
            let arr_a = a.as_array().unwrap();
            let arr_b = b.as_array().unwrap();
            if arr_a.len() != arr_b.len() {
                return false;
            }
            // Same keys in same order, each value ===
            for ((ka, va), (kb, vb)) in arr_a.iter().zip(arr_b.iter()) {
                if ka != kb || !values_identical(va, vb) {
                    return false;
                }
            }
            true
        }
        ValueType::Object => {
            // Objects are identical if they are the same instance (same Rc pointer)
            let rc_a = a.as_object_rc().unwrap();
            let rc_b = b.as_object_rc().unwrap();
            std::rc::Rc::ptr_eq(&rc_a, &rc_b)
        }
        _ => false,
    }
}

pub(super) fn handle_interrupt(eg: &ExecutorGlobals) -> Result<(), VmError> {
    eg.vm_interrupt.store(false, Ordering::Relaxed);

    if eg.timed_out.load(Ordering::Relaxed) {
        eg.timed_out.store(false, Ordering::Relaxed);
        return Err(VmError::Fatal("Maximum execution time exceeded".into()));
    }

    Ok(())
}
