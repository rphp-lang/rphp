//! Hot executor — specialized interpreter for scalar-only recursive functions.
//!
//! Called from DoFcall when a function's `hot_status == Hot`.
//! Handles only the opcode subset used by scalar-recursive patterns (fib, etc.).
//! Returns to baseline interpreter on any unhandled opcode or non-scalar value.
//!
//! Design principles:
//! - `#[inline(never)]` to keep baseline dispatch loop's icache footprint small
//! - Minimal match arms — only opcodes proven hot by profiling
//! - Bailout on anything unexpected — correctness over coverage
//! - Recursive for DoFcall — mirrors the call structure naturally

use crate::value::Value;
use crate::runtime::ExecutorGlobals;
use super::execute::VmError;
use super::frame::{ExecuteData, CALL_FRAME_SLOTS};
use super::function::{CallStrategy, ReturnStrategy, FunctionType, UserFunction, ParamTypeHint, HotStatus, FUNC_HOT_THRESHOLD};
use super::instruction::{Instruction, OpType};
use super::opcode::OpCode;
use super::stack;

/// Result of hot executor: either completed successfully or bailed out.
pub enum HotResult {
    /// Frame executed to completion (Return reached). Caller continues normally.
    Completed,
    /// Hit an unhandled opcode or non-scalar value. Frame's opline is set to
    /// the bailout point — caller must fall through to baseline interpreter.
    Bailout,
}

/// Execute a single hot frame to completion.
///
/// Preconditions (caller must guarantee):
/// - `frame` is a valid, fully set up ExecuteData (opline set to first instruction)
/// - The function is User + (FastScalar | Fast) + Hot
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
    let op_array = unsafe { (*frame).op_array() };
    let mut opline_ptr: *const Instruction = unsafe { (*frame).opline };
    // Hoisted: does this frame's function have globals that need syncing before calls?
    // Constant for the entire invocation (op_array doesn't change within one frame).
    // For fib: always false → Fast DoFcall globals check is a single bool read.
    let caller_has_globals = !op_array.main_scope_vars.is_empty() || !op_array.global_vars.is_empty();

    loop {
        let opline = unsafe { &*opline_ptr };

        match opline.opcode {
            // ── Fused comparison + conditional jump ──
            OpCode::JmpZ_Le_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                match (op1.as_long(), op2.as_long()) {
                    (Some(l1), Some(l2)) => {
                        if !(l1 <= l2) {
                            // Jump
                            opline_ptr = unsafe { op_array.instructions().as_ptr().add(opline.result as usize) };
                            continue;
                        }
                        // Fall through: skip the dead JmpZ instruction
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        continue;
                    }
                    _ => return bailout(frame, opline_ptr),
                }
            }

            OpCode::JmpNZ_Le_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                match (op1.as_long(), op2.as_long()) {
                    (Some(l1), Some(l2)) => {
                        if l1 <= l2 {
                            opline_ptr = unsafe { op_array.instructions().as_ptr().add(opline.result as usize) };
                            continue;
                        }
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        continue;
                    }
                    _ => return bailout(frame, opline_ptr),
                }
            }

            OpCode::JmpZ_Lt_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                match (op1.as_long(), op2.as_long()) {
                    (Some(l1), Some(l2)) => {
                        if !(l1 < l2) {
                            opline_ptr = unsafe { op_array.instructions().as_ptr().add(opline.result as usize) };
                            continue;
                        }
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        continue;
                    }
                    _ => return bailout(frame, opline_ptr),
                }
            }

            OpCode::JmpNZ_Lt_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                match (op1.as_long(), op2.as_long()) {
                    (Some(l1), Some(l2)) => {
                        if l1 < l2 {
                            opline_ptr = unsafe { op_array.instructions().as_ptr().add(opline.result as usize) };
                            continue;
                        }
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        continue;
                    }
                    _ => return bailout(frame, opline_ptr),
                }
            }

            // ── InitFcall with inline Sub_CvConst+SendVal peek-ahead ──
            OpCode::InitFcall => {
                let ip = unsafe { opline_ptr.offset_from(op_array.instructions().as_ptr()) as usize };
                let cached = op_array.cache[ip].func;
                if cached.is_null() {
                    // Cache miss — bail to baseline for function resolution
                    return bailout(frame, opline_ptr);
                }
                let func_ptr = cached;
                let num_args = opline.op1 as u32;
                let call = eg.vm_stack.push_call_frame(func_ptr, num_args);
                unsafe {
                    (*call).prev_execute_data = frame;
                    (*call).call = (*frame).call;
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
                            opline_ptr = unsafe { opline_ptr.add(3) };
                            continue;
                        }
                    }
                }
                // No peek-ahead match — advance normally
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── SendVal (when not fused into InitFcall) ──
            OpCode::SendVal => {
                let call = unsafe { (*frame).call };
                let dst = unsafe {
                    (call as *mut Value).add(CALL_FRAME_SLOTS + opline.op2 as usize)
                };
                if opline.op1_type == OpType::Tmp || opline.op1_type == OpType::Var {
                    let src = unsafe {
                        (frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                    };
                    let src_val = unsafe { &*src };
                    if !src_val.needs_cleanup() && !src_val.is_reference() {
                        unsafe { Value::raw_copy(src, dst) };
                    } else {
                        // Heap value — bail to baseline for proper clone + bitmap tracking
                        return bailout(frame, opline_ptr);
                    }
                } else if opline.op1_type == OpType::Cv {
                    let cv = unsafe { (*frame).cv(opline.op1 as u32) };
                    let val = if cv.is_reference() { unsafe { &*cv.as_ref_ptr() } } else { cv };
                    if !val.needs_cleanup() {
                        unsafe { Value::raw_copy(val as *const Value, dst) };
                    } else {
                        return bailout(frame, opline_ptr);
                    }
                } else {
                    return bailout(frame, opline_ptr);
                }
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── Sub_CvConst (when not fused into InitFcall peek-ahead) ──
            OpCode::Sub_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
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
                    return bailout(frame, opline_ptr);
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
                            // Write sum directly to caller's return_value
                            let return_target = unsafe { (*frame).return_value };
                            if !return_target.is_null() {
                                let prev = unsafe { (*frame).prev_execute_data };
                                if !prev.is_null() && unsafe { (*prev).has_heap_slots } {
                                    unsafe { std::ptr::drop_in_place(return_target) };
                                }
                                unsafe { Value::write_long(return_target, sum) };
                            }
                            // Pop frame and return Completed
                            let prev = unsafe { (*frame).prev_execute_data };
                            eg.current_execute_data.set(prev);
                            eg.vm_stack.pop_call_frame(frame);
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
                    return bailout(frame, opline_ptr);
                }
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── DoFcall — recursive hot call or bailout ──
            OpCode::DoFcall => {
                let call = unsafe { (*frame).call };
                unsafe { (*frame).call = (*call).call };

                let func_common = unsafe { &*(*call).func };

                // Guard: only User functions with FastScalar/Fast call semantics.
                // ReturnStrategy::Fast and no-typed-params are guaranteed by promotion guard
                // (Hot implies ret==Fast and no non-trivial type hints), so we don't re-check here.
                if func_common.fn_type != FunctionType::User
                    || !matches!(func_common.plan.call, CallStrategy::FastScalar | CallStrategy::Fast)
                    || unsafe { (*call).num_args } != func_common.sig.num_args
                {
                    unsafe { (*frame).call = call };
                    return bailout(frame, opline_ptr);
                }

                // For Fast: bail if caller has globals to sync (depends on call site, not callee).
                // Uses hoisted bool — single read per DoFcall, always false for fib.
                if func_common.plan.call == CallStrategy::Fast && caller_has_globals {
                    unsafe { (*frame).call = call };
                    return bailout(frame, opline_ptr);
                }

                // Hotness tracking with promotion guard.
                // Only promote if function is eligible for hot executor:
                //   - ReturnStrategy::Fast (no globals/statics/try-finally/generator obligations)
                //   - No non-trivial param type hints (hot path skips validation)
                // This is checked once at promotion, not per-call.
                let cc = func_common.call_count.get();
                if cc < u32::MAX { func_common.call_count.set(cc + 1); }
                if cc == FUNC_HOT_THRESHOLD && func_common.hot_status.get() == HotStatus::Cold {
                    if func_common.plan.ret == ReturnStrategy::Fast
                        && (func_common.sig.param_type_hints.is_empty()
                            || func_common.sig.param_type_hints.iter().all(|h| matches!(h, ParamTypeHint::None | ParamTypeHint::Mixed)))
                    {
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
                        // Complex result type — bail
                        unsafe { (*frame).call = call };
                        return bailout(frame, opline_ptr);
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
                            // Callee returned successfully. Restore caller state.
                            frame = eg.current_execute_data.get();
                            // Frame should be our caller now — but we need to verify
                            // Actually after Completed, eg.current_execute_data was set to prev
                            // which should be `frame` (our caller). But `frame` here is the
                            // local in THIS invocation. Let me just reload:
                            opline_ptr = unsafe { (*frame).opline };
                            // op_array doesn't change (same function)
                            continue;
                        }
                        HotResult::Bailout => {
                            // Callee bailed — it's still the active frame with opline set.
                            // We need to bail the entire hot chain back to baseline.
                            // The callee frame is active, baseline will pick it up.
                            return Ok(HotResult::Bailout);
                        }
                    }
                } else {
                    // Cold callee — bail to baseline to handle it
                    return Ok(HotResult::Bailout);
                }
            }

            // ── Return — scalar fast path ──
            OpCode::Return => {
                if opline.op1_type != OpType::Unused {
                    let return_target = unsafe { (*frame).return_value };
                    if !return_target.is_null() {
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
                            return bailout(frame, opline_ptr);
                        };

                        let src_val = unsafe { &*retval_ptr };
                        if src_val.needs_cleanup() || src_val.is_reference() {
                            // Heap/ref return — bail to baseline for proper handling
                            return bailout(frame, opline_ptr);
                        }

                        let prev = unsafe { (*frame).prev_execute_data };
                        if !prev.is_null() && unsafe { (*prev).has_heap_slots } {
                            unsafe { std::ptr::drop_in_place(return_target) };
                        }
                        unsafe { Value::raw_copy(retval_ptr, return_target) };
                    }
                }
                // Pop frame
                let prev = unsafe { (*frame).prev_execute_data };
                eg.current_execute_data.set(prev);
                eg.vm_stack.pop_call_frame(frame);
                return Ok(HotResult::Completed);
            }

            // ── AssignCv (for $result = fib(...) in main scope) ──
            OpCode::AssignCv => {
                let src = if opline.op2_type == OpType::Tmp || opline.op2_type == OpType::Var {
                    unsafe { &*(frame as *const Value).add(CALL_FRAME_SLOTS + opline.op2 as usize) }
                } else if opline.op2_type == OpType::Cv {
                    let cv = unsafe { (*frame).cv(opline.op2 as u32) };
                    if cv.is_reference() { unsafe { &*cv.as_ref_ptr() } } else { cv }
                } else {
                    return bailout(frame, opline_ptr);
                };

                if src.needs_cleanup() || src.is_reference() {
                    return bailout(frame, opline_ptr);
                }

                let dst = unsafe { (*frame).cv_mut(opline.op1 as u32) as *mut Value };
                // Runtime guard: if destination holds a heap value (e.g. string param
                // forwarded from baseline) or a reference, bail to baseline which handles
                // drop + bookkeeping correctly. For scalar-recursive patterns (fib) this
                // is always false — CVs are scalar or zero-init within the hot path.
                let dst_val = unsafe { &*dst };
                if dst_val.needs_cleanup() || dst_val.is_reference() {
                    return bailout(frame, opline_ptr);
                }
                unsafe { Value::raw_copy(src as *const Value, dst) };
                opline_ptr = unsafe { opline_ptr.add(1) };
                continue;
            }

            // ── Any unhandled opcode → bail to baseline ──
            _ => {
                return bailout(frame, opline_ptr);
            }
        }
    }
}

/// Set frame's opline to current position and return Bailout.
#[inline(always)]
fn bailout(frame: *mut ExecuteData, opline_ptr: *const Instruction) -> Result<HotResult, VmError> {
    unsafe { (*frame).opline = opline_ptr };
    Ok(HotResult::Bailout)
}
