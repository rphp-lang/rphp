use std::collections::HashMap;
use std::sync::atomic::Ordering;
#[cfg(feature = "vm-stats")]
use std::sync::OnceLock;

use crate::value::{Value, PhpArray, PhpClosure, PhpObject, ArrayKey, ValueType, make_error_value};
use crate::runtime::ExecutorGlobals;
use crate::parser::Visibility;
use crate::vm::stats;
#[cfg(all(
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
use crate::jit::{
    NativeConditionalLongLoopCondition, NativeConditionalLongLoopConfig,
    NativeLongAccumulateState, NativeStraightLongConditionOperand,
    NativeStraightLongLoopConfig, NativeStraightLongLoopOutcome,
    NativeStraightLongOperation,
    NATIVE_QUICK_LONG_MAX_CALL_TARGETS, NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES,
    NATIVE_STRAIGHT_LONG_MAX_OPERATIONS,
    QuickLongAccumulateJitOutcome,
    ScalarLongJitDispatch,
};
use super::opcode::OpCode;
use super::instruction::{
    Instruction, KnownScalarType, OpType, ARRAY_INIT_HASH_HINT,
    CALL_FLAG_DEFERRED_SCALAR_CANDIDATE, CALL_FLAG_EXACT_SCALAR_ARGS,
    CALL_FLAG_OBJECT_ARRAY_CONSUMERS,
    NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE,
};
use super::frame::{ExecuteData, HeapSlotIter, CALL_FRAME_SLOTS};
use super::function::{FunctionCommon, FunctionType, UserFunction, CallStrategy, ReturnStrategy, ParamTypeHint, HotStatus, FUNC_HOT_THRESHOLD, LongPlanSource, LongPropertyMethodPlan, LongPropertyOp, PropertyGetterMethodPlan, PropertyInitMethodPlan, BinaryLongRecursionPlan, LongRecursiveBase, LongRecursiveCombine, LongRecursiveCondition, ComposedScalarLongFunctionPlan, ComposedScalarLongOp, ComposedTypedLongFunctionPlan, ComposedTypedLongOp, ObjectArrayFunctionPlan, ObjectArrayLongCall, ObjectArrayLongOp, ObjectArraySource, ObjectLongFunctionPlan, ObjectLongObjectSource, ObjectLongOp, ObjectLongSource, ScalarLongCall, ScalarLongCallGuard, ScalarLongConditionKind, ScalarLongConditionOperand, ScalarLongFunctionPlan, ScalarLongOp, ScalarLongOpKind, ScalarLongProgram, ScalarLongSource, ScalarStringFunctionPlan, ScalarStringSource};
use super::quick::{
    compose_quick_scalar_leaf_program, QuickArrayIndex, QuickIncrementKind,
    QuickLongAccumulateLoop, QuickLongBound, QuickLongCondition, QuickLongInductionLoop,
    QuickLongOp, QuickLongOperand, QuickLongOpsLoop, QuickLongTarget, QuickLongTerm,
    QuickObjectLongArgument, QuickObjectLongMethodCall, QuickTypedMethodCall,
    QuickObjectArrayConsumer, QuickStringAppendSource, QuickVirtualValueSource,
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

#[inline(always)]
fn deferred_scalar_calls_enabled() -> bool {
    #[cfg(feature = "vm-stats")]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| {
            std::env::var_os("RPHP_DISABLE_DEFERRED_SCALAR_CALLS").is_none()
        })
    }
    #[cfg(not(feature = "vm-stats"))]
    {
        true
    }
}

#[inline(always)]
fn composed_scalar_calls_enabled() -> bool {
    #[cfg(feature = "vm-stats")]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| {
            std::env::var_os("RPHP_DISABLE_COMPOSED_SCALAR_CALLS").is_none()
        })
    }
    #[cfg(not(feature = "vm-stats"))]
    {
        true
    }
}

#[inline(always)]
fn composed_scalar_bodies_enabled() -> bool {
    #[cfg(feature = "vm-stats")]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| {
            std::env::var_os("RPHP_DISABLE_COMPOSED_SCALAR_BODIES").is_none()
        })
    }
    #[cfg(not(feature = "vm-stats"))]
    {
        true
    }
}

#[inline(always)]
fn direct_property_getters_enabled() -> bool {
    #[cfg(feature = "vm-stats")]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| {
            std::env::var_os("RPHP_DISABLE_DIRECT_PROPERTY_GETTERS").is_none()
        })
    }
    #[cfg(not(feature = "vm-stats"))]
    {
        true
    }
}

#[inline(always)]
fn composed_property_calls_enabled() -> bool {
    #[cfg(feature = "vm-stats")]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| {
            std::env::var_os("RPHP_DISABLE_COMPOSED_PROPERTY_CALLS").is_none()
        })
    }
    #[cfg(not(feature = "vm-stats"))]
    {
        true
    }
}

/// Resolve an operand whose exact, non-reference representation was proven by
/// the compiler. Unlike `get_op_ptr`, a CV does not need a reference-tag test.
#[inline(always)]
unsafe fn proven_scalar_op_ptr(
    frame: *const ExecuteData,
    op_array: &crate::compiler::OpArray,
    operand: u16,
    op_type: OpType,
) -> *const Value {
    match op_type {
        OpType::Const => &op_array.literals()[operand as usize] as *const Value,
        OpType::Cv => (*frame).cv(operand as u32) as *const Value,
        OpType::Tmp | OpType::Var => {
            (frame as *const Value).add(CALL_FRAME_SLOTS + operand as usize)
        }
        OpType::Unused => unreachable!("proven scalar operand cannot be unused"),
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

/// Validate hints supported by the compact scalar call/return protocol.
/// `None` means the hint needs the canonical class/union/callable checker.
#[inline(always)]
pub(crate) fn check_fast_scalar_type_hint(
    value: &Value,
    hint: &ParamTypeHint,
    strict: bool,
) -> Option<bool> {
    Some(match hint {
        ParamTypeHint::None | ParamTypeHint::Mixed => true,
        ParamTypeHint::Int => value.value_type() == ValueType::Long,
        ParamTypeHint::Float => {
            value.value_type() == ValueType::Double
                || (!strict && value.value_type() == ValueType::Long)
        }
        ParamTypeHint::String => value.value_type() == ValueType::String,
        ParamTypeHint::Bool => {
            matches!(value.value_type(), ValueType::True | ValueType::False)
        }
        ParamTypeHint::Array => value.value_type() == ValueType::Array,
        _ => return None,
    })
}

/// Whether a compiler-proven representation makes a scalar return check
/// redundant. Unknown facts never bypass the canonical validator.
#[inline(always)]
pub(crate) fn known_scalar_satisfies_type_hint(
    known: KnownScalarType,
    hint: &ParamTypeHint,
    strict: bool,
) -> bool {
    match hint {
        ParamTypeHint::None | ParamTypeHint::Mixed => true,
        ParamTypeHint::Int => known == KnownScalarType::Long,
        ParamTypeHint::Float => {
            known == KnownScalarType::Double
                || (!strict && known == KnownScalarType::Long)
        }
        ParamTypeHint::String => known == KnownScalarType::String,
        ParamTypeHint::Bool => known == KnownScalarType::Bool,
        ParamTypeHint::Nullable(inner) => {
            known_scalar_satisfies_type_hint(known, inner, strict)
        }
        ParamTypeHint::Union(types) => types
            .iter()
            .any(|member| known_scalar_satisfies_type_hint(known, member, strict)),
        _ => false,
    }
}

#[inline(always)]
fn exact_method_return_matches(
    hint: &ParamTypeHint,
    expected: KnownScalarType,
) -> bool {
    matches!(
        (hint, expected),
        (ParamTypeHint::Int, KnownScalarType::Long)
            | (ParamTypeHint::String, KnownScalarType::String)
            | (ParamTypeHint::Bool, KnownScalarType::Bool)
    )
}

/// Validate the exact return contract attached to a statically typed method
/// call against the method selected by the receiver-class inline cache. This
/// is the single dispatch guard that licenses all downstream scalar rewrites.
#[inline(always)]
pub(crate) fn method_return_dispatch_contract_matches(
    initializer: &Instruction,
    common: &FunctionCommon,
) -> bool {
    let expected = initializer.method_return_guard_type();
    let return_contract_matches = expected == KnownScalarType::Unknown
        || (common.fn_type == FunctionType::User
            && exact_method_return_matches(&common.sig.return_type_hint, expected));
    let argument_contract_matches = !initializer.has_method_long_args_guard()
        || (common.fn_type == FunctionType::User
            && common.sig.ref_args == 0
            && common.sig.public_arity() == initializer.extended_value
            && !common.sig.param_type_hints.is_empty()
            && common
                .sig
                .param_type_hints
                .iter()
                .all(|hint| matches!(hint, ParamTypeHint::Int)));
    return_contract_matches && argument_contract_matches
}

/// Validate the already-bound public arguments for compact user-call ABIs.
/// A failed guard leaves the frame untouched so the canonical call path can
/// report or coerce the value according to normal PHP rules.
#[inline(always)]
pub(crate) unsafe fn compact_scalar_call_types_match(
    eg: &ExecutorGlobals,
    call: *mut ExecuteData,
    common: &FunctionCommon,
    strict: bool,
) -> bool {
    let hints = &common.sig.param_type_hints;
    let check_count = std::cmp::min((*call).num_args as usize, hints.len());
    let mut class_guard = 0u64;
    let mut class_count = 0usize;
    let mut class_guard_cacheable = true;
    for (index, hint) in hints.iter().take(check_count).enumerate() {
        if !matches!(hint, ParamTypeHint::ClassName(_)) {
            continue;
        }
        if class_count == 2 {
            class_guard_cacheable = false;
            break;
        }
        let value = &*(*call).cv(common.sig.param_cv_index(index as u32));
        if value.value_type() != ValueType::Object {
            class_guard_cacheable = false;
            break;
        }
        let class_id = value.object_class_id_unchecked();
        if class_id == 0 {
            class_guard_cacheable = false;
            break;
        }
        class_guard |= (class_id as u64) << (class_count * 32);
        class_count += 1;
    }
    class_guard_cacheable &= class_count != 0;
    debug_assert!(common.fn_type == FunctionType::User);
    let user = &*(common as *const FunctionCommon as *const UserFunction);
    let class_guard_matches = class_guard_cacheable
        && user.compact_class_guard.get() == class_guard;

    for (index, hint) in hints.iter().take(check_count).enumerate() {
        if matches!(hint, ParamTypeHint::None | ParamTypeHint::Mixed) {
            continue;
        }
        let value = &*(*call).cv(common.sig.param_cv_index(index as u32));
        let matches = match check_fast_scalar_type_hint(value, hint, strict) {
            Some(matches) => matches,
            None if matches!(hint, ParamTypeHint::ClassName(_)) => {
                class_guard_matches
                    || check_type_hint(
                        value,
                        hint,
                        eg,
                        strict,
                        eg.declaring_class_of(common as *const FunctionCommon),
                    )
            }
            None => false,
        };
        if !matches {
            return false;
        }
    }
    if class_guard_cacheable && !class_guard_matches {
        user.compact_class_guard.set(class_guard);
    }
    true
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
    if user.borrowable_heap_args & (1u64 << public_param) == 0
        || !(*source).needs_cleanup()
    {
        return false;
    }
    Value::raw_copy(source, destination);
    true
}

/// Turn an unowned borrowed frame slot into an ordinary owned heap slot before
/// exposing the variable itself through a PHP reference. A clone increments
/// the Rc but deliberately does not drop the raw borrowed bits it replaces.
#[inline(always)]
unsafe fn materialize_borrowed_slot(frame: *mut ExecuteData, ptr: *mut Value) {
    let total = (*frame).num_cvs + (*frame).num_temps;
    if total > 64 || !(*ptr).needs_cleanup() {
        return;
    }
    let idx = slot_idx(frame, ptr);
    let bit = 1u64 << idx;
    if (*frame).heap_bitmap & bit == 0 {
        let owned = (*ptr).clone();
        ptr.write(owned);
        (*frame).has_heap_slots = true;
        (*frame).heap_bitmap |= bit;
    }
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
            OpCode::SendVarEx if fast_scalar => {
                try_copy_scalar_arg(frame, call, op_array, send)
            }
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
    if cache.class_id != class_id
        || flags & property.required_flags as u32 != property.required_flags as u32
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
                let Some(updated) = value.checked_add(resolve_long_plan_source(rhs, arguments)) else {
                    return false;
                };
                value = updated;
                written = true;
            }
            LongPropertyOp::Sub { property, rhs } => {
                if property != 0 {
                    return false;
                }
                let Some(updated) = value.checked_sub(resolve_long_plan_source(rhs, arguments)) else {
                    return false;
                };
                value = updated;
                written = true;
            }
            LongPropertyOp::Min { property, candidate } => {
                if property != 0 {
                    return false;
                }
                let candidate = resolve_long_plan_source(candidate, arguments);
                if candidate < value {
                    value = candidate;
                    written = true;
                }
            }
            LongPropertyOp::Max { property, candidate } => {
                if property != 0 {
                    return false;
                }
                let candidate = resolve_long_plan_source(candidate, arguments);
                if candidate > value {
                    value = candidate;
                    written = true;
                }
            }
            LongPropertyOp::Set { property, value: source } => {
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
        if cache.class_id != class_id
            || flags & property.required_flags as u32 != property.required_flags as u32
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
                let Some(value) = target.checked_add(resolve_long_plan_source(rhs, arguments)) else {
                    return false;
                };
                *target = value;
                written |= 1 << property;
            }
            LongPropertyOp::Sub { property, rhs } => {
                let target = &mut property_values[property as usize];
                let Some(value) = target.checked_sub(resolve_long_plan_source(rhs, arguments)) else {
                    return false;
                };
                *target = value;
                written |= 1 << property;
            }
            LongPropertyOp::Min { property, candidate } => {
                let candidate = resolve_long_plan_source(candidate, arguments);
                let target = &mut property_values[property as usize];
                if candidate < *target {
                    *target = candidate;
                    written |= 1 << property;
                }
            }
            LongPropertyOp::Max { property, candidate } => {
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
                let Some(value) = target.checked_add(resolve_long_plan_source(rhs, arguments)) else {
                    return false;
                };
                *target = value;
                written |= 1 << property;
            }
            LongPropertyOp::Sub { property, rhs } => {
                let Some(target) = property_values.get_mut(property as usize) else {
                    return false;
                };
                let Some(value) = target.checked_sub(resolve_long_plan_source(rhs, arguments)) else {
                    return false;
                };
                *target = value;
                written |= 1 << property;
            }
            LongPropertyOp::Min { property, candidate } => {
                let Some(target) = property_values.get_mut(property as usize) else {
                    return false;
                };
                let candidate = resolve_long_plan_source(candidate, arguments);
                if candidate < *target {
                    *target = candidate;
                    written |= 1 << property;
                }
            }
            LongPropertyOp::Max { property, candidate } => {
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
        || !matches!(do_fcall.result_type, OpType::Unused | OpType::Tmp | OpType::Var)
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

    if matches!(do_fcall.result_type, OpType::Tmp | OpType::Var) {
        let property = &*receiver.object_property_slot_unchecked(property_slot);
        let result_ptr = (caller as *mut Value)
            .add(CALL_FRAME_SLOTS + do_fcall.result as usize);
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
            &*(*caller).get_op_ptr(
                inner_init.op1 as u32,
                inner_init.op1_type,
                caller_op_array,
            )
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
    let argument = &*inner_receiver
        .object_property_slot_unchecked(getter_cache.property_slot());
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

#[inline(always)]
fn apply_scalar_long_op(kind: ScalarLongOpKind, lhs: i64, rhs: i64) -> Option<i64> {
    match kind {
        ScalarLongOpKind::Add => lhs.checked_add(rhs),
        ScalarLongOpKind::Subtract => lhs.checked_sub(rhs),
        ScalarLongOpKind::Multiply => lhs.checked_mul(rhs),
        ScalarLongOpKind::IntDivide => lhs.checked_div(rhs),
        ScalarLongOpKind::Modulo => lhs.checked_rem(rhs),
        ScalarLongOpKind::BitwiseXor => Some(lhs ^ rhs),
    }
}

#[inline(always)]
fn apply_scalar_long_condition(
    kind: ScalarLongConditionKind,
    lhs: i64,
    rhs: i64,
) -> bool {
    match kind {
        ScalarLongConditionKind::Equal => lhs == rhs,
        ScalarLongConditionKind::NotEqual => lhs != rhs,
        ScalarLongConditionKind::LessThan => lhs < rhs,
        ScalarLongConditionKind::LessThanOrEqual => lhs <= rhs,
    }
}

#[inline(always)]
fn resolve_scalar_function_source(
    source: ScalarLongSource,
    arguments: &[i64; 8],
    temporaries: &[i64; 8],
) -> Option<i64> {
    match source {
        ScalarLongSource::Input(index) => arguments.get(index as usize).copied(),
        ScalarLongSource::Constant(value) => Some(value),
        ScalarLongSource::Temporary(index) => temporaries.get(index as usize).copied(),
    }
}

#[inline(always)]
fn evaluate_scalar_long_plan(
    plan: &ScalarLongFunctionPlan,
    arguments: &[i64; 8],
) -> Option<i64> {
    if plan.program.operations.len() > 8 || plan.program.output_count != 1 {
        return None;
    }
    #[cfg(all(
        feature = "jit-prototype",
        target_arch = "aarch64",
        target_os = "macos"
    ))]
    match plan.native_jit().dispatch(plan, arguments) {
        ScalarLongJitDispatch::Interpret => {}
        ScalarLongJitDispatch::Value(value) => return Some(value),
        ScalarLongJitDispatch::SideExit => return None,
    }
    let mut temporaries = [0i64; 8];
    let evaluate_operations = |start: usize, end: usize, temporaries: &mut [i64; 8]| {
        for index in start..end {
            let operation = plan.program.operations[index];
            let lhs = resolve_scalar_function_source(operation.lhs, arguments, temporaries)?;
            let rhs = resolve_scalar_function_source(operation.rhs, arguments, temporaries)?;
            temporaries[index] = apply_scalar_long_op(operation.kind, lhs, rhs)?;
        }
        Some(())
    };
    let output = if let Some(select) = plan.select {
        let shared_end = select.shared_operation_count as usize;
        let true_end = shared_end.checked_add(select.when_true_operation_count as usize)?;
        if true_end > plan.program.operations.len() {
            return None;
        }
        evaluate_operations(0, shared_end, &mut temporaries)?;
        let resolve_condition_operand = |operand| match operand {
            ScalarLongConditionOperand::Source(source) => {
                resolve_scalar_function_source(source, arguments, &temporaries)
            }
            ScalarLongConditionOperand::BitwiseAnd { lhs, rhs } => Some(
                resolve_scalar_function_source(lhs, arguments, &temporaries)?
                    & resolve_scalar_function_source(rhs, arguments, &temporaries)?,
            ),
        };
        let lhs = resolve_condition_operand(select.lhs)?;
        let rhs = resolve_condition_operand(select.rhs)?;
        let condition = match select.kind {
            ScalarLongConditionKind::Equal => lhs == rhs,
            ScalarLongConditionKind::NotEqual => lhs != rhs,
            ScalarLongConditionKind::LessThan => lhs < rhs,
            ScalarLongConditionKind::LessThanOrEqual => lhs <= rhs,
        };
        if condition {
            evaluate_operations(shared_end, true_end, &mut temporaries)?;
            select.when_true
        } else {
            evaluate_operations(
                true_end,
                plan.program.operations.len(),
                &mut temporaries,
            )?;
            select.when_false
        }
    } else {
        for (index, operation) in plan.program.operations.iter().copied().enumerate() {
            let lhs = resolve_scalar_function_source(operation.lhs, arguments, &temporaries)?;
            let rhs = resolve_scalar_function_source(operation.rhs, arguments, &temporaries)?;
            temporaries[index] = apply_scalar_long_op(operation.kind, lhs, rhs)?;
        }
        plan.program.outputs[0]
    };
    resolve_scalar_function_source(output, arguments, &temporaries)
}

#[inline(always)]
fn evaluate_scalar_string_plan<'a>(
    plan: &'a ScalarStringFunctionPlan,
    arguments: &[i64; 8],
) -> Option<&'a str> {
    if plan.operations.len() > 8 {
        return None;
    }
    let mut temporaries = [0i64; 8];
    for (index, operation) in plan.operations.iter().copied().enumerate() {
        let lhs = resolve_scalar_function_source(operation.lhs, arguments, &temporaries)?;
        let rhs = resolve_scalar_function_source(operation.rhs, arguments, &temporaries)?;
        temporaries[index] = apply_scalar_long_op(operation.kind, lhs, rhs)?;
    }
    let Some(select) = plan.select else {
        return Some(&plan.when_true);
    };
    let resolve_condition_operand = |operand| match operand {
        ScalarLongConditionOperand::Source(source) => {
            resolve_scalar_function_source(source, arguments, &temporaries)
        }
        ScalarLongConditionOperand::BitwiseAnd { lhs, rhs } => Some(
            resolve_scalar_function_source(lhs, arguments, &temporaries)?
                & resolve_scalar_function_source(rhs, arguments, &temporaries)?,
        ),
    };
    let lhs = resolve_condition_operand(select.lhs)?;
    let rhs = resolve_condition_operand(select.rhs)?;
    let condition = match select.kind {
        ScalarLongConditionKind::Equal => lhs == rhs,
        ScalarLongConditionKind::NotEqual => lhs != rhs,
        ScalarLongConditionKind::LessThan => lhs < rhs,
        ScalarLongConditionKind::LessThanOrEqual => lhs <= rhs,
    };
    Some(if condition {
        &plan.when_true
    } else {
        &plan.when_false
    })
}

#[inline(always)]
pub(crate) fn should_defer_scalar_call(
    initializer: &Instruction,
    scalar_plan_eligible: bool,
) -> bool {
    if !deferred_scalar_calls_enabled()
        || initializer._pad & CALL_FLAG_DEFERRED_SCALAR_CANDIDATE == 0
    {
        return false;
    }
    scalar_plan_eligible
}

/// Evaluate a scalar plan from values already captured in a compact pending
/// activation. This is the non-contiguous counterpart of the direct Send scan:
/// argument expressions have run exactly once, but no body frame exists yet.
#[inline(always)]
pub(crate) unsafe fn try_execute_deferred_scalar_long_call(
    eg: &ExecutorGlobals,
    call: *mut ExecuteData,
) -> Option<i64> {
    let common = &*(*call).func;
    if !(*call).deferred_scalar_call
        || common.fn_type != FunctionType::User
        || !common.supports_scalar_long_plan()
        || (*call).num_args != common.sig.public_arity()
        || (*call).named_args_used
    {
        return None;
    }
    let user = &*((*call).func as *const UserFunction);
    let public_args = user
        .scalar_long_plan
        .as_deref()
        .map(|plan| plan.public_args)
        .or_else(|| {
            user.composed_scalar_long_plan
                .as_deref()
                .map(|plan| plan.public_args)
        })?;
    if public_args as u32 != common.sig.public_arity() {
        return None;
    }

    let mut arguments = [0i64; 8];
    for (index, argument) in arguments
        .iter_mut()
        .enumerate()
        .take(public_args as usize)
    {
        let cv_index = common.sig.param_cv_index(index as u32);
        let value = (*call).cv(cv_index);
        if value.value_type() != ValueType::Long || value.is_reference() {
            return None;
        }
        *argument = value.raw_long();
    }

    if let Some(plan) = user.scalar_long_plan.as_deref() {
        if plan.select.is_none()
            && plan.program.operations.len() == 1
            && plan.program.output_count == 1
            && plan.program.outputs[0] == ScalarLongSource::Temporary(0)
        {
            let operation = plan.program.operations[0];
            let operand = |source| match source {
                ScalarLongSource::Input(index) => Some(arguments[index as usize]),
                ScalarLongSource::Constant(value) => Some(value),
                ScalarLongSource::Temporary(_) => None,
            };
            let lhs = operand(operation.lhs)?;
            let rhs = operand(operation.rhs)?;
            return apply_scalar_long_op(operation.kind, lhs, rhs);
        }
        return evaluate_scalar_long_plan(plan, &arguments);
    }

    if !composed_scalar_bodies_enabled() {
        return None;
    }
    let plan = user.composed_scalar_long_plan.as_deref()?;
    let mut calls = [std::ptr::null(); COMPOSED_SCALAR_MAX_CALLS];
    let mut call_count = 0usize;
    let result = evaluate_composed_scalar_body_plan(
        eg,
        user,
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
    Some(result)
}

/// Consume a deferred method activation after all argument expressions have
/// executed, without expanding it into the callee's complete CV/TMP frame.
#[inline(never)]
unsafe fn try_execute_deferred_object_long_call(
    eg: &ExecutorGlobals,
    call: *mut ExecuteData,
) -> Option<i64> {
    let common = &*(*call).func;
    if !(*call).deferred_scalar_call
        || common.fn_type != FunctionType::User
        || !common.plan.call.is_compact_user_call()
        || common.plan.ret != ReturnStrategy::Fast
        || (*call).num_args != common.sig.public_arity()
        || (*call).named_args_used
    {
        return None;
    }
    let callee = &*((*call).func as *const UserFunction);
    let plan = callee.object_long_plan.as_deref()?;
    if plan.public_args as u32 != common.sig.public_arity() {
        return None;
    }

    let receiver = (*call).cv(0);
    if receiver.value_type() != ValueType::Object || receiver.is_reference() {
        return None;
    }
    let caller = (*call).prev_execute_data;
    if caller.is_null() {
        return None;
    }
    let caller_op_array = (*caller).op_array();
    let declaring_class = eg.declaring_class_of(&callee.common as *const FunctionCommon);
    let mut slots = [const { std::mem::MaybeUninit::<i64>::uninit() }; 64];
    let mut initialized = 0u64;
    let mut object_arguments = [ObjectLongArgument::None; 8];
    let mut string_arguments = [std::ptr::null(); 8];

    for index in 0..plan.public_args as usize {
        let value = (*call).cv(common.sig.param_cv_index(index as u32));
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

    evaluate_object_long_plan(
        receiver,
        &object_arguments,
        &string_arguments,
        &mut slots,
        initialized,
        callee,
        plan,
    )
}

/// Execute a deferred compiler-proven property mutator from arguments already
/// captured in its compact activation.  As with the contiguous variant, all
/// type/cache/arithmetic guards complete before the first property write.
#[inline(always)]
unsafe fn try_execute_deferred_long_property_method(
    call: *mut ExecuteData,
) -> bool {
    let common = &*(*call).func;
    if !(*call).deferred_scalar_call
        || common.fn_type != FunctionType::User
        || !common.supports_scalar_long_plan()
        || (*call).num_args != common.sig.public_arity()
        || (*call).named_args_used
    {
        return false;
    }
    let user = &*((*call).func as *const UserFunction);
    let Some(plan) = user.long_property_plan.as_deref() else {
        return false;
    };
    if plan.public_args as u32 != common.sig.public_arity() {
        return false;
    }

    let receiver = (*call).cv(0);
    if receiver.value_type() != ValueType::Object || receiver.is_reference() {
        return false;
    }
    let mut arguments = [0i64; 8];
    for (index, argument) in arguments
        .iter_mut()
        .enumerate()
        .take(plan.public_args as usize)
    {
        let value = (*call).cv(common.sig.param_cv_index(index as u32));
        if value.value_type() != ValueType::Long || value.is_reference() {
            return false;
        }
        *argument = value.raw_long();
    }

    try_execute_long_property_plan(receiver, &arguments, plan, user)
}

/// Expand an argument-only activation into the canonical function ABI after a
/// scalar type/arithmetic guard fails. Values are moved, not re-evaluated.
#[inline(never)]
pub(crate) unsafe fn materialize_deferred_scalar_call(
    eg: &mut ExecutorGlobals,
    compact: *mut ExecuteData,
) -> *mut ExecuteData {
    debug_assert!((*compact).deferred_scalar_call);
    let storage_num_args = (*compact).num_cvs;
    let full = eg.vm_stack.push_call_frame(
        (*compact).func,
        storage_num_args,
        (*compact).num_args,
        (*compact).prev_execute_data,
        (*compact).call,
    );
    for index in 0..storage_num_args {
        Value::raw_copy((*compact).slot_ptr(index), (*full).slot_ptr(index));
    }
    (*full).has_heap_slots = (*compact).has_heap_slots;
    (*full).named_args_used = (*compact).named_args_used;
    (*full).heap_bitmap = (*compact).heap_bitmap;

    // Ownership moved to the ordinary frame. The compact storage is now just
    // raw bump memory and must not release any captured heap value.
    (*compact).has_heap_slots = false;
    (*compact).heap_bitmap = 0;
    eg.pending_call_stack.pop_call_frame(compact);
    full
}

/// Finish a deferred activation outside the main dispatcher body. A null return
/// means the scalar call completed; a non-null return is the materialized frame
/// that must continue through the canonical DoFcall path.
#[inline(never)]
pub(crate) unsafe fn resolve_deferred_scalar_call(
    eg: &mut ExecutorGlobals,
    caller: *mut ExecuteData,
    compact: *mut ExecuteData,
    do_fcall: &Instruction,
    do_fcall_ptr: *const Instruction,
) -> *mut ExecuteData {
    if do_fcall.result_type == OpType::Unused
        && try_execute_deferred_long_property_method(compact)
    {
        let common = &*(*compact).func;
        record_scalar_call(common);
        (*caller).opline = do_fcall_ptr.add(1);
        if (*compact).has_heap_slots {
            cleanup_frame_slots(compact);
        }
        eg.pending_call_stack.pop_call_frame(compact);
        return std::ptr::null_mut();
    }

    let evaluated = if matches!(
        do_fcall.result_type,
        OpType::Tmp | OpType::Var | OpType::Unused
    ) {
        try_execute_deferred_object_long_call(eg, compact)
            .or_else(|| try_execute_deferred_scalar_long_call(eg, compact))
    } else {
        None
    };
    let Some(result) = evaluated else {
        return materialize_deferred_scalar_call(eg, compact);
    };

    let common = &*(*compact).func;
    record_scalar_call(common);
    complete_direct_scalar_long_call(caller, do_fcall_ptr, result);
    if (*compact).has_heap_slots {
        cleanup_frame_slots(compact);
    }
    eg.pending_call_stack.pop_call_frame(compact);
    std::ptr::null_mut()
}

/// Compact hot-executor specialization for the overwhelmingly common leaf
/// shape `return arg OP arg_or_const`. Keeping this separate from the general
/// planner avoids an out-of-line Rust call per PHP leaf invocation without
/// inlining the larger multi-step evaluator into the baseline dispatcher.
#[inline(always)]
pub(crate) unsafe fn try_execute_direct_single_scalar_long_op(
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    sends: *const Instruction,
    common: &FunctionCommon,
    plan: &ScalarLongFunctionPlan,
) -> Option<(i64, *const Instruction)> {
    if !common.supports_scalar_long_plan()
        || common.sig.public_arity() != plan.public_args as u32
        || plan.select.is_some()
        || plan.program.operations.len() != 1
        || plan.program.output_count != 1
        || plan.program.outputs[0] != ScalarLongSource::Temporary(0)
    {
        return None;
    }

    let mut arguments = [0i64; 8];
    for index in 0..plan.public_args as usize {
        let send = &*sends.add(index);
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || send.op2 as u32 != common.sig.param_cv_index(index as u32)
        {
            return None;
        }
        let value = match send.op1_type {
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                &*(*caller).get_op_ptr(
                    send.op1 as u32,
                    send.op1_type,
                    caller_op_array,
                )
            }
            OpType::Unused => return None,
        };
        if value.value_type() != ValueType::Long {
            return None;
        }
        arguments[index] = value.raw_long();
    }

    let operation = plan.program.operations[0];
    let operand = |source| match source {
        ScalarLongSource::Input(index) => Some(arguments[index as usize]),
        ScalarLongSource::Constant(value) => Some(value),
        ScalarLongSource::Temporary(_) => None,
    };
    let lhs = operand(operation.lhs)?;
    let rhs = operand(operation.rhs)?;
    let result = apply_scalar_long_op(operation.kind, lhs, rhs)?;
    let do_fcall_ptr = sends.add(plan.public_args as usize);
    let do_fcall = &*do_fcall_ptr;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var | OpType::Unused)
    {
        return None;
    }
    Some((result, do_fcall_ptr))
}

/// Borrow a contiguous positional Send sequence and evaluate a pure scalar
/// callee before any ExecuteData frame is allocated. Argument expressions that
/// need their own opcodes simply fail this shape guard and retain the ordinary
/// call protocol.
#[inline(never)]
pub(crate) unsafe fn try_execute_direct_scalar_long_call(
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    sends: *const Instruction,
    common: &FunctionCommon,
    plan: &ScalarLongFunctionPlan,
) -> Option<(i64, *const Instruction)> {
    if !common.supports_scalar_long_plan()
        || common.sig.public_arity() != plan.public_args as u32
    {
        return None;
    }

    let mut arguments = [0i64; 8];
    for (index, argument) in arguments
        .iter_mut()
        .enumerate()
        .take(plan.public_args as usize)
    {
        let send = &*sends.add(index);
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || send.op2 as u32 != common.sig.param_cv_index(index as u32)
        {
            return None;
        }
        let value = match send.op1_type {
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                &*(*caller).get_op_ptr(
                    send.op1 as u32,
                    send.op1_type,
                    caller_op_array,
                )
            }
            OpType::Unused => return None,
        };
        if value.value_type() != ValueType::Long {
            return None;
        }
        *argument = value.raw_long();
    }

    let do_fcall_ptr = sends.add(plan.public_args as usize);
    let do_fcall = &*do_fcall_ptr;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var | OpType::Unused)
    {
        return None;
    }
    let result = evaluate_scalar_long_plan(plan, &arguments)?;
    Some((result, do_fcall_ptr))
}

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
        return input
            .checked_mul(arm.multiplier)?
            .checked_div(arm.divisor);
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
                slots[destination as usize]
                    .write(resolve_object_long_source(source, slots, initialized)?);
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
                OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                    &*(*caller).get_op_ptr(
                        instruction.op1 as u32,
                        instruction.op1_type,
                        caller_op_array,
                    )
                }
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
                OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                    &*(*caller).get_op_ptr(
                        instruction.op1 as u32,
                        instruction.op1_type,
                        caller_op_array,
                    )
                }
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
        || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var | OpType::Unused)
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
        ObjectLongObjectSource::Argument(argument) => {
            match *arguments.get(argument as usize)? {
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
            }
        }
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
        ObjectArraySource::Argument(argument) => {
            match *arguments.get(argument as usize)? {
                ObjectLongArgument::Borrowed(pointer) if !pointer.is_null() => {
                    Some(ObjectArrayResolved::Borrowed(pointer))
                }
                ObjectLongArgument::Virtual(pointer) if !pointer.is_null() => {
                    Some(ObjectArrayResolved::Virtual(pointer))
                }
                _ => None,
            }
        }
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
        ObjectArraySource::Property { object, cache_ip } => object_array_property(
            owner,
            receiver,
            arguments,
            object,
            cache_ip,
        ),
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
    match resolve_object_array_source(
        source,
        owner,
        receiver,
        arguments,
        slots,
        initialized,
    )? {
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

    let declaring_class = eg.declaring_class_of(cache.func);
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
                if !matches!(hint, ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int)
                    || plan.object_argument_mask & bit != 0
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
                    || !virtual_object_matches_hint(
                        &*pointer,
                        hint,
                        eg,
                        declaring_class,
                    )
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
        &*call_receiver,
        &object_arguments,
        &string_arguments,
        &mut callee_slots,
        callee_initialized,
        callee,
        plan,
    )?;
    Some((result, cache.func))
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
        for target in self
            .called
            .iter()
            .copied()
            .take(self.called_count as usize)
        {
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
                let (value, target) = evaluate_object_array_call(
                    eg,
                    owner,
                    receiver,
                    arguments,
                    &slots,
                    initialized,
                    call,
                )?;
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
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => &*(*caller)
                .get_op_ptr(send.op1 as u32, send.op1_type, caller_op_array),
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
        || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var | OpType::Unused)
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
    let (arguments, do_fcall_ptr) = direct_object_array_arguments(
        eg,
        caller,
        caller_op_array,
        receiver,
        sends,
        callee,
        plan,
    )?;
    let evaluated = evaluate_object_array_values(eg, receiver, &arguments, callee, plan)?;
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
fn object_array_value_for_key(
    caller_op_array: &crate::compiler::OpArray,
    key_literal: u16,
    callee: &UserFunction,
    plan: &ObjectArrayFunctionPlan,
    evaluated: &ObjectArrayEvaluated,
) -> Option<i64> {
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
            return evaluated.values.get(index).copied();
        }
    }
    None
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
            && matches!(add.opcode, OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp)
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
    let evaluated = evaluate_object_array_values(eg, receiver, &arguments, callee, plan)?;
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
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => &*(*caller)
                .get_op_ptr(send.op1 as u32, send.op1_type, caller_op_array),
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
        OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => &*(*caller)
            .get_op_ptr(method.op1 as u32, method.op1_type, caller_op_array),
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
            if !virtual_object_matches_hint(
                &virtual_object,
                hint,
                eg,
                method_declaring_class,
            ) {
                return None;
            }
            method_arguments[index] =
                ObjectLongArgument::Virtual(&virtual_object as *const VirtualObject);
            virtual_arguments += 1;
        } else {
            let value = match send.op1_type {
                OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => &*(*caller)
                    .get_op_ptr(send.op1 as u32, send.op1_type, caller_op_array),
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
        let result_ptr = (caller as *mut Value)
            .add(CALL_FRAME_SLOTS + do_fcall.result as usize);
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
        let result_ptr = (caller as *mut Value)
            .add(CALL_FRAME_SLOTS + do_fcall.result as usize);
        frame_tmp_set_long(caller, result_ptr, result);
    }
    (*caller).opline = do_fcall_ptr.add(1);
}

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
    if common.fn_type != FunctionType::User
        || common.sig.public_arity() != argument_count as u32
    {
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
    let (target, user) = guarded_cached_user_call_target(
        op_array,
        guard,
        receiver,
        argument_count,
    )?;
    guarded_scalar_user_target(target, argument_count)?;
    Some((target, user))
}

/// Resolve and guard one IR `CallScalar` against the canonical inline cache.
/// A successful result has the exact scalar ABI and arity required by the IR;
/// every executor backend shares this identity contract.
unsafe fn guarded_typed_call_target(
    eg: &ExecutorGlobals,
    owner: &UserFunction,
    call: &ScalarLongCall,
    object_arguments: &[*const Value; 8],
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
            let receiver_index = (receiver_slot as u32)
                .checked_sub(owner.common.sig.this_offset)? as usize;
            let receiver = *object_arguments.get(receiver_index)?;
            if receiver.is_null() {
                return None;
            }
            Some(&*receiver)
        }
    };
    guarded_cached_user_call_target(
        &owner.op_array,
        call.guard,
        receiver,
        call.arguments.len(),
    )
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
                let lhs = resolve_composed_body_source(
                    operation.lhs,
                    arguments,
                    &temporaries,
                );
                let rhs = resolve_composed_body_source(
                    operation.rhs,
                    arguments,
                    &temporaries,
                );
                apply_scalar_long_op(operation.kind, lhs, rhs)?
            }
            ComposedScalarLongOp::Call(call) => {
                let sources = &call.arguments;
                if sources.len() > 8 || *call_count >= COMPOSED_SCALAR_MAX_CALLS {
                    return None;
                }
                let (target, target_user) = guarded_scalar_call_target(
                    eg,
                    owner,
                    call,
                    object_arguments,
                )?;
                let target_user = &*target_user;
                let mut target_arguments = [0i64; 8];
                for (index, source) in sources.iter().copied().enumerate() {
                    target_arguments[index] = resolve_composed_body_source(
                        source,
                        arguments,
                        &temporaries,
                    );
                }
                let result = if let Some(target_plan) = target_user.scalar_long_plan.as_deref() {
                    evaluate_scalar_long_plan(target_plan, &target_arguments)?
                } else if let Some(target_plan) =
                    target_user.composed_scalar_long_plan.as_deref()
                {
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
    if plan.program.operations.len() > COMPOSED_SCALAR_MAX_OPS
        || plan.program.output_count != 1
    {
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
        let Some((target, target_user)) = guarded_typed_call_target(
            eg,
            owner,
            call,
            object_arguments,
        ) else {
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
    if plan.program.operations.len() > COMPOSED_SCALAR_MAX_OPS
        || plan.program.output_count != 1
    {
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
        let Some((target, target_user)) = guarded_scalar_call_target(
            eg,
            owner,
            call,
            object_arguments,
        ) else {
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
                let lhs = resolve_composed_body_source(
                    operation.lhs,
                    arguments,
                    &temporaries,
                );
                let rhs = resolve_composed_body_source(
                    operation.rhs,
                    arguments,
                    &temporaries,
                );
                apply_scalar_long_op(operation.kind, lhs, rhs)?
            }
            ComposedScalarLongOp::Call(call) => {
                let target_plan = scalar_plans[operation_index];
                if target_plan.is_null() {
                    return None;
                }
                let mut target_arguments = [0i64; 8];
                for (index, source) in call.arguments.iter().copied().enumerate() {
                    target_arguments[index] = resolve_composed_body_source(
                        source,
                        arguments,
                        &temporaries,
                    );
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
                let lhs = resolve_composed_body_source(
                    operation.lhs,
                    arguments,
                    &temporaries,
                );
                let rhs = resolve_composed_body_source(
                    operation.rhs,
                    arguments,
                    &temporaries,
                );
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
                    target_arguments[index] = resolve_composed_body_source(
                        source,
                        arguments,
                        &temporaries,
                    );
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
                    target_arguments[index] = resolve_composed_body_source(
                        source,
                        arguments,
                        &temporaries,
                    );
                }
                string_temporaries[operation_index] = Some(
                    evaluate_scalar_string_plan(&*target_plan, &target_arguments)?.len(),
                );
                0
            }
            ComposedTypedLongOp::StringConcatLiteral { value, literal_len } => {
                string_temporaries[operation_index] = Some(
                    resolve_composed_string_source(
                        *value,
                        string_arguments,
                        &string_temporaries,
                    )?
                        .checked_add(*literal_len as usize)?,
                );
                0
            }
            ComposedTypedLongOp::StringLength(source) => {
                i64::try_from(resolve_composed_string_source(
                    *source,
                    string_arguments,
                    &string_temporaries,
                )?).ok()?
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
            OpCode::Add | OpCode::Add_TmpTmp | OpCode::Add_CvTmp => {
                ScalarLongOpKind::Add
            }
            OpCode::Sub | OpCode::Sub_CvConst | OpCode::Sub_TmpTmp => {
                ScalarLongOpKind::Subtract
            }
            OpCode::Mul => ScalarLongOpKind::Multiply,
            OpCode::Mod | OpCode::Mod_LongLong => ScalarLongOpKind::Modulo,
            OpCode::BitwiseXor | OpCode::BitwiseXor_LongLong => {
                ScalarLongOpKind::BitwiseXor
            }
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
        || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var | OpType::Unused)
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
                OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                    &*(*caller).get_op_ptr(
                        initializer.op1 as u32,
                        initializer.op1_type,
                        caller_op_array,
                    )
                }
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

/// Recursively evaluate a compiler-proven scalar call tree encoded by ordinary
/// Init/Send/DoFcall instructions. Only already-cached direct functions and
/// monomorphic methods participate, so failure is read-only and can restart via
/// the canonical VM protocol.
unsafe fn evaluate_composed_scalar_call(
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
                OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                    &*(*caller).get_op_ptr(
                        instruction.op1 as u32,
                        instruction.op1_type,
                        caller_op_array,
                    )
                }
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
            OpCode::Add | OpCode::Add_TmpTmp | OpCode::Add_CvTmp => {
                Some(ScalarLongOpKind::Add)
            }
            OpCode::Sub | OpCode::Sub_CvConst | OpCode::Sub_TmpTmp => {
                Some(ScalarLongOpKind::Subtract)
            }
            OpCode::Mul => Some(ScalarLongOpKind::Multiply),
            OpCode::Mod | OpCode::Mod_LongLong => Some(ScalarLongOpKind::Modulo),
            OpCode::BitwiseXor | OpCode::BitwiseXor_LongLong => {
                Some(ScalarLongOpKind::BitwiseXor)
            }
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

        let (nested_func, nested_plan) = composed_scalar_callee(
            caller,
            caller_op_array,
            cursor,
        )?;
        let (nested_result, nested_do_fcall) = evaluate_composed_scalar_call(
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
        if !matches!(nested_result_instruction.result_type, OpType::Tmp | OpType::Var) {
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
        || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var | OpType::Unused)
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
    let mut calls = [std::ptr::null(); COMPOSED_SCALAR_MAX_CALLS];
    let mut call_count = 0usize;
    let evaluated = evaluate_composed_scalar_call(
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

#[inline(always)]
unsafe fn pop_call_storage(eg: &mut ExecutorGlobals, call: *mut ExecuteData) {
    if (*call).deferred_scalar_call {
        eg.pending_call_stack.pop_call_frame(call);
    } else {
        eg.vm_stack.pop_call_frame(call);
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
        eg.pending_named_variadic.remove(&(call as usize));
        cleanup_frame_slots(call);
        pop_call_storage(eg, call);
        call = next;
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
                    unsafe {
                        cleanup_pending_calls(eg, frame);
                        cleanup_frame_slots(frame);
                    };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                }
                unsafe { cleanup_pending_calls(eg, search_frame) };
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
                    unsafe {
                        cleanup_pending_calls(eg, frame);
                        cleanup_frame_slots(frame);
                    };
                    eg.vm_stack.pop_call_frame(frame);
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

include!("execute/baseline_entry.rs");
include!("execute/baseline_control_ops.rs");
include!("execute/baseline_object_calls.rs");
include!("execute/baseline_iteration.rs");
include!("execute/baseline_named_args.rs");
include!("execute/baseline_object_values.rs");
include!("execute/baseline_concat.rs");

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

include!("execute/quick_induction_runtime.rs");
include!("execute/quick_scalar_runtime.rs");

include!("execute/quick_object_resolution.rs");

include!("execute/quick_native_accumulate.rs");
include!("execute/quick_accumulate_runtime.rs");

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

include!("execute/quick_array_state.rs");
include!("execute/quick_string_state.rs");
include!("execute/quick_virtual_pipeline.rs");

include!("execute/quick_kernel_model.rs");
include!("execute/quick_array_access.rs");
include!("execute/quick_kernel_plan.rs");
include!("execute/quick_kernel_common.rs");
include!("execute/quick_array_runtime.rs");
include!("execute/quick_conditional_runtime.rs");

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
include!("execute/native_mixed_core.rs");
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
include!("execute/native_mixed_virtual.rs");
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
include!("execute/native_mixed_scalar.rs");
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
include!("execute/native_mixed_typed.rs");

include!("execute/native_mixed_kernel.rs");

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
fn native_quick_long_straight_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<NativeQuickLongStraightKernel> {
    if plan.entry_op != 0
        || plan.ops.len() < 3
        || plan.ops.len() > NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 2
    {
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
        post_value,
        post_result,
        post_condition_lhs,
        post_condition_rhs,
        post_condition_tmp,
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
    let body_end = plan.ops.len() - 1;
    if header_lhs != post_value
        || header_next_target.op_index() != Some(1)
        || body_target.op_index() != Some(1)
        || exit_target != header_false_target
        || post_condition_lhs != header_lhs
        || post_condition_rhs != header_rhs
        || post_condition_tmp != header_condition_tmp
        || post_result == Some(post_value)
    {
        return None;
    }

    let mut operations =
        [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut operation_resume_ips = [0usize; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut trace_guard_operation_indices = [0u8; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut trace_guard_condition_slots = [0u8; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut trace_guard_expected = [false; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut trace_guard_count = 0usize;
    let operation_count = std::cell::Cell::new(0usize);
    let mut has_materialized_arithmetic = false;
    let mut plan_to_native = vec![u8::MAX; plan.ops.len()];
    let mut pending_branches = Vec::new();
    let mut pending_jumps = Vec::new();
    let mut append_operation =
        |operation: NativeStraightLongOperation, resume_ip: usize| -> Option<u8> {
            let index = operation_count.get();
            if index == NATIVE_STRAIGHT_LONG_MAX_OPERATIONS {
                return None;
            }
            operations[index] = operation;
            operation_resume_ips[index] = resume_ip;
            operation_count.set(index + 1);
            Some(index as u8)
        };
    let mut plan_index = 1usize;
    while plan_index < body_end {
        plan_to_native[plan_index] = u8::try_from(operation_count.get()).ok()?;
        let operation = plan.ops[plan_index];
        let next_target = match operation {
            QuickLongOp::BranchUnlessLt {
                lhs,
                rhs,
                false_target,
                next_target,
                resume_ip,
                ..
            } => {
                let target = false_target.op_index()?;
                let native_index = append_operation(
                    NativeStraightLongOperation::BranchUnless {
                        kind: ScalarLongConditionKind::LessThan,
                        lhs: NativeStraightLongConditionOperand::Source(
                            QuickLongOperand::Slot(lhs),
                        ),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        false_target: 0,
                    },
                    resume_ip,
                )?;
                pending_branches.push((native_index, target));
                next_target
            }
            QuickLongOp::BranchUnlessEq {
                lhs,
                rhs,
                false_target,
                next_target,
                resume_ip,
                ..
            } => {
                let target = false_target.op_index()?;
                let native_index = append_operation(
                    NativeStraightLongOperation::BranchUnless {
                        kind: ScalarLongConditionKind::Equal,
                        lhs: NativeStraightLongConditionOperand::Source(
                            QuickLongOperand::Slot(lhs),
                        ),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        false_target: 0,
                    },
                    resume_ip,
                )?;
                pending_branches.push((native_index, target));
                next_target
            }
            QuickLongOp::BranchUnlessLe {
                lhs,
                rhs,
                false_target,
                next_target,
                resume_ip,
                ..
            } => {
                let target = false_target.op_index()?;
                let native_index = append_operation(
                    NativeStraightLongOperation::BranchUnless {
                        kind: ScalarLongConditionKind::LessThanOrEqual,
                        lhs: NativeStraightLongConditionOperand::Source(lhs),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        false_target: 0,
                    },
                    resume_ip,
                )?;
                pending_branches.push((native_index, target));
                next_target
            }
            QuickLongOp::Jump { target } => {
                let native_index = append_operation(
                    NativeStraightLongOperation::Jump { target: 0 },
                    plan.target_ip(target)?,
                )?;
                pending_jumps.push((native_index, target.op_index()?));
                plan_index += 1;
                continue;
            }
            QuickLongOp::ModConst {
                value,
                divisor,
                result,
                next_target,
                resume_ip,
            } if result != post_value => {
                append_operation(
                    NativeStraightLongOperation::Modulo {
                        value: QuickLongOperand::Slot(value),
                        divisor,
                        result,
                    },
                    resume_ip,
                )?;
                next_target
            }
            QuickLongOp::Binary {
                kind,
                lhs,
                rhs,
                result,
                next_target,
                resume_ip,
            } if matches!(
                kind,
                ScalarLongOpKind::Add
                    | ScalarLongOpKind::Subtract
                    | ScalarLongOpKind::Multiply
            ) && result != post_value =>
            {
                append_operation(
                    NativeStraightLongOperation::Binary {
                        kind,
                        lhs,
                        rhs,
                        result,
                    },
                    resume_ip,
                )?;
                next_target
            }
            QuickLongOp::BinaryAssign {
                kind,
                lhs,
                rhs,
                result,
                destination,
                next_target,
                resume_ip,
            } if matches!(
                kind,
                ScalarLongOpKind::Add
                    | ScalarLongOpKind::Subtract
                    | ScalarLongOpKind::Multiply
            ) && result != post_value
                && destination != post_value =>
            {
                append_operation(
                    NativeStraightLongOperation::BinaryAssign {
                        kind,
                        lhs,
                        rhs,
                        result,
                        destination,
                    },
                    resume_ip,
                )?;
                has_materialized_arithmetic = true;
                next_target
            }
            QuickLongOp::Add {
                lhs,
                rhs,
                result,
                next_target,
                resume_ip,
            } if result != post_value => {
                append_operation(
                    NativeStraightLongOperation::Binary {
                        kind: ScalarLongOpKind::Add,
                        lhs: QuickLongOperand::Slot(lhs),
                        rhs: QuickLongOperand::Slot(rhs),
                        result,
                    },
                    resume_ip,
                )?;
                next_target
            }
            QuickLongOp::AddAssign {
                lhs,
                rhs,
                result,
                destination,
                next_target,
                add_resume_ip,
            } if result != post_value && destination != post_value => {
                append_operation(
                    NativeStraightLongOperation::BinaryAssign {
                        kind: ScalarLongOpKind::Add,
                        lhs: QuickLongOperand::Slot(lhs),
                        rhs: QuickLongOperand::Slot(rhs),
                        result,
                        destination,
                    },
                    add_resume_ip,
                )?;
                has_materialized_arithmetic = true;
                next_target
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
            } if first_result != post_value
                && second_result != post_value
                && destination != post_value =>
            {
                append_operation(
                    NativeStraightLongOperation::Binary {
                        kind: ScalarLongOpKind::Add,
                        lhs: QuickLongOperand::Slot(first_lhs),
                        rhs: QuickLongOperand::Slot(first_rhs),
                        result: first_result,
                    },
                    first_resume_ip,
                )?;
                append_operation(
                    NativeStraightLongOperation::BinaryAssign {
                        kind: ScalarLongOpKind::Add,
                        lhs: QuickLongOperand::Slot(second_lhs),
                        rhs: QuickLongOperand::Slot(second_rhs),
                        result: second_result,
                        destination,
                    },
                    second_resume_ip,
                )?;
                has_materialized_arithmetic = true;
                next_target
            }
            QuickLongOp::TraceGuard {
                kind,
                lhs,
                rhs,
                expected,
                condition_tmp: Some(condition_tmp),
                next_target,
                resume_ip,
            } => {
                let operation_index = append_operation(
                    NativeStraightLongOperation::Guard {
                        kind,
                        lhs: NativeStraightLongConditionOperand::Source(lhs),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        expected,
                    },
                    resume_ip,
                )?;
                trace_guard_operation_indices[trace_guard_count] = operation_index;
                trace_guard_condition_slots[trace_guard_count] =
                    u8::try_from(condition_tmp).ok()?;
                trace_guard_expected[trace_guard_count] = expected;
                trace_guard_count += 1;
                next_target
            }
            _ => return None,
        };
        if next_target.op_index() != Some(plan_index + 1) {
            return None;
        }
        plan_index += 1;
    }
    plan_to_native[body_end] = u8::try_from(operation_count.get()).ok()?;
    for (native_index, target_plan) in pending_branches {
        let false_target = *plan_to_native.get(target_plan)?;
        if false_target == u8::MAX {
            return None;
        }
        let NativeStraightLongOperation::BranchUnless { kind, lhs, rhs, .. } =
            operations[native_index as usize]
        else {
            return None;
        };
        operations[native_index as usize] = NativeStraightLongOperation::BranchUnless {
            kind,
            lhs,
            rhs,
            false_target,
        };
    }
    for (native_index, target_plan) in pending_jumps {
        let target = *plan_to_native.get(target_plan)?;
        if target == u8::MAX {
            return None;
        }
        operations[native_index as usize] = NativeStraightLongOperation::Jump { target };
    }
    if !has_materialized_arithmetic {
        return None;
    }

    let operation_count = operation_count.get() as u8;
    let config = NativeStraightLongLoopConfig {
        induction_slot: post_value,
        bound: header_rhs,
        operations,
        operation_count,
        post_result,
    };
    let mut mutable_mask = config.body_output_mask() | (1u64 << post_value);
    if let Some(slot) = post_result {
        mutable_mask |= 1u64 << slot;
    }
    if matches!(header_rhs, QuickLongOperand::Slot(slot) if mutable_mask & (1u64 << slot) != 0) {
        return None;
    }

    let mut mutable_slots = [0u8; NATIVE_QUICK_LONG_SLOT_CAPACITY];
    let mut mutable_slot_count = 0usize;
    while mutable_mask != 0 {
        if mutable_slot_count == mutable_slots.len() {
            return None;
        }
        let slot = mutable_mask.trailing_zeros() as u8;
        mutable_mask &= mutable_mask - 1;
        mutable_slots[mutable_slot_count] = slot;
        mutable_slot_count += 1;
    }

    Some(NativeQuickLongStraightKernel {
        config,
        header_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
        operation_resume_ips,
        trace_guard_operation_indices,
        trace_guard_condition_slots,
        trace_guard_expected,
        trace_guard_count: trace_guard_count as u8,
        mutable_slots,
        mutable_slot_count: mutable_slot_count as u8,
    })
}

#[inline(always)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
fn publish_native_quick_long_trace_guards(
    kernel: &NativeQuickLongStraightKernel,
    slots: &mut [i64; 64],
    dirty_bool_mask: &mut u64,
    before_operation: Option<u8>,
) {
    for index in 0..kernel.trace_guard_count as usize {
        if before_operation.is_some_and(|limit| {
            kernel.trace_guard_operation_indices[index] >= limit
        }) {
            continue;
        }
        let slot = kernel.trace_guard_condition_slots[index] as usize;
        slots[slot] = i64::from(kernel.trace_guard_expected[index]);
        *dirty_bool_mask |= 1u64 << slot;
    }
}

#[inline(never)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
unsafe fn run_native_quick_long_straight_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: &mut [i64; 64],
    kernel: &NativeQuickLongStraightKernel,
) -> Result<Option<QuickLoopOutcome>, VmError> {
    let config = &kernel.config;
    let bound = quick_long_operand(slots, config.bound);
    let cache = plan.native_jit();
    let remaining_range_proven = cache.prove_straight_remaining_range(config, slots);
    let cv_mask = if op_array.num_cvs == 64 {
        u64::MAX
    } else {
        (1u64 << op_array.num_cvs) - 1
    };
    let publication_mask = config.body_output_mask() & cv_mask;
    let program = if remaining_range_proven {
        cache.prepare_range_proven_straight_program(
            config,
            NATIVE_LONG_SAFEPOINT_INTERVAL as u16,
            publication_mask,
        )
    } else {
        cache.prepare_straight_program(config)
    };
    let Some(program) = program else {
        return Ok(None);
    };
    let interrupt_flag = eg.vm_interrupt.as_ptr() as *const bool;
    let body_output_mask = if remaining_range_proven {
        publication_mask
    } else {
        config.body_output_mask()
    };
    let post_result_mask = config.post_result.map_or(0, |slot| 1u64 << slot);
    let mut iterations = 0u64;
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut entered_native = false;

    loop {
        let before_induction = slots[config.induction_slot as usize];
        let mut before_values = [0i64; NATIVE_QUICK_LONG_SLOT_CAPACITY];
        for index in 0..kernel.mutable_slot_count as usize {
            before_values[index] = slots[kernel.mutable_slots[index] as usize];
        }

        let native_result = if remaining_range_proven {
            let Some(result) = cache.dispatch_prepared_proven_straight_remaining(
                program,
                config,
                slots,
                interrupt_flag,
                NATIVE_LONG_SAFEPOINT_INTERVAL as u16,
            ) else {
                return Ok(None);
            };
            result
        } else {
            cache.dispatch_prepared_straight_chunk(
                program,
                slots,
                NATIVE_LONG_SAFEPOINT_INTERVAL,
            )
        };
        let mut result = match native_result {
            Ok(result) => {
                if !entered_native {
                    cache.record_region_entry();
                    entered_native = true;
                }
                result
            }
            Err(_) => {
                for index in 0..kernel.mutable_slot_count as usize {
                    slots[kernel.mutable_slots[index] as usize] = before_values[index];
                }
                if iterations != 0 {
                    publish_native_quick_long_trace_guards(
                        kernel,
                        slots,
                        &mut dirty_bool_mask,
                        None,
                    );
                }
                if let Some(slot) = kernel.header_condition_tmp {
                    slots[slot as usize] = 1;
                    dirty_bool_mask |= 1u64 << slot;
                }
                commit_quick_long_ops_slots(
                    slot_base,
                    slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                let next_ip = plan.target_ip(kernel.body_target).unwrap_unchecked();
                (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
        };

        let induction = slots[config.induction_slot as usize];
        let completed_in_chunk =
            (induction as u64).wrapping_sub(before_induction as u64);
        iterations = iterations.saturating_add(completed_in_chunk);
        if completed_in_chunk != 0 {
            dirty_long_mask |=
                (1u64 << config.induction_slot) | body_output_mask | post_result_mask;
        }

        if result.outcome == NativeStraightLongLoopOutcome::ChunkExhausted
            && induction >= bound
        {
            result.outcome = NativeStraightLongLoopOutcome::Completed;
        }
        let completed = result.outcome == NativeStraightLongLoopOutcome::Completed;
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(!completed);
            dirty_bool_mask |= 1u64 << slot;
        }

        match result.outcome {
            NativeStraightLongLoopOutcome::Completed => {
                if iterations != 0 {
                    publish_native_quick_long_trace_guards(
                        kernel,
                        slots,
                        &mut dirty_bool_mask,
                        None,
                    );
                }
                commit_quick_long_ops_slots(
                    slot_base,
                    slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
                (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
                stats::inc_quick_loop_completed(iterations);
                return Ok(Some(QuickLoopOutcome::Completed));
            }
            NativeStraightLongLoopOutcome::ChunkExhausted => {
                debug_assert_ne!(completed_in_chunk, 0);
                debug_assert_eq!(
                    completed_in_chunk % NATIVE_LONG_SAFEPOINT_INTERVAL,
                    0
                );
                if eg.vm_interrupt.load(Ordering::Relaxed) {
                    publish_native_quick_long_trace_guards(
                        kernel,
                        slots,
                        &mut dirty_bool_mask,
                        None,
                    );
                    commit_quick_long_ops_slots(
                        slot_base,
                        slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    let next_ip = plan.target_ip(kernel.body_target).unwrap_unchecked();
                    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
                    handle_interrupt(eg)?;
                }
            }
            NativeStraightLongLoopOutcome::OperationSideExit => {
                let failed_operation = result
                    .failed_operation
                    .expect("operation side exit carries its operation index");
                dirty_long_mask |=
                    config.output_mask_before(failed_operation) & body_output_mask;
                if iterations != 0 {
                    publish_native_quick_long_trace_guards(
                        kernel,
                        slots,
                        &mut dirty_bool_mask,
                        None,
                    );
                } else {
                    publish_native_quick_long_trace_guards(
                        kernel,
                        slots,
                        &mut dirty_bool_mask,
                        Some(failed_operation),
                    );
                }
                commit_quick_long_ops_slots(
                    slot_base,
                    slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                (*frame).opline = op_array.instructions.as_ptr().add(
                    kernel.operation_resume_ips[failed_operation as usize],
                );
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
            NativeStraightLongLoopOutcome::IncrementOverflow => {
                dirty_long_mask |= body_output_mask;
                publish_native_quick_long_trace_guards(
                    kernel,
                    slots,
                    &mut dirty_bool_mask,
                    None,
                );
                commit_quick_long_ops_slots(
                    slot_base,
                    slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(kernel.post_resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
        }
    }
}

include!("execute/native_mixed_runtime.rs");

include!("execute/quick_dispatch.rs");

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn execute_quick_region_entry(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<bool, VmError> {
    let block_idx = opline.extended_value as usize - 1;
    let Some(super::planner::BlockPlan::QuickLongOps(plan)) =
        op_array.block_plans.get(block_idx)
    else {
        return Ok(false);
    };
    if plan.header_ip
        != (opline as *const Instruction)
            .offset_from(op_array.instructions().as_ptr()) as usize
    {
        return Ok(false);
    }

    let hot_counter = &op_array.block_counters[block_idx];
    let count = hot_counter.get();
    if count == QUICK_LOOP_DISABLED {
        return Ok(false);
    }
    let hot_progress = count % QUICK_LOOP_COUNTER_STRIDE;
    if hot_progress < QUICK_LOOP_HOT_THRESHOLD {
        hot_counter.set(count + 1);
        return Ok(false);
    }

    match run_quick_long_ops_loop(eg, frame, op_array, plan)? {
        QuickLoopOutcome::Completed => {
            hot_counter.set(QUICK_LOOP_HOT_THRESHOLD);
            Ok(true)
        }
        QuickLoopOutcome::Deoptimized => {
            let failures = count / QUICK_LOOP_COUNTER_STRIDE + 1;
            hot_counter.set(if failures >= QUICK_LOOP_FAILURE_LIMIT {
                QUICK_LOOP_DISABLED
            } else {
                failures * QUICK_LOOP_COUNTER_STRIDE
            });
            Ok(true)
        }
        QuickLoopOutcome::GuardFailed => {
            let failures = count / QUICK_LOOP_COUNTER_STRIDE + 1;
            hot_counter.set(if failures >= QUICK_LOOP_FAILURE_LIMIT {
                QUICK_LOOP_DISABLED
            } else {
                failures * QUICK_LOOP_COUNTER_STRIDE
            });
            Ok(false)
        }
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
                    run_quick_long_accumulate_loop(eg, frame, op_array, plan)?
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

// Fuse the cache-hit method protocol while its transition cost is material
// relative to the FastScalar body; longer methods keep the normal DoFcall path.
const FAST_SCALAR_METHOD_FUSION_MAX_OPS: usize = 16;

/// Enter a fixed-signature user method after InitMethodCall already created
/// its frame and bound every scalar argument. Inlining lets the compiler merge
/// the adjacent DoFcall setup into the cache-hit InitMethodCall path.
#[inline(always)]
fn execute_fast_scalar_method_call<'a>(
    eg: &mut ExecutorGlobals,
    caller: *mut ExecuteData,
    call: *mut ExecuteData,
    func_ptr: *const FunctionCommon,
    do_fcall: &Instruction,
    do_fcall_ptr: *const Instruction,
) -> Result<ColdResult<'a>, VmError> {
    unsafe { (*caller).call = (*call).call };
    stats::inc_do_fcall_fast();

    let func_common = unsafe { &*func_ptr };
    let cc = func_common.call_count.get();
    if cc < u32::MAX {
        func_common.call_count.set(cc + 1);
    }
    if cc == FUNC_HOT_THRESHOLD
        && func_common.hot_status.get() == HotStatus::Cold
        && func_common.can_promote_to_hot()
    {
        func_common.hot_status.set(HotStatus::Hot);
    }

    let return_value_ptr = match do_fcall.result_type {
        OpType::Tmp | OpType::Var => unsafe {
            (caller as *mut Value).add(CALL_FRAME_SLOTS + do_fcall.result as usize)
        },
        OpType::Unused => std::ptr::null_mut(),
        _ => unsafe {
            (*caller).get_op_mut(do_fcall.result as u32, do_fcall.result_type)
        },
    };
    let user = unsafe { &*(func_ptr as *const UserFunction) };

    unsafe {
        (*call).return_value = return_value_ptr;
        (*call).opline = user.op_array.instructions.as_ptr();
        (*caller).opline = do_fcall_ptr.add(1);
    }
    eg.current_execute_data.set(call);

    if func_common.hot_status.get() == HotStatus::Hot {
        match super::hot::execute_hot_frame(eg, call)? {
            super::hot::HotResult::Completed => Ok(ColdResult::Continue),
            super::hot::HotResult::Bailout => match super::hot::resume_after_long_comparison(eg, call)? {
                super::hot::HotResult::Completed => Ok(ColdResult::Continue),
                super::hot::HotResult::Bailout => {
                    // Promotion happens only after caches are warm. If both
                    // the hot executor and its comparison resume reject this
                    // frame, keep later calls on the canonical baseline path
                    // instead of paying the same failed tier entry forever.
                    func_common.hot_status.set(HotStatus::Cold);
                    let active = eg.current_execute_data.get();
                    Ok(ColdResult::NewFrame(active, unsafe { (*active).op_array() }))
                }
            },
        }
    } else {
        Ok(ColdResult::NewFrame(call, unsafe { (*call).op_array() }))
    }
}

/// Complete a call that could not use one of the compact DoFcall protocols.
///
/// Argument diagnostics, named variadics, dynamic `__invoke`, generators and
/// internal handlers are intentionally kept out of `execute_ex`. These paths
/// are important for PHP semantics but cold for ordinary fixed-signature user
/// calls, so outlining them keeps the baseline dispatch working set smaller.
#[cold]
#[inline(never)]
fn execute_full_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    opline_ptr: *const Instruction,
    call: *mut ExecuteData,
) -> Result<ColdResult<'a>, VmError> {
    stats::inc_do_fcall_full();

    let return_value_ptr = if opline.result_type != OpType::Unused {
        unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) }
    } else {
        std::ptr::null_mut()
    };
    unsafe { (*call).return_value = return_value_ptr };

    // Extract named variadic args eagerly so no error path can leak them.
    let call_key = call as usize;
    let pending_named = eg.pending_named_variadic.remove(&call_key);

    // SendVal filled CV 0..N-1 for a dynamically resolved invokable object.
    // Make room for the hidden method receiver before validating arguments.
    if let Some(this_val) = eg.pending_invoke_this.take() {
        let num = unsafe { (*call).num_args };
        for i in (0..num).rev() {
            let val = unsafe { (*call).cv(i).clone() };
            let dst = unsafe { (*call).cv_mut(i + 1) };
            unsafe { frame_slot_set(call, dst as *mut Value, val) };
        }
        let this_slot = unsafe { (*call).cv_mut(0) };
        unsafe { frame_slot_set(call, this_slot as *mut Value, this_val) };
    }

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

    // Named arguments can leave holes even when the public count is correct.
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

    let callee_class = eg
        .declaring_class_of(unsafe { (*call).func })
        .map(str::to_string);
    let callee_class_ref = callee_class.as_deref();

    if !func_common.sig.param_type_hints.is_empty() {
        let mut type_error = None;
        for (i, hint) in func_common.sig.param_type_hints.iter().enumerate() {
            if matches!(hint, ParamTypeHint::None) {
                continue;
            }
            if (i as u32) >= num_args {
                break;
            }
            let cv_idx = func_common.sig.param_cv_index(i as u32);
            let val = unsafe { &*(*call).cv(cv_idx) };
            if val.is_undef() {
                continue;
            }
            if !check_type_hint(val, hint, eg, op_array.strict_types, callee_class_ref) {
                type_error = Some(make_error_value(
                    "TypeError",
                    &format!(
                        "Argument #{} must be of type {}, {} given",
                        i + 1,
                        hint.display_name(),
                        val.type_name()
                    ),
                ));
                break;
            }
        }
        if let Some(err) = type_error {
            unsafe { cleanup_frame_slots(call) };
            eg.vm_stack.pop_call_frame(call);
            return Ok(match throw_in_frame(eg, frame, err) {
                ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                ThrowResult::Unhandled(t) => ColdResult::Unhandled(t),
            });
        }
    }

    if func_common.sig.is_variadic {
        let extra_count = num_args.saturating_sub(public_max);
        let mut variadic_arr = PhpArray::new();
        let cv_start = func_common.sig.variadic_cv_index;
        for i in 0..extra_count {
            let arg = unsafe { (*call).cv(cv_start + i) }.clone();
            variadic_arr.push(arg);
        }
        if let Some(named_extras) = pending_named {
            let variadic_hint = func_common
                .sig
                .param_type_hints
                .get(public_max as usize);
            for (name, val) in named_extras {
                if let Some(hint) = variadic_hint {
                    if !matches!(hint, ParamTypeHint::None)
                        && !check_type_hint(
                            &val,
                            hint,
                            eg,
                            op_array.strict_types,
                            callee_class_ref,
                        )
                    {
                        let type_err = make_error_value(
                            "TypeError",
                            &format!(
                                "Named parameter ${} must be of type {}, {} given",
                                name,
                                hint.display_name(),
                                val.type_name()
                            ),
                        );
                        unsafe { cleanup_frame_slots(call) };
                        eg.vm_stack.pop_call_frame(call);
                        return Ok(match throw_in_frame(eg, frame, type_err) {
                            ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                            ThrowResult::Unhandled(t) => ColdResult::Unhandled(t),
                        });
                    }
                }
                variadic_arr.set_str(&name, val);
            }
        }
        let variadic_slot = unsafe { (*call).cv_mut(cv_start) };
        unsafe {
            frame_slot_set(call, variadic_slot as *mut Value, Value::array(variadic_arr));
        }
    }

    match unsafe { (*(*call).func).fn_type } {
        FunctionType::User => {
            let user = unsafe { &*((*call).func as *const UserFunction) };
            if user.op_array.is_generator {
                use crate::vm::generator::{new_generator_ref, Generator};

                let mut args = Vec::with_capacity(user.op_array.num_cvs as usize);
                for i in 0..user.op_array.num_cvs {
                    args.push(unsafe { (*call).cv(i).clone() });
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
                    HashMap::new(),
                );
                gen_obj.generator = Some(gen_ref);
                if !return_value_ptr.is_null() {
                    unsafe { slot_set(return_value_ptr, Value::object(gen_obj)) };
                }
                unsafe { cleanup_frame_slots(call) };
                eg.vm_stack.pop_call_frame(call);
                Ok(ColdResult::Done)
            } else {
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
                Ok(ColdResult::NewFrame(call, unsafe { (*call).op_array() }))
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
            if let Some(exc) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exc) {
                    ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                    ThrowResult::Unhandled(t) => ColdResult::Unhandled(t),
                });
            }
            handler_result?;
            Ok(ColdResult::Done)
        }
        FunctionType::Undef => {
            let err = make_error_value("Error", "Call to undefined function");
            Ok(match throw_in_frame(eg, frame, err) {
                ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                ThrowResult::Unhandled(t) => ColdResult::Unhandled(t),
            })
        }
    }
}

include!("execute/baseline_dispatch_cold.rs");
include!("execute/baseline_dispatch.rs");

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
