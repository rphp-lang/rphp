//! Dynamic hot-block planner: profiles basic blocks at runtime and builds
//! compact macro execution plans for hot blocks.
//!
//! Architecture:
//! 1. Basic blocks are identified at compile time (compute_blocks in OpArray).
//! 2. Block counters increment at runtime on each block entry.
//! 3. When a counter exceeds HOT_THRESHOLD, plan_hot_block() analyzes the block
//!    and builds a MacroPlan from a small set of MacroStep micro-operations.
//! 4. execute_macro() runs the plan with type guards; on guard failure,
//!    falls back to the baseline interpreter at the block's start IP.
//!
//! This is NOT a JIT, NOT opcode fusion, NOT new opcodes. It's dynamic
//! block-level planning over the existing opcode representation.

use crate::compiler::OpArray;
use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::frame::{CALL_FRAME_SLOTS, ExecuteData};
use crate::vm::function::{CallStrategy, FunctionCommon, FunctionType, UserFunction};
#[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
use crate::vm::function::{CapturedTypedLongFunctionPlan, IndirectScalarLongFunctionPlan};
use crate::vm::instruction::{Instruction, OpType};
use crate::vm::opcode::OpCode;
use crate::vm::quick::{
    QuickDoubleCallAccumulateLoop, QuickForeachLongAccumulateLoop,
    QuickForeachObjectPropertyAccumulateLoop, QuickLongAccumulateLoop, QuickLongInductionLoop,
    QuickLongOpsLoop,
};

/// Basic block metadata, computed once per OpArray at compile time.
#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub start_ip: u32,
    pub end_ip: u32,
}

/// Per-block execution plan.
pub enum BlockPlan {
    /// Not yet hot — use baseline interpreter.
    Interpret,
    /// Hot block — execute via macro steps.
    Macro(MacroPlan),
    /// Was macro but guards failed too often — permanently reverted.
    Deoptimized,
    /// Guarded scalar loop region selected by the no-JIT quick executor.
    QuickLongAccumulate(QuickLongAccumulateLoop),
    /// Long-controlled loop accumulating exact Double scalar-call results.
    QuickDoubleCallAccumulate(QuickDoubleCallAccumulateLoop),
    /// Guarded induction-only `for` or `while` loop.
    QuickLongInduction(QuickLongInductionLoop),
    /// Guarded value-only foreach accumulation over long array values.
    QuickForeachLongAccumulate(QuickForeachLongAccumulateLoop),
    /// Guarded value-only foreach accumulation over object property projections.
    QuickForeachObjectPropertyAccumulate(QuickForeachObjectPropertyAccumulateLoop),
    /// Typed scalar operations for a closed loop not covered by a superinstruction.
    QuickLongOps(QuickLongOpsLoop),
    /// Function-level proof stored after the indexed per-block prefix. It is
    /// resolved only when an enclosing call region is entered and therefore
    /// adds no field or ordinary-path access to every `UserFunction`.
    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    FunctionIndirectScalarLong(Box<IndirectScalarLongFunctionPlan>),
    /// Capture-aware pure closure body. Like other function-level metadata it
    /// lives after the indexed per-block prefix and is never selected as a
    /// block execution plan.
    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    FunctionCapturedTypedLong(Box<CapturedTypedLongFunctionPlan>),
}

/// A compiled macro plan for a single basic block.
pub struct MacroPlan {
    pub steps: Vec<MacroStep>,
    pub guard_failures: u32,
    pub start_ip: u32,
    pub end_ip: u32,
}

/// Micro-operations for the macro executor. Small, closed set.
/// Each step is pre-validated and type-specialized — no runtime type checks.
/// Steps are emitted dynamically by plan_hot_block based on actual instruction sequences.
pub enum MacroStep {
    /// Guard: check that frame slot holds a Long value. On fail -> GuardFail.
    GuardLong { slot: u16 },

    /// Compare i64: slot[cv] <= const_val, write bool result to slot[result].
    CmpLeLongConst {
        cv_slot: u16,
        const_val: i64,
        result_slot: u16,
    },

    /// Compare i64: slot[cv] < const_val, write bool result to slot[result].
    CmpLtLongConst {
        cv_slot: u16,
        const_val: i64,
        result_slot: u16,
    },

    /// Conditional jump: if slot[cond] is falsy, terminate macro with Jump(target_ip).
    JmpZBool { cond_slot: u16, target_ip: u32 },

    /// Subtract i64: slot[cv] - const_val -> slot[result]. Overflow -> GuardFail.
    SubLongConst {
        cv_slot: u16,
        const_val: i64,
        result_slot: u16,
    },

    /// Add i64: slot[a] + slot[b] -> slot[result]. Overflow -> GuardFail.
    AddLongLong {
        a_slot: u16,
        b_slot: u16,
        result_slot: u16,
    },

    /// Return value from frame slot. Caller handles frame pop.
    ReturnSlot { slot: u16, slot_type: OpType },

    /// InitFcall with pre-resolved function pointer (from inline cache).
    InitFcallCached {
        func_ptr: *const FunctionCommon,
        num_args: u32,
    },

    /// Copy slot[src] to callee's CV[arg_idx]. Raw 16-byte copy, no clone ceremony.
    SendValSlot { src_slot: u16, arg_idx: u32 },

    /// DoFcall via FastScalar protocol. Macro yields here — callee runs in baseline.
    /// On resume, macro continues from next step. result_slot receives return value.
    DoFcallFastScalar { result_slot: u16 },
}

/// Result of macro execution.
pub enum MacroResult {
    /// Block completed via Return. Return value written, caller must pop frame.
    Returned,
    /// Conditional jump taken — set opline to target_ip, continue baseline.
    Jump(u32),
    /// DoFcall yielded — callee frame active. Resume at resume_step after return.
    CallYield { resume_step: u16 },
    /// Guard or overflow failed — fall back to baseline at block start.
    GuardFail,
    /// Fall through to next instruction after block end.
    FallThrough,
}

/// Resume info for macro execution across DoFcall boundaries.
/// Stored in ExecutorGlobals (not in ExecuteData) to avoid frame size increase.
pub struct MacroResume {
    pub plan_ptr: *const MacroPlan,
    pub step_idx: u16,
    /// The frame this resume belongs to. Only fire when current frame matches.
    pub caller_frame: *mut ExecuteData,
}

/// Hot threshold: how many block executions before planning.
pub const HOT_THRESHOLD: u32 = 64;

/// Deopt threshold: how many guard failures before reverting to baseline.
pub const DEOPT_THRESHOLD: u32 = 10;

// ── Dynamic planner ─────────────────────────────────────────────────────

/// Analyze a hot basic block and build a MacroPlan if the block is plannable.
/// Returns None if the block contains unsupported patterns.
///
/// This is the core of the dynamic planning system. It reads the actual instruction
/// sequence, checks safety preconditions, and emits a type-specialized plan.
/// Called at runtime when a block's counter crosses HOT_THRESHOLD.
pub fn plan_hot_block(op_array: &OpArray, block_idx: usize) -> Option<MacroPlan> {
    // Safety bail: function-level preconditions.
    // These are equivalent to FunctionCommon::can_promote_to_hot() checks:
    //   try_entries → ReturnStrategy::Full, generator → Full, globals/statics → Full.
    // We check op_array fields directly because plan_hot_block receives an OpArray,
    // not a FunctionCommon. Both systems share the same eligibility invariants.
    if !op_array.try_entries.is_empty()
        || op_array.is_generator
        || !op_array.global_vars.is_empty()
        || !op_array.static_vars.is_empty()
    {
        return None;
    }

    let block = &op_array.block_info[block_idx];
    let start = block.start_ip as usize;
    let end = block.end_ip as usize;
    let instrs = &op_array.instructions[start..=end];

    let mut steps: Vec<MacroStep> = Vec::new();
    let mut guarded_cvs: u64 = 0; // bitmask of CVs that have a GuardLong

    // First pass: check all instructions are plannable and collect CV usage
    for instr in instrs.iter() {
        if !is_plannable_opcode(instr.opcode) {
            return None;
        }
    }

    // Second pass: emit MacroSteps
    for (offset, instr) in instrs.iter().enumerate() {
        let ip = start + offset;
        match instr.opcode {
            OpCode::IsSmallerOrEqual_CvConst | OpCode::IsSmaller_CvConst => {
                let cv_slot = instr.op1;
                let const_idx = instr.op2 as usize;
                let result_slot = instr.result;

                // Ensure CV is guarded
                ensure_guard_long(&mut steps, &mut guarded_cvs, cv_slot);

                // Read const value — must be Long for planning
                let const_val = &op_array.literals[const_idx];
                let long_val = const_val.as_long()?;

                if instr.opcode == OpCode::IsSmallerOrEqual_CvConst {
                    steps.push(MacroStep::CmpLeLongConst {
                        cv_slot,
                        const_val: long_val,
                        result_slot,
                    });
                } else {
                    steps.push(MacroStep::CmpLtLongConst {
                        cv_slot,
                        const_val: long_val,
                        result_slot,
                    });
                }
            }

            OpCode::JmpZ => {
                let cond_slot = instr.op1;
                let target_ip = instr.op2 as u32;
                steps.push(MacroStep::JmpZBool {
                    cond_slot,
                    target_ip,
                });
            }

            OpCode::JmpNZ => {
                // JmpNZ: jump if truthy. We invert the sense for our step.
                // For now, bail — we can add JmpNZBool later.
                return None;
            }

            OpCode::Sub_CvConst => {
                let cv_slot = instr.op1;
                let const_idx = instr.op2 as usize;
                let result_slot = instr.result;

                ensure_guard_long(&mut steps, &mut guarded_cvs, cv_slot);

                let const_val = &op_array.literals[const_idx];
                let long_val = const_val.as_long()?;

                steps.push(MacroStep::SubLongConst {
                    cv_slot,
                    const_val: long_val,
                    result_slot,
                });
            }

            OpCode::Add_TmpTmp => {
                steps.push(MacroStep::AddLongLong {
                    a_slot: instr.op1,
                    b_slot: instr.op2,
                    result_slot: instr.result,
                });
            }

            OpCode::InitFcall => {
                // Read inline cache for pre-resolved function pointer
                let cache = &op_array.cache[ip];
                let func_ptr = cache.func;
                if func_ptr.is_null() {
                    eprintln!(
                        "[planner] block {} bail: InitFcall at IP {} has null cache",
                        block_idx, ip
                    );
                    return None; // Not cached yet — can't plan
                }
                // Callee eligibility: use same predicate as hot executor.
                // can_promote_to_hot() checks: User, Fast/FastScalar call, Fast return,
                // no non-trivial type hints. Single source of truth.
                let common = unsafe { &*func_ptr };
                if !common.can_promote_to_hot() {
                    return None;
                }
                // Additional plan-time checks: exact arity match (no default params).
                let num_args_check = instr.op1 as u32;
                if num_args_check != common.sig.num_args
                    || num_args_check != common.sig.required_num_args
                {
                    return None;
                }

                let num_args = instr.op1 as u32;
                steps.push(MacroStep::InitFcallCached { func_ptr, num_args });
            }

            OpCode::SendVal => {
                let src_slot = instr.op1;
                let arg_idx = instr.op2 as u32;
                steps.push(MacroStep::SendValSlot { src_slot, arg_idx });
            }

            OpCode::DoFcall => {
                let result_slot = instr.result;
                steps.push(MacroStep::DoFcallFastScalar { result_slot });
            }

            OpCode::Return => {
                steps.push(MacroStep::ReturnSlot {
                    slot: instr.op1,
                    slot_type: instr.op1_type,
                });
            }

            // Add_CvTmp, Sub_TmpConst, etc. — bail for now, add as needed
            _ => return None,
        }
    }

    if steps.is_empty() {
        return None;
    }

    Some(MacroPlan {
        steps,
        guard_failures: 0,
        start_ip: block.start_ip,
        end_ip: block.end_ip,
    })
}

/// Check if an opcode is supported by the planner.
fn is_plannable_opcode(op: OpCode) -> bool {
    matches!(
        op,
        OpCode::IsSmallerOrEqual_CvConst
            | OpCode::IsSmaller_CvConst
            | OpCode::JmpZ
            | OpCode::Sub_CvConst
            | OpCode::Add_TmpTmp
            | OpCode::InitFcall
            | OpCode::SendVal
            | OpCode::DoFcall
            | OpCode::Return
    )
}

/// Ensure a CV slot has a GuardLong step. Emits the guard if not yet present.
fn ensure_guard_long(steps: &mut Vec<MacroStep>, guarded: &mut u64, slot: u16) {
    if slot < 64 {
        let bit = 1u64 << slot;
        if *guarded & bit == 0 {
            steps.push(MacroStep::GuardLong { slot });
            *guarded |= bit;
        }
    }
}

// ── Macro executor ──────────────────────────────────────────────────────

/// Execute a macro plan starting from `start_step`.
///
/// Returns MacroResult indicating what happened:
/// - Returned: callee executed Return, frame pop needed
/// - Jump: conditional jump taken, set opline to target
/// - CallYield: DoFcall entered callee, resume at step
/// - GuardFail: precondition failed, fall back to baseline
/// - FallThrough: block ended without branch/return
///
/// SAFETY: frame must be valid, op_array must match the frame's function.
pub unsafe fn execute_macro(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &OpArray,
    plan: &MacroPlan,
    start_step: usize,
) -> MacroResult {
    let slot_base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    let n = plan.steps.len();

    let mut i = start_step;
    while i < n {
        match &plan.steps[i] {
            MacroStep::GuardLong { slot } => {
                let ptr = slot_base.add(*slot as usize);
                if (*ptr).value_type() != ValueType::Long {
                    return MacroResult::GuardFail;
                }
            }

            MacroStep::CmpLeLongConst {
                cv_slot,
                const_val,
                result_slot,
            } => {
                let val = (*slot_base.add(*cv_slot as usize)).raw_long();
                let result = val <= *const_val;
                Value::write_bool(slot_base.add(*result_slot as usize), result);
            }

            MacroStep::CmpLtLongConst {
                cv_slot,
                const_val,
                result_slot,
            } => {
                let val = (*slot_base.add(*cv_slot as usize)).raw_long();
                let result = val < *const_val;
                Value::write_bool(slot_base.add(*result_slot as usize), result);
            }

            MacroStep::JmpZBool {
                cond_slot,
                target_ip,
            } => {
                let cond_ptr = slot_base.add(*cond_slot as usize);
                // Bool was written by Cmp step — check type directly
                let t = (*cond_ptr).value_type();
                let truthy = match t {
                    ValueType::True => true,
                    ValueType::False => false,
                    ValueType::Long => (*cond_ptr).raw_long() != 0,
                    _ => return MacroResult::GuardFail,
                };
                if !truthy {
                    return MacroResult::Jump(*target_ip);
                }
                // truthy → fall through (block ends here for conditional jumps)
                // The next instruction after JmpZ is in a different block,
                // so we return FallThrough to let the dispatcher advance opline.
                return MacroResult::FallThrough;
            }

            MacroStep::SubLongConst {
                cv_slot,
                const_val,
                result_slot,
            } => {
                let val = (*slot_base.add(*cv_slot as usize)).raw_long();
                match val.checked_sub(*const_val) {
                    Some(result) => {
                        Value::write_long(slot_base.add(*result_slot as usize), result);
                    }
                    None => return MacroResult::GuardFail,
                }
            }

            MacroStep::AddLongLong {
                a_slot,
                b_slot,
                result_slot,
            } => {
                let a = (*slot_base.add(*a_slot as usize)).raw_long();
                let b = (*slot_base.add(*b_slot as usize)).raw_long();
                match a.checked_add(b) {
                    Some(result) => {
                        Value::write_long(slot_base.add(*result_slot as usize), result);
                    }
                    None => return MacroResult::GuardFail,
                }
            }

            MacroStep::ReturnSlot { slot, slot_type } => {
                // Read return value from slot
                let retval_ptr = match slot_type {
                    OpType::Cv => {
                        let cv_ptr = slot_base.add(*slot as usize);
                        if (*cv_ptr).is_reference() {
                            (*cv_ptr).as_ref_ptr()
                        } else {
                            cv_ptr
                        }
                    }
                    _ => slot_base.add(*slot as usize), // Tmp/Const
                };

                let common = &*(*frame).func;
                if super::execute::check_fast_scalar_type_hint(
                    &*retval_ptr,
                    &common.sig.return_type_hint,
                    op_array.strict_types,
                ) != Some(true)
                {
                    return MacroResult::GuardFail;
                }

                let return_target = (*frame).return_value;
                if !return_target.is_null() {
                    super::execute::frame_return_set(frame, return_target, (*retval_ptr).clone());
                }

                return MacroResult::Returned;
            }

            MacroStep::InitFcallCached { func_ptr, num_args } => {
                // Push call frame for the known function
                let call = eg.vm_stack.push_call_frame(
                    *func_ptr,
                    *num_args,
                    *num_args,
                    frame,
                    (*frame).call,
                );
                (*frame).call = call;
            }

            MacroStep::SendValSlot { src_slot, arg_idx } => {
                let call = (*frame).call;
                debug_assert!(!call.is_null());
                let src = slot_base.add(*src_slot as usize);
                let dst = (call as *mut Value)
                    .add(CALL_FRAME_SLOTS)
                    .add(*arg_idx as usize);
                // Check scalar safety at runtime: only raw-copy if value doesn't
                // need cleanup (no heap/reference). This guards against cases where
                // the source TMP unexpectedly holds a heap type.
                let src_val = &*src;
                if !src_val.needs_cleanup() && !src_val.is_reference() {
                    std::ptr::copy_nonoverlapping(src, dst, 1);
                } else {
                    // Heap/reference: must clone + mark callee frame
                    let cloned = src_val.clone();
                    dst.write(cloned);
                    (*call).has_heap_slots = true;
                    let total = (*call).num_cvs + (*call).num_temps;
                    if total <= 64 {
                        (*call).heap_bitmap |= 1u64 << *arg_idx;
                    }
                }
            }

            MacroStep::DoFcallFastScalar { result_slot } => {
                let call = (*frame).call;
                debug_assert!(!call.is_null());

                // Restore caller's pending call chain
                (*frame).call = (*call).call;

                let func_common = &*(*call).func;
                // Runtime guard: callee still uses a compact user-call ABI.
                if func_common.fn_type != FunctionType::User
                    || !func_common.plan.call.is_compact_user_call()
                {
                    (*frame).call = call;
                    return MacroResult::GuardFail;
                }

                if func_common.plan.call != CallStrategy::FastScalar
                    && !super::execute::compact_scalar_call_types_match(
                        eg,
                        call,
                        func_common,
                        op_array.strict_types,
                    )
                {
                    (*frame).call = call;
                    return MacroResult::GuardFail;
                }

                let user = &*((*call).func as *const UserFunction);

                // Set return value target in caller's frame
                let return_value_ptr = slot_base.add(*result_slot as usize);
                (*call).return_value = return_value_ptr;

                // Set callee's opline to first instruction
                (*call).opline = user.op_array.instructions.as_ptr();

                // Advance caller's opline past the DoFcall
                // (plan.end_ip points to the last instruction in the block,
                //  but for DoFcall mid-block, we need the actual DoFcall IP)
                // We compute it from the step index and block start.
                // Actually, we need the IP of the DoFcall instruction itself.
                // The steps map 1:1 to instructions (minus guards), but guards
                // don't correspond to instructions. Instead, set opline to
                // the instruction AFTER the block's end (will be overridden on resume).
                // For mid-block DoFcall: count real instructions up to this step.
                let dofcall_ip = find_instruction_ip_for_step(plan, i, &op_array.instructions);
                (*frame).opline = op_array.instructions.as_ptr().add(dofcall_ip + 1);

                // Switch to callee frame
                eg.current_execute_data.set(call);

                // Signal to the dispatch loop: resume at next step after callee returns
                return MacroResult::CallYield {
                    resume_step: (i + 1) as u16,
                };
            }
        }
        i += 1;
    }

    // Reached end of steps without branch/return — fall through
    MacroResult::FallThrough
}

/// Find the instruction IP corresponding to a macro step index.
/// Guards don't correspond to instructions, so we skip them when counting.
fn find_instruction_ip_for_step(
    plan: &MacroPlan,
    step_idx: usize,
    _instrs: &[Instruction],
) -> usize {
    let mut instr_offset = 0usize;
    for s in 0..=step_idx {
        match &plan.steps[s] {
            MacroStep::GuardLong { .. } => {
                // Guards don't correspond to instructions — don't advance
            }
            _ => {
                if s < step_idx {
                    instr_offset += 1;
                }
            }
        }
    }
    plan.start_ip as usize + instr_offset
}
