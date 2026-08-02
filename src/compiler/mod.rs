pub mod compile;

use crate::value::Value;
use crate::vm::instruction::{Instruction, InlineCache, OpType};
use std::cell::Cell;
use crate::vm::function::{
    FunctionCommon, FunctionType, UserFunction, ParamTypeHint,
    DirectInternalFunctionHandler, InternalFunction, InternalFunctionHandler,
    SignatureInfo, FrameLayout, CallPlan,
    CallStrategy, ReturnStrategy, CleanupMode, HotStatus,
    LongPlanProperty, LongPlanSource, LongPropertyMethodPlan, LongPropertyOp,
    PropertyGetterMethodPlan,
    BinaryLongRecursionPlan, LongRecursiveBase, LongRecursiveCombine,
    LongRecursiveCondition,
    ComposedScalarLongFunctionPlan, ComposedScalarLongOp, ScalarLongCall,
    ScalarLongCallGuard, ScalarLongFunctionPlan, ScalarLongOp, ScalarLongOpKind,
    ScalarLongProgram, ScalarLongSource,
};
use std::collections::HashMap;
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
            let plan = crate::vm::quick::detect_long_induction_loop(
                self,
                header_ip,
                backedge_ip,
            )
            .map(BlockPlan::QuickLongInduction)
            .or_else(|| {
                crate::vm::quick::detect_long_accumulate_loop(
                    self,
                    header_ip,
                    backedge_ip,
                )
                .map(BlockPlan::QuickLongAccumulate)
            })
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
            if matches!(instruction.opcode, OpCode::DoFcall | OpCode::CallUserFuncArray | OpCode::InitUserCall) {
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
        if matches!(instr.opcode, OpCode::DirectInternalCall1 | OpCode::DirectInternalCall2) {
            let Some(kind) = crate::builtin_metadata::DirectInternalKind::from_id(
                instr.extended_value,
            ) else {
                return false;
            };
            if kind.result_may_need_cleanup() {
                return false;
            }
        }

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
                | OpCode::DirectInternalCall1
                | OpCode::DirectInternalCall2
                | OpCode::Strlen
                | OpCode::Strlen_Cv
                | OpCode::InitMethodCall
                | OpCode::InitStaticCall
                | OpCode::InitDynamicCall
                | OpCode::SendVal
                | OpCode::SendRef
                | OpCode::SendVarEx
                | OpCode::SendNamed
                | OpCode::CallUserFuncArray
                | OpCode::InitUserCall
                | OpCode::SendUser
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
    let mut function = UserFunction {
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
        long_property_plan: None,
        property_getter_plan: None,
        binary_long_recursion_plan: None,
        scalar_long_plan: None,
        composed_scalar_long_plan: None,
    };
    let self_name = function.op_array.name.clone();
    function.binary_long_recursion_plan =
        build_binary_long_recursion_plan(&function, &self_name);
    function.scalar_long_plan = build_scalar_long_function_plan(&function);
    function.composed_scalar_long_plan =
        build_composed_scalar_long_function_plan(&function);
    function
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
    let mut function = UserFunction {
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
        long_property_plan: None,
        property_getter_plan: None,
        binary_long_recursion_plan: None,
        scalar_long_plan: None,
        composed_scalar_long_plan: None,
    };
    let self_name = function.op_array.name.clone();
    function.binary_long_recursion_plan =
        build_binary_long_recursion_plan(&function, &self_name);
    function.scalar_long_plan = build_scalar_long_function_plan(&function);
    function.composed_scalar_long_plan =
        build_composed_scalar_long_function_plan(&function);
    function
}

const SCALAR_LONG_PLAN_MAX_ARGS: u32 = 8;
const SCALAR_LONG_PLAN_MAX_OPS: usize = 8;

fn scalar_long_source(
    op_array: &OpArray,
    temporary_results: &HashMap<u16, u8>,
    this_offset: u32,
    public_args: u32,
    op_type: OpType,
    operand: u16,
) -> Option<ScalarLongSource> {
    match op_type {
        OpType::Cv
            if operand as u32 >= this_offset
                && (operand as u32) < this_offset + public_args =>
        {
            Some(ScalarLongSource::Input((operand as u32 - this_offset) as u16))
        }
        OpType::Const => op_array
            .literals
            .get(operand as usize)
            .filter(|value| value.value_type() == crate::value::ValueType::Long)
            .and_then(Value::as_long)
            .map(ScalarLongSource::Constant),
        OpType::Tmp | OpType::Var => temporary_results
            .get(&operand)
            .copied()
            .map(ScalarLongSource::Temporary),
        OpType::Unused => None,
        _ => None,
    }
}

/// Recognize a small straight-line integer expression such as
/// `return ($a + 1) * $b`. This is deliberately narrower than general PHP
/// arithmetic: runtime Long guards and checked operations must all succeed or
/// the untouched canonical frame executes normally.
fn build_scalar_long_function_plan(
    function: &UserFunction,
) -> Option<Box<ScalarLongFunctionPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    if common.plan.call != CallStrategy::FastScalar
        || common.plan.ret != ReturnStrategy::Fast
        || public_args > SCALAR_LONG_PLAN_MAX_ARGS
        || op_array.instructions.len() > SCALAR_LONG_PLAN_MAX_OPS + 2
    {
        return None;
    }

    let mut temporary_results = HashMap::new();
    let mut operations = Vec::new();

    for instruction in &op_array.instructions {
        if instruction.opcode == OpCode::Return {
            // The compiler appends an implicit `return null` even after an
            // explicit return. Only an explicit scalar return proves a plan.
            if instruction.extended_value == 0 {
                return None;
            }
            let result = scalar_long_source(
                op_array,
                &temporary_results,
                common.sig.this_offset,
                public_args,
                instruction.op1_type,
                instruction.op1,
            )?;
            return Some(Box::new(ScalarLongFunctionPlan {
                public_args: public_args as u8,
                program: ScalarLongProgram {
                    operations: operations.into_boxed_slice(),
                    outputs: [result],
                    output_count: 1,
                },
            }));
        }

        let kind = match instruction.opcode {
            OpCode::Add | OpCode::Add_TmpTmp | OpCode::Add_CvTmp => {
                ScalarLongOpKind::Add
            }
            OpCode::Sub | OpCode::Sub_CvConst | OpCode::Sub_TmpTmp => {
                ScalarLongOpKind::Subtract
            }
            OpCode::Mul => ScalarLongOpKind::Multiply,
            _ => return None,
        };
        if operations.len() == SCALAR_LONG_PLAN_MAX_OPS
            || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
        {
            return None;
        }
        let lhs = scalar_long_source(
            op_array,
            &temporary_results,
            common.sig.this_offset,
            public_args,
            instruction.op1_type,
            instruction.op1,
        )?;
        let rhs = scalar_long_source(
            op_array,
            &temporary_results,
            common.sig.this_offset,
            public_args,
            instruction.op2_type,
            instruction.op2,
        )?;
        let result_index = operations.len() as u8;
        operations.push(ScalarLongOp { kind, lhs, rhs });
        temporary_results.insert(instruction.result, result_index);
    }

    None
}

const COMPOSED_SCALAR_LONG_PLAN_MAX_OPS: usize = 16;

/// Recognize a straight-line integer body that composes direct user functions
/// with arithmetic. The target function itself is guarded at runtime and must
/// expose either a scalar leaf plan or another composed body plan.
fn build_composed_scalar_long_function_plan(
    function: &UserFunction,
) -> Option<Box<ComposedScalarLongFunctionPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    if common.plan.call != CallStrategy::FastScalar
        || common.plan.ret != ReturnStrategy::Fast
        || public_args > SCALAR_LONG_PLAN_MAX_ARGS
        || op_array.instructions.len() > 32
    {
        return None;
    }

    let mut temporary_results = HashMap::new();
    let mut operations = Vec::new();
    let mut contains_call = false;
    let mut ip = 0usize;

    while ip < op_array.instructions.len() {
        let instruction = &op_array.instructions[ip];
        if instruction.opcode == OpCode::Return {
            if instruction.extended_value == 0 || !contains_call {
                return None;
            }
            let result = scalar_long_source(
                op_array,
                &temporary_results,
                common.sig.this_offset,
                public_args,
                instruction.op1_type,
                instruction.op1,
            )?;
            return Some(Box::new(ComposedScalarLongFunctionPlan {
                public_args: public_args as u8,
                program: ScalarLongProgram {
                    operations: operations.into_boxed_slice(),
                    outputs: [result],
                    output_count: 1,
                },
            }));
        }

        if instruction.opcode == OpCode::InitFcall {
            let num_args = instruction.op1 as usize;
            if num_args > SCALAR_LONG_PLAN_MAX_ARGS as usize
                || ip > u32::MAX as usize
                || operations.len() == COMPOSED_SCALAR_LONG_PLAN_MAX_OPS
                || ip + num_args + 1 >= op_array.instructions.len()
            {
                return None;
            }
            let mut arguments = Vec::with_capacity(num_args);
            for argument_index in 0..num_args {
                let send = &op_array.instructions[ip + 1 + argument_index];
                if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                    || send.op2 as usize != argument_index
                {
                    return None;
                }
                arguments.push(scalar_long_source(
                    op_array,
                    &temporary_results,
                    common.sig.this_offset,
                    public_args,
                    send.op1_type,
                    send.op1,
                )?);
            }
            let do_fcall = &op_array.instructions[ip + 1 + num_args];
            if do_fcall.opcode != OpCode::DoFcall
                || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
            {
                return None;
            }
            let result_index = operations.len() as u8;
            operations.push(ComposedScalarLongOp::Call(ScalarLongCall {
                guard: ScalarLongCallGuard::FunctionCache {
                    cache_ip: ip as u32,
                },
                arguments: arguments.into_boxed_slice(),
            }));
            temporary_results.insert(do_fcall.result, result_index);
            contains_call = true;
            ip += num_args + 2;
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
            _ => return None,
        };
        if operations.len() == COMPOSED_SCALAR_LONG_PLAN_MAX_OPS
            || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
        {
            return None;
        }
        let lhs = scalar_long_source(
            op_array,
            &temporary_results,
            common.sig.this_offset,
            public_args,
            instruction.op1_type,
            instruction.op1,
        )?;
        let rhs = scalar_long_source(
            op_array,
            &temporary_results,
            common.sig.this_offset,
            public_args,
            instruction.op2_type,
            instruction.op2,
        )?;
        let result_index = operations.len() as u8;
        operations.push(ComposedScalarLongOp::Arithmetic(ScalarLongOp {
            kind,
            lhs,
            rhs,
        }));
        temporary_results.insert(instruction.result, result_index);
        ip += 1;
    }

    None
}

fn build_binary_long_recursion_plan(
    function: &UserFunction,
    self_name: &str,
) -> Option<BinaryLongRecursionPlan> {
    let common = &function.common;
    let op_array = &function.op_array;
    let this_offset = common.sig.this_offset;
    let argument_cv = this_offset as u16;
    let has_no_type_hints = common
        .sig
        .param_type_hints
        .iter()
        .all(|hint| matches!(hint, ParamTypeHint::None | ParamTypeHint::Mixed));
    if self_name.is_empty()
        || common.sig.public_arity() != 1
        || common.sig.required_num_args != 1
        || common.sig.is_variadic
        || common.sig.ref_args != 0
        || !has_no_type_hints
        || !matches!(common.sig.return_type_hint, ParamTypeHint::None | ParamTypeHint::Mixed)
        || op_array.is_generator
        || !op_array.global_vars.is_empty()
        || !op_array.static_vars.is_empty()
        || !op_array.try_entries.is_empty()
        || op_array.num_cvs != common.sig.num_args
        || op_array.instructions.len() != 14
    {
        return None;
    }

    let instructions = &op_array.instructions;
    let condition = match instructions[0].opcode {
        OpCode::JmpZ_Lt_CvConst => LongRecursiveCondition::LessThan,
        OpCode::JmpZ_Le_CvConst => LongRecursiveCondition::LessThanOrEqual,
        _ => return None,
    };
    let condition_instruction = &instructions[0];
    if condition_instruction.op1_type != OpType::Cv
        || condition_instruction.op1 != argument_cv
        || condition_instruction.op2_type != OpType::Const
        || condition_instruction.result as usize != 3
        || instructions[1].opcode != OpCode::JmpZ
        || instructions[1].op2 as usize != 3
    {
        return None;
    }
    let threshold = op_array
        .literals
        .get(condition_instruction.op2 as usize)?
        .as_long()?;

    let base_return = &instructions[2];
    if base_return.opcode != OpCode::Return {
        return None;
    }
    let base = match base_return.op1_type {
        OpType::Cv if base_return.op1 == argument_cv => LongRecursiveBase::Argument,
        OpType::Const => LongRecursiveBase::Constant(
            op_array.literals.get(base_return.op1 as usize)?.as_long()?,
        ),
        _ => return None,
    };

    let recursive_call = |start: usize| -> Option<(i64, u16)> {
        let initializer = &instructions[start];
        let subtract = &instructions[start + 1];
        let send = &instructions[start + 2];
        let call = &instructions[start + 3];

        if this_offset == 0 {
            if initializer.opcode != OpCode::InitFcall
                || initializer.op1 != 1
                || initializer.op2_type != OpType::Const
            {
                return None;
            }
        } else if initializer.opcode != OpCode::InitMethodCall
            || initializer.op1_type != OpType::Cv
            || initializer.op1 != 0
            || initializer.op2_type != OpType::Const
            || initializer.extended_value != 1
        {
            return None;
        }
        let target_name = op_array
            .literals
            .get(initializer.op2 as usize)?
            .as_str()?;
        if !target_name.eq_ignore_ascii_case(self_name) {
            return None;
        }

        if subtract.opcode != OpCode::Sub_CvConst
            || subtract.op1_type != OpType::Cv
            || subtract.op1 != argument_cv
            || subtract.op2_type != OpType::Const
            || !matches!(subtract.result_type, OpType::Tmp | OpType::Var)
            || send.opcode != OpCode::SendVal
            || send.op1_type != subtract.result_type
            || send.op1 != subtract.result
            || send.op2 != argument_cv
            || call.opcode != OpCode::DoFcall
            || !matches!(call.result_type, OpType::Tmp | OpType::Var)
        {
            return None;
        }
        let delta = op_array
            .literals
            .get(subtract.op2 as usize)?
            .as_long()?;
        if delta <= 0 {
            return None;
        }
        Some((delta, call.result))
    };

    let (first_delta, first_result) = recursive_call(3)?;
    let (second_delta, second_result) = recursive_call(7)?;
    let combine_instruction = &instructions[11];
    let operands_match = combine_instruction.op1_type == OpType::Tmp
        && combine_instruction.op2_type == OpType::Tmp
        && combine_instruction.op1 == first_result
        && combine_instruction.op2 == second_result;
    let commutative_operands_match = operands_match
        || (combine_instruction.op1_type == OpType::Tmp
            && combine_instruction.op2_type == OpType::Tmp
            && combine_instruction.op1 == second_result
            && combine_instruction.op2 == first_result);
    let combine = match combine_instruction.opcode {
        OpCode::Add_TmpTmp if commutative_operands_match => LongRecursiveCombine::Add,
        OpCode::Sub_TmpTmp if operands_match => LongRecursiveCombine::Subtract,
        OpCode::Mul if commutative_operands_match => LongRecursiveCombine::Multiply,
        _ => return None,
    };
    let result_return = &instructions[12];
    if !matches!(combine_instruction.result_type, OpType::Tmp | OpType::Var)
        || result_return.opcode != OpCode::Return
        || result_return.op1_type != combine_instruction.result_type
        || result_return.op1 != combine_instruction.result
        || instructions[13].opcode != OpCode::Return
    {
        return None;
    }

    Some(BinaryLongRecursionPlan {
        condition,
        threshold,
        base,
        first_delta,
        second_delta,
        combine,
        method_name: (this_offset == 1).then(|| self_name.to_string().into_boxed_str()),
    })
}

const LONG_PROPERTY_PLAN_MAX_ARGS: u32 = 8;
const LONG_PROPERTY_PLAN_MAX_PROPERTIES: usize = 8;

fn long_plan_source(
    op_array: &OpArray,
    op_type: OpType,
    operand: u16,
    public_args: u32,
) -> Option<LongPlanSource> {
    match op_type {
        OpType::Cv if operand > 0 && operand as u32 <= public_args => {
            Some(LongPlanSource::Argument((operand - 1) as u8))
        }
        OpType::Const => op_array
            .literals
            .get(operand as usize)
            .and_then(Value::as_long)
            .map(LongPlanSource::Constant),
        _ => None,
    }
}

fn property_name<'a>(op_array: &'a OpArray, instruction: &Instruction) -> Option<&'a str> {
    if instruction.op1_type != OpType::Cv
        || instruction.op1 != 0
        || instruction.op2_type != OpType::Const
    {
        return None;
    }
    op_array
        .literals
        .get(instruction.op2 as usize)
        .and_then(Value::as_str)
}

/// Prove the exact zero-argument getter shape emitted for
/// `return $this->property;`.  Keeping this deliberately narrower than the
/// discarded-return property planner means the runtime may safely materialize
/// the fetched value without replaying any other method behavior.
fn build_property_getter_method_plan(
    function: &UserFunction,
) -> Option<PropertyGetterMethodPlan> {
    let common = &function.common;
    let op_array = &function.op_array;
    if common.plan.call != CallStrategy::FastScalar
        || common.plan.ret != ReturnStrategy::Fast
        || common.sig.this_offset != 1
        || common.sig.public_arity() != 0
        || op_array.num_cvs != common.sig.num_args
        || op_array.instructions.len() != 3
    {
        return None;
    }

    let fetch = &op_array.instructions[0];
    let explicit_return = &op_array.instructions[1];
    let implicit_return = &op_array.instructions[2];
    if fetch.opcode != OpCode::FetchObjR
        || !matches!(fetch.result_type, OpType::Tmp | OpType::Var)
        || property_name(op_array, fetch).is_none()
        || explicit_return.opcode != OpCode::Return
        || explicit_return.extended_value != 1
        || explicit_return.op1_type != fetch.result_type
        || explicit_return.op1 != fetch.result
        || implicit_return.opcode != OpCode::Return
        || implicit_return.extended_value != 0
        || implicit_return.op1_type != OpType::Const
        || op_array
            .literals
            .get(implicit_return.op1 as usize)?
            .value_type()
            != crate::value::ValueType::Null
    {
        return None;
    }

    Some(PropertyGetterMethodPlan { cache_ip: 0 })
}

fn register_long_plan_property(
    properties: &mut Vec<LongPlanProperty>,
    indices: &mut HashMap<String, u8>,
    name: &str,
    cache_ip: usize,
    required_flags: u8,
) -> Option<u8> {
    if let Some(&property) = indices.get(name) {
        let guard = &mut properties[property as usize];
        if required_flags > guard.required_flags {
            guard.cache_ip = cache_ip as u16;
            guard.required_flags = required_flags;
        }
        return Some(property);
    }
    if properties.len() == LONG_PROPERTY_PLAN_MAX_PROPERTIES || cache_ip > u16::MAX as usize {
        return None;
    }
    let property = properties.len() as u8;
    properties.push(LongPlanProperty {
        cache_ip: cache_ip as u16,
        required_flags,
    });
    indices.insert(name.to_string(), property);
    Some(property)
}

/// Recognize small, side-effect-free integer property methods once, after
/// opcode specialization. The resulting plan is independent of class and
/// property names; runtime inline caches provide the guarded numeric slots.
fn build_long_property_method_plan(function: &UserFunction) -> Option<Box<LongPropertyMethodPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    if common.plan.call != CallStrategy::FastScalar
        || common.sig.this_offset != 1
        || public_args > LONG_PROPERTY_PLAN_MAX_ARGS
        || op_array.instructions.len() > 32
        || op_array.num_cvs != common.sig.num_args
    {
        return None;
    }

    let instructions = &op_array.instructions;
    let mut properties = Vec::new();
    let mut property_indices = HashMap::new();
    let mut operations = Vec::new();
    let mut ip = 0usize;

    while ip < instructions.len() {
        let instruction = &instructions[ip];

        // $this->p = $this->p +/- scalar
        if instruction.opcode == OpCode::FetchObjR && ip + 2 < instructions.len() {
            let arithmetic = &instructions[ip + 1];
            let assign = &instructions[ip + 2];
            if matches!(arithmetic.opcode, OpCode::Add | OpCode::Add_TmpTmp | OpCode::Add_CvTmp | OpCode::Sub | OpCode::Sub_TmpTmp | OpCode::Sub_CvConst)
                && assign.opcode == OpCode::AssignObjProp
                && arithmetic.result_type != OpType::Unused
                && assign.result_type == arithmetic.result_type
                && assign.result == arithmetic.result
            {
                let fetched_name = property_name(op_array, instruction)?;
                let assigned_name = property_name(op_array, assign)?;
                if fetched_name == assigned_name {
                    let rhs = if arithmetic.op1_type == instruction.result_type
                        && arithmetic.op1 == instruction.result
                    {
                        long_plan_source(op_array, arithmetic.op2_type, arithmetic.op2, public_args)
                    } else if arithmetic.opcode != OpCode::Sub
                        && arithmetic.opcode != OpCode::Sub_TmpTmp
                        && arithmetic.opcode != OpCode::Sub_CvConst
                        && arithmetic.op2_type == instruction.result_type
                        && arithmetic.op2 == instruction.result
                    {
                        long_plan_source(op_array, arithmetic.op1_type, arithmetic.op1, public_args)
                    } else {
                        None
                    }?;
                    let property = register_long_plan_property(
                        &mut properties,
                        &mut property_indices,
                        fetched_name,
                        ip + 2,
                        3,
                    )?;
                    operations.push(if matches!(arithmetic.opcode, OpCode::Sub | OpCode::Sub_TmpTmp | OpCode::Sub_CvConst) {
                        LongPropertyOp::Sub { property, rhs }
                    } else {
                        LongPropertyOp::Add { property, rhs }
                    });
                    ip += 3;
                    continue;
                }
            }
        }

        // if ($candidate < $this->p) $this->p = $candidate (and max mirror)
        if instruction.opcode == OpCode::FetchObjR && ip + 3 < instructions.len() {
            let comparison = &instructions[ip + 1];
            let branch = &instructions[ip + 2];
            let assign = &instructions[ip + 3];
            if matches!(comparison.opcode, OpCode::IsSmaller | OpCode::IsSmallerOrEqual)
                && branch.opcode == OpCode::JmpZ
                && branch.op1_type == comparison.result_type
                && branch.op1 == comparison.result
                && branch.op2 as usize == ip + 4
                && assign.opcode == OpCode::AssignObjProp
            {
                let fetched_name = property_name(op_array, instruction)?;
                let assigned_name = property_name(op_array, assign)?;
                if fetched_name == assigned_name {
                    let (candidate, is_min) = if comparison.op2_type == instruction.result_type
                        && comparison.op2 == instruction.result
                    {
                        (long_plan_source(op_array, comparison.op1_type, comparison.op1, public_args), true)
                    } else if comparison.op1_type == instruction.result_type
                        && comparison.op1 == instruction.result
                    {
                        (long_plan_source(op_array, comparison.op2_type, comparison.op2, public_args), false)
                    } else {
                        (None, false)
                    };
                    let candidate = candidate?;
                    let assigned = long_plan_source(op_array, assign.result_type, assign.result, public_args)?;
                    if candidate != assigned {
                        return None;
                    }
                    let property = register_long_plan_property(
                        &mut properties,
                        &mut property_indices,
                        fetched_name,
                        ip + 3,
                        3,
                    )?;
                    operations.push(if is_min {
                        LongPropertyOp::Min { property, candidate }
                    } else {
                        LongPropertyOp::Max { property, candidate }
                    });
                    ip += 4;
                    continue;
                }
            }
        }

        // A property read used only by Return has no observable result when
        // the call site discards that return, but its cache/type guard remains.
        if instruction.opcode == OpCode::FetchObjR && ip + 1 < instructions.len() {
            let ret = &instructions[ip + 1];
            if ret.opcode == OpCode::Return
                && ret.op1_type == instruction.result_type
                && ret.op1 == instruction.result
            {
                let name = property_name(op_array, instruction)?;
                register_long_plan_property(
                    &mut properties,
                    &mut property_indices,
                    name,
                    ip,
                    1,
                )?;
                ip += 1;
                continue;
            }
        }

        // Direct scalar property assignment is also transactional.
        if instruction.opcode == OpCode::AssignObjProp {
            let name = property_name(op_array, instruction)?;
            let value = long_plan_source(
                op_array,
                instruction.result_type,
                instruction.result,
                public_args,
            )?;
            let property = register_long_plan_property(
                &mut properties,
                &mut property_indices,
                name,
                ip,
                3,
            )?;
            operations.push(LongPropertyOp::Set { property, value });
            ip += 1;
            continue;
        }

        if instruction.opcode == OpCode::Return {
            ip += 1;
            continue;
        }

        return None;
    }

    if properties.is_empty() {
        return None;
    }
    Some(Box::new(LongPropertyMethodPlan {
        public_args: public_args as u8,
        properties: properties.into_boxed_slice(),
        operations: operations.into_boxed_slice(),
    }))
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
        direct_handler: None,
    }
}

/// Create a fixed-arity internal function with an additional frame-free ABI.
/// The ordinary handler remains the canonical path for normal PHP calls; the
/// direct handler is selected only when a callback already exposes borrowed,
/// packed positional arguments.
pub fn make_direct_internal_function(
    handler: InternalFunctionHandler,
    direct_handler: DirectInternalFunctionHandler,
    num_args: u32,
    required_num_args: u32,
    param_names: Vec<String>,
) -> InternalFunction {
    let mut function = make_internal_function(
        handler,
        num_args,
        required_num_args,
        param_names,
    );
    function.direct_handler = Some(direct_handler);
    function
}

/// Finalize a non-static user method after `$this` has been reserved at CV 0.
///
/// `make_user_function_typed()` cannot classify a method as FastScalar while
/// `this_offset` is still zero: `num_args` already includes the hidden `$this`
/// slot while `required_num_args` intentionally counts only public arguments.
/// Re-run that classification once the public signature is known.
pub fn finalize_user_method(mut function: UserFunction, method_name: &str) -> UserFunction {
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

    function.long_property_plan = build_long_property_method_plan(&function);
    function.property_getter_plan = build_property_getter_method_plan(&function);
    function.binary_long_recursion_plan =
        build_binary_long_recursion_plan(&function, method_name);
    function.scalar_long_plan = build_scalar_long_function_plan(&function);
    function.composed_scalar_long_plan =
        build_composed_scalar_long_function_plan(&function);

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
        direct_handler: None,
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
        direct_handler: None,
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
        direct_handler: None,
    }
}
