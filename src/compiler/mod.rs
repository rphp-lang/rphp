pub mod compile;

use crate::value::Value;
use crate::vm::instruction::{Instruction, InlineCache, OpType};
use std::cell::Cell;
use crate::vm::function::{
    FunctionCommon, FunctionType, UserFunction, ParamTypeHint,
    InternalFunction, InternalFunctionHandler,
    SignatureInfo, FrameLayout, CallPlan,
    CallStrategy, ReturnStrategy, CleanupMode, HotStatus,
};
use crate::vm::opcode::OpCode;
use crate::vm::planner::{BlockInfo, BlockPlan};

/// Compiled function body — equivalent to zend_op_array.
pub struct OpArray {
    pub num_cvs: u32,
    pub num_temps: u32,
    pub instructions: Vec<Instruction>,
    pub literals: Vec<Value>,
    pub try_entries: Vec<compile::TryEntry>,
    /// Per-file strict_types flag, set by `declare(strict_types=1);`
    pub strict_types: bool,
    /// True if this function contains yield — it's a generator
    pub is_generator: bool,
    /// CVs bound to global variables via explicit `global $x;`: (cv_index, variable_name)
    pub global_vars: Vec<(u32, String)>,
    /// CVs bound to static variables: (cv_index, variable_name)
    pub static_vars: Vec<(u32, String)>,
    /// Function name (for static variable storage key)
    pub name: String,
    /// Main script scope CVs — all top-level variables synced to eg.globals before function calls.
    /// Empty for non-main-script op_arrays.
    pub main_scope_vars: Vec<(u32, String)>,
    /// All CVs in this op_array: (cv_index, variable_name).
    /// Used by include to share the caller's full local scope.
    pub all_cvs: Vec<(u32, String)>,
    /// Inline cache side table — one entry per instruction.
    /// Only InitFcall/InitMethodCall/InitStaticCall use their entries.
    pub cache: Vec<InlineCache>,
    /// True if this function or any transitive callee may read/write eg.globals.
    /// Used by DoFcall to skip caller→globals sync when callee can't reach globals.
    /// Direct user-function calls are refined through the compilation unit's
    /// call graph. Dynamic, virtual, unknown and include-loaded targets remain
    /// conservative.
    pub may_access_globals: bool,
    /// Basic block metadata, computed once after instruction finalization.
    pub block_info: Vec<BlockInfo>,
    /// Per-block execution counters (for hot-block detection).
    pub block_counters: Vec<Cell<u32>>,
    /// Per-block execution plans (Interpret / Macro / Deoptimized).
    pub block_plans: Vec<BlockPlan>,
    /// Maps instruction IP -> block index.
    pub ip_to_block: Vec<u16>,
}

impl OpArray {
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    pub fn literals(&self) -> &[Value] {
        &self.literals
    }

    /// Rewrite Tmp/Var operand indices from relative (0-based tmp index) to
    /// absolute slot offset (num_cvs + tmp_index). After this pass, runtime
    /// can access Tmp slots as `frame_base.add(operand)` without loading num_cvs.
    pub fn resolve_tmp_offsets(&mut self) {
        use crate::vm::instruction::OpType;
        use crate::vm::opcode::OpCode;
        let offset = self.num_cvs;
        let offset16 = offset as u16;
        for instr in &mut self.instructions {
            if instr.op1_type == OpType::Tmp || instr.op1_type == OpType::Var {
                instr.op1 += offset16;
            }
            if instr.op2_type == OpType::Tmp || instr.op2_type == OpType::Var {
                instr.op2 += offset16;
            }
            if instr.result_type == OpType::Tmp || instr.result_type == OpType::Var {
                instr.result += offset16;
            }
            // ForeachInit stores pos_tmp in extended_value as a TMP index.
            if instr.opcode == OpCode::ForeachInit {
                instr.extended_value += offset;
            }
        }
    }

    /// Specialize opcodes for common operand-type patterns.
    /// Must be called AFTER resolve_tmp_offsets (operands are absolute).
    pub fn specialize_opcodes(&mut self) {
        self.specialize_opcodes_with_hints(&[]);
    }

    /// Specialize opcodes, with knowledge of parameter type hints.
    /// When all params are Int and Const operands are int literals,
    /// emits Int-guaranteed opcodes that skip runtime type checks.
    pub fn specialize_opcodes_with_hints(&mut self, _param_type_hints: &[ParamTypeHint]) {
        use crate::vm::instruction::OpType;
        use crate::vm::opcode::OpCode;

        // Pass 1: operand-type specialization (single-instruction patterns)
        for instr in &mut self.instructions {
            match instr.opcode {
                OpCode::Add => {
                    if instr.op1_type == OpType::Tmp && instr.op2_type == OpType::Tmp {
                        instr.opcode = OpCode::Add_TmpTmp;
                    } else if instr.op1_type == OpType::Cv && instr.op2_type == OpType::Tmp {
                        instr.opcode = OpCode::Add_CvTmp;
                    }
                }
                OpCode::Sub => {
                    if instr.op1_type == OpType::Cv && instr.op2_type == OpType::Const {
                        instr.opcode = OpCode::Sub_CvConst;
                    } else if instr.op1_type == OpType::Tmp && instr.op2_type == OpType::Tmp {
                        instr.opcode = OpCode::Sub_TmpTmp;
                    }
                }
                OpCode::IsSmaller => {
                    if instr.op1_type == OpType::Cv && instr.op2_type == OpType::Const {
                        instr.opcode = OpCode::IsSmaller_CvConst;
                    }
                }
                OpCode::IsSmallerOrEqual => {
                    if instr.op1_type == OpType::Cv && instr.op2_type == OpType::Const {
                        instr.opcode = OpCode::IsSmallerOrEqual_CvConst;
                    }
                }
                OpCode::IsEqual => {
                    if instr.op1_type == OpType::Cv && instr.op2_type == OpType::Const {
                        instr.opcode = OpCode::IsEqual_CvConst;
                    }
                }
                _ => {}
            }
        }

        // Pass 2: superinstructions (fuse comparison + conditional jump)
        // Fuses CmpCvConst + JmpZ/JmpNZ into a single dispatch when the JmpZ/JmpNZ
        // consumes the comparison's TMP result and that TMP is not used elsewhere.
        // Pass 2 active
        let len = self.instructions.len();
        if len < 2 { return; }
        let mut i = 0;
        while i < len - 1 {
            let curr = self.instructions[i];
            let next = self.instructions[i + 1];
            // Pattern A: comparison + conditional jump → fused branch
            let fused_cmp = match (curr.opcode, next.opcode) {
                (OpCode::IsSmallerOrEqual_CvConst, OpCode::JmpZ)
                    if next.op1_type == OpType::Tmp && next.op1 == curr.result
                        && curr.result_type == OpType::Tmp =>
                {
                    Some(OpCode::JmpZ_Le_CvConst)
                }
                (OpCode::IsSmallerOrEqual_CvConst, OpCode::JmpNZ)
                    if next.op1_type == OpType::Tmp && next.op1 == curr.result
                        && curr.result_type == OpType::Tmp =>
                {
                    Some(OpCode::JmpNZ_Le_CvConst)
                }
                (OpCode::IsSmaller_CvConst, OpCode::JmpZ)
                    if next.op1_type == OpType::Tmp && next.op1 == curr.result
                        && curr.result_type == OpType::Tmp =>
                {
                    Some(OpCode::JmpZ_Lt_CvConst)
                }
                (OpCode::IsSmaller_CvConst, OpCode::JmpNZ)
                    if next.op1_type == OpType::Tmp && next.op1 == curr.result
                        && curr.result_type == OpType::Tmp =>
                {
                    Some(OpCode::JmpNZ_Lt_CvConst)
                }
                (OpCode::IsEqual_CvConst, OpCode::JmpZ)
                    if next.op1_type == OpType::Tmp && next.op1 == curr.result
                        && curr.result_type == OpType::Tmp =>
                {
                    Some(OpCode::JmpZ_Eq_CvConst)
                }
                (OpCode::IsEqual_CvConst, OpCode::JmpNZ)
                    if next.op1_type == OpType::Tmp && next.op1 == curr.result
                        && curr.result_type == OpType::Tmp =>
                {
                    Some(OpCode::JmpNZ_Eq_CvConst)
                }
                _ => None,
            };
            if let Some(fused_opcode) = fused_cmp {
                self.instructions[i] = Instruction {
                    opcode: fused_opcode,
                    op1_type: curr.op1_type, // Cv
                    op2_type: curr.op2_type, // Const
                    result_type: OpType::Unused,
                    op1: curr.op1,            // CV index
                    op2: curr.op2,            // Const index
                    result: next.op2,         // jump target IP
                    _pad: 0,
                    extended_value: 0,
                };
                i += 2;
                continue;
            }

            i += 1;
        }
    }

    /// Initialize the inline cache side table to match instruction count.
    /// Called once after instructions are finalized.
    pub fn init_cache(&mut self) {
        let len = self.instructions.len();
        self.cache = Vec::with_capacity(len);
        for _ in 0..len {
            self.cache.push(InlineCache::empty());
        }
    }

    /// Scan instructions and identify basic block boundaries.
    /// Populates block_info, block_counters, block_plans, and ip_to_block.
    pub fn compute_blocks(&mut self) {
        if self.instructions.is_empty() {
            return;
        }

        let n = self.instructions.len();
        let mut is_leader = vec![false; n];
        is_leader[0] = true; // first instruction is always a block start

        // Scan for jump targets and instructions after branches
        for (i, instr) in self.instructions.iter().enumerate() {
            match instr.opcode {
                OpCode::Jmp | OpCode::QuickLongLoopJmp => {
                    // Jmp stores target in op1
                    let target = instr.op1 as usize;
                    if target < n {
                        is_leader[target] = true;
                    }
                    if i + 1 < n {
                        is_leader[i + 1] = true;
                    }
                }
                OpCode::JmpZ | OpCode::JmpNZ => {
                    // JmpZ/JmpNZ store target in op2
                    let target = instr.op2 as usize;
                    if target < n {
                        is_leader[target] = true;
                    }
                    // Instruction after the branch is also a leader (fall-through)
                    if i + 1 < n {
                        is_leader[i + 1] = true;
                    }
                }
                OpCode::Return => {
                    if i + 1 < n {
                        is_leader[i + 1] = true;
                    }
                }
                // DoFcall: the instruction after DoFcall starts a new context
                // but for our purposes, we keep it in the same block
                // (the macro executor handles DoFcall as a yield point within a block)
                _ => {}
            }
        }

        // Build block_info and ip_to_block
        let mut blocks = Vec::new();
        let mut ip_to_block = vec![0u16; n];
        let mut current_block_start = 0u32;

        for i in 0..n {
            if is_leader[i] && i > 0 {
                // Close previous block
                blocks.push(BlockInfo {
                    start_ip: current_block_start,
                    end_ip: (i - 1) as u32,
                });
                current_block_start = i as u32;
            }
            ip_to_block[i] = blocks.len() as u16; // current block index
        }
        // Close last block
        blocks.push(BlockInfo {
            start_ip: current_block_start,
            end_ip: (n - 1) as u32,
        });
        // Fix: ip_to_block for last block
        for i in current_block_start as usize..n {
            ip_to_block[i] = (blocks.len() - 1) as u16;
        }

        let num_blocks = blocks.len();
        self.block_info = blocks;
        self.block_counters = (0..num_blocks).map(|_| Cell::new(0)).collect();
        self.block_plans = (0..num_blocks).map(|_| BlockPlan::Interpret).collect();
        self.ip_to_block = ip_to_block;
    }

    /// Precompute closed scalar loop regions and mark their backedges.
    ///
    /// Matching `Jmp` instructions are rewritten to the dedicated
    /// `QuickLongLoopJmp`; ordinary jumps remain unchanged. `extended_value`
    /// stores the header block index + 1, so runtime activation does not scan
    /// instructions or build a plan.
    pub fn prepare_quick_loops(&mut self) {
        if !cfg!(feature = "quick-loops") {
            return;
        }
        if std::env::var_os("RPHP_DISABLE_QUICK_LOOPS").is_some() {
            return;
        }

        let mut candidates = Vec::new();

        for backedge_ip in 0..self.instructions.len() {
            let backedge = self.instructions[backedge_ip];
            if backedge.opcode != OpCode::Jmp {
                continue;
            }
            let header_ip = backedge.op1 as usize;
            if header_ip >= backedge_ip {
                continue;
            }
            let plan = crate::vm::quick::detect_long_accumulate_loop(
                self,
                header_ip,
                backedge_ip,
            )
            .map(BlockPlan::QuickLongAccumulate)
            .or_else(|| {
                crate::vm::quick::detect_foreach_long_accumulate_loop(
                    self,
                    header_ip,
                    backedge_ip,
                )
                .map(BlockPlan::QuickForeachLongAccumulate)
            })
            .or_else(|| {
                crate::vm::quick::detect_long_ops_loop(self, header_ip, backedge_ip)
                    .map(BlockPlan::QuickLongOps)
            });
            if let Some(plan) = plan {
                let block_idx = *self.ip_to_block.get(header_ip).unwrap_or(&u16::MAX);
                if block_idx != u16::MAX {
                    candidates.push((backedge_ip, block_idx, plan));
                }
            }
        }

        for (backedge_ip, block_idx, plan) in candidates {
            self.block_plans[block_idx as usize] = plan;
            self.instructions[backedge_ip].opcode = OpCode::QuickLongLoopJmp;
            self.instructions[backedge_ip].extended_value = block_idx as u32 + 1;
        }
    }
}

impl Drop for OpArray {
    fn drop(&mut self) {
        // A DoFcall cache may retain an Rc-backed callback-name string. Pair
        // that reference here; all other cache kinds own no heap allocation.
        for (instruction, cache) in self.instructions.iter().zip(&self.cache) {
            if instruction.opcode == OpCode::DoFcall {
                let callback_string = cache.callback_string();
                if !callback_string.is_null() {
                    unsafe { Value::release_cached_string(callback_string) };
                }
            }
        }
    }
}

fn op_array_supports_cleanup_fast(op_array: &OpArray) -> bool {
    if !op_array.global_vars.is_empty()
        || !op_array.static_vars.is_empty()
        || !op_array.try_entries.is_empty()
        || op_array.is_generator
    {
        return false;
    }

    op_array.instructions.iter().all(|instr| {
        matches!(
            instr.opcode,
            OpCode::Add
                | OpCode::Sub
                | OpCode::Mul
                | OpCode::Div
                | OpCode::Mod
                | OpCode::Pow
                | OpCode::BitwiseAnd
                | OpCode::BitwiseOr
                | OpCode::BitwiseXor
                | OpCode::BitwiseNot
                | OpCode::ShiftLeft
                | OpCode::ShiftRight
                | OpCode::IsEqual
                | OpCode::IsNotEqual
                | OpCode::IsSmaller
                | OpCode::IsSmallerOrEqual
                | OpCode::IsIdentical
                | OpCode::IsNotIdentical
                | OpCode::BoolNot
                | OpCode::Spaceship
                | OpCode::Echo
                | OpCode::InitFcall
                | OpCode::InitMethodCall
                | OpCode::InitStaticCall
                | OpCode::InitDynamicCall
                | OpCode::SendVal
                | OpCode::SendRef
                | OpCode::SendVarEx
                | OpCode::SendNamed
                | OpCode::DoFcall
                | OpCode::Return
                | OpCode::Jmp
                | OpCode::QuickLongLoopJmp
                | OpCode::JmpZ
                | OpCode::JmpNZ
                | OpCode::NullSafeCheck
                | OpCode::Instanceof
                // Specialized opcodes (same scalar semantics as originals)
                | OpCode::Add_TmpTmp
                | OpCode::Add_CvTmp
                | OpCode::Sub_CvConst
                | OpCode::Sub_TmpTmp
                | OpCode::IsSmaller_CvConst
                | OpCode::IsSmallerOrEqual_CvConst
                | OpCode::IsEqual_CvConst
        )
    })
}

/// Create a UserFunction wrapping an OpArray (no args — for main script).
pub fn make_user_function(op_array: OpArray) -> UserFunction {
    make_user_function_with_args(op_array, 0)
}

/// Create a UserFunction with the given number of parameters.
pub fn make_user_function_with_args(op_array: OpArray, num_args: u32) -> UserFunction {
    make_user_function_full(op_array, num_args, num_args, false, 0, 0)
}

/// Create a UserFunction with separate total and required arg counts (for default params).
pub fn make_user_function_with_defaults(op_array: OpArray, num_args: u32, required_num_args: u32, is_variadic: bool) -> UserFunction {
    make_user_function_full(op_array, num_args, required_num_args, is_variadic, 0, 0)
}

/// Full constructor with all options.
pub fn make_user_function_full(mut op_array: OpArray, num_args: u32, required_num_args: u32, is_variadic: bool, variadic_cv_index: u32, ref_args: u64) -> UserFunction {
    op_array.resolve_tmp_offsets();
    op_array.specialize_opcodes();
    if op_array.cache.len() != op_array.instructions.len() {
        op_array.init_cache();
    }
    op_array.compute_blocks();
    op_array.prepare_quick_loops();
    let is_fast_scalar = !is_variadic
        && !op_array.is_generator
        && ref_args == 0
        && num_args == required_num_args
        && op_array.global_vars.is_empty()
        && op_array.static_vars.is_empty()
        && op_array.try_entries.is_empty()
        && !op_array.may_access_globals;
    let call = if is_fast_scalar {
        CallStrategy::FastScalar
    } else if !is_variadic && !op_array.is_generator {
        CallStrategy::Fast
    } else {
        CallStrategy::Full
    };
    let cleanup = if op_array_supports_cleanup_fast(&op_array) { CleanupMode::SkipScan } else { CleanupMode::ScanAll };
    let ret = if op_array.global_vars.is_empty()
        && op_array.static_vars.is_empty()
        && op_array.try_entries.is_empty()
        && !op_array.is_generator
    { ReturnStrategy::Fast } else { ReturnStrategy::Full };
    let num_cvs = op_array.num_cvs;
    let num_temps = op_array.num_temps;
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_cvs + num_temps;
    UserFunction {
        common: FunctionCommon {
            fn_type: FunctionType::User,
            sig: SignatureInfo {
                num_args,
                required_num_args,
                is_variadic,
                variadic_cv_index,
                ref_args,
                this_offset: 0,
                param_type_hints: vec![],
                param_names: vec![],
                return_type_hint: ParamTypeHint::None,
            },
            frame: FrameLayout { num_cvs, num_temps, total_slots },
            plan: CallPlan { call, ret, cleanup, borrow_this: false },
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        op_array,
    }
}

/// Extended full constructor with type hints and param names.
pub fn make_user_function_typed(
    mut op_array: OpArray,
    num_args: u32,
    required_num_args: u32,
    is_variadic: bool,
    variadic_cv_index: u32,
    ref_args: u64,
    param_type_hints: Vec<ParamTypeHint>,
    param_names: Vec<String>,
    return_type_hint: ParamTypeHint,
) -> UserFunction {
    op_array.resolve_tmp_offsets();
    op_array.specialize_opcodes_with_hints(&param_type_hints);
    if op_array.cache.len() != op_array.instructions.len() {
        op_array.init_cache();
    }
    op_array.compute_blocks();
    op_array.prepare_quick_loops();
    // FastScalar: tightest path for simple fixed-arity scalar functions.
    // Requires NO actual type hints — DoFcall FastScalar skips type checking entirely.
    let has_only_scalar_hints = param_type_hints.iter().all(|h| matches!(h,
        ParamTypeHint::None | ParamTypeHint::Int | ParamTypeHint::Float
        | ParamTypeHint::String | ParamTypeHint::Bool | ParamTypeHint::Mixed
    ));
    let has_no_type_hints = param_type_hints.iter().all(|h| matches!(h,
        ParamTypeHint::None | ParamTypeHint::Mixed
    ));
    let has_no_return_type = matches!(return_type_hint, ParamTypeHint::None | ParamTypeHint::Mixed);
    let is_fast_scalar = !is_variadic
        && !op_array.is_generator
        && ref_args == 0
        && num_args == required_num_args
        && op_array.global_vars.is_empty()
        && op_array.static_vars.is_empty()
        && op_array.try_entries.is_empty()
        && !op_array.may_access_globals
        && has_no_type_hints
        && has_no_return_type;
    let call = if is_fast_scalar {
        CallStrategy::FastScalar
    } else if !is_variadic && !op_array.is_generator && has_only_scalar_hints {
        CallStrategy::Fast
    } else {
        CallStrategy::Full
    };
    let cleanup = if op_array_supports_cleanup_fast(&op_array) { CleanupMode::SkipScan } else { CleanupMode::ScanAll };
    let ret = if op_array.global_vars.is_empty()
        && op_array.static_vars.is_empty()
        && op_array.try_entries.is_empty()
        && !op_array.is_generator
        && matches!(return_type_hint, ParamTypeHint::None | ParamTypeHint::Int
            | ParamTypeHint::Float | ParamTypeHint::String | ParamTypeHint::Bool | ParamTypeHint::Mixed)
    { ReturnStrategy::Fast } else { ReturnStrategy::Full };
    let num_cvs = op_array.num_cvs;
    let num_temps = op_array.num_temps;
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_cvs + num_temps;
    UserFunction {
        common: FunctionCommon {
            fn_type: FunctionType::User,
            sig: SignatureInfo {
                num_args,
                required_num_args,
                is_variadic,
                variadic_cv_index,
                ref_args,
                this_offset: 0,
                param_type_hints,
                param_names,
                return_type_hint,
            },
            frame: FrameLayout { num_cvs, num_temps, total_slots },
            plan: CallPlan { call, ret, cleanup, borrow_this: false },
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        op_array,
    }
}

/// Create an InternalFunction with the given handler.
pub fn make_internal_function(
    handler: InternalFunctionHandler,
    num_args: u32,
    required_num_args: u32,
    param_names: Vec<String>,
) -> InternalFunction {
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_args;
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            sig: SignatureInfo {
                num_args,
                required_num_args,
                is_variadic: false,
                variadic_cv_index: 0,
                ref_args: 0,
                this_offset: 0,
                param_type_hints: vec![],
                param_names,
                return_type_hint: ParamTypeHint::None,
            },
            frame: FrameLayout { num_cvs: num_args, num_temps: 0, total_slots },
            plan: CallPlan {
                // Fixed-arity internal functions have no VM-level type hints,
                // references, or variadic packing. DoFcall can therefore run
                // the handler after a compact arity/hole check and leave named
                // or otherwise exceptional calls to the full path.
                call: CallStrategy::Fast,
                ret: ReturnStrategy::Full,
                cleanup: CleanupMode::ScanAll,
                borrow_this: false,
            },
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        handler,
    }
}

/// Finalize a non-static user method after `$this` has been reserved at CV 0.
///
/// `make_user_function_typed()` cannot classify a method as FastScalar while
/// `this_offset` is still zero: `num_args` already includes the hidden `$this`
/// slot while `required_num_args` intentionally counts only public arguments.
/// Re-run that classification once the public signature is known.
pub fn finalize_user_method(mut function: UserFunction) -> UserFunction {
    function.common.sig.this_offset = 1;

    let common = &function.common;
    let has_no_type_hints = common
        .sig
        .param_type_hints
        .iter()
        .all(|hint| matches!(hint, ParamTypeHint::None | ParamTypeHint::Mixed));
    let has_no_return_type =
        matches!(common.sig.return_type_hint, ParamTypeHint::None | ParamTypeHint::Mixed);
    let can_use_fast_scalar = !common.sig.is_variadic
        && common.sig.ref_args == 0
        && common.sig.public_arity() == common.sig.required_num_args
        && function.op_array.global_vars.is_empty()
        && function.op_array.static_vars.is_empty()
        && function.op_array.try_entries.is_empty()
        && !function.op_array.is_generator
        && !function.op_array.may_access_globals
        && has_no_type_hints
        && has_no_return_type;

    if can_use_fast_scalar {
        function.common.plan.call = CallStrategy::FastScalar;
        function.common.plan.borrow_this = !function.op_array.instructions.iter().any(|instruction| {
            instruction.opcode == OpCode::Return
                && instruction.op1_type == OpType::Cv
                && instruction.op1 == 0
        });
    }

    function
}

/// Create an InternalFunction for a method (with $this in CV 0).
/// `num_args` includes the hidden $this slot; `this_offset` is set to 1.
pub fn make_internal_method(
    handler: InternalFunctionHandler,
    num_args: u32,
    required_num_args: u32,
    param_names: Vec<String>,
) -> InternalFunction {
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_args;
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            sig: SignatureInfo {
                num_args,
                required_num_args,
                is_variadic: false,
                variadic_cv_index: 0,
                ref_args: 0,
                this_offset: 1,
                param_type_hints: vec![],
                param_names,
                return_type_hint: ParamTypeHint::None,
            },
            frame: FrameLayout { num_cvs: num_args, num_temps: 0, total_slots },
            plan: CallPlan {
                call: CallStrategy::Fast,
                ret: ReturnStrategy::Full,
                cleanup: CleanupMode::ScanAll,
                borrow_this: false,
            },
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        handler,
    }
}

/// Create an InternalFunction with by-ref parameter bitmask.
pub fn make_internal_function_ref(
    handler: InternalFunctionHandler,
    num_args: u32,
    required_num_args: u32,
    ref_args: u64,
    param_names: Vec<String>,
) -> InternalFunction {
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_args;
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            sig: SignatureInfo {
                num_args,
                required_num_args,
                is_variadic: false,
                variadic_cv_index: 0,
                ref_args,
                this_offset: 0,
                param_type_hints: vec![],
                param_names,
                return_type_hint: ParamTypeHint::None,
            },
            frame: FrameLayout { num_cvs: num_args, num_temps: 0, total_slots },
            plan: CallPlan {
                call: CallStrategy::Full,
                ret: ReturnStrategy::Full,
                cleanup: CleanupMode::ScanAll,
                borrow_this: false,
            },
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        handler,
    }
}

/// Create a variadic InternalFunction.
pub fn make_internal_function_variadic(
    handler: InternalFunctionHandler,
    required_num_args: u32,
    param_names: Vec<String>,
) -> InternalFunction {
    let num_cvs = required_num_args + 1;
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_cvs;
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            sig: SignatureInfo {
                num_args: required_num_args,
                required_num_args,
                is_variadic: true,
                variadic_cv_index: required_num_args,
                ref_args: 0,
                this_offset: 0,
                param_type_hints: vec![],
                param_names,
                return_type_hint: ParamTypeHint::None,
            },
            frame: FrameLayout { num_cvs, num_temps: 0, total_slots },
            plan: CallPlan {
                call: CallStrategy::Full,
                ret: ReturnStrategy::Full,
                cleanup: CleanupMode::ScanAll,
                borrow_this: false,
            },
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        handler,
    }
}
