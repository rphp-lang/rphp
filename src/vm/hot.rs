//! Hot executor — specialized interpreter for hot functions.
//!
//! Called from DoFcall when a function's `hot_status == Hot`.
//! Handles the opcode subset used by scalar-recursive and object/method patterns.
//! Returns to baseline interpreter on any unhandled opcode or non-scalar value.
//!
//! # Scope (v1, closed 2026-03)
//!
//! ## In-scope — handled fully
//! - **Scalar recursion**: fib, ack, gcd, power, mutual recursion
//! - **Scalar call chains**: non-recursive functions called from loops
//! - **Closures**: scalar-only closure calls
//! - **Method calls**: `$this->method()` via InitMethodCall (monomorphic IC)
//! - **Property access**: FetchObjR + AssignObjProp (scalar property values, IC cache hit)
//! - **Static methods**: via existing InitStaticCall → DoFcall path
//!
//! ## Out-of-scope — deliberate bailout to baseline
//! - **Heap return values**: `return $this`, `return $string` → HeapReturnValue bail
//! - **Object arguments**: object as SendVal argument → NonScalarOperand bail
//! - **Magic methods**: `__get`/`__set` → ObjCacheMiss bail (no IC entry)
//! - **Dynamic dispatch**: polymorphic call sites, `__call` → bail
//! - **Internal functions**: stdlib calls from hot → IneligibleCallee bail
//!
//! ## Architecture boundary
//! The remaining gaps (HeapReturnValue, NonScalarOperand) require heap/object
//! ownership semantics (refcount, aliasing, lifecycle). This is a new domain,
//! not a continuation of scalar-safe expansion. Opening it requires explicit
//! design decision, not incremental opcode addition.
//!
//! # Design principles
//!
//! - `#[inline(never)]` to keep baseline dispatch loop's icache footprint small
//! - Minimal match arms — only opcodes proven hot by profiling
//! - Bailout on anything unexpected — correctness over coverage
//! - Recursive for DoFcall — mirrors the call structure naturally
//!
//! # Hot contract
//!
//! Single source of truth: [`FunctionCommon::can_promote_to_hot()`].
//! A function is promoted to `HotStatus::Hot` only if:
//! - User function (not internal)
//! - A compact scalar call strategy
//! - `ReturnStrategy::Fast` (no globals/statics/try-finally/generator sync)
//! - Only scalar parameter hints supported by the compact boundary
//!
//! Once `Hot`, these properties are **invariant** — the hot executor's DoFcall
//! relies on them without re-checking (except caller-dependent globals guard).
//!
//! # Slot discipline
//!
//! Every slot (CV or TMP) within a hot frame is in one of three states:
//!
//! | State | Meaning | Drop needed? |
//! |-------|---------|--------------|
//! | `ZeroInit` | Freshly pushed frame, zeroed memory | No |
//! | `Scalar` | Written by hot executor (Long, Double, Bool, Null) | No |
//! | `MaybeHeap` | Written by baseline (String, Array, Object, Ref) | Yes |
//!
//! ## Write invariant
//!
//! All hot executor write paths produce `Scalar`:
//! - **SendVal**: bails if source `needs_cleanup() ∨ is_reference()` → writes scalar
//! - **AssignCv**: bails if source or destination `needs_cleanup() ∨ is_reference()` → writes scalar
//! - **Arithmetic** (Sub/Add): bails on non-integer → produces Long or Double
//! - **Return** (to caller): bails if value `needs_cleanup() ∨ is_reference()` → writes scalar
//!
//! ## Read safety
//!
//! All hot executor reads handle `MaybeHeap` gracefully:
//! - **CV reads**: dereference via `as_ref_ptr()` if reference, then bail on non-integer
//! - **TMP reads**: bail on non-integer (via `as_long()` returning None)
//!
//! ## Overwrite safety
//!
//! - **AssignCv**: bails if destination `needs_cleanup() ∨ is_reference()` (baseline drops)
//! - **Return** write to caller: drops old value if caller has `has_heap_slots`
//!
//! ## How MaybeHeap enters a hot frame
//!
//! Two paths:
//! 1. **Initial entry from baseline**: baseline's SendVal can write heap values
//!    to callee params before DoFcall dispatches to the hot executor.
//! 2. **Method frames ($this)**: InitMethodCall writes an Object value to CV[0].
//!    Frame is marked `has_heap_slots` with bitmap bit 0 set. Return handler
//!    calls `cleanup_frame_slots` to drop $this correctly.
//!
//! Recursive hot calls always have `Scalar` params (hot SendVal bails on heap).
//! For non-method frames: first hot frame may have `MaybeHeap` in parameter CVs,
//! all deeper frames are guaranteed `Scalar`-only.
//! For method frames: CV[0] ($this) is always `MaybeHeap` (Object).

use super::execute::VmError;
use super::frame::{CALL_FRAME_SLOTS, ExecuteData};
use super::function::{CallStrategy, FUNC_HOT_THRESHOLD, FunctionType, HotStatus, UserFunction};
use super::instruction::{
    CALL_FLAG_DEFERRED_SCALAR_CANDIDATE, CALL_FLAG_EXACT_SCALAR_ARGS, Instruction, OpType,
};
use super::opcode::OpCode;
use super::stack;
use super::stats;
use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
struct HotGenericLongContractProof {
    site: *const Instruction,
    object: std::rc::Weak<std::cell::RefCell<crate::value::PhpObject>>,
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
impl HotGenericLongContractProof {
    #[inline(always)]
    fn matches(&self, site: *const Instruction, value: &Value) -> bool {
        let Some(object) = value.as_object_rc() else {
            return false;
        };
        self.site == site && self.object.as_ptr() == std::rc::Rc::as_ptr(&object)
    }

    fn new(site: *const Instruction, value: &Value) -> Option<Self> {
        let object = value.as_object_rc()?;
        Some(Self {
            site,
            object: std::rc::Rc::downgrade(&object),
        })
    }
}

// ── Public types ──────────────────────────────────────────────────────

/// Result of hot executor: either completed successfully or bailed out.
pub enum HotResult {
    /// Frame executed to completion (Return reached). Caller continues normally.
    Completed,
    /// Hit an unhandled opcode or non-scalar value. Frame's opline is set to
    /// the bailout point — caller must fall through to baseline interpreter.
    Bailout,
}

/// Reason for bailing out of the hot executor.
/// Used for diagnostics and coverage analysis.
/// In release builds the reason parameter is optimized away (zero cost).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotBailReason {
    /// Function cache miss in InitFcall
    FuncCacheMiss = 0,
    /// Non-integer operands in comparison or arithmetic
    NonScalarOperand = 1,
    /// Heap value in SendVal Tmp/Var source
    HeapSendVal = 2,
    /// Heap value in SendVal CV source
    HeapSendValCv = 3,
    /// Unsupported SendVal operand type (not Tmp/Var/Cv)
    UnsupportedSendValType = 4,
    /// Callee not User, not Fast/FastScalar, or arity mismatch
    IneligibleCallee = 5,
    /// Caller has globals that need syncing before Fast call
    CallerHasGlobals = 6,
    /// DoFcall result target is not Tmp/Var/Unused
    ComplexResultTarget = 7,
    /// Callee is Cold (not yet promoted)
    ColdCallee = 8,
    /// Heap/ref source in AssignCv
    HeapAssignSrc = 9,
    /// Heap/ref destination in AssignCv (baseline needs to drop old value)
    HeapAssignDst = 10,
    /// Unsupported AssignCv source type (not Tmp/Var/Cv)
    UnsupportedAssignType = 11,
    /// Heap/ref return value
    HeapReturnValue = 12,
    /// Unsupported Return source type
    UnsupportedReturnType = 13,
    /// Opcode not handled by hot executor
    UnsupportedOpcode = 14,
    /// FetchObjR: inline cache miss (class_id mismatch or not cached)
    ObjCacheMiss = 15,
    /// FetchObjR: operand is not an object
    ObjNotObject = 16,
    /// FetchObjR: property value is heap (String/Array/Object/Closure)
    ObjHeapProperty = 17,
    /// FetchObjR: property not found (deleted or dynamic)
    ObjPropertyMissing = 18,
    /// AssignObjProp: inline cache miss
    ObjAssignCacheMiss = 19,
    /// AssignObjProp: source value is heap
    ObjAssignHeapSrc = 20,
}

// ── Debug-only bail counters ──────────────────────────────────────────

#[cfg(debug_assertions)]
mod bail_stats {
    use std::cell::Cell;
    use std::cell::RefCell;
    use std::collections::HashMap;

    const NUM_REASONS: usize = 21;

    thread_local! {
        static COUNTERS: [Cell<u64>; NUM_REASONS] = [const { Cell::new(0) }; NUM_REASONS];
        static HOT_ENTRIES: Cell<u64> = const { Cell::new(0) };
        static HOT_COMPLETED: Cell<u64> = const { Cell::new(0) };
        static OPCODE_BAILS: RefCell<HashMap<u16, u64>> = RefCell::new(HashMap::new());
    }

    pub fn record(reason: super::HotBailReason) {
        COUNTERS.with(|c| {
            let idx = reason as usize;
            if idx < NUM_REASONS {
                c[idx].set(c[idx].get() + 1);
            }
        });
    }

    pub fn record_opcode_bail(opcode: u16) {
        OPCODE_BAILS.with(|m| {
            *m.borrow_mut().entry(opcode).or_insert(0) += 1;
        });
    }

    pub fn record_entry() {
        HOT_ENTRIES.with(|c| c.set(c.get() + 1));
    }

    pub fn record_completed() {
        HOT_COMPLETED.with(|c| c.set(c.get() + 1));
    }

    pub fn snapshot() -> [u64; NUM_REASONS] {
        COUNTERS.with(|c| {
            let mut out = [0u64; NUM_REASONS];
            for (i, cell) in c.iter().enumerate() {
                out[i] = cell.get();
            }
            out
        })
    }

    pub fn opcode_bail_snapshot() -> Vec<(u16, u64)> {
        OPCODE_BAILS.with(|m| {
            let mut v: Vec<_> = m.borrow().iter().map(|(&k, &v)| (k, v)).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v
        })
    }

    pub fn entries() -> u64 {
        HOT_ENTRIES.with(|c| c.get())
    }

    pub fn completed() -> u64 {
        HOT_COMPLETED.with(|c| c.get())
    }
}

/// Dump bail statistics to stderr. Debug builds only.
#[cfg(debug_assertions)]
pub fn dump_bail_stats() {
    use HotBailReason::*;
    const ALL: [(HotBailReason, &str); 21] = [
        (FuncCacheMiss, "FuncCacheMiss"),
        (NonScalarOperand, "NonScalarOperand"),
        (HeapSendVal, "HeapSendVal"),
        (HeapSendValCv, "HeapSendValCv"),
        (UnsupportedSendValType, "UnsupportedSendValType"),
        (IneligibleCallee, "IneligibleCallee"),
        (CallerHasGlobals, "CallerHasGlobals"),
        (ComplexResultTarget, "ComplexResultTarget"),
        (ColdCallee, "ColdCallee"),
        (HeapAssignSrc, "HeapAssignSrc"),
        (HeapAssignDst, "HeapAssignDst"),
        (UnsupportedAssignType, "UnsupportedAssignType"),
        (HeapReturnValue, "HeapReturnValue"),
        (UnsupportedReturnType, "UnsupportedReturnType"),
        (UnsupportedOpcode, "UnsupportedOpcode"),
        (ObjCacheMiss, "ObjCacheMiss"),
        (ObjNotObject, "ObjNotObject"),
        (ObjHeapProperty, "ObjHeapProperty"),
        (ObjPropertyMissing, "ObjPropertyMissing"),
        (ObjAssignCacheMiss, "ObjAssignCacheMiss"),
        (ObjAssignHeapSrc, "ObjAssignHeapSrc"),
    ];
    let entries = bail_stats::entries();
    let completed = bail_stats::completed();
    let counts = bail_stats::snapshot();
    let total_bails: u64 = counts.iter().sum();
    if entries > 0 || total_bails > 0 {
        eprintln!(
            "[hot] coverage: entries={} completed={} bails={} (completion_rate={:.1}%)",
            entries,
            completed,
            total_bails,
            if entries > 0 {
                completed as f64 / entries as f64 * 100.0
            } else {
                0.0
            }
        );
        if total_bails > 0 {
            eprintln!("[hot] bail reasons:");
            for (reason, name) in &ALL {
                let count = counts[*reason as usize];
                if count > 0 {
                    eprintln!(
                        "  {}: {} ({:.1}%)",
                        name,
                        count,
                        count as f64 / total_bails as f64 * 100.0
                    );
                }
            }
        }
        let opcode_bails = bail_stats::opcode_bail_snapshot();
        if !opcode_bails.is_empty() {
            eprintln!("[hot] unsupported opcodes (top bail targets):");
            for (opcode_raw, count) in opcode_bails.iter().take(10) {
                // Safe transmute: OpCode is repr(u8), u16 fits
                let name = if *opcode_raw <= 255 {
                    let oc: OpCode = unsafe { std::mem::transmute(*opcode_raw as u8) };
                    format!("{:?}", oc)
                } else {
                    format!("unknown({})", opcode_raw)
                };
                eprintln!("  {}: {} hits", name, count);
            }
        }
    }
}

// ── Hot executor ──────────────────────────────────────────────────────

/// Execute a single hot frame to completion.
///
/// Preconditions (caller must guarantee):
/// - `frame` is a valid, fully set up ExecuteData (opline set to first instruction)
/// - The function satisfies `can_promote_to_hot()` (User + Fast/FastScalar + Fast ret)
/// - `eg.current_execute_data` is set to `frame`
///
/// On Completed: frame is cleaned up (popped from stack), eg.current_execute_data
/// is restored to caller. Return value is written to frame.return_value.
///
/// On Bailout: frame is still active, opline points to the unhandled instruction.
/// Caller must continue execution via baseline interpreter.
#[inline(never)]
pub fn execute_hot_frame(
    eg: &mut ExecutorGlobals,
    mut frame: *mut ExecuteData,
) -> Result<HotResult, VmError> {
    #[cfg(debug_assertions)]
    bail_stats::record_entry();

    let op_array = unsafe { (*frame).op_array() };
    let mut opline_ptr: *const Instruction = unsafe { (*frame).opline };
    // Hoisted: does this frame's function have globals that need syncing before calls?
    // Constant for the entire invocation (op_array doesn't change within one frame).
    // For fib: always false → Fast DoFcall globals check is a single bool read.
    let caller_has_globals =
        !op_array.main_scope_vars.is_empty() || !op_array.global_vars.is_empty();
    // A weak handle prevents allocation-address reuse from validating a
    // different reified object while keeping repeated calls on one receiver
    // free of metadata and sidecar lookups.
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    let mut generic_long_contract_proof: Option<HotGenericLongContractProof> = None;

    loop {
        let opline = unsafe { &*opline_ptr };

        match opline.opcode {
            // ── Fused comparison + conditional jump ──
            OpCode::JmpZ_Le_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() {
                    unsafe { &*op1_cv.as_ref_ptr() }
                } else {
                    op1_cv
                };
                let op2 = &op_array.literals()[opline.op2 as usize];
                match (op1.as_long(), op2.as_long()) {
                    (Some(l1), Some(l2)) => {
                        if !(l1 <= l2) {
                            opline_ptr = unsafe {
                                op_array.instructions().as_ptr().add(opline.result as usize)
                            };
                            continue;
                        }
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        continue;
                    }
                    _ => return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand),
                }
            }

            OpCode::JmpNZ_Le_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() {
                    unsafe { &*op1_cv.as_ref_ptr() }
                } else {
                    op1_cv
                };
                let op2 = &op_array.literals()[opline.op2 as usize];
                match (op1.as_long(), op2.as_long()) {
                    (Some(l1), Some(l2)) => {
                        if l1 <= l2 {
                            opline_ptr = unsafe {
                                op_array.instructions().as_ptr().add(opline.result as usize)
                            };
                            continue;
                        }
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        continue;
                    }
                    _ => return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand),
                }
            }

            OpCode::JmpZ_Lt_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() {
                    unsafe { &*op1_cv.as_ref_ptr() }
                } else {
                    op1_cv
                };
                let op2 = &op_array.literals()[opline.op2 as usize];
                match (op1.as_long(), op2.as_long()) {
                    (Some(l1), Some(l2)) => {
                        if !(l1 < l2) {
                            opline_ptr = unsafe {
                                op_array.instructions().as_ptr().add(opline.result as usize)
                            };
                            continue;
                        }
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        continue;
                    }
                    _ => return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand),
                }
            }

            OpCode::JmpNZ_Lt_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() {
                    unsafe { &*op1_cv.as_ref_ptr() }
                } else {
                    op1_cv
                };
                let op2 = &op_array.literals()[opline.op2 as usize];
                match (op1.as_long(), op2.as_long()) {
                    (Some(l1), Some(l2)) => {
                        if l1 < l2 {
                            opline_ptr = unsafe {
                                op_array.instructions().as_ptr().add(opline.result as usize)
                            };
                            continue;
                        }
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        continue;
                    }
                    _ => return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand),
                }
            }

            // ── Fused equality + conditional jump ──
            OpCode::JmpZ_Eq_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() {
                    unsafe { &*op1_cv.as_ref_ptr() }
                } else {
                    op1_cv
                };
                let op2 = &op_array.literals()[opline.op2 as usize];
                match (op1.as_long(), op2.as_long()) {
                    (Some(l1), Some(l2)) => {
                        if !(l1 == l2) {
                            opline_ptr = unsafe {
                                op_array.instructions().as_ptr().add(opline.result as usize)
                            };
                            continue;
                        }
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        continue;
                    }
                    _ => return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand),
                }
            }

            OpCode::JmpNZ_Eq_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() {
                    unsafe { &*op1_cv.as_ref_ptr() }
                } else {
                    op1_cv
                };
                let op2 = &op_array.literals()[opline.op2 as usize];
                match (op1.as_long(), op2.as_long()) {
                    (Some(l1), Some(l2)) => {
                        if l1 == l2 {
                            opline_ptr = unsafe {
                                op_array.instructions().as_ptr().add(opline.result as usize)
                            };
                            continue;
                        }
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        continue;
                    }
                    _ => return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand),
                }
            }

            // ── InitFcall with inline Sub_CvConst+SendVal peek-ahead ──
            OpCode::InitFcall => {
                let ip =
                    unsafe { opline_ptr.offset_from(op_array.instructions().as_ptr()) as usize };
                let cached = op_array.cache[ip].func;
                if cached.is_null() {
                    return bailout(frame, opline_ptr, HotBailReason::FuncCacheMiss);
                }
                let func_ptr = cached;
                let num_args = opline.op1 as u32;
                let func_common = unsafe { &*func_ptr };
                let mut scalar_plan_eligible = false;
                if func_common.fn_type == FunctionType::User
                    && num_args == func_common.sig.public_arity()
                {
                    let user = unsafe { &*(func_ptr as *const UserFunction) };
                    scalar_plan_eligible = user.composed_scalar_long_plan.is_some()
                        || user.scalar_double_plan.is_some();
                    if let Some(plan) = user.scalar_long_plan.as_deref() {
                        scalar_plan_eligible = true;
                        let evaluated =
                            if plan.select.is_none() && plan.program.operations.len() == 1 {
                                unsafe {
                                    super::execute::try_execute_direct_single_scalar_long_op(
                                        frame,
                                        op_array,
                                        opline_ptr.add(1),
                                        func_common,
                                        plan,
                                    )
                                }
                            } else {
                                unsafe {
                                    super::execute::try_execute_direct_scalar_long_call(
                                        frame,
                                        op_array,
                                        opline_ptr.add(1),
                                        func_common,
                                        plan,
                                    )
                                }
                            };
                        if let Some((result, do_fcall_ptr)) = evaluated {
                            stats::inc_do_fcall_fast();
                            stats::inc_return_fast();
                            let count = func_common.call_count.get();
                            if count < u32::MAX {
                                func_common.call_count.set(count + 1);
                            }
                            unsafe {
                                super::execute::complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            opline_ptr = unsafe { do_fcall_ptr.add(1) };
                            continue;
                        }
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            super::execute::try_execute_composed_scalar_long_call(
                                eg, frame, op_array, opline_ptr, func_ptr, plan,
                            )
                        } {
                            unsafe {
                                super::execute::complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            opline_ptr = unsafe { do_fcall_ptr.add(1) };
                            continue;
                        }
                    }
                    if let Some(plan) = user.scalar_double_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            super::execute::try_execute_direct_scalar_double_call(
                                frame,
                                op_array,
                                opline_ptr.add(1),
                                func_common,
                                plan,
                            )
                        } {
                            stats::inc_do_fcall_fast();
                            stats::inc_return_fast();
                            let count = func_common.call_count.get();
                            if count < u32::MAX {
                                func_common.call_count.set(count + 1);
                            }
                            unsafe {
                                super::execute::complete_direct_scalar_double_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            opline_ptr = unsafe { do_fcall_ptr.add(1) };
                            continue;
                        }
                    }
                    if let Some(plan) = user.composed_scalar_long_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            super::execute::try_execute_direct_composed_scalar_body_call(
                                eg, frame, op_array, opline_ptr, func_ptr, user, plan,
                            )
                        } {
                            unsafe {
                                super::execute::complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            opline_ptr = unsafe { do_fcall_ptr.add(1) };
                            continue;
                        }
                    }
                }

                let pending_call = unsafe { (*frame).call };
                let deferred =
                    super::execute::should_defer_scalar_call(opline, scalar_plan_eligible);
                let call = if deferred {
                    eg.pending_call_stack.push_deferred_scalar_call(
                        func_ptr,
                        num_args,
                        num_args,
                        frame,
                        pending_call,
                    )
                } else {
                    eg.vm_stack
                        .push_call_frame(func_ptr, num_args, num_args, frame, pending_call)
                };
                unsafe {
                    (*frame).call = call;
                }

                // Peek ahead: InitFcall → Sub_CvConst → SendVal fusion
                let next = unsafe { &*opline_ptr.add(1) };
                if next.opcode == OpCode::Sub_CvConst {
                    let next2 = unsafe { &*opline_ptr.add(2) };
                    if next2.opcode == OpCode::SendVal
                        && next2.op1_type == OpType::Tmp
                        && next2.op1 == next.result
                    {
                        let op1_cv = unsafe { (*frame).cv(next.op1 as u32) };
                        let op1 = if op1_cv.is_reference() {
                            unsafe { &*op1_cv.as_ref_ptr() }
                        } else {
                            op1_cv
                        };
                        let op2 = &op_array.literals()[next.op2 as usize];
                        if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                            let dst = unsafe {
                                (call as *mut Value).add(CALL_FRAME_SLOTS + next2.op2 as usize)
                            };
                            match l1.checked_sub(l2) {
                                Some(diff) => unsafe { Value::write_long(dst, diff) },
                                None => unsafe { dst.write(Value::double(l1 as f64 - l2 as f64)) },
                            }
                            opline_ptr = unsafe { opline_ptr.add(3) };
                            continue;
                        }
                    }
                }
                if next.opcode == OpCode::SendVal
                    && unsafe { super::execute::try_send_scalar_arg(frame, call, op_array, next) }
                {
                    // InitFcall + scalar SendVal fusion.
                    opline_ptr = unsafe { opline_ptr.add(2) };
                    continue;
                }
                // No peek-ahead match — advance normally
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── SendVal (when not fused into InitFcall) ──
            OpCode::SendVal => {
                let call = unsafe { (*frame).call };
                let dst =
                    unsafe { (call as *mut Value).add(CALL_FRAME_SLOTS + opline.op2 as usize) };
                if opline.op1_type == OpType::Tmp || opline.op1_type == OpType::Var {
                    let src = unsafe {
                        (frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                    };
                    let src_val = unsafe { &*src };
                    if !src_val.needs_cleanup() && !src_val.is_reference() {
                        unsafe { Value::raw_copy(src, dst) };
                    } else {
                        return bailout(frame, opline_ptr, HotBailReason::HeapSendVal);
                    }
                } else if opline.op1_type == OpType::Cv {
                    let cv = unsafe { (*frame).cv(opline.op1 as u32) };
                    let val = if cv.is_reference() {
                        unsafe { &*cv.as_ref_ptr() }
                    } else {
                        cv
                    };
                    if !val.needs_cleanup() {
                        unsafe { Value::raw_copy(val as *const Value, dst) };
                    } else {
                        return bailout(frame, opline_ptr, HotBailReason::HeapSendValCv);
                    }
                } else if opline.op1_type == OpType::Const {
                    let val = &op_array.literals()[opline.op1 as usize];
                    if !val.needs_cleanup() {
                        unsafe { Value::raw_copy(val as *const Value, dst) };
                    } else {
                        return bailout(frame, opline_ptr, HotBailReason::HeapSendVal);
                    }
                } else {
                    return bailout(frame, opline_ptr, HotBailReason::UnsupportedSendValType);
                }
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── Sub_CvConst (when not fused into InitFcall peek-ahead) ──
            OpCode::Sub_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() {
                    unsafe { &*op1_cv.as_ref_ptr() }
                } else {
                    op1_cv
                };
                let op2 = &op_array.literals()[opline.op2 as usize];
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    let result_ptr = unsafe {
                        (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                    };
                    match l1.checked_sub(l2) {
                        Some(diff) => unsafe { Value::write_long(result_ptr, diff) },
                        None => unsafe { result_ptr.write(Value::double(l1 as f64 - l2 as f64)) },
                    }
                } else {
                    return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand);
                }
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── Add_TmpTmp with inline Return peek-ahead ──
            OpCode::Add_TmpTmp => {
                let base = frame as *const Value;
                let op1 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op1 as usize) };
                let op2 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op2 as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    if let Some(sum) = l1.checked_add(l2) {
                        // Peek ahead: Add + Return fusion
                        let next = unsafe { &*opline_ptr.add(1) };
                        if next.opcode == OpCode::Return
                            && next.op1_type == OpType::Tmp
                            && next.op1 == opline.result
                        {
                            let return_target = unsafe { (*frame).return_value };
                            if !return_target.is_null() {
                                unsafe {
                                    super::execute::frame_return_set_long(frame, return_target, sum)
                                };
                            }
                            // Cleanup heap slots (e.g. $this) before popping
                            if unsafe { (*frame).has_heap_slots } {
                                unsafe { super::execute::cleanup_frame_slots(frame) };
                            }
                            let prev = unsafe { (*frame).prev_execute_data };
                            eg.current_execute_data.set(prev);
                            eg.vm_stack.pop_call_frame(frame);
                            #[cfg(debug_assertions)]
                            bail_stats::record_completed();
                            return Ok(HotResult::Completed);
                        }
                        // Normal path: write to TMP
                        let result_ptr = unsafe {
                            (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                        };
                        unsafe { Value::write_long(result_ptr, sum) };
                    } else {
                        // Overflow to double
                        let result_ptr = unsafe {
                            (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                        };
                        unsafe { result_ptr.write(Value::double(l1 as f64 + l2 as f64)) };
                    }
                } else {
                    return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand);
                }
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── Declaration/flow-proven Long arithmetic ──
            // The compiler has already proved both representations. Do not
            // repeat reference and Value-tag guards in the hot tier.
            OpCode::Add_LongLong
            | OpCode::Sub_LongLong
            | OpCode::Mul_LongLong
            | OpCode::Mod_LongLong
            | OpCode::BitwiseXor_LongLong => {
                let operand = |op_type: OpType, slot: u16| -> Option<*const Value> {
                    match op_type {
                        OpType::Cv => Some(unsafe { (*frame).cv(slot as u32) as *const Value }),
                        OpType::Tmp | OpType::Var => Some(unsafe {
                            (frame as *const Value).add(CALL_FRAME_SLOTS + slot as usize)
                        }),
                        OpType::Const => Some(&op_array.literals()[slot as usize] as *const Value),
                        OpType::Unused => None,
                    }
                };
                let Some(left_ptr) = operand(opline.op1_type, opline.op1) else {
                    return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand);
                };
                let Some(right_ptr) = operand(opline.op2_type, opline.op2) else {
                    return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand);
                };
                debug_assert_eq!(unsafe { (*left_ptr).value_type() }, ValueType::Long);
                debug_assert_eq!(unsafe { (*right_ptr).value_type() }, ValueType::Long);
                let left = unsafe { (*left_ptr).raw_long() };
                let right = unsafe { (*right_ptr).raw_long() };
                let result_ptr = match opline.result_type {
                    OpType::Tmp | OpType::Var => unsafe {
                        (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                    },
                    OpType::Cv => unsafe { (*frame).cv_mut(opline.result as u32) as *mut Value },
                    _ => return bailout(frame, opline_ptr, HotBailReason::ComplexResultTarget),
                };

                match opline.opcode {
                    OpCode::Add_LongLong => match left.checked_add(right) {
                        Some(value) => unsafe { Value::write_long(result_ptr, value) },
                        None => unsafe {
                            result_ptr.write(Value::double(left as f64 + right as f64))
                        },
                    },
                    OpCode::Sub_LongLong => match left.checked_sub(right) {
                        Some(value) => unsafe { Value::write_long(result_ptr, value) },
                        None => unsafe {
                            result_ptr.write(Value::double(left as f64 - right as f64))
                        },
                    },
                    OpCode::Mul_LongLong => match left.checked_mul(right) {
                        Some(value) => unsafe { Value::write_long(result_ptr, value) },
                        None => unsafe {
                            result_ptr.write(Value::double(left as f64 * right as f64))
                        },
                    },
                    OpCode::Mod_LongLong => {
                        if right == 0 {
                            return Err(VmError::Fatal("Division by zero".into()));
                        }
                        let remainder = left.checked_rem(right).unwrap_or(0);
                        unsafe { Value::write_long(result_ptr, remainder) };
                    }
                    OpCode::BitwiseXor_LongLong => unsafe {
                        Value::write_long(result_ptr, left ^ right)
                    },
                    _ => unreachable!(),
                }
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── General arithmetic — integer-only fast path ──
            // Handles any operand type combo (Cv, Tmp, Var, Const).
            // Bails on non-integer. Covers Add, Sub, Mul, Div, Mod.
            // Div: exact PHP semantics — divisible → long, else → double. Div-by-zero → fatal.
            // Mod: integer-only (PHP semantics). Div-by-zero → fatal.
            OpCode::Add
            | OpCode::Add_CvTmp
            | OpCode::Sub_TmpTmp
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod => {
                let op1_val = match opline.op1_type {
                    OpType::Cv => {
                        let cv = unsafe { (*frame).cv(opline.op1 as u32) };
                        if cv.is_reference() {
                            unsafe { &*cv.as_ref_ptr() }
                        } else {
                            cv
                        }
                    }
                    OpType::Tmp | OpType::Var => unsafe {
                        &*(frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                    },
                    OpType::Const => &op_array.literals()[opline.op1 as usize],
                    _ => return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand),
                };
                let op2_val = match opline.op2_type {
                    OpType::Cv => {
                        let cv = unsafe { (*frame).cv(opline.op2 as u32) };
                        if cv.is_reference() {
                            unsafe { &*cv.as_ref_ptr() }
                        } else {
                            cv
                        }
                    }
                    OpType::Tmp | OpType::Var => unsafe {
                        &*(frame as *const Value).add(CALL_FRAME_SLOTS + opline.op2 as usize)
                    },
                    OpType::Const => &op_array.literals()[opline.op2 as usize],
                    _ => return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand),
                };
                if let (Some(l1), Some(l2)) = (op1_val.as_long(), op2_val.as_long()) {
                    let result_ptr = unsafe {
                        (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                    };
                    match opline.opcode {
                        OpCode::Add | OpCode::Add_CvTmp => match l1.checked_add(l2) {
                            Some(r) => unsafe { Value::write_long(result_ptr, r) },
                            None => unsafe {
                                result_ptr.write(Value::double(l1 as f64 + l2 as f64))
                            },
                        },
                        OpCode::Sub_TmpTmp => match l1.checked_sub(l2) {
                            Some(r) => unsafe { Value::write_long(result_ptr, r) },
                            None => unsafe {
                                result_ptr.write(Value::double(l1 as f64 - l2 as f64))
                            },
                        },
                        OpCode::Mul => match l1.checked_mul(l2) {
                            Some(r) => unsafe { Value::write_long(result_ptr, r) },
                            None => unsafe {
                                result_ptr.write(Value::double(l1 as f64 * l2 as f64))
                            },
                        },
                        OpCode::Div => {
                            if l2 == 0 {
                                return Err(VmError::Fatal("Division by zero".into()));
                            }
                            if let Some(quotient) = l1.checked_div(l2) {
                                if l1.checked_rem(l2) == Some(0) {
                                    unsafe { Value::write_long(result_ptr, quotient) };
                                } else {
                                    unsafe {
                                        result_ptr.write(Value::double(l1 as f64 / l2 as f64))
                                    };
                                }
                            } else {
                                unsafe { result_ptr.write(Value::double(l1 as f64 / l2 as f64)) };
                            }
                        }
                        OpCode::Mod => {
                            if l2 == 0 {
                                return Err(VmError::Fatal("Division by zero".into()));
                            }
                            let remainder = l1.checked_rem(l2).unwrap_or(0);
                            unsafe { Value::write_long(result_ptr, remainder) };
                        }
                        _ => unreachable!(),
                    }
                } else {
                    return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand);
                }
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── DoFcall — recursive hot call or bailout ──
            OpCode::DoFcall => {
                let mut call = unsafe { (*frame).call };
                unsafe { (*frame).call = (*call).call };

                if unsafe { (*call).deferred_scalar_call } {
                    call = unsafe {
                        super::execute::resolve_deferred_scalar_call(
                            eg, frame, call, opline, opline_ptr,
                        )
                    };
                    if call.is_null() {
                        opline_ptr = unsafe { (*frame).opline };
                        continue;
                    }
                }

                let func_common = unsafe { &*(*call).func };

                // Guard: only User functions with FastScalar/Fast call semantics + matching arity.
                // Promotion guard (can_promote_to_hot) guarantees ret==Fast and no typed params
                // for any Hot function, so we don't re-check those here.
                // Arity: use public_arity() (= num_args - this_offset) so methods work —
                // call.num_args excludes $this, sig.num_args includes it.
                if func_common.fn_type != FunctionType::User
                    || !func_common.plan.call.is_compact_user_call()
                    || unsafe { (*call).num_args } != func_common.sig.public_arity()
                {
                    unsafe { (*frame).call = call };
                    return bailout(frame, opline_ptr, HotBailReason::IneligibleCallee);
                }

                if func_common.plan.call != CallStrategy::FastScalar
                    && opline._pad & CALL_FLAG_EXACT_SCALAR_ARGS == 0
                    && !unsafe {
                        super::execute::compact_scalar_call_types_match(
                            eg,
                            call,
                            func_common,
                            op_array.strict_types,
                        )
                    }
                {
                    unsafe { (*frame).call = call };
                    return bailout(frame, opline_ptr, HotBailReason::NonScalarOperand);
                }

                // For Fast: bail if caller has globals to sync (depends on call site, not callee).
                // Uses hoisted bool — single read per DoFcall, always false for fib.
                if func_common.plan.call == CallStrategy::Fast && caller_has_globals {
                    unsafe { (*frame).call = call };
                    return bailout(frame, opline_ptr, HotBailReason::CallerHasGlobals);
                }

                // Hotness tracking — promotion uses can_promote_to_hot() as single source of truth.
                let cc = func_common.call_count.get();
                if cc < u32::MAX {
                    func_common.call_count.set(cc + 1);
                }
                if cc == FUNC_HOT_THRESHOLD && func_common.hot_status.get() == HotStatus::Cold {
                    if func_common.can_promote_to_hot() {
                        func_common.hot_status.set(HotStatus::Hot);
                    }
                }

                let user = unsafe { &*((*call).func as *const UserFunction) };

                // Set up return value target
                let return_value_ptr = match opline.result_type {
                    OpType::Tmp | OpType::Var => unsafe {
                        (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                    },
                    OpType::Unused => std::ptr::null_mut(),
                    _ => {
                        unsafe { (*frame).call = call };
                        return bailout(frame, opline_ptr, HotBailReason::ComplexResultTarget);
                    }
                };
                unsafe {
                    (*call).return_value = return_value_ptr;
                    (*call).opline = user.op_array.instructions.as_ptr();
                    (*frame).opline = opline_ptr.add(1);
                }
                eg.current_execute_data.set(call);

                // Recursive hot execution for hot callees
                if func_common.hot_status.get() == HotStatus::Hot {
                    match execute_hot_frame(eg, call)? {
                        HotResult::Completed => {
                            frame = eg.current_execute_data.get();
                            opline_ptr = unsafe { (*frame).opline };
                            continue;
                        }
                        HotResult::Bailout => {
                            // Propagate bailout — callee frame is active, baseline picks it up.
                            // Don't double-count: the nested call already recorded its reason.
                            return Ok(HotResult::Bailout);
                        }
                    }
                } else {
                    // Cold callee — bail to baseline to handle it.
                    // Frame state is already set up for baseline (callee is current_execute_data).
                    #[cfg(debug_assertions)]
                    bail_stats::record(HotBailReason::ColdCallee);
                    return Ok(HotResult::Bailout);
                }
            }

            // ── Return — scalar fast path ──
            OpCode::Return => {
                if opline.op1_type != OpType::Unused {
                    let retval_ptr = if opline.op1_type == OpType::Cv {
                        let cv_val = unsafe { (*frame).cv(opline.op1 as u32) };
                        if cv_val.is_reference() {
                            unsafe { cv_val.as_ref_ptr() as *const Value }
                        } else {
                            cv_val as *const Value
                        }
                    } else if opline.op1_type == OpType::Tmp || opline.op1_type == OpType::Var {
                        unsafe {
                            (frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                        }
                    } else if opline.op1_type == OpType::Const {
                        &op_array.literals()[opline.op1 as usize] as *const Value
                    } else {
                        return bailout(frame, opline_ptr, HotBailReason::UnsupportedReturnType);
                    };

                    // A typed return must be checked even when the caller
                    // discards its value. Side-exit at the untouched Return so
                    // baseline constructs the canonical TypeError.
                    let common = unsafe { &*(*frame).func };
                    let return_type_proven = super::execute::known_scalar_satisfies_type_hint(
                        opline.known_result_type(),
                        &common.sig.return_type_hint,
                        op_array.strict_types,
                    );
                    if !return_type_proven
                        && super::execute::check_fast_scalar_type_hint(
                            unsafe { &*retval_ptr },
                            &common.sig.return_type_hint,
                            op_array.strict_types,
                        ) != Some(true)
                    {
                        return bailout(frame, opline_ptr, HotBailReason::UnsupportedReturnType);
                    }

                    let return_target = unsafe { (*frame).return_value };
                    if !return_target.is_null() {
                        let src_val = unsafe { &*retval_ptr };
                        if src_val.needs_cleanup() || src_val.is_reference() {
                            return bailout(frame, opline_ptr, HotBailReason::HeapReturnValue);
                        }

                        unsafe {
                            super::execute::frame_return_copy_scalar(
                                frame,
                                return_target,
                                retval_ptr,
                            )
                        };
                    }
                }
                // Cleanup heap slots (e.g. $this in method frames) before popping
                if unsafe { (*frame).has_heap_slots } {
                    unsafe { super::execute::cleanup_frame_slots(frame) };
                }
                // Pop frame
                let prev = unsafe { (*frame).prev_execute_data };
                eg.current_execute_data.set(prev);
                eg.vm_stack.pop_call_frame(frame);
                #[cfg(debug_assertions)]
                bail_stats::record_completed();
                return Ok(HotResult::Completed);
            }

            // ── AssignCv — scalar-only assignment ──
            OpCode::AssignCv => {
                if opline.op1_type != OpType::Cv {
                    return bailout(frame, opline_ptr, HotBailReason::UnsupportedAssignType);
                }
                let src = if opline.op2_type == OpType::Tmp || opline.op2_type == OpType::Var {
                    unsafe { &*(frame as *const Value).add(CALL_FRAME_SLOTS + opline.op2 as usize) }
                } else if opline.op2_type == OpType::Cv {
                    let cv = unsafe { (*frame).cv(opline.op2 as u32) };
                    if cv.is_reference() {
                        unsafe { &*cv.as_ref_ptr() }
                    } else {
                        cv
                    }
                } else {
                    return bailout(frame, opline_ptr, HotBailReason::UnsupportedAssignType);
                };

                if src.needs_cleanup() || src.is_reference() {
                    return bailout(frame, opline_ptr, HotBailReason::HeapAssignSrc);
                }

                let dst = unsafe { (*frame).cv_mut(opline.op1 as u32) as *mut Value };
                // Runtime guard: if destination holds a heap value (e.g. string param
                // forwarded from baseline) or a reference, bail to baseline which handles
                // drop + bookkeeping correctly. For scalar-recursive patterns (fib) this
                // is always false — CVs are scalar or zero-init within the hot path.
                let dst_val = unsafe { &*dst };
                if dst_val.needs_cleanup() || dst_val.is_reference() {
                    return bailout(frame, opline_ptr, HotBailReason::HeapAssignDst);
                }
                unsafe { Value::raw_copy(src as *const Value, dst) };
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── FetchObjR — scalar-safe property read ──
            // Contract: cache hit + public property + scalar value only.
            // Bails on: cache miss, non-object, heap property value, missing property.
            OpCode::FetchObjR => {
                // op1 = CV (object), op2 = Const (property name)
                let obj_val = unsafe { (*frame).cv(opline.op1 as u32) };
                let obj_val = if obj_val.is_reference() {
                    unsafe { &*obj_val.as_ref_ptr() }
                } else {
                    obj_val
                };
                if obj_val.value_type() != ValueType::Object {
                    return bailout(frame, opline_ptr, HotBailReason::ObjNotObject);
                }
                let obj_class_id = unsafe { obj_val.object_class_id_unchecked() };
                let ip =
                    unsafe { opline_ptr.offset_from(op_array.instructions().as_ptr()) as usize };
                let ic = &op_array.cache[ip];
                // Inline cache check: public property (bit 0) + same class
                if ic.property_flags() & 1 == 0 || ic.class_id != obj_class_id || obj_class_id == 0
                {
                    return bailout(frame, opline_ptr, HotBailReason::ObjCacheMiss);
                }
                let prop_ptr =
                    unsafe { obj_val.object_property_slot_unchecked(ic.property_slot()) };
                let prop_val = unsafe { &*prop_ptr };
                // Scalar-only: bail if property value is heap type
                if prop_val.needs_cleanup() || prop_val.is_reference() {
                    return bailout(frame, opline_ptr, HotBailReason::ObjHeapProperty);
                }
                let result_ptr =
                    unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                unsafe { Value::raw_copy(prop_ptr, result_ptr) };
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── AssignObjProp — scalar-safe property write ──
            // Contract: cache hit (read-safe + write-safe) + scalar source value only.
            // Bails on: cache miss, non-object, heap source.
            OpCode::AssignObjProp => {
                // op1 = CV (object), op2 = Const (property name), result = source value
                let obj_val = unsafe { (*frame).cv(opline.op1 as u32) };
                let obj_val = if obj_val.is_reference() {
                    unsafe { &*obj_val.as_ref_ptr() }
                } else {
                    obj_val
                };
                if obj_val.value_type() != ValueType::Object {
                    return bailout(frame, opline_ptr, HotBailReason::ObjNotObject);
                }
                let obj_class_id = unsafe { obj_val.object_class_id_unchecked() };
                let ip =
                    unsafe { opline_ptr.offset_from(op_array.instructions().as_ptr()) as usize };
                let ic = &op_array.cache[ip];
                if ic.class_id != obj_class_id || obj_class_id == 0 {
                    return bailout(frame, opline_ptr, HotBailReason::ObjAssignCacheMiss);
                }
                // Read the source value
                let src_val = match opline.result_type {
                    OpType::Cv => {
                        let cv = unsafe { (*frame).cv(opline.result as u32) };
                        if cv.is_reference() {
                            unsafe { &*cv.as_ref_ptr() }
                        } else {
                            cv
                        }
                    }
                    OpType::Tmp | OpType::Var => unsafe {
                        &*(frame as *const Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                    },
                    OpType::Const => &op_array.literals()[opline.result as usize],
                    _ => return bailout(frame, opline_ptr, HotBailReason::ObjAssignHeapSrc),
                };
                if src_val.needs_cleanup() || src_val.is_reference() {
                    return bailout(frame, opline_ptr, HotBailReason::ObjAssignHeapSrc);
                }
                let typed_definition = if ic.property_flags() == 2 {
                    ic.typed_instance_property_definition()
                } else {
                    None
                };
                let typed_exact = typed_definition.is_some_and(|definition| {
                    if definition.generic_declaration.is_some() {
                        return false;
                    }
                    match ic.typed_instance_property_tag() {
                        crate::vm::instruction::InlineCache::TYPED_PROPERTY_INT => {
                            src_val.value_type() == ValueType::Long
                        }
                        crate::vm::instruction::InlineCache::TYPED_PROPERTY_FLOAT => {
                            matches!(src_val.value_type(), ValueType::Long | ValueType::Double)
                        }
                        crate::vm::instruction::InlineCache::TYPED_PROPERTY_BOOL => {
                            matches!(src_val.value_type(), ValueType::True | ValueType::False)
                        }
                        _ => false,
                    }
                });
                if ic.property_flags() != 3 && !typed_exact {
                    return bailout(frame, opline_ptr, HotBailReason::ObjAssignCacheMiss);
                }
                // Write scalar value directly to the cached declared-property slot.
                let new_val = if ic.property_flags() == 2
                    && ic.typed_instance_property_tag()
                        == crate::vm::instruction::InlineCache::TYPED_PROPERTY_FLOAT
                    && src_val.value_type() == ValueType::Long
                {
                    // SAFETY: the preceding tag guard proves the Long union
                    // field is initialized.
                    Value::double(unsafe { src_val.raw_long() } as f64)
                } else {
                    let mut copied = Value::undef();
                    // SAFETY: heap/reference sources bailed above, so this is a
                    // plain scalar bit-copy into an initialized local Value.
                    unsafe { Value::raw_copy(src_val as *const Value, &mut copied as *mut Value) };
                    copied
                };
                // SAFETY: the class-id/cache guards prove this declared slot;
                // heap sources have bailed. Reference cells must side-exit so
                // baseline assignment preserves their aliases.
                unsafe {
                    let property = obj_val.object_property_slot_unchecked(ic.property_slot());
                    if (*property).is_reference() {
                        return bailout(frame, opline_ptr, HotBailReason::ObjHeapProperty);
                    }
                    obj_val.object_set_property_slot_unchecked(ic.property_slot(), new_val);
                }
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── InitMethodCall — monomorphic inline cache fast path ──
            // Contract: cache hit (func + class_id) + object in CV.
            // Sets up call frame with $this at CV[0]. $this is a heap value — frame
            // is marked has_heap_slots so Return cleanup drops it correctly.
            OpCode::InitMethodCall => {
                // op1 = CV (object), op2 = Const (method name), extended_value = num_args
                let obj_val = unsafe { (*frame).cv(opline.op1 as u32) };
                let obj_val = if obj_val.is_reference() {
                    unsafe { &*obj_val.as_ref_ptr() }
                } else {
                    obj_val
                };
                if obj_val.value_type() != ValueType::Object {
                    return bailout(frame, opline_ptr, HotBailReason::ObjNotObject);
                }
                let obj_class_id = unsafe { obj_val.object_class_id_unchecked() };
                let ip =
                    unsafe { opline_ptr.offset_from(op_array.instructions().as_ptr()) as usize };
                let ic = &op_array.cache[ip];
                if ic.func.is_null() || ic.class_id != obj_class_id || obj_class_id == 0 {
                    return bailout(frame, opline_ptr, HotBailReason::ObjCacheMiss);
                }
                let func_ptr = ic.func;
                let func_common = unsafe { &*func_ptr };
                if !super::execute::method_return_dispatch_contract_matches(opline, func_common) {
                    return bailout(frame, opline_ptr, HotBailReason::ObjCacheMiss);
                }
                let num_args = opline.extended_value;
                let mut scalar_plan_eligible = false;

                #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                if ic.method_has_generic_contract() {
                    let contract_admits_long = if ic.method_has_linked_generic_long_contract() {
                        true
                    } else if generic_long_contract_proof
                        .as_ref()
                        .is_some_and(|proof| proof.matches(opline_ptr, obj_val))
                    {
                        true
                    } else {
                        let method = op_array
                            .literals()
                            .get(opline.op2 as usize)
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let admits = eg
                            .generic_instance_method_contract(obj_val, method)
                            .as_deref()
                            .is_some_and(|contract| contract.admits_exact_long_call(num_args));
                        if admits {
                            generic_long_contract_proof =
                                HotGenericLongContractProof::new(opline_ptr, obj_val);
                        }
                        admits
                    };
                    if !contract_admits_long
                        || func_common.fn_type != FunctionType::User
                        || num_args != func_common.sig.public_arity()
                    {
                        return bailout(frame, opline_ptr, HotBailReason::ObjCacheMiss);
                    }
                    let user = unsafe { &*(func_ptr as *const UserFunction) };
                    let Some(plan) = user.scalar_long_plan.as_deref() else {
                        return bailout(frame, opline_ptr, HotBailReason::ObjCacheMiss);
                    };
                    let evaluated = if plan.select.is_none() && plan.program.operations.len() == 1 {
                        unsafe {
                            super::execute::try_execute_direct_single_scalar_long_op(
                                frame,
                                op_array,
                                opline_ptr.add(1),
                                func_common,
                                plan,
                            )
                        }
                    } else {
                        unsafe {
                            super::execute::try_execute_direct_scalar_long_call(
                                frame,
                                op_array,
                                opline_ptr.add(1),
                                func_common,
                                plan,
                            )
                        }
                    };
                    let Some((result, do_fcall_ptr)) = evaluated else {
                        return bailout(frame, opline_ptr, HotBailReason::ObjCacheMiss);
                    };
                    stats::inc_do_fcall_fast();
                    stats::inc_return_fast();
                    let count = func_common.call_count.get();
                    if count < u32::MAX {
                        func_common.call_count.set(count + 1);
                    }
                    unsafe {
                        super::execute::complete_direct_scalar_long_call(
                            frame,
                            do_fcall_ptr,
                            result,
                        );
                    }
                    opline_ptr = unsafe { do_fcall_ptr.add(1) };
                    continue;
                }

                if func_common.fn_type == FunctionType::User
                    && num_args == func_common.sig.public_arity()
                {
                    let user = unsafe { &*(func_ptr as *const UserFunction) };

                    if ic.method_has_long_property_plan() {
                        if let Some(plan) = user.long_property_plan.as_deref() {
                            if opline._pad & CALL_FLAG_DEFERRED_SCALAR_CANDIDATE != 0
                                && unsafe {
                                    super::execute::try_execute_composed_long_property_call(
                                        frame, op_array, opline_ptr, obj_val, user, plan,
                                    )
                                }
                            {
                                opline_ptr = unsafe { (*frame).opline };
                                continue;
                            }
                            let sends = unsafe { opline_ptr.add(1) };
                            let do_fcall_ptr = unsafe { sends.add(num_args as usize) };
                            let do_fcall = unsafe { &*do_fcall_ptr };
                            if plan.public_args as u32 == num_args
                                && do_fcall.opcode == OpCode::DoFcall
                                && do_fcall.result_type == OpType::Unused
                                && unsafe {
                                    super::execute::try_execute_hot_long_property_method(
                                        frame, op_array, obj_val, sends, plan, user,
                                    )
                                }
                            {
                                stats::inc_do_fcall_fast();
                                stats::inc_return_fast();
                                let count = func_common.call_count.get();
                                if count < u32::MAX {
                                    func_common.call_count.set(count + 1);
                                }
                                unsafe { (*frame).opline = do_fcall_ptr.add(1) };
                                opline_ptr = unsafe { do_fcall_ptr.add(1) };
                                continue;
                            }
                        }
                    }
                    if ic.method_has_property_getter_plan() {
                        if let Some(plan) = user.property_getter_plan.as_ref() {
                            let do_fcall_ptr = unsafe { opline_ptr.add(1) };
                            if unsafe {
                                super::execute::try_execute_hot_property_getter(
                                    frame,
                                    obj_val,
                                    do_fcall_ptr,
                                    user,
                                    plan,
                                )
                            } {
                                opline_ptr = unsafe { do_fcall_ptr.add(1) };
                                continue;
                            }
                        }
                    }

                    if let Some(plan) = user.object_array_plan.as_deref() {
                        if opline._pad & crate::vm::instruction::CALL_FLAG_OBJECT_ARRAY_CONSUMERS
                            != 0
                            && let Some(next_ptr) = unsafe {
                                super::execute::try_execute_direct_object_array_consumers(
                                    eg, frame, op_array, opline_ptr, obj_val, user, plan,
                                )
                            }
                        {
                            super::execute::record_scalar_call(func_common);
                            opline_ptr = next_ptr;
                            continue;
                        }
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            super::execute::try_execute_direct_object_array_call(
                                eg,
                                frame,
                                op_array,
                                obj_val,
                                opline_ptr.add(1),
                                user,
                                plan,
                            )
                        } {
                            super::execute::record_scalar_call(func_common);
                            unsafe {
                                super::execute::complete_direct_object_array_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            opline_ptr = unsafe { do_fcall_ptr.add(1) };
                            continue;
                        }
                    }

                    if let Some(plan) = user.object_long_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            super::execute::try_execute_direct_object_long_call(
                                eg,
                                frame,
                                op_array,
                                obj_val,
                                opline_ptr.add(1),
                                user,
                                plan,
                            )
                        } {
                            super::execute::record_scalar_call(func_common);
                            unsafe {
                                super::execute::complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            opline_ptr = unsafe { do_fcall_ptr.add(1) };
                            continue;
                        }
                    }

                    scalar_plan_eligible = scalar_plan_eligible
                        || user.composed_scalar_long_plan.is_some()
                        || user.long_property_plan.is_some()
                        || user.scalar_double_plan.is_some();
                    if let Some(plan) = user.scalar_long_plan.as_deref() {
                        scalar_plan_eligible = true;
                        let evaluated =
                            if plan.select.is_none() && plan.program.operations.len() == 1 {
                                unsafe {
                                    super::execute::try_execute_direct_single_scalar_long_op(
                                        frame,
                                        op_array,
                                        opline_ptr.add(1),
                                        func_common,
                                        plan,
                                    )
                                }
                            } else {
                                unsafe {
                                    super::execute::try_execute_direct_scalar_long_call(
                                        frame,
                                        op_array,
                                        opline_ptr.add(1),
                                        func_common,
                                        plan,
                                    )
                                }
                            };
                        if let Some((result, do_fcall_ptr)) = evaluated {
                            stats::inc_do_fcall_fast();
                            stats::inc_return_fast();
                            let count = func_common.call_count.get();
                            if count < u32::MAX {
                                func_common.call_count.set(count + 1);
                            }
                            unsafe {
                                super::execute::complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            opline_ptr = unsafe { do_fcall_ptr.add(1) };
                            continue;
                        }
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            super::execute::try_execute_composed_scalar_long_call(
                                eg, frame, op_array, opline_ptr, func_ptr, plan,
                            )
                        } {
                            unsafe {
                                super::execute::complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            opline_ptr = unsafe { do_fcall_ptr.add(1) };
                            continue;
                        }
                    }
                    if let Some(plan) = user.scalar_double_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            super::execute::try_execute_direct_scalar_double_call(
                                frame,
                                op_array,
                                opline_ptr.add(1),
                                func_common,
                                plan,
                            )
                        } {
                            stats::inc_do_fcall_fast();
                            stats::inc_return_fast();
                            let count = func_common.call_count.get();
                            if count < u32::MAX {
                                func_common.call_count.set(count + 1);
                            }
                            unsafe {
                                super::execute::complete_direct_scalar_double_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            opline_ptr = unsafe { do_fcall_ptr.add(1) };
                            continue;
                        }
                    }
                    if let Some(plan) = user.composed_scalar_long_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            super::execute::try_execute_direct_composed_scalar_body_call(
                                eg, frame, op_array, opline_ptr, func_ptr, user, plan,
                            )
                        } {
                            unsafe {
                                super::execute::complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            opline_ptr = unsafe { do_fcall_ptr.add(1) };
                            continue;
                        }
                    }
                }

                let pending_call = unsafe { (*frame).call };
                let deferred =
                    super::execute::should_defer_scalar_call(opline, scalar_plan_eligible);
                let call = if deferred {
                    eg.pending_call_stack.push_deferred_scalar_call(
                        func_ptr,
                        num_args + 1,
                        num_args,
                        frame,
                        pending_call,
                    )
                } else {
                    eg.vm_stack.push_call_frame(
                        func_ptr,
                        num_args + 1,
                        num_args,
                        frame,
                        pending_call,
                    )
                };
                unsafe {
                    (*frame).call = call;
                    let this_ptr = (call as *mut Value).add(CALL_FRAME_SLOTS);
                    if func_common.plan.borrow_this() {
                        // The caller owns the object for the complete nested
                        // call; do not add this borrowed slot to cleanup.
                        Value::raw_copy(obj_val as *const Value, this_ptr);
                    } else {
                        this_ptr.write(obj_val.clone());
                        (*call).has_heap_slots = true;
                        let total = (*call).num_cvs + (*call).num_temps;
                        if total <= 64 {
                            (*call).heap_bitmap |= 1u64;
                        }
                    }
                }

                // Bind the contiguous scalar argument prefix. A nested
                // expression stops the scan and is handled normally.
                let mut next = unsafe { opline_ptr.add(1) };
                let end = unsafe { next.add(num_args as usize) };
                while next < end {
                    let send = unsafe { &*next };
                    if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                        || !unsafe {
                            super::execute::try_send_scalar_method_arg(frame, call, op_array, send)
                        }
                    {
                        break;
                    }
                    next = unsafe { next.add(1) };
                }
                opline_ptr = next;
                continue;
            }

            // ── Any unhandled opcode → bail to baseline ──
            _ => {
                #[cfg(debug_assertions)]
                bail_stats::record_opcode_bail(opline.opcode as u16);
                return bailout(frame, opline_ptr, HotBailReason::UnsupportedOpcode);
            }
        }
    }
}

/// Execute a general long comparison outside the main hot dispatch body.
///
/// Keeping the operand decoding here prevents less common comparison shapes
/// from displacing the recursive call/return path in `execute_hot_frame`.
#[inline(never)]
fn execute_hot_long_comparison(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline_ptr: *const Instruction,
) -> Option<*const Instruction> {
    let opline = unsafe { &*opline_ptr };
    let op1 = match opline.op1_type {
        OpType::Cv => {
            let cv = unsafe { (*frame).cv(opline.op1 as u32) };
            if cv.is_reference() {
                unsafe { &*cv.as_ref_ptr() }
            } else {
                cv
            }
        }
        OpType::Tmp | OpType::Var => unsafe {
            &*(frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
        },
        OpType::Const => &op_array.literals()[opline.op1 as usize],
        _ => return None,
    };
    let op2 = match opline.op2_type {
        OpType::Cv => {
            let cv = unsafe { (*frame).cv(opline.op2 as u32) };
            if cv.is_reference() {
                unsafe { &*cv.as_ref_ptr() }
            } else {
                cv
            }
        }
        OpType::Tmp | OpType::Var => unsafe {
            &*(frame as *const Value).add(CALL_FRAME_SLOTS + opline.op2 as usize)
        },
        OpType::Const => &op_array.literals()[opline.op2 as usize],
        _ => return None,
    };
    let (l1, l2) = (op1.as_long()?, op2.as_long()?);
    let result = match opline.opcode {
        OpCode::IsEqual => l1 == l2,
        OpCode::IsNotEqual => l1 != l2,
        OpCode::IsSmaller => l1 < l2,
        OpCode::IsSmallerOrEqual => l1 <= l2,
        _ => return None,
    };

    // Comparison results are normally consumed immediately by a conditional
    // jump. Fuse that pair and avoid materializing the temporary boolean.
    let ip = unsafe { opline_ptr.offset_from(op_array.instructions().as_ptr()) as usize };
    if let Some(next) = op_array.instructions().get(ip + 1)
        && next.op1_type == OpType::Tmp
        && next.op1 == opline.result
        && matches!(next.opcode, OpCode::JmpZ | OpCode::JmpNZ)
    {
        if (next.opcode == OpCode::JmpZ) == result {
            return Some(unsafe { opline_ptr.add(2) });
        }
        return Some(unsafe { op_array.instructions().as_ptr().add(next.op2 as usize) });
    }

    if !matches!(opline.result_type, OpType::Tmp | OpType::Var) {
        return None;
    }
    let result_ptr =
        unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
    unsafe { Value::write_bool(result_ptr, result) };
    Some(unsafe { opline_ptr.add(1) })
}

/// Continue the same hot frame after a scalar comparison bailout.
///
/// The normal completion path calls `execute_hot_frame` directly and never
/// enters this function.  Keeping recovery out of the primary executor makes
/// comparison support zero-overhead for recursive call-heavy workloads.
#[inline(never)]
pub fn resume_after_long_comparison(
    eg: &mut ExecutorGlobals,
    root_frame: *mut ExecuteData,
) -> Result<HotResult, VmError> {
    loop {
        // A nested callee may have caused the bailout. Baseline must retain
        // ownership of that transition; this recovery only resumes the frame
        // whose dispatch originally entered the hot executor.
        if eg.current_execute_data.get() != root_frame {
            return Ok(HotResult::Bailout);
        }
        let op_array = unsafe { (*root_frame).op_array() };
        let opline_ptr = unsafe { (*root_frame).opline };
        if !matches!(
            unsafe { (*opline_ptr).opcode },
            OpCode::IsEqual | OpCode::IsNotEqual | OpCode::IsSmaller | OpCode::IsSmallerOrEqual
        ) {
            return Ok(HotResult::Bailout);
        }
        let Some(next) = execute_hot_long_comparison(root_frame, op_array, opline_ptr) else {
            return Ok(HotResult::Bailout);
        };
        unsafe { (*root_frame).opline = next };

        match execute_hot_frame(eg, root_frame)? {
            HotResult::Completed => return Ok(HotResult::Completed),
            HotResult::Bailout => continue,
        }
    }
}

// ── Bailout helper ────────────────────────────────────────────────────

/// Set frame's opline to current position, record bail reason, and return Bailout.
#[inline(always)]
fn bailout(
    frame: *mut ExecuteData,
    opline_ptr: *const Instruction,
    _reason: HotBailReason,
) -> Result<HotResult, VmError> {
    #[cfg(debug_assertions)]
    bail_stats::record(_reason);
    unsafe { (*frame).opline = opline_ptr };
    Ok(HotResult::Bailout)
}
